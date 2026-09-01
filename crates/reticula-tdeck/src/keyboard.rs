//! T-Deck keyboard: poll the ESP32-C3 keyboard controller over I²C.

use std::time::{Duration, Instant};

use esp_idf_hal::i2c::I2cDriver;

use reticula_hal::input::{KeyCode, KeyEvent, Keyboard};

use crate::KEYBOARD_I2C_ADDRESS;

/// The keyboard controller should not be polled more often than this.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Reads key presses from the T-Deck keyboard over I²C.
///
/// The keyboard firmware sends one ASCII byte per key press. Polling is
/// rate-limited so the ESP32-C3 controller is not overrun.
pub struct TdeckKeyboard<'a> {
    i2c: I2cDriver<'a>,
    last_poll: Instant,
}

impl<'a> TdeckKeyboard<'a> {
    pub fn new(i2c: I2cDriver<'a>) -> Self {
        Self { i2c, last_poll: Instant::now() - POLL_INTERVAL }
    }
}

impl Keyboard for TdeckKeyboard<'_> {
    fn pending(&mut self) -> usize {
        // The default keyboard firmware does not use the interrupt line, so
        // we cannot know without polling.
        0
    }

    fn read(&mut self, events: &mut [KeyEvent]) -> usize {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return 0;
        }
        self.last_poll = Instant::now();

        let mut n = 0;
        for slot in events.iter_mut() {
            let mut byte = [0u8; 1];
            match self.i2c.read(KEYBOARD_I2C_ADDRESS, &mut byte) {
                Ok(_) => {
                    if byte[0] == 0x00 {
                        break; // no more keys queued
                    }
                    if let Some(code) = map_byte(byte[0]) {
                        *slot = KeyEvent::pressed(code);
                        n += 1;
                    }
                }
                Err(_) => break, // bus error / keyboard not ready
            }
        }
        n
    }
}

/// Map a keyboard byte to a logical key.
fn map_byte(b: u8) -> Option<KeyCode> {
    match b {
        0x00 => None,
        0x08 => Some(KeyCode::Backspace),
        0x09 => Some(KeyCode::Tab),
        0x0d | 0x0a => Some(KeyCode::Enter),
        0x1b => Some(KeyCode::Esc),
        b' ' => Some(KeyCode::Space),
        b if b.is_ascii_graphic() => Some(KeyCode::Char(b as char)),
        b => Some(KeyCode::Unknown(b)),
    }
}