//! Settings screen: identity info and a couple of actions.

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

#[derive(Default)]
pub struct SettingsScreen {
    pub state: ListState,
    /// True while editing the display name.
    pub editing: bool,
    pub name_input: String,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self { state: ListState::default(), editing: false, name_input: String::new() }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if self.editing {
            return match key {
                KeyCode::Char(c) if c.is_ascii() => {
                    self.name_input.push(c);
                    Command::None
                }
                KeyCode::Space => {
                    self.name_input.push(' ');
                    Command::None
                }
                KeyCode::Backspace => {
                    self.name_input.pop();
                    Command::None
                }
                KeyCode::Enter => {
                    let name = self.name_input.trim().to_string();
                    self.editing = false;
                    if name.is_empty() {
                        Command::None
                    } else {
                        Command::SetDisplayName(name)
                    }
                }
                KeyCode::Esc => {
                    self.editing = false;
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
                self.state.move_down(2);
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.editing = true;
                    self.name_input.clear();
                    Command::None
                }
                _ => Command::Announce,
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

        let header = Rectangle::new(Point::new(0, 0), px(width, theme.line_h));
        widgets::draw_bar(target, header, "Settings", "", theme.header, theme.header_text, theme)
            .ok();

        let mut y = header.size.height as i32;

        // Row 0: display name.
        if self.editing {
            widgets::draw_text(target, Point::new(0, y), "Display name:", theme.text_dim, theme)
                .ok();
            y += theme.line_h;
            let text = widgets::truncate(&self.name_input, theme.chars_per_line(width - 12));
            widgets::fill_rect(
                target,
                Rectangle::new(Point::new(0, y), px(width, theme.line_h)),
                theme.surface,
            )
            .ok();
            widgets::draw_text(target, Point::new(0, y), &text, theme.text, theme).ok();
            y += theme.line_h;
        } else {
            let label = format!("Name: {}", ctx.display_name);
            let at = Point::new(0, y);
            if self.state.selected == 0 {
                widgets::draw_highlight(
                    target,
                    at,
                    &label,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, at, &label, theme.text, theme).ok();
            }
            y += theme.line_h;
        }

        // Row 1: re-announce.
        let at = Point::new(0, y);
        if self.state.selected == 1 {
            widgets::draw_highlight(
                target,
                at,
                "Re-announce identities",
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, "Re-announce identities", theme.text, theme).ok();
        }
        y += theme.line_h;

        // Static info.
        let status = if ctx.network.connected {
            "Network: connected"
        } else {
            "Network: connecting..."
        };
        widgets::draw_text(target, Point::new(0, y), status, theme.text_dim, theme).ok();
        y += theme.line_h;
        widgets::draw_text(
            target,
            Point::new(0, y),
            &format!("Links: {}", ctx.network.peer_links),
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h;
        let secs = ctx.network.uptime_ms / 1000;
        widgets::draw_text(
            target,
            Point::new(0, y),
            &format!("Uptime: {secs}s"),
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h;

        widgets::draw_text(target, Point::new(0, y), "LXMF address:", theme.text_dim, theme).ok();
        y += theme.line_h;
        widgets::draw_wrapped(
            target,
            Point::new(0, y),
            ctx.identity_hex,
            width - 6,
            4,
            theme.text,
            theme,
        )
        .ok();
    }
}