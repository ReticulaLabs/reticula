//! NomadNet node list screen.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::{NodeEntry, ViewContext};
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
    /// Live search filter: matches the node's name or address. Empty = no
    /// filtering. Typing on this screen edits it.
    filter: String,
}

impl NomadListScreen {
    pub fn new() -> Self {
        Self { state: ListState::default(), selected: None, count: 0, filter: String::new() }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        // Typing edits the live filter.
        match key {
            KeyCode::Char(c) if c.is_ascii_graphic() => {
                self.filter.push(c);
                self.state.selected = 0;
                self.state.scroll = 0;
                return Command::None;
            }
            KeyCode::Space => {
                self.filter.push(' ');
                self.state.selected = 0;
                self.state.scroll = 0;
                return Command::None;
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.state.selected = 0;
                self.state.scroll = 0;
                return Command::None;
            }
            _ => {}
        }

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
        let filtered: Vec<&NodeEntry> = ctx
            .nodes
            .iter()
            .filter(|n| filter_matches(n, &self.filter))
            .collect();
        self.count = filtered.len();
        self.selected = filtered.get(self.state.selected).map(|n| n.address);
        self.state.clamp(self.count);

        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        let count_label = format!("{} nodes", filtered.len());
        widgets::draw_header(target, width, "NomadNet", &count_label, &ctx.network, theme).ok();

        let header_h = theme.line_h;
        let search_h = if self.filter.is_empty() { 0 } else { theme.line_h };
        let body_top = header_h + search_h;
        let visible = theme.lines_fit(height - body_top - theme.line_h);
        self.state.keep_visible(visible);

        // Live search line.
        let mut y = header_h;
        if !self.filter.is_empty() {
            let label = format!("> {}", self.filter);
            widgets::draw_text(target, Point::new(0, header_h), &label, theme.accent, theme).ok();
            y = body_top;
        }

        if filtered.is_empty() {
            if self.filter.is_empty() {
                widgets::draw_text(
                    target,
                    Point::new(0, y),
                    "No nodes discovered yet.",
                    theme.text_dim,
                    theme,
                )
                .ok();
                widgets::draw_text(
                    target,
                    Point::new(0, y + theme.line_h),
                    "Make sure the mesh is within reach.",
                    theme.text_dim,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(
                    target,
                    Point::new(0, y),
                    "No matches.",
                    theme.text_dim,
                    theme,
                )
                .ok();
            }
        } else {
            for (i, node) in filtered.iter().enumerate().skip(self.state.scroll) {
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
        let hint = if self.filter.is_empty() {
            "TYPE to search | ENTER browse | ESC back"
        } else {
            "filtering | ENTER browse | ESC back"
        };
        widgets::draw_bar(
            target,
            footer,
            hint,
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

/// Whether a node matches the search filter (by display name or address,
/// case-insensitively).
fn filter_matches(node: &NodeEntry, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let q = filter.to_lowercase();
    node.name.to_lowercase().contains(&q) || node.hex.contains(&q)
}