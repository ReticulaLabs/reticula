//! `reticula-hal` — platform abstraction layer.
//!
//! Reticula is meant to run on a wide range of devices, from desktop machines
//! to microcontroller handhelds such as the LILYGO T-Deck. This crate defines
//! the thin, `no_std`-friendly traits that a *board support package* (BSP)
//! implements so the rest of the application stays completely portable:
//!
//! * [`display::Display`] — anything that can be drawn on with
//!   `embedded-graphics` and flushed.
//! * [`input::Keyboard`] — anything that produces logical [`KeyCode`]s.
//! * [`board::Board`] — a whole device, bundling its display, keyboard and
//!   timing primitives.
//!
//! A BSP is expected to depend on this crate and implement [`board::Board`].
//! Shipping BSPs live in `crates/reticula-host` (terminal simulator for
//! development on a desktop) and `crates/reticula-tdeck` (the ESP32-S3
//! handheld). New devices only need a new BSP crate implementing these traits.
#![no_std]

extern crate alloc;

pub mod board;
pub mod display;
pub mod input;

pub use board::Board;
pub use display::Display;
pub use input::{KeyCode, KeyEvent, KeyState, Keyboard};