//! Colour theme and typography for the UI.

use embedded_graphics::mono_font::MonoFont;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;

/// The monospace font used throughout the UI. 6×10 px per glyph.
pub const FONT: &MonoFont<'static> = &FONT_6X10;
/// Width of one glyph in pixels.
pub const CHAR_W: i32 = 6;
/// Height of one line of glyphs in pixels.
pub const LINE_H: i32 = 10;

/// Standard 320×240 display (the T-Deck LCD in landscape).
pub const DISPLAY_W: i32 = 320;
pub const DISPLAY_H: i32 = 240;

/// Colour palette and font used by all screens.
#[derive(Debug, Clone)]
pub struct Theme {
    pub background: Rgb565,
    pub surface: Rgb565,
    pub header: Rgb565,
    pub header_text: Rgb565,
    pub primary: Rgb565,
    pub accent: Rgb565,
    pub text: Rgb565,
    pub text_dim: Rgb565,
    pub selection: Rgb565,
    pub selection_text: Rgb565,
    pub border: Rgb565,
    pub danger: Rgb565,
    pub ok: Rgb565,
    pub font: &'static MonoFont<'static>,
    pub char_w: i32,
    pub line_h: i32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Near-black blue background for comfortable contrast on the LCD.
            background: Rgb565::new(4, 5, 7),
            surface: Rgb565::new(12, 14, 18),
            header: Rgb565::new(7, 13, 19),
            header_text: Rgb565::new(23, 27, 31),
            primary: Rgb565::new(0, 24, 31),
            accent: Rgb565::new(0, 31, 31),
            text: Rgb565::new(29, 31, 31),
            text_dim: Rgb565::new(21, 25, 28),
            selection: Rgb565::new(0, 30, 38),
            selection_text: Rgb565::new(31, 31, 31),
            border: Rgb565::new(19, 23, 27),
            danger: Rgb565::new(31, 14, 14),
            ok: Rgb565::new(12, 31, 14),
            font: FONT,
            char_w: CHAR_W,
            line_h: LINE_H,
        }
    }
}

impl Theme {
    /// How many characters fit on a line of the given width.
    pub fn chars_per_line(&self, width_px: i32) -> usize {
        (width_px / self.char_w).max(1) as usize
    }

    /// How many lines of text fit in the given height.
    pub fn lines_fit(&self, height_px: i32) -> usize {
        (height_px / self.line_h).max(1) as usize
    }
}