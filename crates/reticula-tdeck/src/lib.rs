//! `reticula-tdeck` — LILYGO T-Deck board support package.
//!
//! Implements the [`reticula_hal::Board`] traits for the
//! [LILYGO T-Deck](https://lilygo.cc/products/t-deck) (ESP32-S3FN16R8):
//!
//! * **Display** — 320×240 ST7789 LCD over SPI (`mipidsi`), exposed as an
//!   `embedded-graphics` draw target.
//! * **Keyboard** — the secondary ESP32-C3 keyboard, polled over I²C at
//!   address `0x55`. Keys arrive as ASCII bytes.
//! * **Trackball** — currently unread; see [`trackball`].
//!
//! This crate builds for `xtensa-esp32s3-espidf` and is kept out of the
//! default workspace (see `tools/build-esp32.sh`). The application itself is
//! device-independent: `reticula-app` runs identically on the simulator and
//! on this board.
//!
//! ## Pins (from the LilyGO T-Deck schematic)
//!
//! | Function | GPIO |
//! |----------|------|
//! | TFT DC    | 11 |
//! | TFT CS    | 12 |
//! | TFT SCK   | 40 |
//! | TFT MOSI  | 41 |
//! | TFT MISO  | 38 |
//! | TFT EN    | 42 |
//! | TFT BL    | 45 |
//! | KB SDA    | 18 |
//! | KB SCL    | 8  |
//! | KB INT    | 9  |

pub mod board;
pub mod display;
pub mod keyboard;
pub mod trackball;

pub use board::TDeckBoard;
pub use display::TdeckScreen;

/// I²C address of the T-Deck keyboard controller.
pub const KEYBOARD_I2C_ADDRESS: u8 = 0x55;

/// Pin numbers (see module docs).
pub mod pins {
    pub const TFT_DC: u32 = 11;
    pub const TFT_CS: u32 = 12;
    pub const TFT_SCK: u32 = 40;
    pub const TFT_MOSI: u32 = 41;
    pub const TFT_MISO: u32 = 38;
    pub const TFT_EN: u32 = 42;
    pub const TFT_BL: u32 = 45;
    pub const KB_SDA: u32 = 18;
    pub const KB_SCL: u32 = 8;
    pub const KB_INT: u32 = 9;
}