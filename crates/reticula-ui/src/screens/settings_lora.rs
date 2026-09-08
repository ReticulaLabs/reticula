//! Settings sub-menu: LoRa radio — enable/disable and radio parameters.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, LoraSettings};
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Frequency,
    Bandwidth,
    SpreadingFactor,
    CodingRate,
    TxPower,
}

#[derive(Default)]
pub struct SettingsLoraScreen {
    pub state: ListState,
    enabled: bool,
    editing: Option<Field>,
    freq_input: String,
    bw_input: String,
    sf_input: String,
    cr_input: String,
    txp_input: String,
    /// Whether the editable values have been seeded from the current config.
    seeded: bool,
}

impl SettingsLoraScreen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if let Some(field) = self.editing {
            let input = match field {
                Field::Frequency => &mut self.freq_input,
                Field::Bandwidth => &mut self.bw_input,
                Field::SpreadingFactor => &mut self.sf_input,
                Field::CodingRate => &mut self.cr_input,
                Field::TxPower => &mut self.txp_input,
            };
            return match key {
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    input.push(c);
                    Command::None
                }
                // TX power is signed; allow a leading minus.
                KeyCode::Char('-') if input.is_empty() && field == Field::TxPower => {
                    input.push('-');
                    Command::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    Command::None
                }
                KeyCode::Enter => {
                    self.editing = None;
                    Command::None
                }
                KeyCode::Esc => {
                    self.editing = None;
                    Command::None
                }
                _ => Command::None,
            };
        }

        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(7);
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.enabled = !self.enabled;
                    Command::None
                }
                1 => {
                    self.editing = Some(Field::Frequency);
                    Command::None
                }
                2 => {
                    self.editing = Some(Field::Bandwidth);
                    Command::None
                }
                3 => {
                    self.editing = Some(Field::SpreadingFactor);
                    Command::None
                }
                4 => {
                    self.editing = Some(Field::CodingRate);
                    Command::None
                }
                5 => {
                    self.editing = Some(Field::TxPower);
                    Command::None
                }
                _ => Command::SaveLora(self.settings()),
            },
            KeyCode::Esc => Command::Back,
            _ => Command::None,
        }
    }

    /// Parse the edited fields into a [`LoraSettings`], clamping out-of-range
    /// values to sane limits.
    fn settings(&self) -> LoraSettings {
        // Frequency is entered in kHz (e.g. 914875 = 914.875 MHz), so it can
        // represent fractional MHz without decimals.
        let freq_khz = parse(&self.freq_input).clamp(100_000, 2_500_000) as u64;
        let bw_khz = parse(&self.bw_input).clamp(8, 500) as u64;
        LoraSettings {
            enabled: self.enabled,
            frequency_hz: freq_khz * 1000,
            bandwidth_hz: bw_khz * 1000,
            spreading_factor: parse(&self.sf_input).clamp(7, 12) as u8,
            coding_rate: parse(&self.cr_input).clamp(5, 8) as u8,
            tx_power_dbm: parse(&self.txp_input).clamp(-9, 22) as i8,
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        if !self.seeded {
            self.seeded = true;
            let s = ctx.lora_settings.copied().unwrap_or_default();
            self.enabled = s.enabled;
            self.freq_input = (s.frequency_hz / 1000).to_string();
            self.bw_input = (s.bandwidth_hz / 1000).to_string();
            self.sf_input = s.spreading_factor.to_string();
            self.cr_input = s.coding_rate.to_string();
            self.txp_input = s.tx_power_dbm.to_string();
        }

        widgets::draw_header(target, width, "LoRa", "", &ctx.network, theme).ok();

        let mut y = theme.line_h;

        let status = if self.enabled {
            "Radio: enabled"
        } else {
            "Radio: disabled"
        };
        widgets::draw_text(target, Point::new(0, y), status, theme.text_dim, theme).ok();
        y += theme.line_h + 4;

        // Row 0: enabled toggle.
        y = self.draw_toggle(target, width, y, theme, 0, "Enabled", self.enabled);
        // Row 1: frequency (kHz).
        y = self.draw_value(target, width, y, theme, 1, "Frequency (kHz)", &self.freq_input, Field::Frequency);
        // Row 2: bandwidth (kHz).
        y = self.draw_value(target, width, y, theme, 2, "Bandwidth (kHz)", &self.bw_input, Field::Bandwidth);
        // Row 3: spreading factor.
        y = self.draw_value(target, width, y, theme, 3, "Spreading factor", &self.sf_input, Field::SpreadingFactor);
        // Row 4: coding rate.
        y = self.draw_value(target, width, y, theme, 4, "Coding rate (4/n)", &self.cr_input, Field::CodingRate);
        // Row 5: TX power (dBm).
        y = self.draw_value(target, width, y, theme, 5, "TX power (dBm)", &self.txp_input, Field::TxPower);

        // Row 6: save & restart.
        let at = Point::new(0, y);
        let label = "Save & restart";
        if self.state.selected == 6 {
            widgets::draw_highlight(
                target,
                at,
                label,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, label, theme.text, theme).ok();
        }
        y += theme.line_h;

        if !ctx.notice.is_empty() {
            y += theme.line_h;
            widgets::draw_text(target, Point::new(0, y), ctx.notice, theme.ok, theme).ok();
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(
            target,
            footer,
            "ALT+Backspace back",
            &short,
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();
    }

    fn draw_toggle<D>(
        &self,
        target: &mut D,
        width: i32,
        y: i32,
        theme: &Theme,
        index: usize,
        label: &str,
        on: bool,
    ) -> i32
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let value = if on { "on" } else { "off" };
        let line = format!("{label}: {value}");
        let at = Point::new(0, y);
        if self.state.selected == index {
            widgets::draw_highlight(
                target,
                at,
                &line,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, &line, theme.text, theme).ok();
        }
        y + theme.line_h
    }

    fn draw_value<D>(
        &self,
        target: &mut D,
        width: i32,
        y: i32,
        theme: &Theme,
        index: usize,
        label: &str,
        value: &str,
        field: Field,
    ) -> i32
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let line = format!("{label}: {value}");
        let at = Point::new(0, y);
        let is_editing = self.editing == Some(field);
        if self.state.selected == index || is_editing {
            widgets::draw_highlight(
                target,
                at,
                &line,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, &line, theme.text, theme).ok();
        }
        y + theme.line_h
    }
}

/// Parse a (possibly signed) integer, returning 0 on failure.
fn parse(input: &str) -> i64 {
    input.trim().parse().unwrap_or(0)
}