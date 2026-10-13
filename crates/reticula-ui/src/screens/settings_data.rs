//! Settings sub-menu: Data Management — destructive operations such as
//! regenerating the Reticulum identity.

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
pub struct SettingsDataScreen {
    pub state: ListState,
}

impl SettingsDataScreen {
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
                self.state.move_down(1);
                Command::None
            }
            KeyCode::Enter => Command::RegenerateIdentity,
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

        widgets::draw_header(target, width, "Data Management", "", &ctx.network, theme).ok();

        let mut y = theme.line_h;

        // Warning about the consequences of regenerating the identity.
        widgets::draw_text(
            target,
            Point::new(0, y),
            "WARNING: a new LXMF address",
            theme.danger,
            theme,
        )
        .ok();
        y += theme.line_h;
        widgets::draw_text(
            target,
            Point::new(0, y),
            "means peers must rediscover you.",
            theme.danger,
            theme,
        )
        .ok();
        y += theme.line_h + 4;

        // Row 0: regenerate identity.
        let at = Point::new(0, y);
        let label = "Regenerate identity";
        if self.state.selected == 0 {
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
        }

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
