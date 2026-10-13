//! Low-level drawing helpers used by the screens.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Angle, Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    Arc, Circle, Ellipse, Line, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle,
};
use embedded_graphics::text::{Baseline, Text};

use crate::context::NetworkState;
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

/// Draw the top status bar: title, an optional right label, and the WiFi /
/// LoRa status icons at the far right.
pub fn draw_header<D>(
    target: &mut D,
    width: i32,
    title: &str,
    right_label: &str,
    network: &NetworkState,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let height = theme.line_h;
    let header = Rectangle::new(Point::new(0, 0), px(width, height));
    fill_rect(target, header, theme.header)?;
    // 1-px underline separates the header from the body below it.
    fill_rect(target, Rectangle::new(Point::new(0, height - 1), px(width, 1)), theme.border)?;
    draw_text(target, Point::new(0, 0), title, theme.header_text, theme)?;

    let cw = theme.char_w;

    // LoRa icon (far right): an antenna, green when online, red (struck
    // through) when offline.
    let lora_online = matches!(network.lora_online, Some(true));
    let lora_color = if lora_online {
        theme.ok
    } else {
        theme.danger
    };
    let lora_icon_w = 5;
    let lora_x = width - 2 - lora_icon_w;
    draw_lora_icon(target, Point::new(lora_x, 1), lora_online, lora_color)?;

    // WiFi icon (left of LoRa): a signal fan, green when online, red when
    // offline.
    let wifi_online = network.wifi_connected;
    let wifi_level = if wifi_online {
        match network.wifi_rssi.unwrap_or(-100) {
            r if r >= -50 => 4,
            r if r >= -60 => 3,
            r if r >= -70 => 2,
            _ => 1,
        }
    } else {
        1
    };
    let wifi_color = if wifi_online {
        theme.ok
    } else {
        theme.danger
    };
    let wifi_icon_w = 13;
    let wifi_x = lora_x - 2 - wifi_icon_w;
    draw_wifi_icon(target, Point::new(wifi_x, 1), wifi_level, wifi_color)?;

    // A solid red line under the WiFi or LoRa icon marks the interface the
    // currently shown destination is available via. Drawn at the header's
    // base row, directly under the icon glyphs.
    if let Some(dest_iface) = network.dest_iface {
        let underline_y = height - 1;
        if network.wifi_iface == Some(dest_iface) {
            fill_rect(
                target,
                Rectangle::new(Point::new(wifi_x, underline_y), px(wifi_icon_w, 1)),
                theme.danger,
            )?;
        }
        if network.lora_iface == Some(dest_iface) {
            fill_rect(
                target,
                Rectangle::new(Point::new(lora_x, underline_y), px(lora_icon_w, 1)),
                theme.danger,
            )?;
        }
    }

    // Right label (counts etc.) to the left of the icons.
    if !right_label.is_empty() {
        let max = ((wifi_x - 2) / cw).max(1) as usize;
        let trimmed: String = right_label.chars().take(max).collect();
        let rl_x = wifi_x - 2 - (trimmed.chars().count() as i32) * cw;
        draw_text(target, Point::new(rl_x, 0), &trimmed, theme.header_text, theme)?;
    }

    Ok(())
}

/// Draw a WiFi signal icon at `at` (top-left of its bounding box).
///
/// A base dot plus up to three radiating arcs; `level` (1–4) selects how many
/// arcs are lit, so stronger signals show a fuller fan.
fn draw_wifi_icon<D>(
    target: &mut D,
    at: Point,
    level: u8,
    color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    // The arcs share a centre above the base dot. `cx`/`cy` place the 13×9 px
    // glyph inside a bar `line_h` (10) tall with ~1 px to spare.
    let cx = at.x + 6;
    let cy = at.y + 7;
    let stroke = PrimitiveStyle::with_stroke(color, 1);

    // Base dot.
    Circle::new(Point::new(cx - 1, cy - 1), 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;

    // Signal arcs (upper semicircles). Weak signals light only the innermost
    // arc; stronger signals fill outward. Radii step by 2 so there is a
    // pixel of clear space between the strokes.
    let arcs = level.clamp(1, 4);
    for r in [2u32, 4, 6].iter().take(arcs as usize) {
        let diameter = r * 2;
        let rect = Rectangle::new(
            Point::new(cx - *r as i32, cy - *r as i32),
            Size::new(diameter, diameter),
        );
        Arc::new(
            rect.top_left,
            diameter,
            Angle::from_degrees(180.0),
            Angle::from_degrees(180.0),
        )
        .into_styled(stroke)
        .draw(target)?;
    }
    Ok(())
}

/// Draw a LoRa radio antenna icon at `at` (top-left of its bounding box).
///
/// A single vertical mast with a tip ball. When `online` is false a diagonal
/// strike is drawn through it.
fn draw_lora_icon<D>(
    target: &mut D,
    at: Point,
    online: bool,
    color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = at.x + 2;
    let top = at.y + 1;
    let bottom = at.y + 8;
    let stroke = PrimitiveStyle::with_stroke(color, 1);

    // Antenna mast.
    Line::new(Point::new(x, top), Point::new(x, bottom))
        .into_styled(stroke)
        .draw(target)?;
    // Tip ball.
    Circle::new(Point::new(x - 1, top - 1), 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;
    // Strike-through when the radio is offline.
    if !online {
        Line::new(Point::new(at.x, at.y + 1), Point::new(at.x + 4, at.y + 8))
            .into_styled(stroke)
            .draw(target)?;
    }
    Ok(())
}

/// Side length of a root-menu icon's square bounding box, in pixels.
pub const MENU_ICON: i32 = 24;

/// A section icon shown beside an entry on the root menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuIcon {
    /// LXMF chat: a speech bubble.
    Chat,
    /// NomadNet: a globe.
    Nomad,
    /// Identity: a person.
    Identity,
    /// Settings: a gear.
    Settings,
}

/// Draw the [`MenuIcon`] `icon` with its `MENU_ICON`×`MENU_ICON` bounding box
/// at `at` (top-left), stroked/filled in `color`.
pub fn draw_menu_icon<D>(
    target: &mut D,
    icon: MenuIcon,
    at: Point,
    color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match icon {
        MenuIcon::Chat => draw_chat_icon(target, at, color),
        MenuIcon::Nomad => draw_nomad_icon(target, at, color),
        MenuIcon::Identity => draw_identity_icon(target, at, color),
        MenuIcon::Settings => draw_settings_icon(target, at, color),
    }
}

/// A speech bubble with a tail and three dots, for the chat section.
fn draw_chat_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Bubble body (rounded rectangle).
    RoundedRectangle::with_equal_corners(
        Rectangle::new(at + Point::new(2, 3), px(20, 14)),
        Size::new(3, 3),
    )
    .into_styled(stroke)
    .draw(target)?;
    // Tail, filled so it reads as part of the bubble.
    Triangle::new(
        at + Point::new(6, 16),
        at + Point::new(6, 22),
        at + Point::new(13, 16),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(target)?;
    // Three "typing" dots inside the bubble.
    for dx in [8, 12, 16] {
        Circle::new(at + Point::new(dx - 1, 9), 2)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(target)?;
    }
    Ok(())
}

/// A globe (sphere plus meridian and equator), for the NomadNet section.
fn draw_nomad_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Outer sphere.
    Circle::new(at + Point::new(3, 3), 18).into_styled(stroke).draw(target)?;
    // Vertical meridian.
    Ellipse::new(at + Point::new(8, 3), Size::new(8, 18))
        .into_styled(stroke)
        .draw(target)?;
    // Equator.
    Line::new(at + Point::new(3, 12), at + Point::new(21, 12))
        .into_styled(stroke)
        .draw(target)?;
    Ok(())
}

/// A head-and-shoulders figure, for the identity section.
fn draw_identity_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Head.
    Circle::new(at + Point::new(8, 3), 8).into_styled(stroke).draw(target)?;
    // Shoulders: the upper half of a circle.
    Arc::new(
        at + Point::new(3, 12),
        18,
        Angle::from_degrees(180.0),
        Angle::from_degrees(180.0),
    )
    .into_styled(stroke)
    .draw(target)?;
    Ok(())
}

/// A gear (ring with eight teeth and a hub), for the settings section.
fn draw_settings_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let c = at + Point::new(12, 12);
    // Eight short teeth radiating from the rim.
    let teeth = [
        (Point::new(0, -6), Point::new(0, -9)),
        (Point::new(0, 6), Point::new(0, 9)),
        (Point::new(6, 0), Point::new(9, 0)),
        (Point::new(-6, 0), Point::new(-9, 0)),
        (Point::new(4, -4), Point::new(6, -6)),
        (Point::new(-4, -4), Point::new(-6, -6)),
        (Point::new(4, 4), Point::new(6, 6)),
        (Point::new(-4, 4), Point::new(-6, 6)),
    ];
    for (a, b) in teeth {
        Line::new(c + a, c + b).into_styled(stroke).draw(target)?;
    }
    // Rim and hub.
    Circle::new(c - Point::new(7, 7), 14).into_styled(stroke).draw(target)?;
    Circle::new(c - Point::new(2, 2), 4).into_styled(stroke).draw(target)?;
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

/// Draw a QR code encoding `text`, centred within a `max_size`×`max_size`
/// box whose top-left corner is `top_left`.
///
/// The symbol is scaled to whole pixels per module (no fractional scaling, so
/// edges stay crisp on the low-resolution LCD) and includes the standard
/// 4-module quiet zone. Returns the on-screen size of the drawn symbol
/// (including the quiet zone), or `0` if the text could not be encoded.
pub fn draw_qr<D>(
    target: &mut D,
    top_left: Point,
    text: &str,
    max_size: i32,
    dark: Rgb565,
    light: Rgb565,
) -> Result<i32, D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let qr = match qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium) {
        Ok(qr) => qr,
        Err(_) => return Ok(0),
    };

    let modules = qr.size();
    let quiet = 4;
    let total = modules + quiet * 2;
    // Whole-pixel scale so each module is a crisp block; at least 1 px.
    let scale = (max_size / total).max(1);
    let size = total * scale;
    // Centre the symbol within the requested box.
    let origin = top_left + Point::new((max_size - size) / 2, (max_size - size) / 2);

    // Quiet zone + light modules.
    fill_rect(target, Rectangle::new(origin, px(size, size)), light)?;

    // Dark modules.
    for y in 0..modules {
        for x in 0..modules {
            if qr.get_module(x, y) {
                let at = origin + Point::new((x + quiet) * scale, (y + quiet) * scale);
                fill_rect(target, Rectangle::new(at, px(scale, scale)), dark)?;
            }
        }
    }

    Ok(size)
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

/// Format a destination's hop count for the status bar: `(5 hops)`, or the
/// empty string when the hop count is unknown.
pub fn hops_label(hops: Option<u8>) -> String {
    match hops {
        Some(1) => "(1 hop)".to_string(),
        Some(n) => format!("({n} hops)"),
        None => String::new(),
    }
}
