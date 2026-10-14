//! Home / root menu screen.
//!
//! The root menu is rendered as a list of full-width rows, each roughly a
//! sixth of the screen tall, with a section icon on the left and the section
//! name (plus a short description) beside it.

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
pub struct HomeScreen {
    pub state: ListState,
}

impl HomeScreen {
    const ITEMS: [MenuItem; 4] = [
        MenuItem {
            title: "LXMF Chat",
            subtitle: "Send and receive messages",
            icon: widgets::MenuIcon::Chat,
        },
        MenuItem {
            title: "NomadNet",
            subtitle: "Browse mesh pages",
            icon: widgets::MenuIcon::Nomad,
        },
        MenuItem {
            title: "Identity",
            subtitle: "Keys and address",
            icon: widgets::MenuIcon::Identity,
        },
        MenuItem {
            title: "Settings",
            subtitle: "WiFi, radio and data",
            icon: widgets::MenuIcon::Settings,
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
                0 => Command::ShowScreen(ScreenId::ChatList),
                1 => Command::ShowScreen(ScreenId::NomadList),
                2 => Command::ShowScreen(ScreenId::SettingsIdentity),
                _ => Command::ShowScreen(ScreenId::Settings),
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

        widgets::draw_header(target, width, "RETICULA", &ctx.network, theme).ok();

        // Device/identity status band directly under the header.
        let name = if ctx.display_name.is_empty() {
            "unnamed".to_string()
        } else {
            ctx.display_name.to_string()
        };
        let status_line = format!(
            "{}   {} link{}   up {}s",
            name,
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

        // Full-width rows, top-aligned just under the status band and starting
        // on the text grid so labels stay crisp on the low-resolution LCD (and
        // readable in the terminal simulator). The row height comes from the
        // shared list renderer, so the settings menu matches this one exactly.
        widgets::draw_menu_list(
            target,
            width,
            height,
            body_top,
            &Self::ITEMS,
            self.state.selected,
            theme,
        );

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(target, footer, "ALT+Backspace back", &short, theme.surface, theme.text_dim, theme)
            .ok();
    }
}
