//! Settings menu: sub-menus for WiFi, LoRa, and data management.
//!
//! Styled like the root menu: full-width rows, each roughly a sixth of the
//! screen tall, with a section icon on the left and the section name (plus a
//! short description) beside it.

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

/// One entry on the settings menu.
struct MenuItem {
    /// Section name, shown next to the icon.
    title: &'static str,
    /// Short description shown beneath the name.
    subtitle: &'static str,
    /// Icon shown on the left of the row.
    icon: widgets::MenuIcon,
}

#[derive(Default)]
pub struct SettingsScreen {
    pub state: ListState,
}

impl SettingsScreen {
    const ITEMS: [MenuItem; 3] = [
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

        widgets::draw_header(target, width, "Settings", "", &ctx.network, theme).ok();

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
            Point::new(6, theme.line_h),
            &status_line,
            theme.text_dim,
            theme,
        )
        .ok();
        widgets::fill_rect(
            target,
            Rectangle::new(Point::new(0, theme.line_h * 2), px(width, 1)),
            theme.border,
        )
        .ok();

        // Each row is about a sixth of the screen tall and spans the full
        // width.
        let row_h = (height / 6).max(theme.line_h + 8);
        let rows_total = row_h * Self::ITEMS.len() as i32;
        let list_top = theme.line_h * 2 + 1;
        let list_bottom = height - theme.line_h;
        let list_h = (list_bottom - list_top).max(0);
        // Centre the rows, snapped to the text grid so labels stay crisp on
        // the low-resolution LCD (and readable in the terminal simulator).
        let mut y = list_top + ((list_h - rows_total).max(0) / 2);
        y -= y % theme.line_h;

        for (i, item) in Self::ITEMS.iter().enumerate() {
            let selected = i == self.state.selected;
            let row = Rectangle::new(Point::new(0, y), px(width, row_h));

            if selected {
                widgets::fill_rect(target, row, theme.selection).ok();
                // Left accent bar marks the active row.
                widgets::fill_rect(
                    target,
                    Rectangle::new(Point::new(0, y), px(3, row_h)),
                    theme.accent,
                )
                .ok();
            }

            // A hairline divider between rows, but not below the last one.
            if i + 1 < Self::ITEMS.len() {
                widgets::fill_rect(
                    target,
                    Rectangle::new(Point::new(0, y + row_h - 1), px(width, 1)),
                    theme.border,
                )
                .ok();
            }

            // Icon, vertically centred in the row.
            let icon_x = 12;
            let icon_y = y + (row_h - widgets::MENU_ICON) / 2;
            let icon_color = if selected { theme.selection_text } else { theme.accent };
            widgets::draw_menu_icon(target, item.icon, Point::new(icon_x, icon_y), icon_color)
                .ok();

            // Title and description, to the right of the icon.
            let text_x = 48;
            let title_color = if selected { theme.selection_text } else { theme.text };
            let sub_color = if selected { theme.selection_text } else { theme.text_dim };
            widgets::draw_text(
                target,
                Point::new(text_x, y + theme.line_h),
                item.title,
                title_color,
                theme,
            )
            .ok();
            widgets::draw_text(
                target,
                Point::new(text_x, y + theme.line_h * 2),
                item.subtitle,
                sub_color,
                theme,
            )
            .ok();

            y += row_h;
        }

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
