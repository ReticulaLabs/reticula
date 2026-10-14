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

/// Draw a single line of text so its last glyph ends at `right_x`.
///
/// Shared by the bars, which all right-align a short label against a margin, so
/// the 2 px inset and the glyph-grid alignment are defined once.
pub fn draw_text_right<D>(
    target: &mut D,
    right_x: i32,
    y: i32,
    text: &str,
    color: Rgb565,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = text.chars().count() as i32 * theme.char_w;
    draw_text(target, Point::new(right_x - w, y), text, color, theme)
}

/// Draw the standard bottom status bar: a keymap hint on the left and, on the
/// same line, `right_label` (the hop count) at the right.
///
/// One helper rather than three hand-built copies, so the bar's height, inset and
/// colours cannot drift apart between screens.
pub fn draw_footer<D>(
    target: &mut D,
    width: i32,
    height: i32,
    hint: &str,
    right_label: &str,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let rect = Rectangle::new(
        Point::new(0, height - theme.line_h),
        px(width, theme.line_h),
    );
    // The hint and the hop count share this one line, so the hint gives up
    // whatever room the count needs. Truncating the count instead would hide
    // the very thing the user is looking at; truncating the hint loses only the
    // tail of a key legend, and only on screens too narrow to show both.
    let reserved = if right_label.is_empty() {
        0
    } else {
        (right_label.chars().count() as i32 + 1) * theme.char_w
    };
    let hint = truncate(hint, theme.chars_per_line(width - reserved));
    draw_bar(
        target,
        rect,
        &hint,
        right_label,
        theme.surface,
        theme.text_dim,
        theme,
    )
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
        draw_text_right(
            target,
            rect.top_left.x + width - 2,
            rect.top_left.y,
            &trimmed,
            text_color,
            theme,
        )?;
    }
    Ok(())
}

/// Draw the top status bar: a title on the left and the status icons at the far
/// right.
///
/// There is deliberately no right-hand label slot. The only thing that ever
/// wanted one was the hop count, and it belongs on the footer: three status
/// icons now reach far enough left that a label beside them had nowhere to go,
/// and truncating it was not enough — it still collided. Removing the slot
/// makes that class of overlap impossible instead of tuned away.
pub fn draw_header<D>(
    target: &mut D,
    width: i32,
    title: &str,
    network: &NetworkState,
    theme: &Theme,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let height = theme.header_h;
    let header = Rectangle::new(Point::new(0, 0), px(width, height));
    fill_rect(target, header, theme.header)?;
    // 1-px underline separates the header from the body below it.
    fill_rect(
        target,
        Rectangle::new(Point::new(0, height - 1), px(width, 1)),
        theme.border,
    )?;

    // The bar is taller than a text line so the icons have room, so both the
    // title and the glyphs are centred vertically rather than pinned to the
    // top — otherwise they sit at different heights and the bar looks lopsided.
    let text_y = (height - theme.line_h) / 2;
    // Centre the taller status glyphs, leaving the last row for the underline.
    let icon_y = (height - WIFI_ICON_H) / 2;

    draw_text(
        target,
        Point::new(0, text_y),
        title,
        theme.header_text,
        theme,
    )?;

    // The icons are laid out right-to-left and their positions are fixed whether or
    // not each one is drawn. A GNSS fix can arrive at any moment, and letting
    // the other icons slide over to fill its place would make the whole bar jump
    // mid-read; an absent icon just leaves a gap.

    // LoRa icon (far right): an antenna standing on its ground line, or a ball
    // resting on it when the radio is down.
    let lora_online = matches!(network.lora_online, Some(true));
    let lora_color = if lora_online { theme.ok } else { theme.danger };
    let lora_x = width - 2 - LORA_ICON_W;
    draw_lora_icon(target, Point::new(lora_x, icon_y), lora_online, lora_color)?;

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
    let wifi_x = lora_x - 3 - WIFI_ICON_W;
    draw_wifi_icon(target, Point::new(wifi_x, icon_y), wifi_level, wifi_color)?;

    // GNSS icon (left of WiFi): a crosshair, red while it is still acquiring a
    // fix and green once it has one. Only drawn when a receiver is configured
    // — `None` means this device has no GNSS at all, not that it has lost one.
    let gnss_x = wifi_x - 3 - GNSS_ICON_W;
    if let Some(locked) = network.gnss_locked {
        let gnss_color = if locked { theme.ok } else { theme.danger };
        // Anchored to the bar's baseline rather than centred, so the sphere
        // appears to sit *on* the underline like a planet on a horizon. The cut
        // is the row above the underline, which stays its own colour.
        let gnss_y = height - 1 - GNSS_ICON_H;
        draw_gnss_icon(target, Point::new(gnss_x, gnss_y), locked, gnss_color)?;
    }

    // A solid red line under the WiFi or LoRa icon marks the interface the
    // currently shown destination is available via. Drawn at the header's
    // base row, directly under the icon glyphs.
    if let Some(dest_iface) = network.dest_iface {
        let underline_y = height - 1;
        if network.wifi_iface == Some(dest_iface) {
            fill_rect(
                target,
                Rectangle::new(Point::new(wifi_x, underline_y), px(WIFI_ICON_W, 1)),
                theme.danger,
            )?;
        }
        if network.lora_iface == Some(dest_iface) {
            fill_rect(
                target,
                Rectangle::new(Point::new(lora_x, underline_y), px(LORA_ICON_W, 1)),
                theme.danger,
            )?;
        }
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
    // The arcs share a centre above the base dot. Radii step by 3 so each
    // stroke is separated by two clear pixels, which is what makes the level
    // readable at this size — the old 2 px pitch left single-pixel gaps that
    // filled in at a glance.
    let cx = at.x + WIFI_ICON_W / 2;
    let cy = at.y + WIFI_ICON_H - 1;
    let stroke = PrimitiveStyle::with_stroke(color, 1);

    // Base dot.
    Circle::new(Point::new(cx - 1, cy - 1), 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;

    // Signal arcs (upper semicircles). Weak signals light only the innermost
    // arc; stronger signals fill outward.
    let arcs = level.clamp(1, 4);
    for r in [3u32, 6, 9].iter().take(arcs as usize) {
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
/// A mast standing on a ground line, with the antenna element as a ball at the
/// tip. When `online` is false the mast is retracted and the ball drops to the
/// ground line, so "no radio" reads as an antenna that is *down* rather than
/// as an icon with a line through it — a strike-through is easy to mistake for
/// the red "problem" colour at a glance.
fn draw_lora_icon<D>(target: &mut D, at: Point, online: bool, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = at.x + LORA_ICON_W / 2;
    let ground_y = at.y + LORA_ICON_H - 1;
    let stroke = PrimitiveStyle::with_stroke(color, 1);

    // Ground line the mast stands on. Drawn in both states so the two read as
    // the same object in different positions.
    Line::new(
        Point::new(at.x, ground_y),
        Point::new(at.x + LORA_ICON_W - 1, ground_y),
    )
    .into_styled(stroke)
    .draw(target)?;

    if online {
        // Mast, with the antenna element as a ball at the tip.
        Line::new(Point::new(x, at.y + 2), Point::new(x, ground_y - 1))
            .into_styled(stroke)
            .draw(target)?;
        Circle::new(Point::new(x - 1, at.y), 2)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(target)?;
    } else {
        // Retracted: the antenna element sits on the ground line.
        Circle::new(Point::new(x - 2, ground_y - 3), 4)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(target)?;
    }

    Ok(())
}

/// Draw a GNSS status icon at `at` (top-left of its bounding box).
///
/// The top of a sphere rising from below the status bar, cut off by its
/// underline — a planet's limb on a horizon.
///
/// `locked` fills the cap solid; while still acquiring only the limb's edge is
/// drawn, so the two states differ in shape as well as colour. That matters
/// because the bar is 15 px tall and a red-versus-green arc on its own is easy
/// to misread, and it is exactly at a glance, carrying the device, that the user
/// most needs to know whether the clock is up.
fn draw_gnss_icon<D>(target: &mut D, at: Point, locked: bool, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = at.x + GNSS_ICON_W / 2;
    let rows = GNSS_CAP_HALF.len() as i32;
    let bottom = at.y + GNSS_ICON_H - 1;

    // The sphere is mostly below the glyph box: only its cap is drawn, sitting
    // on the bar's underline the way a planet's limb sits on a horizon. The cap
    // is rasterised row by row from a precomputed half-width table rather than
    // with a `Circle`, because a circle primitive would also paint the hidden
    // lower half over the underline.
    for (i, &half) in GNSS_CAP_HALF.iter().enumerate() {
        let y = bottom - (rows - 1) + i as i32;
        if locked {
            // A solid body: the receiver has a position, and the planet is
            // there in full.
            fill_rect(
                target,
                Rectangle::new(Point::new(cx - half, y), px(half * 2 + 1, 1)),
                color,
            )?;
        } else {
            // An outline: the limb is visible but the body behind it is not,
            // which is the whole meaning of "still looking".
            for x in [cx - half, cx + half] {
                fill_rect(target, Rectangle::new(Point::new(x, y), px(1, 1)), color)?;
            }
        }
    }
    Ok(())
}

/// Side length of a root-menu icon's square bounding box, in pixels.
pub const MENU_ICON: i32 = 24;

/// Bounding box of the WiFi status glyph, in pixels.
const WIFI_ICON_W: i32 = 19;
const WIFI_ICON_H: i32 = 12;

/// Bounding box of the LoRa status glyph, in pixels.
const LORA_ICON_W: i32 = 9;
const LORA_ICON_H: i32 = 12;

/// Bounding box of the GNSS status glyph, in pixels.
const GNSS_ICON_W: i32 = 19;
/// Height of the visible cap. The sphere is cut off by the bar's baseline, so
/// this is a sliver of a much larger circle rather than a glyph of its own.
const GNSS_ICON_H: i32 = 6;

/// Half-width of the sphere's visible cap, per row, widest last.
///
/// Sampled from a circle of radius 11 whose top sits half a pixel above the
/// first row: at each row centre `hw = sqrt(r² − (r − (i + ½))²)` gives 3.28,
/// 5.55, 6.98, 8.05, 8.87, 9.53. Sampling it matters — the first row opens at
/// 3 px rather than 1, because a real cap is flat where a hand-picked sequence
/// tapers to a point and reads as a spire.
///
/// The table is precomputed so the glyph costs six `fill_rect` calls with no
/// trigonometry, and so a test can assert the profile without re-deriving it.
/// These rows are the top few percent of a circle far wider than the bar: the
/// rest is below the cut, which is what makes this read as a planet on a horizon
/// rather than as a ball.
const GNSS_CAP_HALF: [i32; 6] = [3, 5, 7, 8, 9, 9];

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
    // Room between the bottom of the status band and the footer bar.
    let available = (height - body_top(theme) - theme.line_h).max(theme.line_h);
    let preferred = height / (MENU_REFERENCE_ROWS + 2);
    let per_item = available / item_count.max(1) as i32;

    // Fitting wins over the preferred height, and a cramped row beats one that
    // runs off the bottom of the screen — a row the footer hides is worse than
    // a row with less padding. The `max(1)` only guards against a degenerate
    // zero-height display.
    preferred.min(per_item).max(1)
}

/// Top of a menu body: below the status bar and its status line.
pub fn body_top(theme: &Theme) -> i32 {
    theme.header_h + theme.line_h
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

/// Test-only draw target, shared by the glyph and layout test modules.
#[cfg(test)]
pub(crate) mod test_support {
    use embedded_graphics::draw_target::DrawTarget;
    use embedded_graphics::geometry::{OriginDimensions, Size};
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::primitives::Rectangle;
    use embedded_graphics::Pixel;

    /// A draw target that records painted pixels at any size.
    ///
    /// `MockDisplay` is fixed at 64x64, which is smaller than the real 320x240
    /// panel. Anything that lays out across the full width — the header's icon
    /// strip, the footer's two-sided line — has to be checked at true dimensions,
    /// or the test is measuring a screen nobody uses.
    pub struct Painted {
        w: i32,
        h: i32,
        px: Vec<Option<Rgb565>>,
    }

    impl Painted {
        pub fn new(w: i32, h: i32) -> Self {
            Self {
                w,
                h,
                px: vec![None; (w * h) as usize],
            }
        }

        pub fn get(&self, x: i32, y: i32) -> Option<Rgb565> {
            if x < 0 || y < 0 || x >= self.w || y >= self.h {
                return None;
            }
            self.px[(y * self.w + x) as usize]
        }

        fn set(&mut self, x: i32, y: i32, c: Rgb565) {
            if x >= 0 && y >= 0 && x < self.w && y < self.h {
                self.px[(y * self.w + x) as usize] = Some(c);
            }
        }

        /// Columns holding a painted pixel on any of `rows` that is not one of
        /// `ignore`.
        ///
        /// `ignore` exists because bars paint their whole background and
        /// underline across the full width; without it every column would look
        /// painted and the test would measure nothing.
        pub fn glyph_cols(&self, rows: std::ops::Range<i32>, ignore: &[Rgb565]) -> Vec<i32> {
            (0..self.w)
                .filter(|x| {
                    rows.clone()
                        .any(|y| self.get(*x, y).is_some_and(|c| !ignore.contains(&c)))
                })
                .collect()
        }
    }

    impl OriginDimensions for Painted {
        fn size(&self) -> Size {
            Size::new(self.w as u32, self.h as u32)
        }
    }

    impl DrawTarget for Painted {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for p in pixels {
                self.set(p.0.x, p.0.y, p.1);
            }
            Ok(())
        }

        fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
            for y in area.top_left.y..area.top_left.y + area.size.height as i32 {
                for x in area.top_left.x..area.top_left.x + area.size.width as i32 {
                    self.set(x, y, color);
                }
            }
            Ok(())
        }

        fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
            self.px.fill(Some(color));
            Ok(())
        }
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
    use super::test_support::Painted;
    use super::*;

    use crate::theme::{Theme, DISPLAY_H, DISPLAY_W, HEADER_EXTRA_H};

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

    /// The status bar is one text line plus the extra room the icons need.
    #[test]
    fn the_status_bar_is_five_px_taller_than_a_text_line() {
        let theme = Theme::default();
        assert_eq!(
            theme.header_h,
            theme.line_h + HEADER_EXTRA_H,
            "the bar should be one text line plus HEADER_EXTRA_H"
        );
        assert_eq!(HEADER_EXTRA_H, 5, "the bar was asked to grow by 5px");
        assert_eq!(theme.header_h, 15);
    }

    /// The bar must be tall enough for the icon glyphs plus the underline that
    /// marks the current interface, with something to spare either side.
    #[test]
    fn the_status_bar_fits_its_icons() {
        let theme = Theme::default();
        assert!(
            theme.header_h >= WIFI_ICON_H.max(LORA_ICON_H) + 2,
            "bar {} is too short for a {WIFI_ICON_H}px glyph plus the underline",
            theme.header_h
        );
        // Body content must start below the bar, not under it.
        assert!(
            body_top(&theme) > theme.header_h,
            "body content must clear the status bar"
        );
    }

    /// Where each status glyph is actually drawn, in absolute pixels.
    fn glyph_boxes(width: i32, theme: &Theme) -> [Rectangle; 3] {
        // The icon glyphs are centred; the sphere is anchored to the baseline so
        // it sits on the underline. Mirrors `draw_header` exactly — if these two
        // ever disagree, the containment test below checks the wrong box.
        let icon_y = (theme.header_h - WIFI_ICON_H) / 2;
        let gnss_y = theme.header_h - 1 - GNSS_ICON_H;
        let lora_x = width - 2 - LORA_ICON_W;
        let wifi_x = lora_x - 3 - WIFI_ICON_W;
        let gnss_x = wifi_x - 3 - GNSS_ICON_W;
        let box_at = |x, y, w, h| Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
        [
            box_at(gnss_x, gnss_y, GNSS_ICON_W, GNSS_ICON_H),
            box_at(wifi_x, icon_y, WIFI_ICON_W, WIFI_ICON_H),
            box_at(lora_x, icon_y, LORA_ICON_W, LORA_ICON_H),
        ]
    }

    /// Every status glyph must fit inside the bar, clear of its underline, and must
    /// not collide with its neighbours — the header is only wide enough for one
    /// of each, and there are now three.
    #[test]
    fn status_glyphs_fit_inside_the_bar_without_overlapping() {
        let theme = Theme::default();
        let [gnss, wifi, lora] = glyph_boxes(DISPLAY_W, &theme);
        let bar = Rectangle::new(
            Point::new(0, 0),
            Size::new(DISPLAY_W as u32, theme.header_h as u32),
        );

        for (name, glyph) in [("GNSS", gnss), ("WiFi", wifi), ("LoRa", lora)] {
            assert!(
                bar.intersection(&glyph) == glyph,
                "{name} glyph {glyph:?} escapes the {}-tall bar",
                theme.header_h
            );
            // The last row belongs to the interface underline.
            assert!(
                glyph.bottom_right().map(|p| p.y).unwrap_or(0) < theme.header_h - 1,
                "{name} glyph must leave the underline row free"
            );
            assert!(glyph.top_left.x >= 0, "{name} glyph runs off the left edge");
        }

        // A gap between each pair, so no two ever read as one blob. Laid out
        // right-to-left, so check each against the one to its left.
        for (right_name, right, left_name, left) in
            [("LoRa", lora, "WiFi", wifi), ("WiFi", wifi, "GNSS", gnss)]
        {
            assert!(
                right.intersection(&left) != right,
                "{right_name} and {left_name} glyphs overlap: {right:?} vs {left:?}"
            );
        }
    }

    /// A stronger signal must paint more of the fan, or the glyph is not
    /// actually communicating RSSI.
    #[test]
    fn a_stronger_signal_paints_more_of_the_wifi_fan() {
        use embedded_graphics::mock_display::MockDisplay;

        let painted = |level: u8| {
            let mut c: MockDisplay<Rgb565> = MockDisplay::new();
            c.set_allow_overdraw(true);
            draw_wifi_icon(&mut c, Point::new(0, 0), level, Rgb565::new(31, 31, 31)).unwrap();
            let area = c.affected_area();
            (area.size.width as u32) * (area.size.height as u32)
        };

        assert!(
            painted(1) < painted(2),
            "level 2 should be bigger than level 1"
        );
        assert!(
            painted(2) < painted(3),
            "level 3 should be bigger than level 2"
        );
        // Level 4 clamps to three arcs, the same as level 3.
        assert_eq!(painted(3), painted(4));
    }

    /// An offline radio must read as an antenna that is *down*, not as an icon with
    /// a line through it — a strike-through collides with the red "problem"
    /// colour and is easy to misread.
    #[test]
    fn an_offline_radio_draws_a_dropped_antenna() {
        use embedded_graphics::mock_display::MockDisplay;

        let glyph_y = |online: bool| {
            let mut c: MockDisplay<Rgb565> = MockDisplay::new();
            c.set_allow_overdraw(true);
            draw_lora_icon(&mut c, Point::new(0, 0), online, Rgb565::new(31, 31, 31)).unwrap();
            c.affected_area()
        };

        let up = glyph_y(true);
        let down = glyph_y(false);

        // Standing: the antenna element is at the top of the glyph.
        assert!(
            up.top_left.y < LORA_ICON_H / 3,
            "an online antenna should reach the top of the glyph, starts at {:?}",
            up.top_left
        );
        assert!(
            down.top_left.y > LORA_ICON_H / 2,
            "a retracted antenna should only occupy the bottom, starts at {:?}",
            down.top_left
        );

        // Both states share the ground line, so they read as one object.
        assert_eq!(up.bottom_right(), down.bottom_right());
    }

    /// A GNSS icon must actually change shape between searching and locked, not
    /// just colour: on a 15 px bar a red ring and a green ring are easy to
    /// confuse, and the user needs to know at a glance whether the clock is up.
    #[test]
    fn the_gnss_icon_distinguishes_searching_from_locked() {
        use embedded_graphics::mock_display::MockDisplay;

        let render = |locked: bool| {
            let mut c: MockDisplay<Rgb565> = MockDisplay::new();
            c.set_allow_overdraw(true);
            draw_gnss_icon(&mut c, Point::new(0, 0), locked, Rgb565::new(31, 31, 31)).unwrap();
            c
        };

        let white = Rgb565::new(31, 31, 31);
        let cx = GNSS_ICON_W / 2;

        // Take a pixel just inside the limb, in the middle row of the cap. The
        // limb is drawn in both states, so this is specifically what separates a
        // filled sphere from an outlined one — the property a glance reads.
        let inside = Point::new(cx, GNSS_ICON_H / 2);
        assert_eq!(
            render(true).get_pixel(inside),
            Some(white),
            "a locked GNSS must be a solid body, not just an outline"
        );
        assert_eq!(
            render(false).get_pixel(inside),
            None,
            "a searching GNSS must be outline only, or the two states look alike"
        );

        // The limb is drawn in both states, so "still looking" is never
        // mistaken for "broken" or "absent". Checked at the extreme left of the
        // widest row, which only the outline ever touches.
        for (state, locked) in [("searching", false), ("locked", true)] {
            assert_eq!(
                render(locked).get_pixel(Point::new(
                    cx - *GNSS_CAP_HALF.last().unwrap(),
                    GNSS_ICON_H - 1
                )),
                Some(white),
                "the {state} sphere must draw its limb at the edge of the cap"
            );
        }
    }

    /// The cap's width profile must be monotonically non-decreasing top to
    /// bottom. That is what makes the glyph read as the top of a sphere — a
    /// profile that narrows again, or jumps around, reads as a spire or a lump.
    #[test]
    fn the_gnss_cap_widens_towards_its_cut() {
        assert!(
            GNSS_CAP_HALF.windows(2).all(|w| w[0] <= w[1]),
            "cap profile {GNSS_CAP_HALF:?} must widen or hold, never narrow"
        );
        // And it must actually reach the box edge, or the sphere is floating
        // inside its own bounding box and the widest row is wasted space.
        assert_eq!(
            *GNSS_CAP_HALF.last().unwrap() * 2 + 1,
            GNSS_ICON_W,
            "the widest row must span the glyph's full width"
        );
    }

    /// The status bar must not shift when a GNSS fix lands. A fix can arrive at
    /// any moment, and sliding the other icons over to fill its place would
    /// move the WiFi/LoRa glyphs — and their interface underlines — mid-read.
    #[test]
    fn acquiring_a_gnss_fix_does_not_move_the_other_icons() {
        use embedded_graphics::mock_display::MockDisplay;

        // `MockDisplay` is 64×64, so render into that rather than the real 320-wide
        // panel; the icons are placed from the right edge, so every relative
        // position under test is the same.
        const W: i32 = 64;
        let bar = |gnss_locked: Option<bool>| {
            let theme = Theme::default();
            let mut c: MockDisplay<Rgb565> = MockDisplay::new();
            c.set_allow_overdraw(true);
            let net = NetworkState {
                wifi_connected: true,
                wifi_rssi: Some(-55),
                lora_online: Some(true),
                gnss_locked,
                ..Default::default()
            };
            draw_header(&mut c, W, "", &net, &theme).unwrap();
            c
        };

        // Both WiFi and LoRa are drawn; compare the same two rows in each render.
        // If the layout shifted, the glyph pixels would land in different
        // columns.
        let before = bar(Some(false));
        let after = bar(Some(true));

        // Everything from the WiFi icon rightwards. The GNSS icon is placed
        // left of `wifi_x`, so within this window the two renders must be
        // pixel-identical.
        let wifi_x = W - 2 - LORA_ICON_W - 3 - WIFI_ICON_W;
        let differs = |x: i32, y: i32| {
            let p = Point::new(x, y);
            before.get_pixel(p) != after.get_pixel(p)
        };

        let shifted = (0..Theme::default().header_h)
            .flat_map(|y| (wifi_x..W).map(move |x| (x, y)))
            .filter(|(x, y)| differs(*x, *y))
            .count();

        assert_eq!(
            shifted, 0,
            "the WiFi/LoRa icons moved when the GNSS icon appeared"
        );
    }

    /// The hop count sits on the same line as the key legend, so the legend must
    /// yield room for it. Rendered separately and compared, because both are
    /// the same colour and overlap would otherwise be invisible to the eye.
    #[test]
    fn the_footer_hint_and_hop_count_never_overlap() {
        let theme = Theme::default();

        // Every hint currently used on a screen with a footer, at every hop
        // count `hops_label` can produce, so a longer legend or a wider count
        // fails here rather than on the device.
        let hints = [
            "TYPE to search | ENTER browse | ALT+Backspace",
            "TYPE to search | ENTER open | ALT+Backspace",
            "filtering | ENTER browse | ALT+Backspace",
            "UP/DOWN scroll | ALT+Backspace",
        ];
        let counts: Vec<String> = (1..=24).map(|n| hops_label(Some(n))).collect();

        let (w, h) = (DISPLAY_W, DISPLAY_H);

        // Both labels are the same colour, so a collision is invisible to the eye.
        // Render the real bar, then subtract a render of the hint alone: what
        // is left is exactly the hop count, in the same render that has to fit.
        let cols = |hint: &str, right: &str| {
            let mut c = Painted::new(w, h);
            draw_footer(&mut c, w, h, hint, right, &theme).unwrap();
            c.glyph_cols(h - theme.line_h..h, &[theme.surface, theme.border])
        };

        for hint in hints {
            for count in &counts {
                let hint_alone = cols(hint, "");
                let both = cols(hint, count);
                let hops_only: Vec<i32> = both
                    .iter()
                    .copied()
                    .filter(|x| !hint_alone.contains(x))
                    .collect();

                let l_max = hint_alone.iter().max().copied();
                let r_min = hops_only.iter().min().copied();
                if let (Some(l_max), Some(r_min)) = (l_max, r_min) {
                    assert!(
                        l_max + 2 <= r_min,
                        "hint {hint:?} reaches column {l_max} and hop count {count:?} starts \
                         at {r_min}: they collide on a {w}px bar"
                    );
                }
            }
        }
    }

    /// The status bar has no right-hand label slot at all, so nothing drawn in
    /// the body — a hop count, a count, a title — can ever reach the status
    /// icons. This is the regression guard for the overlap that started it.
    #[test]
    fn the_status_bar_leaves_its_icon_strip_clear() {
        let theme = Theme::default();
        let mut c = Painted::new(DISPLAY_W, DISPLAY_H);
        let net = NetworkState {
            wifi_connected: true,
            wifi_rssi: Some(-55),
            lora_online: Some(true),
            gnss_locked: Some(true),
            ..Default::default()
        };
        draw_header(&mut c, DISPLAY_W, "RETICULA", &net, &theme).unwrap();

        // Between the end of the title and the left edge of the icon strip the
        // bar must be bare. This is the strip a right-hand label used to be
        // squeezed into, and the reason the slot was removed rather than
        // truncated.
        let [gnss, _, _] = glyph_boxes(DISPLAY_W, &theme);
        let title_w = "RETICULA".chars().count() as i32 * theme.char_w;
        let gaps = c.glyph_cols(0..theme.header_h - 1, &[theme.header, theme.border]);
        let intruders: Vec<i32> = gaps
            .into_iter()
            .filter(|x| *x > title_w && *x < gnss.top_left.x - 2)
            .collect();

        assert!(
            intruders.is_empty(),
            "the status bar has content at columns {intruders:?}, between the title (ends at \
             {title_w}) and the icons (start at {}): the icon strip is not clear",
            gnss.top_left.x
        );
    }

    /// A list must never run into the footer bar, whatever its length. This is
    /// the regression the settings menu hit when it gained its fifth entry.
    #[test]
    fn a_long_list_stays_clear_of_the_footer() {
        let theme = Theme::default();
        for count in 1..=12 {
            let row_h = menu_row_height(DISPLAY_H, &theme, count);
            let bottom = body_top(&theme) + row_h * count as i32;
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
        let available = DISPLAY_H - body_top(&theme) - theme.line_h;

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
        let bottom = body_top(&theme) + row_h * 5;

        assert_eq!(body_top(&theme), 25, "bar 15 + status line 10");
        assert_eq!(
            (row_h, bottom),
            (40, 225),
            "5 settings rows should be 40px each, ending at y=225 with the footer at 230"
        );
        assert!(
            bottom <= DISPLAY_H - theme.line_h,
            "the last row must stay clear of the footer"
        );
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
