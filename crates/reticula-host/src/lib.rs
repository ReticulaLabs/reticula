//! `reticula-host` — desktop simulator board.
//!
//! This BSP renders the UI to a terminal window as colour ASCII art and reads
//! the real keyboard, so the whole application can be developed and tested on
//! a desktop machine with no hardware at all. It implements the exact same
//! [`reticula_hal::Board`] traits as a real device.

pub mod board;
pub mod display;
pub mod keyboard;

pub use board::HostBoard;
pub use display::HostDisplay;
pub use keyboard::HostKeyboard;