//! Settings menu: sub-menus for identity and WiFi, plus a few actions.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, ScreenId};
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

#[derive(Default)]
pub struct SettingsScreen {
    pub state: ListState,
}

impl SettingsScreen {
    const ITEMS: [&'static str; 4] = ["Identity", "WiFi", "LoRa", "Re-announce identities"];

    pub fn new() -> Self {
        Self { state: ListState::default() }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(Self::ITEMS.len());
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => Command::ShowScreen(ScreenId::SettingsIdentity),
                1 => Command::ShowScreen(ScreenId::SettingsWifi),
                2 => Command::ShowScreen(ScreenId::SettingsLora),
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
        let height = size.height as i32;

        widgets::draw_header(target, width, "Settings", "", &ctx.network, theme).ok();

        let mut y = theme.line_h;
        for (i, item) in Self::ITEMS.iter().enumerate() {
            let label = format!("{}. {item}", i + 1);
            let row = Point::new(0, y);
            if i == self.state.selected {
                widgets::draw_highlight(
                    target,
                    row,
                    &label,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, row, &label, theme.text, theme).ok();
            }
            y += theme.line_h;
        }

        y += theme.line_h;

        // Static network info.
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