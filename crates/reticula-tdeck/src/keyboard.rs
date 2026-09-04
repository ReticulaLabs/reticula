//! T-Deck keyboard: poll the ESP32-C3 keyboard controller over I²C in raw
//! matrix mode.
//!
//! The default key mode reports one ASCII byte per key and silently swallows
//! modifiers (Alt, Shift, Symbol) — so Alt can never be seen. The controller
//! supports a raw mode that reports the full 5×7 matrix, exposing every key.

use std::time::{Duration, Instant};

use esp_idf_hal::i2c::I2cDriver;
use log::debug;

use reticula_hal::input::{KeyCode, KeyEvent, Keyboard, KeyState};

use crate::KEYBOARD_I2C_ADDRESS;

/// The keyboard controller should not be polled more often than this.
const POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Command that switches the controller into raw matrix mode.
const KBD_MODE_RAW: u8 = 0x03;
/// Number of matrix columns (5), each byte a 7-row bitmask.
const MATRIX_COLS: usize = 5;

/// Base (unshifted) keycap per matrix position; `0` = not printable.
const BASE: [[u8; MATRIX_COLS]; 7] = [
    [b'q', b'e', b'r', b'u', b'o'],
    [b'w', b's', b'g', b'h', b'l'],
    [0, b'd', b't', b'y', b'i'],
    [b'a', b'p', 0, 0, 0],
    [0, b'x', b'v', b'b', b'$'],
    [b' ', b'z', b'c', b'n', b'm'],
    [0, 0, b'f', b'j', b'k'],
];
/// Symbol-layer keycap per matrix position; `0` = not printable.
const SYM: [[u8; MATRIX_COLS]; 7] = [
    [b'#', b'2', b'3', b'_', b'+'],
    [b'1', b'4', b'/', b':', b'"'],
    [0, b'5', b'(', b')', b'-'],
    [b'*', b'@', 0, 0, 0],
    [0, b'8', b'?', b'!', 0],
    [0, b'7', b'9', b',', b'.'],
    [b'0', 0, b'6', b';', b'\''],
];

/// Reads the raw keyboard matrix from the T-Deck keyboard over I²C.
pub struct TdeckKeyboard<'a> {
    i2c: I2cDriver<'a>,
    last_poll: Instant,
    /// Last raw matrix (column bitmasks), for edge detection.
    prev: [u8; MATRIX_COLS],
    initialized: bool,
    /// Held modifier state, tracked between polls.
    alt_down: bool,
    sym_down: bool,
    shift_down: bool,
}

impl<'a> TdeckKeyboard<'a> {
    pub fn new(mut i2c: I2cDriver<'a>) -> Self {
        // Switch the controller into raw matrix mode. The keyboard is
        // initialised well after power-on (the board powers it up and drives
        // the LCD first), so it is ready to accept commands.
        let _ = i2c.write(KEYBOARD_I2C_ADDRESS, &[KBD_MODE_RAW], 100);
        Self {
            i2c,
            last_poll: Instant::now() - POLL_INTERVAL,
            prev: [0; MATRIX_COLS],
            initialized: false,
            alt_down: false,
            sym_down: false,
            shift_down: false,
        }
    }
}

impl Keyboard for TdeckKeyboard<'_> {
    fn pending(&mut self) -> usize {
        // The keyboard has no interrupt line; we always poll.
        0
    }

    fn read(&mut self, events: &mut [KeyEvent]) -> usize {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return 0;
        }
        self.last_poll = Instant::now();

        let mut matrix = [0u8; MATRIX_COLS];
        if self.i2c.read(KEYBOARD_I2C_ADDRESS, &mut matrix, 100).is_err() {
            return 0;
        }

        if !self.initialized {
            // Seed the edge detectors without emitting anything.
            self.initialized = true;
            self.prev = matrix;
            self.alt_down = bit(&matrix, 0, 4);
            self.sym_down = bit(&matrix, 0, 2);
            self.shift_down = bit(&matrix, 1, 6) || bit(&matrix, 2, 3);
            return 0;
        }

        let sym_down = bit(&matrix, 0, 2);
        let alt_down = bit(&matrix, 0, 4);
        let shift_down = bit(&matrix, 1, 6) || bit(&matrix, 2, 3);

        let mut n = 0;
        let mut emit = |code: KeyCode, state: KeyState| {
            if n < events.len() {
                events[n] = KeyEvent { code, state };
                n += 1;
            }
        };

        // Alt press/release edge first, so the application sees the modifier
        // held before any key pressed while it is down.
        if alt_down != self.alt_down {
            let state = if alt_down {
                KeyState::Pressed
            } else {
                KeyState::Released
            };
            emit(KeyCode::Alt, state);
        }
        self.alt_down = alt_down;

        // Rising edges for every key position.
        for row in 0..7 {
            for col in 0..MATRIX_COLS {
                if bit(&matrix, col, row) && !bit(&self.prev, col, row) {
                    if let Some(code) = key_for(col, row, sym_down, shift_down) {
                        debug!("kbd: matrix ({col},{row}) -> {code:?}");
                        emit(code, KeyState::Pressed);
                    }
                }
            }
        }

        self.prev = matrix;
        self.sym_down = sym_down;
        self.shift_down = shift_down;
        n
    }
}

/// Read one bit of the raw matrix (column bitmask, row bit).
fn bit(matrix: &[u8; MATRIX_COLS], col: usize, row: usize) -> bool {
    (matrix[col] >> row) & 1 == 1
}

/// Map a pressed matrix position (plus the held Symbol/Shift layers) to a key.
fn key_for(col: usize, row: usize, sym_down: bool, shift_down: bool) -> Option<KeyCode> {
    match (col, row) {
        (3, 3) => return Some(KeyCode::Enter),
        (4, 3) => return Some(KeyCode::Backspace),
        (0, 5) => return Some(KeyCode::Space),
        // Non-output keys: Alt, Symbol, Mic, Left/Right Shift.
        (0, 4) | (0, 2) | (0, 6) | (1, 6) | (2, 3) => return None,
        _ => {}
    }
    let base = BASE[row][col];
    let sym = SYM[row][col];
    let mut c = if sym_down { sym } else { base };
    if c == 0 {
        c = base;
    }
    if c == 0 {
        return None;
    }
    if shift_down && c.is_ascii_lowercase() {
        c = c.to_ascii_uppercase();
    }
    Some(KeyCode::Char(c as char))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_printable_keys() {
        assert_eq!(key_for(0, 0, false, false), Some(KeyCode::Char('q')));
        assert_eq!(key_for(4, 6, false, false), Some(KeyCode::Char('k')));
        // Symbol layer: sym held gives the symbol, else the base letter.
        assert_eq!(key_for(0, 1, false, false), Some(KeyCode::Char('w')));
        assert_eq!(key_for(0, 1, true, false), Some(KeyCode::Char('1')));
        // Shift uppercases letters only.
        assert_eq!(key_for(0, 0, false, true), Some(KeyCode::Char('Q')));
    }

    #[test]
    fn special_keys() {
        assert_eq!(key_for(3, 3, false, false), Some(KeyCode::Enter));
        assert_eq!(key_for(4, 3, false, false), Some(KeyCode::Backspace));
        assert_eq!(key_for(0, 5, false, false), Some(KeyCode::Space));
        assert_eq!(key_for(0, 4, false, false), None); // Alt: handled separately
        assert_eq!(key_for(0, 2, false, false), None); // Symbol layer key
    }
}