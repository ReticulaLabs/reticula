use crate::display::Display;
use crate::input::Keyboard;
use alloc::string::String;

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

    /// A secondary pointer/trackball input device, if the board has one.
    ///
    /// Reports the same logical key events as the keyboard (arrows, enter,
    /// escape). Most boards do not have one and return `None`.
    fn trackball(&mut self) -> Option<&mut dyn Keyboard> {
        None
    }

    /// WiFi link status: `(connected, rssi_dbm)`.
    ///
    /// Returns `None` when the board has no WiFi (e.g. the desktop simulator).
    fn wifi_status(&self) -> Option<(bool, i8)> {
        None
    }

    /// The WiFi network SSID currently configured, if the board has WiFi.
    fn wifi_ssid(&self) -> Option<String> {
        None
    }

    /// Block for `ms` milliseconds.
    fn delay_ms(&mut self, ms: u32);

    /// Milliseconds since the board was powered on.
    fn uptime_ms(&self) -> u64;
}