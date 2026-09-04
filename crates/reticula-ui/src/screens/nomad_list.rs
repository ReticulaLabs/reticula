//! NomadNet node list screen.

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
pub struct NomadListScreen {
    pub state: ListState,
    /// Address of the currently selected node, resolved at render time.
    selected: Option<[u8; 16]>,
    /// Number of nodes known at render time.
    count: usize,
}

impl NomadListScreen {
    pub fn new() -> Self {
        Self { state: ListState::default(), selected: None, count: 0 }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(self.count);
                Command::None
            }
            KeyCode::Enter => match self.selected {
                Some(node) => Command::OpenNode(node),
                None => Command::None,
            },
            KeyCode::Esc => Command::Back,
            _ => Command::None,
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        self.count = ctx.nodes.len();
        self.selected = ctx.nodes.get(self.state.selected).map(|n| n.address);
        self.state.clamp(self.count); // keep selection in range without moving it

        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        let count_label = format!("{} nodes", self.count);
        widgets::draw_header(target, width, "NomadNet", &count_label, &ctx.network, theme).ok();

        let body_top = theme.line_h;
        let visible = theme.lines_fit(height - body_top - theme.line_h);
        self.state.keep_visible(visible);

        if self.count == 0 {
            widgets::draw_text(
                target,
                Point::new(0, body_top),
                "No nodes discovered yet.",
                theme.text_dim,
                theme,
            )
            .ok();
            widgets::draw_text(
                target,
                Point::new(0, body_top + theme.line_h),
                "Make sure the mesh is within reach.",
                theme.text_dim,
                theme,
            )
            .ok();
        } else {
            let mut y = body_top;
            for (i, node) in ctx.nodes.iter().enumerate().skip(self.state.scroll) {
                let row = i - self.state.scroll;
                if row >= visible {
                    break;
                }
                let label = if node.name.is_empty() {
                    widgets::truncate(&node.hex, theme.chars_per_line(width - 6))
                } else {
                    widgets::truncate(&node.name, theme.chars_per_line(width - 6))
                };
                let at = Point::new(0, y);
                if i == self.state.selected {
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
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        widgets::draw_bar(
            target,
            footer,
            "ENTER browse / ESC back",
            "",
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();

        widgets::draw_scrollbar(
            target,
            Rectangle::new(
                Point::new(0, body_top),
                px(width, height - body_top),
            ),
            self.state.scroll,
            self.count,
            theme,
        )
        .ok();
    }
}