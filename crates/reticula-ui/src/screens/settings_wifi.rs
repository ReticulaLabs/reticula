//! Settings sub-menu: WiFi network — enable/disable, SSID and password.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::{Command, PeerProtocol, WifiSettings};
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::widgets::{self, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Ssid,
    Password,
    Peer,
}

#[derive(Default)]
pub struct SettingsWifiScreen {
    pub state: ListState,
    enabled: bool,
    editing: Option<Field>,
    ssid_input: String,
    pass_input: String,
    peer_input: String,
    peer_proto: PeerProtocol,
    /// Whether the enabled/peer values have been seeded from the current config.
    seeded: bool,
}

impl SettingsWifiScreen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if let Some(field) = self.editing {
            let input = match field {
                Field::Ssid => &mut self.ssid_input,
                Field::Password => &mut self.pass_input,
                Field::Peer => &mut self.peer_input,
            };
            return match key {
                KeyCode::Char(c) if c.is_ascii() => {
                    input.push(c);
                    Command::None
                }
                KeyCode::Space => {
                    input.push(' ');
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
                self.state.move_down(6);
                Command::None
            }
            KeyCode::Enter => match self.state.selected {
                0 => {
                    self.enabled = !self.enabled;
                    Command::None
                }
                1 => {
                    self.editing = Some(Field::Ssid);
                    Command::None
                }
                2 => {
                    self.editing = Some(Field::Password);
                    Command::None
                }
                3 => {
                    self.editing = Some(Field::Peer);
                    Command::None
                }
                4 => {
                    self.peer_proto = match self.peer_proto {
                        PeerProtocol::Tcp => PeerProtocol::Udp,
                        PeerProtocol::Udp => PeerProtocol::Tcp,
                    };
                    Command::None
                }
                _ => Command::SaveWifi(WifiSettings {
                    enabled: self.enabled,
                    ssid: self.ssid_input.trim().to_string(),
                    password: self.pass_input.clone(),
                    peer_addr: self.peer_input.trim().to_string(),
                    peer_proto: self.peer_proto,
                }),
            },
            KeyCode::Esc => Command::Back,
            _ => Command::None,
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
            self.enabled = ctx.network.wifi_enabled;
            // Seed the SSID from the current config so saving without retyping
            // does not wipe it.
            self.ssid_input = ctx.wifi_ssid.to_string();
            self.peer_input = ctx.peer_addr.to_string();
            self.peer_proto = ctx.peer_proto;
        }

        widgets::draw_header(target, width, "WiFi", "", &ctx.network, theme).ok();

        let mut y = theme.line_h;

        // Current configuration / status.
        let status = if !self.enabled {
            "Network: disabled".to_string()
        } else if !ctx.wifi_ssid.is_empty() {
            format!("Network: {}", ctx.wifi_ssid)
        } else {
            "Network: not configured".to_string()
        };
        widgets::draw_text(target, Point::new(0, y), &status, theme.text_dim, theme).ok();
        y += theme.line_h;
        let link = if ctx.network.wifi_connected {
            "Link: connected"
        } else {
            "Link: not connected"
        };
        widgets::draw_text(target, Point::new(0, y), link, theme.text_dim, theme).ok();
        y += theme.line_h + 4;

        // Row 0: enabled toggle.
        let line = format!("Enabled: {}", if self.enabled { "on" } else { "off" });
        let at = Point::new(0, y);
        if self.state.selected == 0 {
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
        y += theme.line_h;

        // Row 1: SSID.
        y = self.draw_field(target, width, y, ctx, theme, 1, "SSID", &self.ssid_input, Field::Ssid);
        // Row 2: password.
        y = self.draw_field(target, width, y, ctx, theme, 2, "Password", &self.pass_input, Field::Password);
        // Row 3: remote peer endpoint.
        y = self.draw_field(target, width, y, ctx, theme, 3, "Remote endpoint", &self.peer_input, Field::Peer);

        // Row 4: remote protocol (TCP/UDP).
        let proto = match self.peer_proto {
            PeerProtocol::Tcp => "TCP",
            PeerProtocol::Udp => "UDP",
        };
        let line = format!("Remote protocol: {proto}");
        let at = Point::new(0, y);
        if self.state.selected == 4 {
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
        y += theme.line_h;

        // Row 5: save & restart.
        let at = Point::new(0, y);
        let label = "Save & restart";
        if self.state.selected == 5 {
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

    /// Draw one editable field row (`SSID` or `Password`) with its current
    /// value, highlighting it when selected or being edited.
    fn draw_field<D>(
        &self,
        target: &mut D,
        width: i32,
        y: i32,
        _ctx: &ViewContext,
        theme: &Theme,
        index: usize,
        label: &str,
        value: &str,
        field: Field,
    ) -> i32
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let is_selected = self.state.selected == index;
        let is_editing = self.editing == Some(field);

        let text = if is_editing {
            value.to_string()
        } else {
            // Mask the password when not being edited.
            if field == Field::Password && !value.is_empty() {
                mask_password(value)
            } else if value.is_empty() {
                if field == Field::Password {
                    "••••".to_string()
                } else {
                    "(none)".to_string()
                }
            } else {
                value.to_string()
            }
        };

        let line = format!("{label}: {text}");
        if is_selected || is_editing {
            widgets::draw_highlight(
                target,
                Point::new(0, y),
                &line,
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, Point::new(0, y), &line, theme.text, theme).ok();
        }
        y + theme.line_h
    }
}

fn mask_password(pass: &str) -> String {
    let n = pass.chars().count();
    if n == 0 {
        return String::new();
    }
    // Show bullets for all but the last character, which stays visible so the
    // user can see what they typed.
    let mut out: String = "•".repeat(n.saturating_sub(1));
    out.push(pass.chars().last().unwrap());
    out
}