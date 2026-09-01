//! Per-frame view data handed from the application to the active screen.

use reticula_nomad::page::Page;

/// A conversation in the chat list.
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    /// Peer LXMF address hash.
    pub peer: [u8; 16],
    /// Peer address hash as hex (for display).
    pub peer_hex: String,
    /// Text of the most recent message in this conversation.
    pub last_content: String,
    /// Title of the most recent message, if any.
    pub last_title: String,
    /// Number of unread inbound messages.
    pub unread: u32,
    /// Timestamp of the most recent message.
    pub last_ts: f64,
}

/// A single message shown in an open chat.
#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    /// True if the message was received (as opposed to sent).
    pub incoming: bool,
    pub title: String,
    pub content: String,
    pub ts: f64,
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
    /// The last fetched NomadNet page, if any.
    pub page: Option<&'a Page>,
    /// Node the currently shown page came from.
    pub page_node: Option<&'a NodeEntry>,
    /// Loading/error message while no page is available yet.
    pub page_notice: &'a str,
    /// Our LXMF address hash as hex.
    pub identity_hex: &'a str,
    /// Our configured display name.
    pub display_name: &'a str,
    pub network: NetworkState,
}