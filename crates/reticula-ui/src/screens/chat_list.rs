//! Conversation list screen.

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

pub struct ChatListScreen {
    pub state: ListState,
    /// Peer of the currently selected conversation, resolved at render time.
    selected_peer: Option<[u8; 16]>,
    /// Total rows known at render time ("New message" + conversations).
    item_count: usize,
}

impl Default for ChatListScreen {
    fn default() -> Self {
        Self {
            state: ListState::default(),
            selected_peer: None,
            item_count: 1,
        }
    }
}

impl ChatListScreen {
    const NEW_LABEL: &'static str = "+ New message";

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
                self.state.move_down(self.item_count);
                Command::None
            }
            KeyCode::Enter => {
                if self.state.selected == 0 {
                    Command::ShowScreen(ScreenId::NewChat)
                } else {
                    match self.selected_peer {
                        Some(peer) => Command::StartChat(peer),
                        None => Command::None,
                    }
                }
            }
            KeyCode::Esc => Command::Back,
            _ => Command::None,
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        self.item_count = ctx.conversations.len() + 1;
        self.selected_peer = ctx
            .conversations
            .get(self.state.selected.saturating_sub(1))
            .map(|c| c.peer);

        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        let header = Rectangle::new(Point::new(0, 0), px(width, theme.line_h));
        let count = format!("{} conv", ctx.conversations.len());
        widgets::draw_bar(target, header, "Chat", &count, theme.header, theme.header_text, theme)
            .ok();

        let body_top = header.size.height as i32;
        let visible = theme.lines_fit(height - body_top - theme.line_h);
        self.state.keep_visible(visible);

        let mut row_y = body_top;

        // "New message" row.
        if self.state.scroll == 0 {
            let at = Point::new(0, row_y);
            if self.state.selected == 0 {
                widgets::draw_highlight(
                    target,
                    at,
                    Self::NEW_LABEL,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, at, Self::NEW_LABEL, theme.ok, theme).ok();
            }
            row_y += theme.line_h;
        }

        let preview_chars = theme.chars_per_line(width - 30);
        for (i, conv) in ctx.conversations.iter().enumerate().skip(self.state.scroll) {
            let row_in_view = i + 1 - self.state.scroll;
            if row_in_view >= visible {
                break;
            }
            let marker = if conv.unread > 0 { " *" } else { "" };
            let preview = widgets::truncate(
                if conv.last_content.is_empty() {
                    &conv.last_title
                } else {
                    &conv.last_content
                },
                preview_chars,
            );
            // Prefer the peer's display name; fall back to the address prefix.
            let who = if conv.peer_name.is_empty() {
                &conv.peer_hex[..8]
            } else {
                &conv.peer_name
            };
            let line = format!("{}{} {}", who, marker, preview);

            let at = Point::new(0, row_y);
            if self.state.selected == i + 1 {
                widgets::draw_highlight(
                    target,
                    at,
                    &line,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, at, &line, theme.text, theme).ok();
            }
            row_y += theme.line_h;
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        widgets::draw_bar(
            target,
            footer,
            "ENTER open / ESC back",
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
            self.item_count,
            theme,
        )
        .ok();
    }
}