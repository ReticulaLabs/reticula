//! Per-frame view data handed from the application to the active screen.

use reticula_nomad::page::Page;

use crate::command::{GpsSettings, LoraSettings, PeerProtocol};

/// A conversation in the chat list.
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    /// Peer LXMF address hash.
    pub peer: [u8; 16],
    /// Peer address hash as hex (for display).
    pub peer_hex: String,
    /// Peer display name, if known (e.g. from its LXMF announce).
    pub peer_name: String,
    /// Text of the most recent message in this conversation.
    pub last_content: String,
    /// Title of the most recent message, if any.
    pub last_title: String,
    /// Number of unread inbound messages.
    pub unread: u32,
    /// Timestamp of the most recent message.
    pub last_ts: f64,
    /// Hop count to this peer's delivery destination, if known.
    pub hops: Option<u8>,
}

/// A single message shown in an open chat.
#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    /// True if the message was received (as opposed to sent).
    pub incoming: bool,
    /// True while an outgoing message has been handed to the transport but not
    /// yet acknowledged by the peer's link layer. Such messages are drawn in
    /// the warning colour.
    ///
    /// This means "no acknowledgement yet", not "still transmitting": the
    /// acknowledgement can already be meaningless to the sender, because it
    /// only shows the peer's delivery destination took the bytes — not that
    /// the peer displayed anything.
    pub unacknowledged: bool,
    pub title: String,
    pub content: String,
    pub ts: f64,
}

/// Why the most recent outgoing message could not be delivered.
///
/// Rendering a failure is the only way the sender learns about it: the
/// composer is cleared as soon as Enter is pressed, and the transfer itself
/// happens on a background task.
#[derive(Debug, Clone, Default)]
pub struct SendFailure {
    /// Peer the failed send was addressed to.
    pub peer: [u8; 16],
    /// Human-readable reason, e.g. "no path to 3f2a…".
    pub reason: String,
}

/// A discovered NomadNet node.
#[derive(Debug, Clone, Default)]
pub struct NodeEntry {
    /// Node destination address hash.
    pub address: [u8; 16],
    /// Node address hash as hex (for display).
    pub hex: String,
    /// Node name from its announce, if any.
    pub name: String,
    /// Hop count to this node's destination, if known.
    pub hops: Option<u8>,
}

/// Global network / device state shown in headers and status bars.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkState {
    /// True once the Reticulum transport is up and connected.
    pub connected: bool,
    /// Milliseconds since boot.
    pub uptime_ms: u64,
    /// Number of currently open peer links.
    pub peer_links: u32,
    /// Whether the WiFi link is up.
    pub wifi_connected: bool,
    /// Whether the WiFi interface is enabled (independent of link state).
    pub wifi_enabled: bool,
    /// WiFi RSSI in dBm, when the board has WiFi.
    pub wifi_rssi: Option<i8>,
    /// Whether the LoRa radio interface is online. `None` when not configured.
    pub lora_online: Option<bool>,
    /// Whether the GNSS has a fix carrying a usable UTC time.
    ///
    /// `None` when no receiver is configured, which hides the icon entirely;
    /// `Some(false)` means it is still acquiring and the clock may be unset.
    pub gnss_locked: Option<bool>,
    /// Interface the currently shown destination is reachable over, if known.
    pub dest_iface: Option<[u8; 16]>,
    /// Interface address of the IP (WiFi) Reticulum interface, if configured.
    pub wifi_iface: Option<[u8; 16]>,
    /// Interface address of the LoRa radio interface, if configured.
    pub lora_iface: Option<[u8; 16]>,
}

/// Immutable snapshot of application state for one rendered frame.
///
/// The application builds this cheaply each frame (borrowing from its own
/// shared state) and passes it to the active screen's render method.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewContext<'a> {
    pub conversations: &'a [Conversation],
    pub messages: &'a [ChatMessage],
    pub nodes: &'a [NodeEntry],
    /// Failure of the most recent send, shown on the chat screen it belongs
    /// to. `None` when the last send succeeded or no send has been attempted.
    pub send_failure: Option<&'a SendFailure>,
    /// The last fetched NomadNet page, if any.
    pub page: Option<&'a Page>,
    /// Node the currently shown page came from.
    pub page_node: Option<&'a NodeEntry>,
    /// Loading/error message while no page is available yet.
    pub page_notice: &'a str,
    /// Our LXMF address hash as hex.
    pub identity_hex: &'a str,
    /// Our public identity key as hex (128 chars), for the `lxma://` QR format.
    pub public_key_hex: &'a str,
    /// Our configured display name.
    pub display_name: &'a str,
    /// A transient notice (e.g. "restarting…") shown on the settings screens.
    pub notice: &'a str,
/// The WiFi network SSID currently configured, if any.
    pub wifi_ssid: &'a str,
    /// The currently configured LoRa radio settings.
    pub lora_settings: Option<&'a LoraSettings>,
    /// The currently configured GNSS settings.
    pub gps_settings: Option<&'a GpsSettings>,
    /// Current system clock, in Unix seconds.
    ///
    /// `None` on hosts with no settable clock. On an embedded target that has
    /// not yet recovered a time this can be below `PLAUSIBLE_UNIX_TIME`
    /// (~1970), which the System page reports rather than hides.
    pub system_time: Option<u64>,
    /// Remote Reticulum peer (`host:port`) currently configured, if any.
    pub peer_addr: &'a str,
    /// Protocol used to reach the remote peer.
    pub peer_proto: PeerProtocol,
    pub network: NetworkState,
}