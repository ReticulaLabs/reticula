/// Logical keys, independent of any physical keyboard layout or transport.
///
/// BSPs map their raw input (I2C bytes, terminal escape sequences, matrix
/// scan codes, ...) onto this enum, so the UI never has to know about
/// hardware specifics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A printable ASCII character.
    Char(char),
    /// Return / Enter.
    Enter,
    /// Horizontal tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete / forward-erase.
    Delete,
    /// Escape.
    Esc,
    /// D-Pad / arrow keys.
    Up,
    Down,
    Left,
    Right,
    /// Navigational keys.
    Home,
    End,
    PageUp,
    PageDown,
    /// Function keys.
    F(u8),
    /// Modifier keys (reported as press/release where the hardware supports it).
    Shift,
    Alt,
    Ctrl,
    Fn,
    /// Symbol/alt-graph key found on several handheld keyboards.
    Symbol,
    /// Space bar.
    Space,
    /// Application menu key.
    Menu,
    /// Any key we did not recognise.
    Unknown(u8),
}

impl KeyCode {
    /// Returns true if this key produces text when pressed.
    pub fn is_text(&self) -> bool {
        matches!(self, KeyCode::Char(_) | KeyCode::Space)
    }

    /// Returns the ASCII byte this key represents, if any.
    pub fn as_byte(&self) -> Option<u8> {
        match self {
            KeyCode::Char(c) => {
                let b = *c as u32;
                if b <= 0x7f {
                    Some(b as u8)
                } else {
                    None
                }
            }
            KeyCode::Space => Some(b' '),
            KeyCode::Enter => Some(b'\n'),
            KeyCode::Backspace => Some(8),
            KeyCode::Esc => Some(27),
            KeyCode::Tab => Some(b'\t'),
            _ => None,
        }
    }
}

impl From<char> for KeyCode {
    fn from(c: char) -> Self {
        match c {
            '\n' | '\r' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            '\u{8}' | '\u{7f}' => KeyCode::Backspace,
            ' ' => KeyCode::Space,
            '\u{1b}' => KeyCode::Esc,
            c if c.is_ascii() => KeyCode::Char(c),
            _ => KeyCode::Unknown(0),
        }
    }
}

/// Press or release state of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// A single key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub state: KeyState,
}

impl KeyEvent {
    pub fn pressed(code: KeyCode) -> Self {
        Self { code, state: KeyState::Pressed }
    }

    pub fn released(code: KeyCode) -> Self {
        Self { code, state: KeyState::Released }
    }
}

/// A source of [`KeyEvent`]s.
///
/// This is a polling interface so it works both in a desktop simulator (a
/// thread drains stdin into a queue) and on microcontrollers (an I2C poll
/// loop). `read` is non-blocking and returns how many events were written.
pub trait Keyboard {
    /// Number of events currently queued without blocking.
    fn pending(&mut self) -> usize;

    /// Drain up to `events.len()` queued key events into `events`.
    ///
    /// Returns the number of events written. Never blocks.
    fn read(&mut self, events: &mut [KeyEvent]) -> usize;
}