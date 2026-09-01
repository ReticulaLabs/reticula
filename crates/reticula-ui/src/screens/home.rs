//! Home / main menu screen.

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
pub struct HomeScreen {
    pub state: ListState,
}

impl HomeScreen {
    const ITEMS: [&'static str; 3] = ["LXMF Chat", "NomadNet", "Settings"];

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

        let header = Rectangle::new(Point::new(0, 0), px(width, theme.line_h));
        let status = if ctx.network.connected {
            "connected"
        } else {
            "connecting..."
        };
        widgets::draw_bar(target, header, "RETICULA", status, theme.header, theme.header_text, theme)
            .ok();

        let body_top = header.size.height as i32;
        let mut y = body_top;
        let visible = theme.lines_fit(height - y - theme.line_h);
        self.state.keep_visible(visible);

        for (i, item) in Self::ITEMS.iter().enumerate() {
            if i < self.state.scroll {
                continue;
            }
            if i >= self.state.scroll + visible {
                break;
            }
            let row = Point::new(0, y);
            if i == self.state.selected {
                widgets::draw_highlight(
                    target,
                    row,
                    item,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, row, item, theme.text, theme).ok();
            }
            y += theme.line_h;
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(target, footer, "ESC back", &short, theme.surface, theme.text_dim, theme)
            .ok();
    }
}