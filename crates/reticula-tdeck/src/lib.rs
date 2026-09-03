//! `reticula-tdeck` — LILYGO T-Deck board support package.
//!
//! Implements the [`reticula_hal::Board`] traits for the
//! [LILYGO T-Deck](https://lilygo.cc/products/t-deck) (ESP32-S3FN16R8):
//!
//! * **Display** — 320×240 ST7789 LCD over SPI (`mipidsi`), exposed as an
//!   `embedded-graphics` draw target.
//! * **Keyboard** — the secondary ESP32-C3 keyboard, polled over I²C at
//!   address `0x55`. Keys arrive as ASCII bytes.
//! * **Trackball** — a four-directional ball with a click switch, read from
//!   five GPIO lines (up/down/left/right/click). See [`trackball`].
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
//! | TFT backlight | 42 |
//! | KB SDA    | 18 |
//! | KB SCL    | 8  |
//! | Trackball up | 3  |
//! | Trackball down | 15 |
//! | Trackball left | 1  |
//! | Trackball right | 2  |
//! | Trackball click | 0  |
//! | LoRa CS   | 9  |
//! | LoRa busy | 13 |
//! | LoRa reset | 17 |
//! | LoRa DIO1 | 45 |

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
    pub const TFT_BACKLIGHT: u32 = 42;
    pub const KB_SDA: u32 = 18;
    pub const KB_SCL: u32 = 8;
    pub const TRACKBALL_UP: u32 = 3;
    pub const TRACKBALL_DOWN: u32 = 15;
    pub const TRACKBALL_LEFT: u32 = 1;
    pub const TRACKBALL_RIGHT: u32 = 2;
    pub const TRACKBALL_CLICK: u32 = 0;
    pub const LORA_CS: u32 = 9;
    pub const LORA_BUSY: u32 = 13;
    pub const LORA_RESET: u32 = 17;
    pub const LORA_DIO1: u32 = 45;
}