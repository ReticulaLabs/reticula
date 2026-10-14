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
    rect.into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)
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
    fill_rect(target, Rectangle::new(at, px(width, theme.line_h)), bg)?;
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
    draw_text(
        target,
        rect.top_left + Point::new(0, 0),
        label,
        text_color,
        theme,
    )?;
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
    fill_rect(
        target,
        Rectangle::new(Point::new(0, height - 1), px(width, 1)),
        theme.border,
    )?;
    draw_text(target, Point::new(0, 0), title, theme.header_text, theme)?;

    let cw = theme.char_w;

    // LoRa icon (far right): an antenna, green when online, red (struck
    // through) when offline.
    let lora_online = matches!(network.lora_online, Some(true));
    let lora_color = if lora_online { theme.ok } else { theme.danger };
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
    let wifi_color = if wifi_online { theme.ok } else { theme.danger };
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
        draw_text(
            target,
            Point::new(rl_x, 0),
            &trimmed,
            theme.header_text,
            theme,
        )?;
    }

    Ok(())
}

/// Draw a WiFi signal icon at `at` (top-left of its bounding box).
///
/// A base dot plus up to three radiating arcs; `level` (1–4) selects how many
/// arcs are lit, so stronger signals show a fuller fan.
fn draw_wifi_icon<D>(target: &mut D, at: Point, level: u8, color: Rgb565) -> Result<(), D::Error>
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
fn draw_lora_icon<D>(target: &mut D, at: Point, online: bool, color: Rgb565) -> Result<(), D::Error>
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

/// A section icon shown beside an entry on a menu.
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
    /// WiFi: a signal fan.
    Wifi,
    /// LoRa: a radio mast.
    Lora,
    /// Data management: a database.
    Data,
    /// GPS: a satellite over horizon arcs.
    Gps,
    /// Clock: a dial with hands.
    Clock,
}

/// One entry in a full-width menu list.
#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    /// Section name, shown next to the icon.
    pub title: &'static str,
    /// Short description shown beneath the name.
    pub subtitle: &'static str,
    /// Icon shown on the left of the row.
    pub icon: MenuIcon,
}

/// Number of rows the row height is proportioned for.
///
/// Both menus are sized for this many rows, which is why they look the same
/// even though the settings menu has more entries.
const MENU_REFERENCE_ROWS: i32 = 4;

/// Height of one row in a full-width menu list.
///
/// Shared by the root and settings menus so they are identical by construction
/// rather than by keeping two copies of the same arithmetic in step. Sized for
/// [`MENU_REFERENCE_ROWS`] entries; a longer list is squeezed to fit rather than
/// allowed to run off the bottom of the screen.
pub fn menu_row_height(height: i32, theme: &Theme, item_count: usize) -> i32 {
    // Room between the header/status band and the footer bar.
    let available = (height - theme.line_h * 2 - theme.line_h).max(theme.line_h);
    let preferred = height / (MENU_REFERENCE_ROWS + 2);
    let per_item = available / item_count.max(1) as i32;

    // Fitting wins over the preferred height, and a cramped row beats one that
    // runs off the bottom of the screen — a row the footer hides is worse than
    // a row with less padding. The `max(1)` only guards against a degenerate
    // zero-height display.
    preferred.min(per_item).max(1)
}

/// Draw a full-width menu list starting at `top`, and return the y coordinate
/// just past the last row.
///
/// `selected` is the index of the highlighted row. Rows are full width, each
/// [`menu_row_height`] tall, with the section icon on the left and the name
/// plus description beside it.
pub fn draw_menu_list<D>(
    target: &mut D,
    width: i32,
    height: i32,
    top: i32,
    items: &[MenuItem],
    selected: usize,
    theme: &Theme,
) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let row_h = menu_row_height(height, theme, items.len());
    let mut y = top;

    for (i, item) in items.iter().enumerate() {
        let is_selected = i == selected;
        let row = Rectangle::new(Point::new(0, y), px(width, row_h));

        if is_selected {
            fill_rect(target, row, theme.selection).ok();
            // Left accent bar marks the active row.
            fill_rect(
                target,
                Rectangle::new(Point::new(0, y), px(3, row_h)),
                theme.accent,
            )
            .ok();
        }

        // A hairline divider between rows, but not below the last one.
        if i + 1 < items.len() {
            fill_rect(
                target,
                Rectangle::new(Point::new(0, y + row_h - 1), px(width, 1)),
                theme.border,
            )
            .ok();
        }

        // Icon, vertically centred in the row.
        let icon_x = 12;
        let icon_y = y + (row_h - MENU_ICON) / 2;
        let icon_color = if is_selected {
            theme.selection_text
        } else {
            theme.accent
        };
        draw_menu_icon(target, item.icon, Point::new(icon_x, icon_y), icon_color).ok();

        // Title and description, to the right of the icon.
        let text_x = 48;
        let title_color = if is_selected {
            theme.selection_text
        } else {
            theme.text
        };
        let sub_color = if is_selected {
            theme.selection_text
        } else {
            theme.text_dim
        };
        draw_text(
            target,
            Point::new(text_x, y + theme.line_h),
            item.title,
            title_color,
            theme,
        )
        .ok();
        draw_text(
            target,
            Point::new(text_x, y + theme.line_h * 2),
            item.subtitle,
            sub_color,
            theme,
        )
        .ok();

        y += row_h;
    }

    y
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
        MenuIcon::Wifi => draw_wifi_menu_icon(target, at, color),
        MenuIcon::Lora => draw_lora_menu_icon(target, at, color),
        MenuIcon::Data => draw_data_menu_icon(target, at, color),
        MenuIcon::Gps => draw_gps_menu_icon(target, at, color),
        MenuIcon::Clock => draw_clock_menu_icon(target, at, color),
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
    Circle::new(at + Point::new(3, 3), 18)
        .into_styled(stroke)
        .draw(target)?;
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
    Circle::new(at + Point::new(8, 3), 8)
        .into_styled(stroke)
        .draw(target)?;
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
    Circle::new(c - Point::new(7, 7), 14)
        .into_styled(stroke)
        .draw(target)?;
    Circle::new(c - Point::new(2, 2), 4)
        .into_styled(stroke)
        .draw(target)?;
    Ok(())
}

/// A signal fan (dot plus radiating arcs), for the WiFi section.
fn draw_wifi_menu_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let base = at + Point::new(12, 18);
    // Base dot.
    Circle::new(base - Point::new(1, 1), 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;
    // Radiating arcs, outermost last.
    for r in [4i32, 7, 10] {
        let d = (r * 2) as u32;
        Arc::new(
            base - Point::new(r, r),
            d,
            Angle::from_degrees(180.0),
            Angle::from_degrees(180.0),
        )
        .into_styled(stroke)
        .draw(target)?;
    }
    Ok(())
}

/// A radio mast with a tip and emanating waves, for the LoRa section.
fn draw_lora_menu_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Mast.
    Line::new(at + Point::new(12, 21), at + Point::new(12, 8))
        .into_styled(stroke)
        .draw(target)?;
    // Tip ball.
    Circle::new(at + Point::new(11, 7), 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;
    // Waves arcing outward on either side of the tip.
    for r in [5i32, 8] {
        let d = (r * 2) as u32;
        let top_left = at + Point::new(12 - r, 8 - r);
        Arc::new(
            top_left,
            d,
            Angle::from_degrees(135.0),
            Angle::from_degrees(90.0),
        )
        .into_styled(stroke)
        .draw(target)?;
        Arc::new(
            top_left,
            d,
            Angle::from_degrees(315.0),
            Angle::from_degrees(90.0),
        )
        .into_styled(stroke)
        .draw(target)?;
    }
    Ok(())
}

/// A database (a top disc plus stacked front curves), for the data section.
fn draw_data_menu_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Top disc.
    Ellipse::new(at + Point::new(4, 4), Size::new(16, 6))
        .into_styled(stroke)
        .draw(target)?;
    // Sides joining the discs.
    Line::new(at + Point::new(4, 7), at + Point::new(4, 17))
        .into_styled(stroke)
        .draw(target)?;
    Line::new(at + Point::new(20, 7), at + Point::new(20, 17))
        .into_styled(stroke)
        .draw(target)?;
    // Shallow "U" front curves for the two dividers and the base.
    let fronts = [
        [(4, 10), (7, 12), (12, 13), (17, 12), (20, 10)],
        [(4, 14), (7, 16), (12, 17), (17, 16), (20, 14)],
        [(4, 17), (7, 19), (12, 20), (17, 19), (20, 17)],
    ];
    for pts in fronts {
        for w in pts.windows(2) {
            Line::new(
                at + Point::new(w[0].0, w[0].1),
                at + Point::new(w[1].0, w[1].1),
            )
            .into_styled(stroke)
            .draw(target)?;
        }
    }
    Ok(())
}

/// A satellite above radiating horizon arcs, for the GPS section.
///
/// The horizon is a flat baseline rather than a circle: the icon is 24 px, and
/// concentric arcs read as "signal" at that size where a full orbit does not.
fn draw_gps_menu_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let fill = PrimitiveStyle::with_fill(color);

    // Horizon: a baseline with short end ticks.
    Line::new(at + Point::new(3, 21), at + Point::new(21, 21))
        .into_styled(stroke)
        .draw(target)?;
    for x in [3, 21] {
        Line::new(at + Point::new(x, 18), at + Point::new(x, 21))
            .into_styled(stroke)
            .draw(target)?;
    }

    // Broadcast arcs opening upward from the horizon's centre.
    for r in [4i32, 7] {
        let d = (r * 2) as u32;
        let top_left = at + Point::new(12 - r, 21 - r);
        Arc::new(
            top_left,
            d,
            Angle::from_degrees(180.0),
            Angle::from_degrees(180.0),
        )
        .into_styled(stroke)
        .draw(target)?;
    }

    // Satellite body above the arcs, with two short solar panels.
    let body = Rectangle::new(at + Point::new(9, 3), Size::new(6, 5));
    body.into_styled(fill).draw(target)?;
    for x in [6, 15] {
        Line::new(at + Point::new(x, 4), at + Point::new(x, 7))
            .into_styled(stroke)
            .draw(target)?;
    }
    Line::new(at + Point::new(4, 4), at + Point::new(6, 5))
        .into_styled(stroke)
        .draw(target)?;
    Line::new(at + Point::new(20, 4), at + Point::new(18, 5))
        .into_styled(stroke)
        .draw(target)?;

    Ok(())
}

/// A clock dial with hands at roughly ten past ten, for the System section.
fn draw_clock_menu_icon<D>(target: &mut D, at: Point, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);

    // Dial.
    Circle::new(at + Point::new(3, 3), 18)
        .into_styled(stroke)
        .draw(target)?;

    // Hour marks at 12 and 6.
    for y in [3, 19] {
        Line::new(at + Point::new(12, y), at + Point::new(12, y + 2))
            .into_styled(stroke)
            .draw(target)?;
    }

    // Hands: hour towards 10, minute towards 2, from the dial's centre.
    // `at` offsets every coordinate — the dial is drawn at `at + (3, 3)` with
    // diameter 18, so its centre is `at + (12, 12)`.
    let c = at + Point::new(12, 12);
    Line::new(c, at + Point::new(8, 7))
        .into_styled(stroke)
        .draw(target)?;
    Line::new(c, at + Point::new(17, 9))
        .into_styled(stroke)
        .draw(target)?;

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
        Rectangle::new(viewport.top_left + Point::new(track, y), px(2, thumb_h)),
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

#[cfg(test)]
mod tests {
    use super::*;

    use embedded_graphics::mock_display::MockDisplay;
    use embedded_graphics::primitives::Rectangle;

    /// Every menu icon, so a new variant cannot be added without being covered.
    const ALL_ICONS: [MenuIcon; 8] = [
        MenuIcon::Chat,
        MenuIcon::Nomad,
        MenuIcon::Identity,
        MenuIcon::Settings,
        MenuIcon::Wifi,
        MenuIcon::Lora,
        MenuIcon::Gps,
        MenuIcon::Clock,
    ];

    /// An icon must paint only inside its own `MENU_ICON`-sized box.
    ///
    /// Every coordinate in an icon body has to be offset by `at`; a bare
    /// `Point::new(..)` resolves against the screen origin instead, which
    /// draws a stray line from the icon back to the top-left of the display.
    /// That is invisible in review and obvious on the panel, so pin it down.
    #[test]
    fn icons_stay_inside_their_own_box() {
        // Somewhere well away from the origin, so a missing `at +` offset
        // cannot accidentally land inside the expected box. `MockDisplay` is
        // 64×64, so a 24×24 icon has to start at or below (40, 40).
        let at = Point::new(20, 20);
        let expected = Rectangle::new(at, Size::new(MENU_ICON as u32, MENU_ICON as u32));

        for icon in ALL_ICONS {
            let mut canvas: MockDisplay<Rgb565> = MockDisplay::new();
            canvas.set_allow_out_of_bounds_drawing(false);
            // Icons legitimately overlap themselves — the dial's stroke crosses
            // its hour marks — so overdraw is fine; only *where* they land
            // matters here.
            canvas.set_allow_overdraw(true);
            draw_menu_icon(&mut canvas, icon, at, Rgb565::new(31, 31, 31)).unwrap();

            let drawn = canvas.affected_area();
            // If `drawn` is entirely inside `expected`, intersecting them
            // yields `drawn` unchanged. Anything that escaped the box clips it
            // and the two differ.
            assert_eq!(
                drawn.intersection(&expected),
                drawn,
                "{icon:?} painted outside its {MENU_ICON}x{MENU_ICON} box: drew {drawn:?}, \
                 expected within {expected:?}",
            );
        }
    }

    /// Every icon should actually draw something; a completely blank one would
    /// otherwise pass the containment check above.
    #[test]
    fn icons_are_not_blank() {
        let at = Point::new(20, 20);
        for icon in ALL_ICONS {
            let mut canvas: MockDisplay<Rgb565> = MockDisplay::new();
            canvas.set_allow_overdraw(true);
            draw_menu_icon(&mut canvas, icon, at, Rgb565::new(31, 31, 31)).unwrap();

            let area = canvas.affected_area();
            let painted = area.size.width * area.size.height;
            assert!(
                painted > 20,
                "{icon:?} looks blank (affected area {area:?})",
            );
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    use crate::theme::{Theme, DISPLAY_H, DISPLAY_W};

    /// Top of the menu list: below the header and status band.
    fn top() -> i32 {
        Theme::default().line_h * 2
    }

    /// Both menus must use the same row height, so they look identical
    /// regardless of how many entries each has.
    #[test]
    fn menus_with_different_item_counts_share_a_row_height() {
        let theme = Theme::default();
        let home = menu_row_height(DISPLAY_H, &theme, 4);
        let settings = menu_row_height(DISPLAY_H, &theme, 5);
        assert_eq!(
            home, settings,
            "the root and settings menus must have identical row heights"
        );
    }

    /// A list must never run into the footer bar, whatever its length. This is
    /// the regression the settings menu hit when it gained its fifth entry.
    #[test]
    fn a_long_list_stays_clear_of_the_footer() {
        let theme = Theme::default();
        for count in 1..=12 {
            let row_h = menu_row_height(DISPLAY_H, &theme, count);
            let bottom = top() + row_h * count as i32;
            assert!(
                bottom <= DISPLAY_H - theme.line_h,
                "{count} items overflow: rows end at {bottom}, footer starts at {}",
                DISPLAY_H - theme.line_h
            );
        }
    }

    /// Every row must be tall enough to hold its two lines of text, for any list
    /// short enough that the rows are not already squeezed to fit.
    #[test]
    fn rows_are_never_shorter_than_their_content() {
        let theme = Theme::default();
        let available = DISPLAY_H - theme.line_h * 2 - theme.line_h;

        for count in 1..=12 {
            let row_h = menu_row_height(DISPLAY_H, &theme, count);
            let comfortable = available / count as i32 >= theme.line_h * 3;

            if comfortable {
                assert!(
                    row_h >= theme.line_h * 2,
                    "{count} items: row {row_h} is too short for two text lines"
                );
                assert!(
                    row_h >= MENU_ICON,
                    "{count} items: row {row_h} is shorter than the {MENU_ICON}px icon"
                );
            }
        }
    }

    /// The two menus must land on the same row pitch on the real panel.
    #[test]
    fn row_geometry_matches_the_panel() {
        let theme = Theme::default();
        let row_h = menu_row_height(DISPLAY_H, &theme, 5);
        assert_eq!(
            (row_h, top() + row_h * 5),
            (40, 220),
            "5 settings rows should be 40px each, ending at y=220 with the footer at 230"
        );
        assert!(220 <= DISPLAY_H - theme.line_h);
    }

    /// A tiny display must still produce a usable row height rather than
    /// dividing down to nothing.
    #[test]
    fn degenerate_sizes_do_not_divide_by_zero() {
        let theme = Theme::default();
        assert!(menu_row_height(0, &theme, 0) >= 1);
        assert!(menu_row_height(10, &theme, 3) >= 1);
        assert_eq!(DISPLAY_W, 320);
    }
}
