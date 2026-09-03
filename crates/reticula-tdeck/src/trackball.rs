//! T-Deck trackball: a four-directional ball with a click switch, read as GPIO.

use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{Gpio0, Gpio1, Gpio2, Gpio3, Gpio15, Input, PinDriver, Pull};

use reticula_hal::input::{KeyCode, KeyEvent, Keyboard};

/// Ignore further edges for this long after accepting one, debouncing the
/// mechanical contacts.
const DEBOUNCE: Duration = Duration::from_millis(60);

/// Reads the T-Deck trackball from its five GPIO lines.
///
/// The trackball is a mechanical ball: moving it in a direction, or pressing
/// the click switch, pulls the corresponding line low (the pins sit high via
/// internal pull-ups). The lines are polled for falling edges and converted
/// into the same logical keys the UI already understands:
///
/// * up → `Up`, down → `Down` (cursor movement)
/// * left → `Esc` (swipe left goes back a page)
/// * right → `Right`
/// * click → `Enter` (select)
pub struct TdeckTrackball {
    up: PinDriver<'static, Gpio3, Input>,
    down: PinDriver<'static, Gpio15, Input>,
    left: PinDriver<'static, Gpio1, Input>,
    right: PinDriver<'static, Gpio2, Input>,
    click: PinDriver<'static, Gpio0, Input>,
    prev_up: bool,
    prev_down: bool,
    prev_left: bool,
    prev_right: bool,
    prev_click: bool,
    last_event: Instant,
}

impl TdeckTrackball {
    /// Initialise the trackball from its five GPIO pins.
    pub fn new(
        up: Gpio3,
        down: Gpio15,
        left: Gpio1,
        right: Gpio2,
        click: Gpio0,
    ) -> Result<Self, esp_idf_sys::EspError> {
        let mut up = PinDriver::input(up)?;
        up.set_pull(Pull::Up)?;
        let mut down = PinDriver::input(down)?;
        down.set_pull(Pull::Up)?;
        let mut left = PinDriver::input(left)?;
        left.set_pull(Pull::Up)?;
        let mut right = PinDriver::input(right)?;
        right.set_pull(Pull::Up)?;
        let mut click = PinDriver::input(click)?;
        click.set_pull(Pull::Up)?;

        Ok(Self {
            up,
            down,
            left,
            right,
            click,
            prev_up: true,
            prev_down: true,
            prev_left: true,
            prev_right: true,
            prev_click: true,
            last_event: Instant::now(),
        })
    }
}

impl Keyboard for TdeckTrackball {
    fn pending(&mut self) -> usize {
        0
    }

    fn read(&mut self, events: &mut [KeyEvent]) -> usize {
        let up = self.up.is_low();
        let down = self.down.is_low();
        let left = self.left.is_low();
        let right = self.right.is_low();
        let click = self.click.is_low();

        let mut n = 0;
        if self.last_event.elapsed() >= DEBOUNCE {
            let mut push = |code: KeyCode| {
                if n < events.len() {
                    events[n] = KeyEvent::pressed(code);
                    n += 1;
                }
            };
            // Falling edges only: each movement/click produces one key press.
            if up && !self.prev_up {
                push(KeyCode::Up);
            }
            if down && !self.prev_down {
                push(KeyCode::Down);
            }
            if right && !self.prev_right {
                push(KeyCode::Right);
            }
            if left && !self.prev_left {
                push(KeyCode::Esc);
            }
            if click && !self.prev_click {
                push(KeyCode::Enter);
            }
            if n > 0 {
                self.last_event = Instant::now();
            }
        }

        self.prev_up = up;
        self.prev_down = down;
        self.prev_left = left;
        self.prev_right = right;
        self.prev_click = click;
        n
    }
}