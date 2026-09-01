use crate::display::Display;
use crate::input::Keyboard;

/// A complete device board.
///
/// Implemented by each BSP (host simulator, T-Deck, ...). This is the single
/// entry point the application uses to talk to a device's hardware, keeping
/// the app itself fully portable.
pub trait Board {
    /// The display attached to this board.
    type Display: Display;
    /// The keyboard / input device attached to this board.
    type Keyboard: Keyboard;

    /// The display, if the board has one.
    fn display(&mut self) -> Option<&mut Self::Display>;

    /// The keyboard / input device.
    fn keyboard(&mut self) -> &mut Self::Keyboard;

    /// Block for `ms` milliseconds.
    fn delay_ms(&mut self, ms: u32);

    /// Milliseconds since the board was powered on.
    fn uptime_ms(&self) -> u64;
}