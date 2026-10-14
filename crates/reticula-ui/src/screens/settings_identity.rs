//! Settings sub-menu: Reticulum identity — LXMF address, display name, and
//! re-announcing our identities.

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

/// Maximum on-screen size (px) of the identity QR code, including its quiet
/// zone. The symbol is scaled to whole pixels and centred within this box;
/// the actual size depends on the QR version (a 168-char `lxma://` identity is
/// 53×53 modules at Medium ECC, so ~122 px at 2 px/module).
const QR_MAX: i32 = 130;

#[derive(Default)]
pub struct SettingsIdentityScreen {
    pub state: ListState,
    /// True while editing the display name.
    pub editing: bool,
    pub name_input: String,
}

impl SettingsIdentityScreen {
    pub fn new() -> Self {
        Self {
            state: ListState::default(),
            editing: false,
            name_input: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if self.editing {
            return match key {
                KeyCode::Char(c) if c.is_ascii() => {
                    self.name_input.push(c);
                    Command::None
                }
                KeyCode::Space => {
                    self.name_input.push(' ');
                    Command::None
                }
                KeyCode::Backspace => {
                    self.name_input.pop();
                    Command::None
                }
                KeyCode::Enter => {
                    let name = self.name_input.trim().to_string();
                    self.editing = false;
                    if name.is_empty() {
                        Command::None
                    } else {
                        Command::SetDisplayName(name)
                    }
                }
                KeyCode::Esc => {
                    self.editing = false;
                    Command::None
                }
                _ => Command::None,
            };
        }

        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(2);
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.editing = true;
                    self.name_input.clear();
                    Command::None
                }
                _ => Command::Announce,
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

        widgets::draw_header(target, width, "Identity", "", &ctx.network, theme).ok();

        let mut y = theme.header_h;

        // Row 0: display name (editable).
        if self.editing {
            widgets::draw_text(target, Point::new(0, y), "Display name:", theme.text_dim, theme)
                .ok();
            y += theme.line_h;
            widgets::draw_text(target, Point::new(0, y), "> ", theme.accent, theme).ok();
            let prompt_w = 2 * theme.char_w;
            let text = widgets::truncate(&self.name_input, theme.chars_per_line(width - 12) - 2);
            widgets::fill_rect(
                target,
                Rectangle::new(Point::new(prompt_w, y), px(width - prompt_w, theme.line_h)),
                theme.surface,
            )
            .ok();
            widgets::draw_text(target, Point::new(prompt_w, y), &text, theme.text, theme).ok();
            y += theme.line_h;
        } else {
            let label = format!("Name: {}", ctx.display_name);
            let at = Point::new(0, y);
            if self.state.selected == 0 {
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

        // Row 1: re-announce identities.
        let at = Point::new(0, y);
        let label = "Re-announce identities";
        if self.state.selected == 1 {
            widgets::draw_highlight(
                target,
                at,
                label,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, label, theme.text, theme).ok();
        }
        y += theme.line_h;

        if !ctx.notice.is_empty() {
            y += theme.line_h;
            widgets::draw_text(target, Point::new(0, y), ctx.notice, theme.ok, theme).ok();
            y += theme.line_h;
        }

        // QR code of the identity, centred in the remaining space. Encoded in
        // the `lxma://<destination_hash_hex>:<public_key_hex>` format used by
        // the Columba client, so a scan yields a value peers can import to
        // add us as a contact and encrypt to us.
        y += theme.line_h;
        widgets::draw_text(
            target,
            Point::new(0, y),
            "Scan to add contact:",
            theme.text_dim,
            theme,
        )
        .ok();
        y += theme.line_h;
        let qr_text = format!("lxma://{}:{}", ctx.identity_hex, ctx.public_key_hex);
        let qr_size = widgets::draw_qr(
            target,
            Point::new((width - QR_MAX) / 2, y),
            &qr_text,
            QR_MAX,
            theme.background,
            theme.text,
        )
        .unwrap_or(0);
        y += qr_size + theme.line_h;

        // LXMF address.
        widgets::draw_text(target, Point::new(0, y), "LXMF address:", theme.text_dim, theme).ok();
        y += theme.line_h;
        widgets::draw_wrapped(
            target,
            Point::new(0, y),
            ctx.identity_hex,
            width - 6,
            4,
            theme.text,
            theme,
        )
        .ok();

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(
            target,
            footer,
            "ALT+Backspace back",
            &short,
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();
    }
}
