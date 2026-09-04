//! Conversation list screen.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, ScreenId};
use crate::context::{Conversation, ViewContext};
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

pub struct ChatListScreen {
    pub state: ListState,
    /// Peer of the currently selected conversation, resolved at render time.
    selected_peer: Option<[u8; 16]>,
    /// Total rows known at render time ("New message" + conversations).
    item_count: usize,
    /// Live search filter: matches the peer's name or address. Empty = no
    /// filtering. Typing on this screen edits it.
    filter: String,
}

impl Default for ChatListScreen {
    fn default() -> Self {
        Self {
            state: ListState::default(),
            selected_peer: None,
            item_count: 1,
            filter: String::new(),
        }
    }
}

impl ChatListScreen {
    const NEW_LABEL: &'static str = "+ New message";

    pub fn new() -> Self {
        Self::default()
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
                self.state.move_down(self.item_count);
                Command::None
            }
            KeyCode::Enter => {
                if !self.filter.is_empty() {
                    // Searching: ENTER opens the selected match.
                    return match self.selected_peer {
                        Some(peer) => Command::StartChat(peer),
                        None => Command::None,
                    };
                }
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
        let filtered: Vec<&Conversation> = ctx
            .conversations
            .iter()
            .filter(|c| filter_matches(c, &self.filter))
            .collect();

        // The "+ New message" row is hidden while searching.
        let has_new_row = self.filter.is_empty();
        self.item_count = filtered.len() + has_new_row as usize;
        let selected_idx = if has_new_row {
            self.state.selected.saturating_sub(1)
        } else {
            self.state.selected
        };
        self.selected_peer = filtered.get(selected_idx).map(|c| c.peer);
        self.state.clamp(self.item_count);

        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        let count = format!("{} conv", filtered.len());
        widgets::draw_header(target, width, "Chat", &count, &ctx.network, theme).ok();

        let header_h = theme.line_h;
        let search_h = if self.filter.is_empty() { 0 } else { theme.line_h };
        let body_top = header_h + search_h;
        let visible = theme.lines_fit(height - body_top - theme.line_h);
        self.state.keep_visible(visible);

        // Live search line.
        let mut row_y = header_h;
        if !self.filter.is_empty() {
            let label = format!("> {}", self.filter);
            widgets::draw_text(target, Point::new(0, header_h), &label, theme.accent, theme).ok();
            row_y = body_top;
        }

        if self.filter.is_empty() {
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
        }

        if filtered.is_empty() && !self.filter.is_empty() {
            widgets::draw_text(
                target,
                Point::new(0, row_y),
                "No matches.",
                theme.text_dim,
                theme,
            )
            .ok();
        }

        let preview_chars = theme.chars_per_line(width - 30);
        for (i, conv) in filtered.iter().enumerate().skip(self.state.scroll) {
            let row_index = i + has_new_row as usize;
            let row_in_view = row_index - self.state.scroll;
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
            if row_index == self.state.selected {
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
        let hint = if self.filter.is_empty() {
            "TYPE to search | ENTER open | ALT+Backspace"
        } else {
            "filtering | ENTER open | ALT+Backspace"
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
            self.item_count,
            theme,
        )
        .ok();
    }
}

/// Whether a conversation matches the search filter (by display name or
/// address, case-insensitively).
fn filter_matches(conv: &Conversation, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let q = filter.to_lowercase();
    conv.peer_name.to_lowercase().contains(&q) || conv.peer_hex.contains(&q)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn conv(name: &str, hex: &str) -> Conversation {
        Conversation {
            peer: [0u8; 16],
            peer_hex: hex.to_string(),
            peer_name: name.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_filter_matches_everything() {
        assert!(filter_matches(&conv("", "aabbccdd"), ""));
    }

    #[test]
    fn matches_name_case_insensitively() {
        assert!(filter_matches(&conv("Pepper", "aabbccdd"), "pep"));
        assert!(filter_matches(&conv("Pepper", "aabbccdd"), "PEPPER"));
        assert!(!filter_matches(&conv("Pepper", "aabbccdd"), "zeno"));
    }

    #[test]
    fn matches_address_prefix() {
        assert!(filter_matches(&conv("", "aabbccddeeff00112233445566778899"), "aabb"));
    }
}
