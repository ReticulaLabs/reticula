//! ST7789 display wrapper.
//!
//! `mipidsi`'s `Display` draws directly to the SPI bus, so redrawing a static
//! screen every frame wipes the panel with the background colour before the
//! widgets are drawn again — visible as a constant blink. This wrapper renders
//! into an offscreen RGB565 framebuffer instead, and only pushes the frame to
//! the panel (`flush`) when its content actually changed. Rendering into RAM is
//! cheap; the SPI transfer is the expensive part and is skipped when the
//! screen is unchanged.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use mipidsi::interface::Interface;
use mipidsi::models::Model;

/// Display wrapper over a `mipidsi::Display`, backed by an offscreen
/// framebuffer.
pub struct TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    RST: embedded_hal::digital::OutputPin,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
{
    inner: mipidsi::Display<DI, MODEL, RST>,
    size: Size,
    framebuffer: Vec<Rgb565>,
    flushed: Vec<Rgb565>,
}

impl<DI, MODEL, RST> TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    RST: embedded_hal::digital::OutputPin,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565>,
{
    /// Wrap a fully initialised `mipidsi::Display`.
    pub fn new(inner: mipidsi::Display<DI, MODEL, RST>, size: Size) -> Self {
        let pixels = (size.width * size.height) as usize;
        Self {
            inner,
            size,
            framebuffer: vec![Rgb565::new(0, 0, 0); pixels],
            flushed: vec![Rgb565::new(0, 0, 0); pixels],
        }
    }

    /// Access the underlying driver (e.g. to set orientation).
    pub fn inner(&mut self) -> &mut mipidsi::Display<DI, MODEL, RST> {
        &mut self.inner
    }
}

impl<DI, MODEL, RST> DrawTarget for TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    RST: embedded_hal::digital::OutputPin,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565>,
{
    type Color = Rgb565;
    type Error = DI::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let (w, h) = (self.size.width as i32, self.size.height as i32);
        for Pixel(pos, color) in pixels {
            if pos.x >= 0 && pos.y >= 0 && pos.x < w && pos.y < h {
                let idx = (pos.y as usize) * (w as usize) + pos.x as usize;
                self.framebuffer[idx] = color;
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &Rectangle,
        colors: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let (w, h) = (self.size.width as i32, self.size.height as i32);
        let x0 = area.top_left.x.max(0);
        let y0 = area.top_left.y.max(0);
        let width = area.size.width.max(1) as i32;
        for (i, color) in colors.into_iter().enumerate() {
            let i = i as i32;
            let (x, y) = (x0 + i % width, y0 + i / width);
            if x >= 0 && y >= 0 && x < w && y < h {
                let idx = (y as usize) * (w as usize) + x as usize;
                self.framebuffer[idx] = color;
            }
        }
        Ok(())
    }

    fn fill_solid(
        &mut self,
        area: &Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let (w, h) = (self.size.width as i32, self.size.height as i32);
        let x0 = area.top_left.x.max(0);
        let y0 = area.top_left.y.max(0);
        let x1 = (area.top_left.x + area.size.width as i32).min(w);
        let y1 = (area.top_left.y + area.size.height as i32).min(h);
        for y in y0..y1 {
            let row = (y as usize) * (w as usize);
            for x in x0..x1 {
                self.framebuffer[row + x as usize] = color;
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.framebuffer.fill(color);
        Ok(())
    }
}

impl<DI, MODEL, RST> OriginDimensions for TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    RST: embedded_hal::digital::OutputPin,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565>,
{
    fn size(&self) -> Size {
        self.size
    }
}

impl<DI, MODEL, RST> reticula_hal::Display for TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    RST: embedded_hal::digital::OutputPin,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565>,
{
    type Target = Self;

    fn size(&self) -> Size {
        self.size
    }

    fn target(&mut self) -> &mut Self::Target {
        self
    }

    fn flush(&mut self) {
        // Skip the SPI transfer when the frame is unchanged: this is what
        // stops the constant clear-and-redraw flicker on a static screen.
        if self.framebuffer == self.flushed {
            return;
        }
        let area = Rectangle::new(Point::new(0, 0), self.size);
        let colors = self.framebuffer.iter().copied().map(Into::into);
        if self.inner.fill_contiguous(&area, colors).is_ok() {
            self.flushed.copy_from_slice(&self.framebuffer);
        }
    }
}