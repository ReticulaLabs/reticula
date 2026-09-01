//! Network configuration for the Reticulum transport.

use std::time::Duration;

/// How the device connects to the mesh.
///
/// Reticula is a pure end client: it never acts as a transport, so it only
/// needs outbound connectivity to the mesh, e.g. a UDP peer on the local
/// network or a reachable relay.
#[derive(Debug, Clone)]
pub enum TransportKind {
    /// Bind a UDP interface. `forward` optionally points at a reachable
    /// Reticulum node to peer with.
    Udp {
        /// Local bind address, e.g. `0.0.0.0:5238`.
        bind: String,
        /// Optional remote node to forward to, e.g. `192.168.1.10:5238`.
        forward: Option<String>,
    },
    /// Connect out to a Reticulum TCP server.
    TcpPeer {
        /// Remote server address, e.g. `my-node.example:5242`.
        addr: String,
    },
    /// No network interface (offline / development).
    None,
}

/// Full network setup for the application.
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub transport: TransportKind,
    /// How often to re-announce the delivery identity.
    pub announce_interval: Duration,
    /// Whether pressing back on the home screen exits the application.
    /// True on the desktop simulator, false on a device.
    pub quit_on_root_back: bool,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            transport: TransportKind::None,
            announce_interval: Duration::from_secs(300),
            quit_on_root_back: false,
        }
    }
}