//! Trackball support (placeholder).
//!
//! The T-Deck trackball is an I²C sensor on the same bus as the keyboard.
//! Reading it is not wired into the application yet; this module documents
//! the interface so it can be added as a pointer device later.

/// A direction delta produced by the trackball.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackballEvent {
    Up,
    Down,
    Left,
    Right,
    Click,
}

/// Reads directional deltas from the T-Deck trackball (I²C address `0x02`).
///
/// The trackball reports key codes via the keyboard controller; the exact
/// register layout depends on the board revision. Reserved for future use.
pub struct Trackball;

impl Trackball {
    pub const I2C_ADDRESS: u8 = 0x02;
}