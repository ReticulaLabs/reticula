//! Low-level drawing helpers used by the screens.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};

use crate::theme::Theme;

/// Convenience constructor for a pixel size from `i32` values.
pub fn px(w: i32, h: i32) -> Size {
    Size::new(w.max(0) as u32, h.max(0) as u32)
}

/// Fill a rectangle with a solid colour.
pub fn fill_rect<D>(target: &mut D, rect: Rectangle, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    rect.into_styled(PrimitiveStyle::with_fill(color)).draw(target)
}

/// Draw a single line of text at `at` (top-left), using `Baseline::Top`.
pub fn draw_text<D>(
    target: &mut D,
    at: Point,
    text: &str,
    color: Rgb565,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Text::with_baseline(
        text,
        at,
        MonoTextStyle::new(theme.font, color),
        Baseline::Top,
    )
    .draw(target)
    .map(|_| ())
}

/// Draw an inverse (highlighted) line: a filled background with a leading
/// cursor marker and text on top.
pub fn draw_highlight<D>(
    target: &mut D,
    at: Point,
    text: &str,
    width: i32,
    bg: Rgb565,
    fg: Rgb565,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    fill_rect(
        target,
        Rectangle::new(at, px(width, theme.line_h)),
        bg,
    )?;
    // A leading cursor marker makes the selected row obvious.
    draw_text(target, at, ">", fg, theme)?;
    let max_chars = theme.chars_per_line(width - theme.char_w);
    let text = truncate(text, max_chars);
    draw_text(target, at + Point::new(theme.char_w, 0), &text, fg, theme)
}

/// Wrap `text` into lines of at most `max_chars` characters.
///
/// Existing line breaks are preserved; words are not split when possible.
pub fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end();
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        if max_chars == 0 {
            out.push(line.to_string());
            continue;
        }
        let words: Vec<&str> = line.split(' ').collect();
        let mut current = String::new();
        for word in words {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

/// Draw wrapped text starting at `at`, within `width_px`. At most `max_lines`
/// lines are drawn. Returns the y position below the last drawn line.
pub fn draw_wrapped<D>(
    target: &mut D,
    at: Point,
    text: &str,
    width_px: i32,
    max_lines: usize,
    color: Rgb565,
    theme: &Theme,
) -> Result<i32, D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let max_chars = theme.chars_per_line(width_px);
    let lines = wrap(text, max_chars);
    let mut y = at.y;
    let mut drawn = 0;
    for line in lines.iter().take(max_lines) {
        if line.is_empty() {
            y += theme.line_h;
            drawn += 1;
            continue;
        }
        draw_text(target, Point::new(at.x, y), line, color, theme)?;
        y += theme.line_h;
        drawn += 1;
    }
    Ok(at.y + drawn * theme.line_h)
}

/// Draw a labelled bar (used for headers and footers), with a 1-px underline
/// so it reads as a distinct band.
pub fn draw_bar<D>(
    target: &mut D,
    rect: Rectangle,
    label: &str,
    right_label: &str,
    bg: Rgb565,
    fg: Rgb565,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    fill_rect(target, rect, bg)?;
    // 1-px underline separates the bar from the body below it.
    fill_rect(
        target,
        Rectangle::new(
            rect.top_left + Point::new(0, rect.size.height as i32 - 1),
            px(rect.size.width as i32, 1),
        ),
        theme.border,
    )?;
    let text_color = fg;
    // Text is aligned to the 6×10 glyph grid so the terminal simulator can
    // reconstruct characters from the framebuffer.
    draw_text(target, rect.top_left + Point::new(0, 0), label, text_color, theme)?;
    if !right_label.is_empty() {
        let width = rect.size.width as i32;
        let max_chars = theme.chars_per_line(width);
        let trimmed: String = right_label.chars().take(max_chars).collect();
        let right = rect.top_left
            + Point::new(
                width - 2 - (trimmed.chars().count() as i32) * theme.char_w,
                0,
            );
        draw_text(target, right, &trimmed, text_color, theme)?;
    }
    Ok(())
}

/// Draw a scrollbar on the right edge of `viewport` representing
/// `offset`/`total` visible lines.
pub fn draw_scrollbar<D>(
    target: &mut D,
    viewport: Rectangle,
    offset: usize,
    total: usize,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if total == 0 || total <= viewport.size.height as usize / theme.line_h as usize {
        return Ok(());
    }
    let track = viewport.size.width as i32 - 2;
    let thumb_h = 12i32;
    let track_h = viewport.size.height as i32;
    let scrollable = total.saturating_sub(viewport.size.height as usize / theme.line_h as usize);
    let ratio = (offset as f32 / scrollable as f32).clamp(0.0, 1.0);
    let y = viewport.top_left.y + ((track_h - thumb_h) as f32 * ratio) as i32;
    fill_rect(
        target,
        Rectangle::new(
            viewport.top_left + Point::new(track, y),
            px(2, thumb_h),
        ),
        theme.border,
    )
}

/// Truncate `text` to `max_chars` for single-line display.
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('~');
        s
    }
}