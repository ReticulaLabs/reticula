//! Commands produced by screens for the application to execute.

/// Identifiers of the screens in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreenId {
    Home,
    ChatList,
    Chat,
    NewChat,
    NomadList,
    NomadView,
    Settings,
    /// Sub-menu: identity info and identity regeneration.
    SettingsIdentity,
    /// Sub-menu: WiFi network SSID / password.
    SettingsWifi,
    /// Sub-menu: LoRa radio configuration.
    SettingsLora,
}

/// LoRa radio configuration, as shown in the Settings → LoRa sub-menu.
///
/// Frequencies/bandwidths are held in Hz (matching the SDK's `LoRaConfig`);
/// the UI displays frequency in kHz (e.g. 914875 = 914.875 MHz, so fractional
/// MHz needs no decimal input) and bandwidth in kHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoraSettings {
    /// Whether the LoRa interface is enabled.
    pub enabled: bool,
    /// Carrier frequency in Hz (displayed in MHz).
    pub frequency_hz: u64,
    /// Bandwidth in Hz (displayed in kHz).
    pub bandwidth_hz: u64,
    /// Spreading factor (7–12).
    pub spreading_factor: u8,
    /// Coding rate denominator (5–8, i.e. 4/5..4/8).
    pub coding_rate: u8,
    /// TX power in dBm.
    pub tx_power_dbm: i8,
}

impl Default for LoraSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency_hz: 868_000_000,
            bandwidth_hz: 125_000,
            spreading_factor: 7,
            coding_rate: 5,
            tx_power_dbm: 14,
        }
    }
}

/// An action the UI asks the application to perform.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// No action.
    None,
    /// Switch to another screen (pushing onto the back stack).
    ShowScreen(ScreenId),
    /// Start composing a message to the given peer.
    StartChat([u8; 16]),
    /// Send a message to a peer.
    SendMessage { peer: [u8; 16], text: String },
    /// Fetch a page from a NomadNet node.
    FetchPage { node: [u8; 16], path: String },
    /// Open the fetched page view for a node.
    OpenNode([u8; 16]),
    /// Re-announce our identities.
    Announce,
    /// Set a new display name for our LXMF delivery identity.
    SetDisplayName(String),
    /// Generate a brand-new Reticulum identity, persist it, and restart so the
    /// new LXMF address becomes active.
    RegenerateIdentity,
    /// Persist new WiFi credentials and restart so the device reconnects.
    SaveWifi { ssid: String, password: String },
    /// Persist new LoRa radio settings and restart so they take effect.
    SaveLora(LoraSettings),
    /// Navigate back in the screen stack.
    Back,
    /// Quit the application (host simulator).
    Quit,
}