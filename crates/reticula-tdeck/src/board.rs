//! The T-Deck board: display + keyboard.

use std::time::Instant;

use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{GpioPin, Output, PinDriver};
use esp_idf_hal::i2c::{I2cDriver, config::Config as I2cConfig};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::config::{Config as SpiConfig, MODE_3, SpiDriverConfig};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver};
use esp_idf_hal::units::*;

use embedded_graphics::geometry::Size;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::{NoResetPin, Orientation, Rotation};
use mipidsi::Builder;

use reticula_hal::Board;

use crate::display::TdeckScreen;
use crate::keyboard::TdeckKeyboard;
use crate::pins;

/// LCD chip-select pin.
pub type LcdCs = GpioPin<pins::TFT_CS>;
/// LCD data/command pin (as an output).
pub type LcdDc = PinDriver<'static, GpioPin<pins::TFT_DC>, Output>;
/// SPI device driving the LCD.
pub type LcdSpiDevice = SpiDeviceDriver<'static, SpiDriver<'static>, LcdCs>;
/// mipidsi SPI interface (SCLK/MOSI/MISO + DC).
pub type LcdInterface = SpiInterface<'static, LcdSpiDevice, LcdDc>;
/// The LCD, presented as a `reticula_hal::Display`.
pub type TdeckDisplay = TdeckScreen<LcdInterface, ST7789, NoResetPin>;
/// The keyboard, polled over I²C.
pub type TdeckKeyboardType = TdeckKeyboard<'static>;

/// Width of the T-Deck LCD in landscape orientation.
pub const DISPLAY_W: u32 = 320;
/// Height of the T-Deck LCD in landscape orientation.
pub const DISPLAY_H: u32 = 240;

/// The LILYGO T-Deck board.
pub struct TDeckBoard {
    display: TdeckDisplay,
    keyboard: TdeckKeyboardType,
    started: Instant,
}

impl TDeckBoard {
    /// Initialise the board from the ESP-IDF peripherals.
    pub fn new(peripherals: Peripherals) -> Result<Self, EspError> {
        // --- LCD power ---
        // TFT_EN and backlight are just pulled high.
        let mut en = PinDriver::output(peripherals.pins.gpio42)?;
        en.set_high()?;
        let mut backlight = PinDriver::output(peripherals.pins.gpio45)?;
        backlight.set_high()?;

        // --- SPI display ---
        let spi = SpiDriver::new(
            peripherals.spi2,
            peripherals.pins.gpio40,          // SCLK
            peripherals.pins.gpio41,          // MOSI
            Some(peripherals.pins.gpio38),    // MISO
            &SpiDriverConfig::new(),
        )?;
        let cs = PinDriver::output(peripherals.pins.gpio12)?;
        let dc = PinDriver::output(peripherals.pins.gpio11)?;

        let device = SpiDeviceDriver::new_single(
            spi,
            cs,
            &SpiConfig::new()
                .baudrate(40.MHz().into())
                .data_mode(MODE_3),
        )?;

        // The SPI interface needs a byte buffer for the lifetime of the
        // driver; leak a heap allocation so it outlives this function.
        let buffer: &'static mut [u8; 512] = Box::leak(Box::new([0u8; 512]));
        let interface = SpiInterface::new(device, dc, buffer);

        let mut display = Builder::st7789(interface).init(&mut Ets)?;
        // The panel is mounted sideways; rotate for landscape 320×240.
        display.set_orientation(Orientation::default().rotate(Rotation::Deg90))?;
        let display = TdeckScreen::new(display, Size::new(DISPLAY_W, DISPLAY_H));

        // --- Keyboard over I²C ---
        let i2c = I2cDriver::new(
            peripherals.i2c0,
            peripherals.pins.gpio18,          // SDA
            peripherals.pins.gpio8,           // SCL
            &I2cConfig::new().baudrate(100.kHz().into()),
        )?;
        let keyboard = TdeckKeyboard::new(i2c);

        Ok(Self {
            display,
            keyboard,
            started: Instant::now(),
        })
    }
}

impl Board for TDeckBoard {
    type Display = TdeckDisplay;
    type Keyboard = TdeckKeyboardType;

    fn display(&mut self) -> Option<&mut Self::Display> {
        Some(&mut self.display)
    }

    fn keyboard(&mut self) -> &mut Self::Keyboard {
        &mut self.keyboard
    }

    fn delay_ms(&mut self, ms: u32) {
        Ets.delay_ms(ms);
    }

    fn uptime_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

/// Convenience alias for ESP-IDF errors.
pub type EspError = esp_idf_sys::EspError;