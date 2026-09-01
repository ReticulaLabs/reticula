//! "Start a new chat" screen: enter a peer LXMF address.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::ViewContext;
use crate::theme::Theme;
use crate::widgets::{self, px};

#[derive(Default)]
pub struct NewChatScreen {
    pub input: String,
    pub error: Option<String>,
}

impl NewChatScreen {
    pub fn new() -> Self {
        Self { input: String::new(), error: None }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Char(c) if c.is_ascii_hexdigit() => {
                self.input.push(c.to_ascii_lowercase());
                self.error = None;
                Command::None
            }
            KeyCode::Char('x' | 'X') if self.input.is_empty() || self.input.starts_with('0') => {
                self.input.push('x');
                Command::None
            }
            KeyCode::Backspace => {
                self.input.pop();
                Command::None
            }
            KeyCode::Enter => match parse_address(&self.input) {
                Ok(peer) => Command::StartChat(peer),
                Err(e) => {
                    self.error = Some(e);
                    Command::None
                }
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

        let header = Rectangle::new(Point::new(0, 0), px(width, theme.line_h));
        widgets::draw_bar(target, header, "New message", "", theme.header, theme.header_text, theme)
            .ok();

        let mut y = header.size.height as i32;

        widgets::draw_text(
            target,
            Point::new(0, y),
            "Peer address (32 hex chars):",
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h;

        let input_bar = Rectangle::new(Point::new(0, y), px(width, theme.line_h));
        widgets::fill_rect(target, input_bar, theme.surface).ok();
        let text = if self.input.is_empty() {
            "e.g. 00112233445566778899aabbccddeeff"
        } else {
            &self.input
        };
        let text = widgets::truncate(text, theme.chars_per_line(width - 12));
        widgets::draw_text(target, Point::new(0, y), &text, theme.text, theme).ok();

        let blink_on = (ctx.network.uptime_ms / 500) % 2 == 0;
        if blink_on && !self.input.is_empty() {
            let cx = (self.input.chars().count() as i32) * theme.char_w;
            widgets::fill_rect(
                target,
                Rectangle::new(Point::new(cx, y), px(theme.char_w, theme.line_h)),
                theme.accent,
            )
            .ok();
        }

        y += theme.line_h + 10;
        if let Some(err) = &self.error {
            widgets::draw_text(target, Point::new(0, y), err, theme.danger, theme).ok();
            y += theme.line_h;
        }
        widgets::draw_text(
            target,
            Point::new(0, y),
            "ENTER to start chat | ESC back",
            theme.text_dim,
            theme,
        )
        .ok();
    }
}

fn parse_address(input: &str) -> Result<[u8; 16], String> {
    let hex = input.trim().strip_prefix("0x").unwrap_or(input.trim());
    if hex.len() != 32 {
        return Err(format!("expected 32 hex chars, got {}", hex.len()));
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        let byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "invalid hex digit".to_string())?;
        out[i] = byte;
    }
    Ok(out)
}