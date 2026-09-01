//! A framebuffer display that renders to the terminal as readable text.
//!
//! The UI draws a 6×10 monospace font (`FONT_6X10`). Each output cell of the
//! terminal corresponds to exactly one 6×10 font cell, and the renderer
//! *reconstructs the actual glyph* by matching the cell's pixel pattern
//! against the font's atlas. Text therefore appears as real, readable
//! characters in the terminal instead of ASCII noise.
//!
//! Cells that do not match any glyph (solid fills, scrollbars, ...) fall back
//! to a luminance-mapped block character. Every cell is coloured with ANSI
//! 256-colour so the whole screen still reads as a coherent image.

use core::convert::Infallible;
use std::sync::OnceLock;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Point, Size};
use embedded_graphics::image::Image;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::{BinaryColor, Rgb565};
use embedded_graphics::prelude::*;

use reticula_hal::display::DisplayFlush;

/// A 320×240 RGB565 framebuffer that renders to the terminal on flush.
pub struct HostDisplay {
    width: u32,
    height: u32,
    buf: Vec<Rgb565>,
}

impl HostDisplay {
    pub fn new(width: u32, height: u32) -> Self {
        let buf = vec![Rgb565::BLACK; (width * height) as usize];
        Self { width, height, buf }
    }

    fn put(&mut self, x: i32, y: i32, color: Rgb565) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = (y as u32 * self.width + x as u32) as usize;
        if let Some(pixel) = self.buf.get_mut(idx) {
            *pixel = color;
        }
    }

    /// Render the framebuffer to the terminal as one ANSI cell per font cell.
    ///
    /// The frame is written at *absolute* row/column positions (relative to
    /// the top of the alternate screen buffer), and clipped to the actual
    /// terminal size so it never overflows and scrolls. Each flush overwrites
    /// the previous frame in place, so content stays anchored.
    pub fn render_cells(&self) -> String {
        let cols = (self.width / 6).max(1);
        let rows = (self.height / 10).max(1);

        // Clip to the real terminal viewport: never draw below the last row
        // or past the last column, otherwise the terminal scrolls the frame
        // and content drifts.
        let (term_rows, term_cols) = terminal_size();
        let rows = rows.min(term_rows as u32);
        let cols = cols.min((term_cols as u32).max(1));

        let mut out = String::with_capacity((cols * rows) as usize * 8);
        out.push_str("\x1b[?25l"); // hide cursor

        for cy in 0..rows {
            // Move to the absolute start of this row (1-based).
            out.push_str(&format!("\x1b[{};1H", cy + 1));
            for cx in 0..cols {
                let (ch, color) = self.render_cell(cx, cy);
                out.push_str(&format!("\x1b[38;5;{}m{}", color, ch));
            }
            out.push_str("\x1b[0m");
        }
        out
    }

    /// Reconstruct one 6×10 cell as a character plus its ANSI colour.
    fn render_cell(&self, cx: u32, cy: u32) -> (char, u8) {
        const N: usize = 60; // 6 × 10 pixels

        let x0 = cx * 6;
        let y0 = cy * 10;
        let mut lums = [0u8; N];
        let mut sum = 0u32;

        for (i, l) in lums.iter_mut().enumerate() {
            let x = x0 + (i % 6) as u32;
            let y = y0 + (i / 6) as u32;
            let c = self.buf[(y * self.width + x) as usize];
            let lv = lum8(c);
            *l = lv;
            sum += lv as u32;
        }
        let avg = (sum / N as u32) as u8;

        // Ink is anything brighter than the cell average. Accumulate average
        // colours of the ink (foreground) and background classes.
        let mut bits = 0u64;
        let (mut ir, mut ig, mut ib, mut in_) = (0u32, 0u32, 0u32, 0u32);
        let (mut br, mut bg, mut bb, mut bn) = (0u32, 0u32, 0u32, 0u32);

        for (i, &lv) in lums.iter().enumerate() {
            let x = x0 + (i % 6) as u32;
            let y = y0 + (i / 6) as u32;
            let c = self.buf[(y * self.width + x) as usize];
            let r = c.r() as u32 * 255 / 31;
            let g = c.g() as u32 * 255 / 63;
            let b = c.b() as u32 * 255 / 31;
            if lv > avg {
                bits |= 1u64 << i;
                ir += r;
                ig += g;
                ib += b;
                in_ += 1;
            } else {
                br += r;
                bg += g;
                bb += b;
                bn += 1;
            }
        }

        // Solid (single colour) cells: blank space / filled block.
        if in_ == 0 {
            return (' ', avg_ansi(br, bg, bb, bn));
        }

        // Exact glyph match (normal text).
        for g in glyphs() {
            if g.bits == bits {
                return (g.ch, avg_ansi(ir, ig, ib, in_));
            }
        }

        // Inverted glyph (e.g. light text on a dark highlight).
        let inv = !bits & CELL_MASK;
        for g in glyphs() {
            if g.bits == inv {
                return (g.ch, avg_ansi(br, bg, bb, bn));
            }
        }

        // Fallback: luminance-mapped block character.
        let ramp: &[u8] = b" .:-=+*#%@";
        let idx = (avg as usize * ramp.len()) / 256;
        (ramp[idx.clamp(0, ramp.len() - 1)] as char, avg_ansi(br, bg, bb, bn))
    }
}

const CELL_MASK: u64 = (1 << 60) - 1;

/// 8-bit luminance of an RGB565 colour.
fn lum8(c: Rgb565) -> u8 {
    ((c.r() as u16 * 255 / 31 + c.g() as u16 * 255 / 63 + c.b() as u16 * 255 / 31) / 3) as u8
}

fn avg_ansi(r: u32, g: u32, b: u32, n: u32) -> u8 {
    let n = n.max(1);
    ansi256((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

/// Approximate an RGB colour with the ANSI 256-colour palette.
fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    let idx = |v: u8| ((v as u16 * 5) / 255) as u8;
    16 + 36 * idx(r) + 6 * idx(g) + idx(b)
}

/// One glyph of the 6×10 font, as a 60-bit pattern (row-major).
struct Glyph {
    ch: char,
    bits: u64,
}

static GLYPHS: OnceLock<Vec<Glyph>> = OnceLock::new();

/// The ASCII glyph patterns of `FONT_6X10`, extracted once by drawing the
/// font atlas into a bitmap.
fn glyphs() -> &'static [Glyph] {
    GLYPHS.get_or_init(|| {
        let font = &FONT_6X10;
        let cw = font.character_size.width;
        let ch = font.character_size.height;
        let glyphs_per_row = (font.image.size().width / cw) as usize;

        // Draw the whole atlas into a 1-bpp bitmap (public API only).
        let mut atlas = BitMap {
            w: font.image.size().width,
            h: font.image.size().height,
            bits: vec![0u8; (font.image.size().width * font.image.size().height / 8) as usize],
        };
        Image::new(&font.image, Point::new(0, 0)).draw(&mut atlas).unwrap();

        let mut out = Vec::with_capacity(95);
        for code in 0x20u8..=0x7e {
            let c = code as char;
            let gi = font.glyph_mapping.index(c);
            let gx = (gi % glyphs_per_row) as u32 * cw;
            let gy = (gi / glyphs_per_row) as u32 * ch;
            let mut bits = 0u64;
            for y in 0..ch {
                for x in 0..cw {
                    if atlas.bit(gx + x, gy + y) {
                        bits |= 1u64 << (y * cw + x) as usize;
                    }
                }
            }
            out.push(Glyph { ch: c, bits });
        }
        out
    })
}

/// A tiny 1-bit-per-pixel draw target used to read the font atlas.
struct BitMap {
    w: u32,
    h: u32,
    bits: Vec<u8>,
}

impl BitMap {
    fn bit(&self, x: u32, y: u32) -> bool {
        let i = (y * self.w + x) as usize;
        (self.bits[i >> 3] >> (7 - (i & 7))) & 1 == 1
    }
}

impl DrawTarget for BitMap {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as u32) < self.w && (p.y as u32) < self.h {
                let i = (p.y as u32 * self.w + p.x as u32) as usize;
                let bit = 1u8 << (7 - (i & 7));
                if c == BinaryColor::On {
                    self.bits[i >> 3] |= bit;
                } else {
                    self.bits[i >> 3] &= !bit;
                }
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.bits.fill(if color == BinaryColor::On { 0xff } else { 0 });
        Ok(())
    }
}

impl OriginDimensions for BitMap {
    fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }
}

impl DrawTarget for HostDisplay {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            self.put(coord.x, coord.y, color);
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        for pixel in self.buf.iter_mut() {
            *pixel = color;
        }
        Ok(())
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        colors: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        // The default fill_solid passes an infinite iterator, so iterate the
        // area's points and pull one colour per point.
        for (point, color) in area.points().zip(colors) {
            self.put(point.x, point.y, color);
        }
        Ok(())
    }
}

impl OriginDimensions for HostDisplay {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DisplayFlush for HostDisplay {
    fn flush_display(&mut self) {
        let art = self.render_cells();
        use std::io::Write;
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(art.as_bytes());
        let _ = stdout.flush();
    }
}

/// Enter the terminal alternate screen buffer for a stable, non-scrolling
/// viewport the size of the terminal window.
pub fn enter_viewport() {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x1b[?1049h\x1b[?25l");
    let _ = stdout.flush();
}

/// Leave the alternate screen buffer and restore the previous terminal state.
pub fn leave_viewport() {
    use std::io::Write;
    let _ = crossterm::terminal::disable_raw_mode();
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x1b[?25h\x1b[0m\x1b[?1049l");
    let _ = stdout.flush();
}

/// The current terminal size in rows and columns, with sensible fallbacks.
///
/// Querying the size may fail (e.g. when stdout is not a TTY), so we fall
/// back to a size that comfortably fits a 320×240 viewport.
fn terminal_size() -> (usize, usize) {
    use crossterm::terminal::size;
    match size() {
        Ok((cols, rows)) => (rows as usize, cols as usize),
        Err(_) => (40, 120),
    }
}