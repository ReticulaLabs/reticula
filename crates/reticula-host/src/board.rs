//! The host board bundling display and keyboard.

use std::time::Instant;

use reticula_hal::Board;

use crate::display::HostDisplay;
use crate::keyboard::HostKeyboard;

/// A complete desktop simulator board.
pub struct HostBoard {
    display: HostDisplay,
    keyboard: HostKeyboard,
    started: Instant,
}

impl HostBoard {
    /// Create a simulator board sized like the T-Deck's LCD (320×240).
    pub fn new() -> Self {
        Self::with_size(320, 240)
    }

    /// Create a simulator board with a custom framebuffer size.
    pub fn with_size(width: u32, height: u32) -> Self {
        crate::display::enter_viewport();
        Self {
            display: HostDisplay::new(width, height),
            keyboard: HostKeyboard::new(),
            started: Instant::now(),
        }
    }
}

impl Drop for HostBoard {
    fn drop(&mut self) {
        crate::display::leave_viewport();
    }
}

impl Default for HostBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl Board for HostBoard {
    type Display = HostDisplay;
    type Keyboard = HostKeyboard;

    fn display(&mut self) -> Option<&mut Self::Display> {
        Some(&mut self.display)
    }

    fn keyboard(&mut self) -> &mut Self::Keyboard {
        &mut self.keyboard
    }

    fn delay_ms(&mut self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }

    fn uptime_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}