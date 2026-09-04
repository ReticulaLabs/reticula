//! NomadNet page viewer screen.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;
use reticula_nomad::Link;

use crate::command::Command;
use crate::context::ViewContext;
use crate::theme::Theme;
use crate::widgets::{self, px};

pub struct NomadViewScreen {
    /// Node this page was fetched from.
    pub node: [u8; 16],
    /// First visible page line.
    pub scroll: usize,
    /// The link on the currently scrolled line, resolved at render time.
    current_link: Option<Link>,
}

impl NomadViewScreen {
    pub fn new(node: [u8; 16]) -> Self {
        Self { node, scroll: 0, current_link: None }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match key {
            KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                Command::None
            }
            KeyCode::Down => {
                self.scroll += 1;
                Command::None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_add(6);
                Command::None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(6);
                Command::None
            }
            KeyCode::Enter | KeyCode::Right => {
                // Follow the first link on the current line.
                self.current_link
                    .as_ref()
                    .and_then(parse_link_target)
                    .unwrap_or(Command::None)
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
        // Show the node's display name when known, else its address prefix.
        let fallback = hex(&self.node)[..8].to_string();
        let node_name = ctx
            .page_node
            .filter(|n| !n.name.is_empty())
            .map(|n| n.name.as_str())
            .unwrap_or(&fallback);
        let title = format!("{node_name}");
        widgets::draw_bar(target, header, &title, "", theme.header, theme.header_text, theme).ok();

        let body_top = header.size.height as i32;
        let body = Rectangle::new(
            Point::new(0, body_top),
            px(width, height - body_top - theme.line_h),
        );
        // The page area is rendered terminal-style: black background, white
        // text (independent of the app theme).
        let page_bg = Rgb565::BLACK;
        target.fill_solid(&body, page_bg).ok();
        let visible = theme.lines_fit(body.size.height as i32);

        let Some(page) = ctx.page else {
            let notice = if ctx.page_notice.is_empty() {
                "No page loaded.".to_string()
            } else {
                ctx.page_notice.to_string()
            };
            widgets::draw_text(target, Point::new(0, body_top), &notice, Rgb565::WHITE, theme)
                .ok();
            return;
        };

        self.current_link = page.lines().get(self.scroll).and_then(|l| l.links.first().cloned());

        let max_chars = theme.chars_per_line(width - 6);
        let mut y = body.top_left.y;
        let mut drawn = 0usize;
        for (i, line) in page.lines().iter().enumerate() {
            if i < self.scroll {
                continue;
            }
            if drawn >= visible {
                break;
            }
            let color = Rgb565::WHITE;
            let text = widgets::truncate(&line.text, max_chars);
            widgets::draw_text(target, Point::new(0, y), &text, color, theme).ok();
            y += theme.line_h;
            drawn += 1;
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let hint = match &self.current_link {
            Some(link) => format!("ENTER {}", link.label),
            None => "UP/DOWN scroll | ESC back".to_string(),
        };
        widgets::draw_bar(
            target,
            footer,
            &hint,
            "",
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();

        widgets::draw_scrollbar(target, body, self.scroll, page.lines().len(), theme).ok();
    }
}

/// Parse an `rns://<16-byte-hex>/<path>` link target into a fetch command.
fn parse_link_target(link: &Link) -> Option<Command> {
    let target = link.target.strip_prefix("rns://")?;
    let (hex, path) = target.split_once('/')?;
    if hex.len() != 32 {
        return None;
    }
    let mut node = [0u8; 16];
    for i in 0..16 {
        node[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(Command::FetchPage { node, path: path.to_string() })
}

fn hex(addr: &[u8; 16]) -> String {
    let mut s = String::new();
    for b in addr {
        s.push_str(&format!("{b:02x}"));
    }
    s
}