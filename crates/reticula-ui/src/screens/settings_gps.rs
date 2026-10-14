//! Settings sub-menu: GNSS (GPS) receiver.
//!
//! The receiver is used as a wall-clock source, not for positioning. That is
//! why it is on by default: a node whose clock is still at 1970 gets its
//! announces discarded by every peer as stale, so a device away from WiFi has no
//! other way to become visible to the network.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, GpsSettings};
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

/// Number of selectable rows: the toggle and the save button.
const ROWS: usize = 2;

pub struct SettingsGpsScreen {
    pub state: ListState,
    enabled: bool,
    /// Whether `enabled` has been seeded from the current config.
    seeded: bool,
}

impl SettingsGpsScreen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(ROWS);
                Command::None
            }
            // Row 0 toggles, row 1 saves.
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.enabled = !self.enabled;
                    Command::None
                }
                _ => Command::SaveGps(GpsSettings {
                    enabled: self.enabled,
                }),
            },
            KeyCode::Esc => Command::Back,
            _ => Command::None,
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        if !self.seeded {
            self.seeded = true;
            self.enabled = ctx.gps_settings.copied().unwrap_or_default().enabled;
        }

        widgets::draw_header(target, width, "GPS", &ctx.network, theme).ok();

        let mut y = theme.header_h;

        // What the receiver is for, and why it matters even with no fix.
        widgets::draw_text(
            target,
            Point::new(0, y),
            "Receiver supplies the clock, not",
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h;
        widgets::draw_text(
            target,
            Point::new(0, y),
            "a position. Without it a node",
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h + 4;

        let selected = self.state.selected;

        // Row 0: enabled toggle.
        let value = if self.enabled { "on" } else { "off" };
        y = draw_row(
            target,
            width,
            y,
            theme,
            selected,
            0,
            &format!("Receiver: {value}"),
        );

        // Row 1: save & restart.
        y = draw_row(target, width, y, theme, selected, 1, "Save & restart");

        if !ctx.notice.is_empty() {
            y += theme.line_h;
            widgets::draw_text(target, Point::new(0, y), ctx.notice, theme.ok, theme).ok();
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(
            target,
            footer,
            "ALT+Backspace back",
            &short,
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();
    }
}

/// Draw one selectable row at `y`, highlighted when it is `index` and the
/// list's current selection. Returns the next row's y.
fn draw_row<D>(
    target: &mut D,
    width: i32,
    y: i32,
    theme: &Theme,
    selected_row: usize,
    index: usize,
    label: &str,
) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let at = Point::new(0, y);
    if selected_row == index {
        widgets::draw_highlight(
            target,
            at,
            label,
            width,
            theme.selection,
            theme.selection_text,
            theme,
        )
        .ok();
    } else {
        widgets::draw_text(target, at, label, theme.text, theme).ok();
    }
    y + theme.line_h
}

impl Default for SettingsGpsScreen {
    fn default() -> Self {
        Self {
            state: ListState::default(),
            enabled: true,
            seeded: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Row indices, in the order they are rendered.
    const ROW_TOGGLE: usize = 0;
    const ROW_SAVE: usize = 1;

    fn screen() -> SettingsGpsScreen {
        SettingsGpsScreen::new()
    }

    /// Seed from a context carrying `gps_settings`, as the first render does.
    fn seed_from(s: &mut SettingsGpsScreen, settings: Option<GpsSettings>) {
        s.seeded = true;
        s.enabled = settings.unwrap_or_default().enabled;
    }

    #[test]
    fn defaults_to_enabled() {
        // The receiver is the only time source available without WiFi, so it
        // is on unless the user turns it off.
        assert!(GpsSettings::default().enabled);
        assert!(screen().enabled);
    }

    #[test]
    fn enter_toggles_the_receiver() {
        let mut s = screen();
        s.state.selected = ROW_TOGGLE;
        assert_eq!(s.handle_key(KeyCode::Enter), Command::None);
        assert!(!s.enabled, "Enter on the toggle should switch it off");

        s.handle_key(KeyCode::Enter);
        assert!(s.enabled, "Enter again should switch it back on");
    }

    #[test]
    fn saving_emits_the_current_toggle_state() {
        let mut s = screen();
        s.state.selected = ROW_TOGGLE;
        s.handle_key(KeyCode::Enter); // -> off
        s.state.selected = ROW_SAVE;

        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::SaveGps(GpsSettings { enabled: false })
        );
    }

    #[test]
    fn saving_an_untouched_screen_stays_enabled() {
        let mut s = screen();
        s.state.selected = ROW_SAVE;
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::SaveGps(GpsSettings { enabled: true })
        );
    }

    /// A stored "off" must be shown as off, so saving without touching the
    /// toggle does not silently switch the receiver back on.
    #[test]
    fn seeding_picks_up_a_stored_preference() {
        let mut s = screen();
        seed_from(&mut s, Some(GpsSettings { enabled: false }));
        assert!(!s.enabled, "the screen must show the stored preference");

        s.state.selected = ROW_SAVE;
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::SaveGps(GpsSettings { enabled: false }),
            "saving must round-trip the stored value"
        );
    }

    /// With no stored settings at all, the default stands.
    #[test]
    fn seeding_without_settings_keeps_the_default() {
        let mut s = screen();
        seed_from(&mut s, None);
        assert!(s.enabled);
    }

    #[test]
    fn escape_goes_back() {
        let mut s = screen();
        assert_eq!(s.handle_key(KeyCode::Esc), Command::Back);
    }

    #[test]
    fn navigation_is_bounded_to_the_two_rows() {
        let mut s = screen();
        for _ in 0..10 {
            s.handle_key(KeyCode::Down);
        }
        assert_eq!(s.state.selected, ROW_SAVE);

        for _ in 0..10 {
            s.handle_key(KeyCode::Up);
        }
        assert_eq!(s.state.selected, ROW_TOGGLE);
    }
}
