//! The T-Deck board: display + keyboard + LoRa radio hardware.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{Gpio11, Gpio12, Gpio13, Gpio17, Gpio45, Input, Output, PinDriver};
use esp_idf_hal::i2c::{I2cDriver, config::Config as I2cConfig};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::config::{Config as SpiConfig, MODE_0, MODE_3};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver, SpiDriverConfig};
use esp_idf_hal::units::*;

use embedded_graphics::geometry::Size;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::{Orientation, Rotation};
use mipidsi::{Builder, NoResetPin};

use embedded_hal::delay::DelayNs;
use reticulum_sdk::iface::lora::embedded::EmbeddedLoRaHw;
use reticulum_sdk::iface::lora::LoRaHwProvider;

use reticula_hal::Board;

use crate::display::TdeckScreen;
use crate::keyboard::TdeckKeyboard;
use crate::pins;

/// LCD chip-select pin.
pub type LcdCs = Gpio12;
/// LCD data/command pin (as an output).
pub type LcdDc = PinDriver<'static, Gpio11, Output>;
/// Shared SPI2 bus (TFT + LoRa).
pub type SharedSpi = Arc<SpiDriver<'static>>;
/// SPI device driving the LCD.
pub type LcdSpiDevice = SpiDeviceDriver<'static, SharedSpi>;
/// mipidsi SPI interface (SCLK/MOSI/MISO + DC).
pub type LcdInterface = SpiInterface<'static, LcdSpiDevice, LcdDc>;
/// The LCD, presented as a `reticula_hal::Display`.
pub type TdeckDisplay = TdeckScreen<LcdInterface, ST7789, NoResetPin>;
/// The keyboard, polled over I²C.
pub type TdeckKeyboardType = TdeckKeyboard<'static>;

/// SPI device driving the SX1262 LoRa radio.
pub type LoraSpiDevice = SpiDeviceDriver<'static, SharedSpi>;
/// Busy pin (input) of the SX1262.
pub type LoraBusyPin = PinDriver<'static, Gpio13, Input>;
/// Reset pin (output) of the SX1262.
pub type LoraResetPin = PinDriver<'static, Gpio17, Output>;
/// DIO1 pin (input) of the SX1262.
pub type LoraDio1Pin = PinDriver<'static, Gpio45, Input>;
/// The LoRa hardware provider, hiding the concrete pin/SPI types.
pub type LoraHw = Arc<dyn LoRaHwProvider>;

/// Width of the T-Deck LCD in landscape orientation.
pub const DISPLAY_W: u32 = 320;
/// Height of the T-Deck LCD in landscape orientation.
pub const DISPLAY_H: u32 = 240;

/// The LILYGO T-Deck board.
pub struct TDeckBoard {
    display: TdeckDisplay,
    keyboard: TdeckKeyboardType,
    lora_hw: Option<LoraHw>,
    started: Instant,
}

impl TDeckBoard {
    /// Initialise the board from the ESP-IDF peripherals.
    pub fn new(peripherals: Peripherals) -> Result<Self, EspError> {
        // --- LCD power ---
        // GPIO42 is the backlight/TFT enable.
        let mut backlight = PinDriver::output(peripherals.pins.gpio42)?;
        backlight.set_high()?;

        // --- Shared SPI2 bus (LCD + LoRa) ---
        let spi = Arc::new(SpiDriver::new(
            peripherals.spi2,
            peripherals.pins.gpio40,            // SCLK
            peripherals.pins.gpio41,            // MOSI
            Some(peripherals.pins.gpio38),      // MISO
            &SpiDriverConfig::new(),
        )?);

        let dc = PinDriver::output(peripherals.pins.gpio11)?;
        let lcd_device = SpiDeviceDriver::new(
            spi.clone(),
            Some(peripherals.pins.gpio12),      // LCD CS
            &SpiConfig::new()
                .baudrate(40.MHz().into())
                .data_mode(MODE_3),
        )?;

        // The SPI interface needs a byte buffer for the lifetime of the
        // driver; leak a heap allocation so it outlives this function.
        let buffer: &'static mut [u8; 512] = Box::leak(Box::new([0u8; 512]));
        let interface = SpiInterface::new(lcd_device, dc, buffer);

        let mut display = Builder::new(ST7789, interface)
            .init(&mut Ets)
            .map_err(lcd_init_error)?;
        // The panel is mounted sideways; rotate for landscape 320×240.
        display
            .set_orientation(Orientation::default().rotate(Rotation::Deg90))
            .map_err(lcd_init_error)?;
        let display = TdeckScreen::new(display, Size::new(DISPLAY_W, DISPLAY_H));

        // --- Keyboard over I²C ---
        let i2c = I2cDriver::new(
            peripherals.i2c0,
            peripherals.pins.gpio18,          // SDA
            peripherals.pins.gpio8,           // SCL
            &I2cConfig::new().baudrate(100.kHz().into()),
        )?;
        let keyboard = TdeckKeyboard::new(i2c);

        // --- SX1262 LoRa radio on the shared SPI2 bus ---
        let lora_device = SpiDeviceDriver::new(
            spi.clone(),
            Some(peripherals.pins.gpio9),       // LoRa CS
            &SpiConfig::new()
                .baudrate(1.MHz().into())
                .data_mode(MODE_0),
        )?;
        let lora_busy = PinDriver::input(peripherals.pins.gpio13)?;   // input
        let lora_reset = PinDriver::output(peripherals.pins.gpio17)?; // output
        let lora_dio1 = PinDriver::input(peripherals.pins.gpio45)?;   // input

        let provider = Arc::new(EmbeddedLoRaHw::new(
            Arc::new(Mutex::new(lora_device)),
            Some(Arc::new(Mutex::new(lora_busy))),
            Some(Arc::new(Mutex::new(lora_reset))),
            Some(Arc::new(Mutex::new(lora_dio1))),
        )) as LoraHw;
        log::info!("tdeck: SX1262 LoRa hardware ready");

        Ok(Self {
            display,
            keyboard,
            lora_hw: Some(provider),
            started: Instant::now(),
        })
    }

    /// The LoRa radio hardware provider, for wiring an SX1262 interface into
    /// the application's network configuration.
    pub fn lora_hw(&self) -> Option<LoraHw> {
        self.lora_hw.clone()
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

/// Map a mipidsi init/orientation error (which is not an `EspError`) to a
/// generic ESP-IDF failure so `TDeckBoard::new` keeps a uniform error type.
fn lcd_init_error(e: impl core::fmt::Debug) -> EspError {
    log::error!("LCD init failed: {e:?}");
    EspError::from(esp_idf_sys::ESP_FAIL).expect("ESP_FAIL is an error")
}