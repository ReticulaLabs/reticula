//! ST7789 display wrapper.
//!
//! `mipidsi`'s `Display` already implements `DrawTarget`, so this crate just
//! wraps it in a local type that also implements [`reticula_hal::Display`].
//! Writes go straight to the SPI bus; there is no framebuffer, so `flush` is
//! a no-op.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use mipidsi::interface::Interface;
use mipidsi::models::Model;

/// Display wrapper over a `mipidsi::Display`.
pub struct TdeckScreen<DI, MODEL, RST> {
    inner: mipidsi::Display<DI, MODEL, RST>,
    size: Size,
}

impl<DI, MODEL, RST> TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
{
    /// Wrap a fully initialised `mipidsi::Display`.
    pub fn new(inner: mipidsi::Display<DI, MODEL, RST>, size: Size) -> Self {
        Self { inner, size }
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
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565> + Into<Rgb565>,
{
    type Color = Rgb565;
    type Error = DI::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.inner.draw_iter(pixels.map(|p| {
            let (pos, color) = p.into();
            Pixel(pos, color.into())
        }))
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        colors: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        self.inner
            .fill_contiguous(area, colors.into_iter().map(Into::into))
    }

    fn fill_solid(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        self.inner.fill_solid(area, color.into())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.inner.clear(color.into())
    }
}

impl<DI, MODEL, RST> OriginDimensions for TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
{
    fn size(&self) -> Size {
        self.size
    }
}

impl<DI, MODEL, RST> reticula_hal::Display for TdeckScreen<DI, MODEL, RST>
where
    DI: Interface,
    MODEL: Model,
    MODEL::ColorFormat: mipidsi::interface::InterfacePixelFormat<DI::Word>,
    MODEL::ColorFormat: From<Rgb565> + Into<Rgb565>,
{
    type Target = Self;

    fn size(&self) -> Size {
        self.size
    }

    fn target(&mut self) -> &mut Self::Target {
        self
    }

    fn flush(&mut self) {
        // mipidsi draws directly to the SPI bus; nothing to flush.
    }
}