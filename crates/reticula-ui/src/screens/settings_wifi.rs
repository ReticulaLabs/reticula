//! Settings sub-menu: WiFi network — SSID and password.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Ssid,
    Password,
}

#[derive(Default)]
pub struct SettingsWifiScreen {
    pub state: ListState,
    editing: Option<Field>,
    ssid_input: String,
    pass_input: String,
}

impl SettingsWifiScreen {
    pub fn new() -> Self {
        Self {
            state: ListState::default(),
            editing: None,
            ssid_input: String::new(),
            pass_input: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if let Some(field) = self.editing {
            let input = match field {
                Field::Ssid => &mut self.ssid_input,
                Field::Password => &mut self.pass_input,
            };
            return match key {
                KeyCode::Char(c) if c.is_ascii() => {
                    input.push(c);
                    Command::None
                }
                KeyCode::Space => {
                    input.push(' ');
                    Command::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    Command::None
                }
                KeyCode::Enter => {
                    self.editing = None;
                    Command::None
                }
                KeyCode::Esc => {
                    self.editing = None;
                    Command::None
                }
                _ => Command::None,
            };
        }

        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(3);
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.editing = Some(Field::Ssid);
                    Command::None
                }
                1 => {
                    self.editing = Some(Field::Password);
                    Command::None
                }
                _ => {
                    if self.ssid_input.trim().is_empty() {
                        Command::None
                    } else {
                        Command::SaveWifi {
                            ssid: self.ssid_input.trim().to_string(),
                            password: self.pass_input.clone(),
                        }
                    }
                }
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

        widgets::draw_header(target, width, "WiFi", "", &ctx.network, theme).ok();

        let mut y = theme.line_h;

        // Current configuration / status.
        let status = if !ctx.wifi_ssid.is_empty() {
            format!("Network: {}", ctx.wifi_ssid)
        } else {
            "Network: not configured".to_string()
        };
        widgets::draw_text(target, Point::new(0, y), &status, theme.text_dim, theme).ok();
        y += theme.line_h;
        let link = if ctx.network.wifi_connected {
            "Link: connected"
        } else {
            "Link: not connected"
        };
        widgets::draw_text(target, Point::new(0, y), link, theme.text_dim, theme).ok();
        y += theme.line_h + 4;

        // Row 0: SSID.
        y = self.draw_field(target, width, y, ctx, theme, 0, "SSID", &self.ssid_input, Field::Ssid);
        // Row 1: password.
        y = self.draw_field(target, width, y, ctx, theme, 1, "Password", &self.pass_input, Field::Password);

        // Row 2: save & reconnect.
        let at = Point::new(0, y);
        let label = "Save & reconnect";
        if self.state.selected == 2 {
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
        y += theme.line_h;

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

    /// Draw one editable field row (`SSID` or `Password`) with its current
    /// value, highlighting it when selected or being edited.
    fn draw_field<D>(
        &self,
        target: &mut D,
        width: i32,
        y: i32,
        _ctx: &ViewContext,
        theme: &Theme,
        index: usize,
        label: &str,
        value: &str,
        field: Field,
    ) -> i32
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let is_selected = self.state.selected == index;
        let is_editing = self.editing == Some(field);

        let text = if is_editing {
            value.to_string()
        } else {
            // Mask the password when not being edited.
            if field == Field::Password && !value.is_empty() {
                mask_password(value)
            } else if value.is_empty() {
                if field == Field::Password {
                    "••••".to_string()
                } else {
                    "(none)".to_string()
                }
            } else {
                value.to_string()
            }
        };

        let line = format!("{label}: {text}");
        if is_selected || is_editing {
            widgets::draw_highlight(
                target,
                Point::new(0, y),
                &line,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, Point::new(0, y), &line, theme.text, theme).ok();
        }
        y + theme.line_h
    }
}

fn mask_password(pass: &str) -> String {
    let n = pass.chars().count();
    if n == 0 {
        return String::new();
    }
    // Show bullets for all but the last character, which stays visible so the
    // user can see what they typed.
    let mut out: String = "•".repeat(n.saturating_sub(1));
    out.push(pass.chars().last().unwrap());
    out
}