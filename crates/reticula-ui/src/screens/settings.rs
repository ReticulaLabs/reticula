//! Settings menu: sub-menus for WiFi, LoRa, GPS, system and data management.
//!
//! Styled like the root menu: full-width rows with a section icon on the left
//! and the section name (plus a short description) beside it. Both menus share
//! [`crate::widgets::draw_menu_list`], so their rows are identical in size
//! regardless of how many entries each has.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, ScreenId};
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px, MenuItem};

#[derive(Default)]
pub struct SettingsScreen {
    pub state: ListState,
}

impl SettingsScreen {
    const ITEMS: [MenuItem; 5] = [
        MenuItem {
            title: "WiFi",
            subtitle: "Network connection",
            icon: widgets::MenuIcon::Wifi,
        },
        MenuItem {
            title: "LoRa",
            subtitle: "Radio settings",
            icon: widgets::MenuIcon::Lora,
        },
        MenuItem {
            title: "GPS",
            subtitle: "Satellite receiver",
            icon: widgets::MenuIcon::Gps,
        },
        MenuItem {
            title: "System",
            subtitle: "Date and time",
            icon: widgets::MenuIcon::Clock,
        },
        MenuItem {
            title: "Data Management",
            subtitle: "Identity and storage",
            icon: widgets::MenuIcon::Data,
        },
    ];

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
                0 => Command::ShowScreen(ScreenId::SettingsWifi),
                1 => Command::ShowScreen(ScreenId::SettingsLora),
                2 => Command::ShowScreen(ScreenId::SettingsGps),
                3 => Command::ShowScreen(ScreenId::SettingsSystem),
                _ => Command::ShowScreen(ScreenId::SettingsData),
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

        widgets::draw_header(target, width, "Settings", &ctx.network, theme).ok();

        // Network status band directly under the header.
        let status = if ctx.network.connected {
            "network connected"
        } else {
            "network connecting..."
        };
        let status_line = format!(
            "{}   {} link{}   up {}s",
            status,
            ctx.network.peer_links,
            if ctx.network.peer_links == 1 { "" } else { "s" },
            ctx.network.uptime_ms / 1000,
        );
        let status_line = widgets::truncate(&status_line, theme.chars_per_line(width - 6));
        widgets::draw_text(
            target,
            Point::new(6, theme.header_h),
            &status_line,
            theme.text_dim,
            theme,
        )
        .ok();
        let body_top = widgets::body_top(theme);
        widgets::fill_rect(
            target,
            Rectangle::new(Point::new(0, body_top), px(width, 1)),
            theme.border,
        )
        .ok();

        // Full-width rows, sized by the shared list renderer so this menu and
        // the root menu are identical row-for-row.
        let y = widgets::draw_menu_list(
            target,
            width,
            height,
            body_top,
            &Self::ITEMS,
            self.state.selected,
            theme,
        );

        // Transient notice (e.g. "restarting...") below the list.
        if !ctx.notice.is_empty() && y + theme.line_h <= height - theme.line_h {
            widgets::draw_text(
                target,
                Point::new(6, y + theme.line_h),
                ctx.notice,
                theme.ok,
                theme,
            )
            .ok();
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
