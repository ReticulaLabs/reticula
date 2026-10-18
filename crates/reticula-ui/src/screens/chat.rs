//! Open conversation screen: message history + composer.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::{Delivery, ViewContext};
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
        // delivery state so the colour matches what is actually known.
        let max_chars = theme.chars_per_line(width - 10);
        let mut rows: Vec<(bool, Delivery, String)> = Vec::new();
        for msg in ctx.messages {
            for line in widgets::wrap(&msg.content, max_chars) {
                rows.push((msg.incoming, msg.delivery, line));
            }
            if !rows.is_empty() {
                rows.push((false, Delivery::Acknowledged, String::new())); // spacer
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
        for (i, (incoming, delivery, line)) in rows.iter().enumerate() {
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
                    // The colour states only what is actually known:
                    // amber while still in flight, dim once written to the link
                    // but unproven, normal on a proof, red once failed. None of
                    // them means the peer read it.
                    let color = match delivery {
                        Delivery::Pending => theme.warning,
                        Delivery::Sent => theme.text_dim,
                        Delivery::Acknowledged => theme.text,
                        Delivery::Failed => theme.danger,
                    };
                    let text_w = line.chars().count() as i32 * theme.char_w;
                    let x = (width - 5 - text_w).max(4);
                    widgets::draw_text(target, Point::new(x, y), line, color, theme).ok();
                    widgets::fill_rect(
                        target,
                        Rectangle::new(Point::new(width - 2, y), px(2, theme.line_h)),
                        color,
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
        // A peer that demands LXMF stamps cannot be sent to at all. Say so here
        // rather than leaving the composer looking usable: without this, the
        // only sign is a refusal after the fact, on a message the user believed
        // was going out.
        let stamp_cost = ctx
            .conversations
            .iter()
            .find(|c| c.peer == self.peer)
            .and_then(|c| c.stamp_cost);
        let room = theme.chars_per_line(width - hops_w - 2);
        let (status, dim) = match (error, stamp_cost) {
            (Some(e), _) => {
                let prefix = "Not sent: ";
                (
                    widgets::truncate(&format!("{prefix}{e}"), room.saturating_sub(prefix.len())),
                    theme.danger,
                )
            }
            (None, Some(cost)) => (
                widgets::truncate(&format!("Needs LXMF stamps (cost {cost})"), room),
                theme.warning,
            ),
            (None, None) => (
                widgets::truncate("ENTER send | PgUp/PgDn scroll", room),
                theme.text_dim,
            ),
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
    use crate::context::{ChatMessage, Conversation, Delivery, SendFailure};
    use crate::testing::Recorder;
    use crate::context::ViewContext;
    use crate::theme::Theme;

    fn context<'a>(messages: &'a [ChatMessage], failure: Option<&'a SendFailure>) -> ViewContext<'a> {
        ViewContext {
            messages,
            send_failure: failure,
            ..ViewContext::default()
        }
    }

    fn outbound(delivery: Delivery, content: &str) -> ChatMessage {
        ChatMessage {
            incoming: false,
            delivery,
            title: String::new(),
            content: content.to_string(),
            ts: 0.0,
        }
    }

    fn with_contact<'a>(
        messages: &'a [ChatMessage],
        failure: Option<&'a SendFailure>,
        contacts: &'a [Conversation],
    ) -> ViewContext<'a> {
        ViewContext {
            messages,
            send_failure: failure,
            conversations: contacts,
            ..ViewContext::default()
        }
    }

    #[test]
    fn a_pending_outgoing_message_renders_in_the_warning_colour() {
        let theme = Theme::default();
        let messages = [outbound(Delivery::Pending, "pending message")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(
            target.contains_in_body(theme.warning, &theme),
            "a message still in flight must be drawn amber"
        );
        // Scoped to the message body: the composer's "Type a message…"
        // placeholder is drawn in the normal text colour regardless.
        assert!(
            !target.contains_in_body(theme.text, &theme),
            "a pending message must not be drawn in the normal text colour"
        );
    }

    #[test]
    fn an_acknowledged_outgoing_message_is_not_amber() {
        let theme = Theme::default();
        let messages = [outbound(Delivery::Acknowledged, "confirmed message")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(!target.contains_in_body(theme.warning, &theme));
        assert!(target.contains_in_body(theme.text, &theme));
    }

    #[test]
    fn a_sent_but_unproven_message_is_dim_not_plain() {
        // Written to a link, but the peer issued no proof. It must not be
        // mistaken for a proven delivery — which is exactly what happened
        // when every state collapsed into one "unacknowledged" flag and a
        // silently-discarded message looked identical to a delivered one.
        let theme = Theme::default();
        let messages = [outbound(Delivery::Sent, "unproven message")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(
            !target.contains_in_body(theme.warning, &theme),
            "an unproven send is not in flight"
        );
        assert!(
            !target.contains_in_body(theme.danger, &theme),
            "an unproven send is not a failure"
        );
        assert!(
            target.contains_in_body(theme.text_dim, &theme),
            "an unproven send must be visibly distinct from a proven one"
        );
    }

    #[test]
    fn a_failed_message_is_drawn_in_the_danger_colour() {
        // Revives the "failed as red" intent: `Delivery::Failed` used to be
        // stored but never rendered differently, so a message the client had
        // given up on looked like any other.
        let theme = Theme::default();
        let messages = [outbound(Delivery::Failed, "never sent")];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(&mut target, &context(&messages, None), &theme);

        assert!(target.contains_in_body(theme.danger, &theme));
    }

    #[test]
    fn the_delivery_states_render_differently() {
        // Guards against the colours being indistinguishable on a real panel.
        let theme = Theme::default();
        assert_ne!(theme.warning, theme.text);
        assert_ne!(theme.text_dim, theme.text);
        assert_ne!(theme.danger, theme.text);
        assert_ne!(theme.danger, theme.warning);
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
        assert!(target.contains_in_rows(target.status_y(&theme)..target.status_y(&theme) + theme.line_h, theme.danger));
    }

    #[test]
    fn a_stamp_requiring_peer_is_flagged_before_composing() {
        // The whole point of detecting the stamp cost: the composer must not
        // look usable for a peer this client cannot send to.
        let theme = Theme::default();
        let messages: [ChatMessage; 0] = [];
        let contacts = [Conversation {
            peer: [1u8; 16],
            stamp_cost: Some(8),
            ..Conversation::default()
        }];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(
            &mut target,
            &with_contact(&messages, None, &contacts),
            &theme,
        );

        assert!(
            target.contains_in_rows(target.status_y(&theme)..target.status_y(&theme) + theme.line_h, theme.warning),
            "a peer requiring stamps must be flagged in the warning colour"
        );
    }

    #[test]
    fn a_stamp_requiring_flag_is_scoped_to_its_own_peer() {
        let theme = Theme::default();
        let messages: [ChatMessage; 0] = [];
        let contacts = [Conversation {
            peer: [9u8; 16], // a different peer than the open chat
            stamp_cost: Some(8),
            ..Conversation::default()
        }];
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(
            &mut target,
            &with_contact(&messages, None, &contacts),
            &theme,
        );

        assert!(
            !target.contains_in_rows(target.status_y(&theme)..target.status_y(&theme) + theme.line_h, theme.warning),
            "another peer's stamp requirement must not bleed into this chat"
        );
    }

    #[test]
    fn a_send_failure_outranks_the_stamp_notice() {
        // After a refused send the reason is the useful thing to read; the
        // standing stamp notice must not replace it.
        let theme = Theme::default();
        let messages: [ChatMessage; 0] = [];
        let contacts = [Conversation {
            peer: [1u8; 16],
            stamp_cost: Some(8),
            ..Conversation::default()
        }];
        let failure = SendFailure {
            peer: [1u8; 16],
            reason: "peer requires LXMF stamps (cost 8)".to_string(),
        };
        let mut target = Recorder::new(320, 240);
        ChatScreen::new([1u8; 16]).render(
            &mut target,
            &with_contact(&messages, Some(&failure), &contacts),
            &theme,
        );

        assert!(
            target.contains_in_rows(target.status_y(&theme)..target.status_y(&theme) + theme.line_h, theme.danger),
            "the refusal reason must be shown"
        );
    }
}
