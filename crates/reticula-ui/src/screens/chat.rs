//! Open conversation screen: message history + composer.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::ViewContext;
use crate::theme::Theme;
use crate::widgets::{self, px};

pub struct ChatScreen {
    /// Peer this conversation is with.
    pub peer: [u8; 16],
    /// Composer input buffer.
    pub input: String,
    /// Last send error, shown in the status line.
    pub error: Option<String>,
    /// First visible message row (0 = bottom, larger = older).
    scroll: usize,
    /// Follow new messages (auto-scroll to bottom).
    following: bool,
}

impl ChatScreen {
    pub fn new(peer: [u8; 16]) -> Self {
        Self {
            peer,
            input: String::new(),
            error: None,
            scroll: 0,
            following: true,
        }
    }

    /// Re-target this screen at a different peer (used when re-opening).
    pub fn open(&mut self, peer: [u8; 16]) {
        self.peer = peer;
        self.input.clear();
        self.error = None;
        self.scroll = 0;
        self.following = true;
    }

    /// Jump to the newest messages.
    pub fn follow(&mut self) {
        self.following = true;
        self.scroll = 0;
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Char(c) if c.is_ascii() => {
                self.input.push(c);
                Command::None
            }
            KeyCode::Space => {
                self.input.push(' ');
                Command::None
            }
            KeyCode::Backspace => {
                self.input.pop();
                Command::None
            }
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return Command::None;
                }
                self.input.clear();
                self.following = true;
                Command::SendMessage { peer: self.peer, text }
            }
            KeyCode::PageUp => {
                self.following = false;
                self.scroll += 6;
                Command::None
            }
            KeyCode::PageDown => {
                self.following = false;
                self.scroll = self.scroll.saturating_sub(6);
                Command::None
            }
            KeyCode::Down => {
                self.scroll = self.scroll.saturating_sub(1);
                Command::None
            }
            KeyCode::Up => {
                self.following = false;
                self.scroll += 1;
                Command::None
            }
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
        // Show the peer's display name when known, else the address prefix.
        let fallback = hex_prefix(&self.peer);
        let peer_name = ctx
            .conversations
            .iter()
            .find(|c| c.peer == self.peer)
            .and_then(|c| {
                if c.peer_name.is_empty() {
                    None
                } else {
                    Some(c.peer_name.as_str())
                }
            })
            .unwrap_or(&fallback);
        let title = format!("{peer_name}");
        widgets::draw_bar(target, header, &title, "", theme.header, theme.header_text, theme).ok();

        let body_top = header.size.height as i32;
        let composer_y = height - theme.line_h;
        let status_y = composer_y - theme.line_h;
        let body = Rectangle::new(Point::new(0, body_top), px(width, status_y - body_top));

        // Build the flattened message rows.
        let max_chars = theme.chars_per_line(width - 8);
        let mut rows: Vec<(bool, String)> = Vec::new();
        for msg in ctx.messages {
            for line in widgets::wrap(&msg.content, max_chars) {
                let prefix = if msg.incoming { "<" } else { ">" };
                rows.push((msg.incoming, format!("{prefix} {line}")));
            }
            if !rows.is_empty() {
                rows.push((false, String::new())); // spacer
            }
        }
        if !rows.is_empty() {
            rows.pop(); // drop trailing spacer
        }

        let visible = theme.lines_fit(body.size.height as i32);
        let total = rows.len();
        if self.following {
            self.scroll = total.saturating_sub(visible);
        }
        self.scroll = self.scroll.min(total.saturating_sub(visible));

        let mut y = body.top_left.y;
        let mut row = 0usize;
        for (i, (incoming, line)) in rows.iter().enumerate() {
            if i < self.scroll {
                continue;
            }
            if row >= visible {
                break;
            }
            if !line.is_empty() {
                // Incoming messages stand out; outgoing are dimmed.
                let color = if *incoming { theme.text } else { theme.text_dim };
                widgets::draw_text(target, Point::new(0, y), line, color, theme).ok();
            }
            y += theme.line_h;
            row += 1;
        }

        // Composer with a prompt marker.
        let input_bar = Rectangle::new(Point::new(0, composer_y), px(width, theme.line_h));
        widgets::fill_rect(target, input_bar, theme.surface).ok();
        widgets::draw_text(target, Point::new(0, composer_y), "> ", theme.accent, theme).ok();
        let prompt_w = 2 * theme.char_w;
        let text = if self.input.is_empty() {
            "Type a message..."
        } else {
            &self.input
        };
        let text = widgets::truncate(text, theme.chars_per_line(width - 8) - 2);
        widgets::draw_text(
            target,
            Point::new(prompt_w, composer_y),
            &text,
            theme.text,
            theme,
        )
        .ok();

        // Cursor (blinking).
        let blink_on = (ctx.network.uptime_ms / 500) % 2 == 0;
        if blink_on {
            let cursor_chars = self.input.chars().count() as i32;
            let cx = prompt_w + cursor_chars * theme.char_w;
            widgets::fill_rect(
                target,
                Rectangle::new(Point::new(cx, composer_y), px(theme.char_w, theme.line_h)),
                theme.accent,
            )
            .ok();
        }

        // Status line (send errors / hints).
        let status = match &self.error {
            Some(e) => widgets::truncate(e, theme.chars_per_line(width - 4)),
            None => "ENTER send | PgUp/PgDn scroll".to_string(),
        };
        widgets::draw_text(
            target,
            Point::new(0, status_y),
            &status,
            if self.error.is_some() { theme.danger } else { theme.text_dim },
            theme,
        )
        .ok();
    }
}

fn hex_prefix(peer: &[u8; 16]) -> String {
    let mut s = String::new();
    for b in &peer[..4] {
        s.push_str(&format!("{b:02x}"));
    }
    s.push('~');
    s
}