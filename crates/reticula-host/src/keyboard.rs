//! Keyboard reading for the host simulator.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crossterm::event::{self, Event, KeyCode as CTKeyCode, KeyModifiers};

use reticula_hal::input::{KeyCode, KeyEvent, Keyboard};

/// A keyboard fed by a background thread reading `crossterm` events.
pub struct HostKeyboard {
    queue: Arc<Mutex<VecDeque<KeyEvent>>>,
}

impl HostKeyboard {
    pub fn new() -> Self {
        let queue = Arc::new(Mutex::new(VecDeque::new()));

        // Raw mode so key presses arrive without echo/line buffering, and the
        // terminal is restored even on panic.
        let _ = crossterm::terminal::enable_raw_mode();
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = crossterm::terminal::disable_raw_mode();
            prev_hook(info);
        }));

        let thread_queue = queue.clone();
        std::thread::spawn(move || {
            loop {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        if key.kind == event::KeyEventKind::Press {
                            let code = map_key(key);
                            let ev = KeyEvent::pressed(code);
                            if let Ok(mut q) = thread_queue.lock() {
                                q.push_back(ev);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Self { queue }
    }
}

impl Default for HostKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Keyboard for HostKeyboard {
    fn pending(&mut self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or(0)
    }

    fn read(&mut self, events: &mut [KeyEvent]) -> usize {
        let Ok(mut q) = self.queue.lock() else {
            return 0;
        };
        let mut n = 0;
        for slot in events.iter_mut() {
            match q.pop_front() {
                Some(ev) => {
                    *slot = ev;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }
}

fn map_key(key: event::KeyEvent) -> KeyCode {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        CTKeyCode::Char(c) => {
            if ctrl {
                KeyCode::Unknown(c as u8)
            } else {
                KeyCode::from(c)
            }
        }
        CTKeyCode::Enter => KeyCode::Enter,
        CTKeyCode::Tab => KeyCode::Tab,
        CTKeyCode::Backspace => KeyCode::Backspace,
        CTKeyCode::Delete => KeyCode::Delete,
        CTKeyCode::Esc => KeyCode::Esc,
        CTKeyCode::Left => KeyCode::Left,
        CTKeyCode::Right => KeyCode::Right,
        CTKeyCode::Up => KeyCode::Up,
        CTKeyCode::Down => KeyCode::Down,
        CTKeyCode::Home => KeyCode::Home,
        CTKeyCode::End => KeyCode::End,
        CTKeyCode::PageUp => KeyCode::PageUp,
        CTKeyCode::PageDown => KeyCode::PageDown,
        CTKeyCode::F(n) => KeyCode::F(n),
        CTKeyCode::Menu => KeyCode::Menu,
        _ => KeyCode::Unknown(0),
    }
}