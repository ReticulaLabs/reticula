use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Size;
use embedded_graphics::pixelcolor::Rgb565;

/// A drawable display.
///
/// BSPs implement this for their concrete display hardware. The associated
/// [`Self::Target`] type is the `embedded-graphics` draw target that the UI
/// renders into. Because [`DrawTarget`] is not object-safe, everything that
/// wants to render takes the target type as a generic parameter; the app is
/// compiled once per target with the concrete display type.
pub trait Display {
    /// The underlying `embedded-graphics` draw target.
    ///
    /// All drawing operations use `Rgb565`; colour-depth conversion is the
    /// BSP's responsibility (e.g. via [`DrawTarget`] adapters).
    type Target: DrawTarget<Color = Rgb565>;

    /// Physical size of the display in pixels.
    fn size(&self) -> Size;

    /// Returns the draw target for this frame's rendering.
    fn target(&mut self) -> &mut Self::Target;

    /// Push the rendered framebuffer to the physical display.
    ///
    /// This is a no-op on displays that are already visible in real-time.
    fn flush(&mut self);

    /// Fill the entire display with `color`.
    fn clear(&mut self, color: Rgb565) {
        self.target().clear(color).ok();
    }
}

/// Blanket implementation for any type that directly is a draw target and
/// can flush itself, which keeps BSPs that use a raw framebuffer simple.
impl<T> Display for T
where
    T: DrawTarget<Color = Rgb565> + DisplayFlush,
{
    type Target = T;

    fn size(&self) -> Size {
        self.bounding_box().size
    }

    fn target(&mut self) -> &mut Self::Target {
        self
    }

    fn flush(&mut self) {
        self.flush_display();
    }
}

/// Helper trait for the blanket [`Display`] implementation: anything that can
/// flush its own framebuffer.
pub trait DisplayFlush {
    fn flush_display(&mut self);
}