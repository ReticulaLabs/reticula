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
            scroll: 0,
            following: true,
        }
    }

    /// Re-target this screen at a different peer (used when re-opening).
    pub fn open(&mut self, peer: [u8; 16]) {
        self.peer = peer;
        self.input.clear();
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
        let hops = ctx
            .conversations
            .iter()
            .find(|c| c.peer == self.peer)
            .and_then(|c| c.hops);
        let hops_label = widgets::hops_label(hops);
        widgets::draw_header(target, width, &title, &ctx.network, theme).ok();

        let body_top = theme.header_h;
        let composer_y = height - theme.line_h;
        let status_y = composer_y - theme.line_h;
        let body = Rectangle::new(Point::new(0, body_top), px(width, status_y - body_top));

        // Build the flattened message rows, carrying each row's direction and
        // whether it is still awaiting delivery confirmation.
        let max_chars = theme.chars_per_line(width - 10);
        let mut rows: Vec<(bool, bool, String)> = Vec::new();
        for msg in ctx.messages {
            for line in widgets::wrap(&msg.content, max_chars) {
                rows.push((msg.incoming, msg.sending, line));
            }
            if !rows.is_empty() {
                rows.push((false, false, String::new())); // spacer
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
        for (i, (incoming, sending, line)) in rows.iter().enumerate() {
            if i < self.scroll {
                continue;
            }
            if row >= visible {
                break;
            }
            if !line.is_empty() {
                if *incoming {
                    // Received messages sit on the left, with a left accent bar.
                    widgets::fill_rect(
                        target,
                        Rectangle::new(Point::new(0, y), px(2, theme.line_h)),
                        theme.accent,
                    )
                    .ok();
                    widgets::draw_text(target, Point::new(4, y), line, theme.text, theme).ok();
                } else {
                    // Sent messages sit on the right, with a right accent bar.
                    // Until the recipient's delivery proof arrives they are
                    // drawn in the warning colour: the bytes are out, but
                    // nothing has confirmed they landed.
                    let color = if *sending { theme.warning } else { theme.text };
                    let text_w = line.chars().count() as i32 * theme.char_w;
                    let x = (width - 5 - text_w).max(4);
                    widgets::draw_text(target, Point::new(x, y), line, color, theme).ok();
                    widgets::fill_rect(
                        target,
                        Rectangle::new(Point::new(width - 2, y), px(2, theme.line_h)),
                        if *sending { theme.warning } else { theme.accent },
                    )
                    .ok();
                }
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

        // Status line (send errors / hints), with the hop count right-aligned on the
        // same row — the chat screen has no footer bar, so this is its bottom
        // status line. The hint is shortened to whatever the hops leave, so the
        // two can never meet.
        let hops_w = (hops_label.chars().count() as i32 + 1) * theme.char_w;
        // Only report a send failure for the conversation it belongs to; a
        // failure raised while another chat was open must not bleed in here.
        let error = ctx
            .send_failure
            .filter(|f| f.peer == self.peer)
            .map(|f| f.reason.as_str());
        let status = match error {
            Some(e) => {
                let prefix = "Not sent: ";
                let room = theme.chars_per_line(width - hops_w - 2).saturating_sub(prefix.len());
                widgets::truncate(&format!("{prefix}{e}"), room)
            }
            None => widgets::truncate(
                "ENTER send | PgUp/PgDn scroll",
                theme.chars_per_line(width - hops_w - 2),
            ),
        };
        let dim = if error.is_some() {
            theme.danger
        } else {
            theme.text_dim
        };
        widgets::draw_text(target, Point::new(0, status_y), &status, dim, theme).ok();
        if !hops_label.is_empty() {
            widgets::draw_text_right(target, width - 2, status_y, &hops_label, dim, theme).ok();
        }
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ChatMessage, SendFailure};
    use crate::context::ViewContext;
    use crate::theme::Theme;
    use embedded_graphics::draw_target::DrawTarget;
    use embedded_graphics::geometry::{OriginDimensions, Size};
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::Pixel;

    /// Minimal draw target that records every colour written, so a render can
    /// be asserted on without a display.
    struct Recorder {
        width: i32,
        height: i32,
        pixels: Vec<Option<Rgb565>>,
    }

    impl Recorder {
        fn new(width: i32, height: i32) -> Self {
            Self { width, height, pixels: vec![None; (width * height) as usize] }
        }

        fn contains(&self, color: Rgb565) -> bool {
            self.pixels.contains(&Some(color))
        }

        /// Whether `color` was drawn anywhere in the message body, i.e. below
        /// the header and above the composer/status rows.
        fn contains_in_body(&self, color: Rgb565, theme: &Theme) -> bool {
            let status_y = self.height - theme.line_h - theme.line_h;
            (theme.header_h..status_y).any(|y| {
                (0..self.width).any(|x| {
                    self.pixels[(y * self.width + x) as usize] == Some(color)
                })
            })
        }
    }

    impl OriginDimensions for Recorder {
        fn size(&self) -> Size {
            Size::new(self.width as u32, self.height as u32)
        }
    }

    impl DrawTarget for Recorder {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for pixel in pixels {
                let x = pixel.0.x;
                let y = pixel.0.y;
                if x >= 0 && y >= 0 && x < self.width && y < self.height {
                    self.pixels[(y * self.width + x) as usize] = Some(pixel.1);
                }
            }
            Ok(())
        }

        fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
            for y in area.rows() {
                for x in area.columns() {
                    if x >= 0 && y >= 0 && x < self.width && y < self.height {
                        self.pixels[(y * self.width + x) as usize] = Some(color);
                    }
                }
            }
            Ok(())
        }
    }

    fn context<'a>(messages: &'a [ChatMessage], failure: Option<&'a SendFailure>) -> ViewContext<'a> {
        ViewContext {
            messages,
            send_failure: failure,
            ..ViewContext::default()
        }
    }

    fn outbound(sending: bool, content: &str) -> ChatMessage {
        ChatMessage {
            incoming: false,
            sending,
            title: String::new(),
            content: content.to_string(),
            ts: 0.0,
        }
    }

    #[test]
    fn unconfirmed_outgoing_message_renders_in_the_warning_colour() {
        let theme = Theme::default();
        let messages = [outbound(true, "pending message")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(
            target.contains(theme.warning),
            "a message still awaiting its delivery proof must be drawn amber"
        );
        // Scoped to the message body: the composer's "Type a message…"
        // placeholder is drawn in the normal text colour regardless.
        assert!(
            !target.contains_in_body(theme.text, &theme),
            "an unconfirmed message must not be drawn in the normal text colour"
        );
    }

    #[test]
    fn confirmed_outgoing_message_is_not_amber() {
        let theme = Theme::default();
        let messages = [outbound(false, "confirmed message")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(!target.contains(theme.warning));
        assert!(target.contains_in_body(theme.text, &theme));
    }

    #[test]
    fn the_two_states_render_differently() {
        // Guards against the colours being indistinguishable on a real panel.
        let theme = Theme::default();
        assert_ne!(theme.warning, theme.text);
    }

    #[test]
    fn a_failed_send_is_shown_in_the_status_line() {
        let theme = Theme::default();
        let messages: [ChatMessage; 0] = [];
        let failure = SendFailure {
            peer: [1u8; 16],
            reason: "no path".to_string(),
        };
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, Some(&failure)), &theme);

        // The status line is drawn in the danger colour when a send failed.
        assert!(target.contains(theme.danger));
    }
}
