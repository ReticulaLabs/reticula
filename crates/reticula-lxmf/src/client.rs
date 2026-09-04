//! The LXMF client: announces a delivery identity, receives messages over
//! links and packets, and sends messages by establishing links to peers.

use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, info, trace, warn};
use rmpv::Value;
use sha2::Digest;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

use reticulum_sdk::destination::link::{LinkEvent, LinkEventData, LinkId, LinkStatus};
use reticulum_sdk::destination::{DestinationName, SingleInputDestination};
use reticulum_sdk::hash::{AddressHash, Hash};
use reticulum_sdk::identity::PrivateIdentity;
use reticulum_sdk::transport::{AnnounceEvent, ReceivedData, Transport};

use crate::message::LxmfMessage;
use crate::store::{Direction, MessageStore};
use crate::LxmfError;

/// LXMF application name, as used in the delivery destination name.
pub const APP_NAME: &str = "lxmf";
/// Aspect used for the delivery destination (`lxmf/delivery`).
pub const DELIVERY_ASPECT: &str = "delivery";
/// How long to wait for a link to a peer to activate before giving up.
pub const LINK_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Events the LXMF client emits to the rest of the application.
#[derive(Debug, Clone)]
pub enum LxmfEvent {
    /// A new inbound message was received, verified and stored.
    MessageReceived(Arc<LxmfMessage>),
    /// An outbound message was handed to the transport for delivery.
    MessageSent(Arc<LxmfMessage>),
    /// A peer announced its `lxmf/delivery` destination (a chat contact).
    ContactDiscovered { address: [u8; 16], name: Option<String> },
    /// A link to or from a peer became active.
    PeerConnected(AddressHash),
    /// A link to or from a peer closed.
    PeerDisconnected(AddressHash),
    /// A previously sent message was acknowledged by the link layer.
    Delivered([u8; 32]),
    /// A previously sent message failed to reach its destination.
    Undeliverable { message_id: [u8; 32], reason: String },
}

/// An LXMF end client.
///
/// The Reticulum [`Transport`] is owned by the application and shared with
/// other clients. This client registers (through the transport) an inbound
/// `lxmf/delivery` destination derived from `identity` and listens for
/// messages arriving over links or as direct packets.
pub struct LxmfClient {
    transport: Arc<Transport>,
    identity: PrivateIdentity,
    display_name: String,
    delivery: Arc<Mutex<SingleInputDestination>>,
    store: Arc<std::sync::Mutex<MessageStore>>,
    events: broadcast::Sender<LxmfEvent>,
    /// Outbound links we established per peer, keyed by peer address hash.
    links: Mutex<HashMap<AddressHash, LinkId>>,
    /// LXMF delivery destinations discovered from announces, in discovery order.
    discovered: Mutex<Vec<AddressHash>>,
}

impl LxmfClient {
    /// Create a new client.
    ///
    /// `delivery` must be a destination created by the application via
    /// [`Transport::add_destination`] with name
    /// `DestinationName::new(APP_NAME, DELIVERY_ASPECT)`.
    pub fn new(
        transport: Arc<Transport>,
        identity: PrivateIdentity,
        delivery: Arc<Mutex<SingleInputDestination>>,
        display_name: impl Into<String>,
        store: Arc<std::sync::Mutex<MessageStore>>,
        event_capacity: usize,
    ) -> Self {
        let (events, _) = broadcast::channel(event_capacity);
        Self {
            transport,
            identity,
            display_name: display_name.into(),
            delivery,
            store,
            events,
            links: Mutex::new(HashMap::new()),
            discovered: Mutex::new(Vec::new()),
        }
    }

    /// Subscribe to client events.
    pub fn events(&self) -> broadcast::Receiver<LxmfEvent> {
        self.events.subscribe()
    }

    /// The delivery identity (source of our messages).
    pub fn identity(&self) -> &PrivateIdentity {
        &self.identity
    }

    /// The delivery destination's address hash (our LXMF address).
    pub fn delivery_address(&self) -> AddressHash {
        *self.identity.address_hash()
    }

    /// Our display name, as announced to peers.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Announce the delivery identity so peers can discover a path to us.
    pub async fn announce(&self) {
        let app_data = delivery_app_data(Some(&self.display_name));
        self.transport
            .send_announce(&self.delivery, Some(&app_data))
            .await;
        debug!(
            "lxmf: announced delivery destination {}",
            self.identity.address_hash()
        );
    }

    /// The address hash of the `lxmf/delivery` destination that `identity`
    /// would announce. Used to recognise LXMF delivery announces.
    pub fn lxmf_address_for(identity: &[u8; 16]) -> AddressHash {
        let name = DestinationName::new(APP_NAME, DELIVERY_ASPECT);
        let full: [u8; 32] = Hash::generator()
            .chain_update(name.as_name_hash_slice())
            .chain_update(identity)
            .finalize()
            .into();
        AddressHash::new_from_hash(&Hash::new(full))
    }

    /// Whether an announce event is for an `lxmf/delivery` destination.
    pub async fn is_lxmf_delivery(announce: &AnnounceEvent) -> bool {
        let dest = announce.destination.lock().await;
        let Ok(identity_hash) = dest
            .desc
            .identity
            .address_hash
            .as_slice()
            .try_into()
        else {
            return false;
        };
        let expected = Self::lxmf_address_for(&identity_hash);
        dest.desc.address_hash == expected
    }

    /// Addresses of LXMF delivery destinations discovered so far.
    pub async fn discovered(&self) -> Vec<AddressHash> {
        self.discovered.lock().await.clone()
    }

    /// The long-running discovery loop. Watches for `lxmf/delivery` announces
    /// so chat contacts show up without requiring an inbound message first.
    pub async fn run_discovery(&self) -> Result<(), LxmfError> {
        let mut announces = self.transport.recv_announces().await;
        while let Ok(announce) = announces.recv().await {
            if Self::is_lxmf_delivery(&announce).await {
                let address = announce.destination.lock().await.desc.address_hash;
                let name = delivery_name(&announce);
                let mut seen = self.discovered.lock().await;
                if !seen.contains(&address) {
                    seen.push(address);
                    info!("lxmf: discovered contact {address} ({name:?})");
                    let _ = self.events.send(LxmfEvent::ContactDiscovered {
                        address: address.as_slice().try_into().unwrap(),
                        name,
                    });
                }
            }
        }
        Ok(())
    }

    /// Send a message to a peer by LXMF address hash.
    ///
    /// Establishes (or reuses) a direct link to the recipient and transmits
    /// the packed message over it. Returns the message hash on success.
    pub async fn send(
        &self,
        destination_hash: [u8; 16],
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<[u8; 32], LxmfError> {
        let mut message = LxmfMessage::new(
            destination_hash,
            self.identity.address_hash().as_slice().try_into().unwrap(),
            title.into().into_bytes(),
            content.into().into_bytes(),
        );
        let packed = message.pack(&self.identity)?;
        let hash = message.hash;

        self.send_packed(destination_hash, packed).await?;

        let stored = self.store.lock().unwrap().push(message, Direction::Outbound);
        let _ = self.events.send(LxmfEvent::MessageSent(stored));
        Ok(hash)
    }

    /// The long-running event loop. Spawn this on the tokio runtime.
    pub async fn run(&self) -> Result<(), LxmfError> {
        let mut in_links = self.transport.in_link_events();
        let mut out_links = self.transport.out_link_events();
        let mut data = self.transport.received_data_events();

        loop {
            tokio::select! {
                ev = in_links.recv() => {
                    if let Ok(ev) = ev {
                        self.handle_link_event(ev).await;
                    }
                }
                ev = out_links.recv() => {
                    if let Ok(ev) = ev {
                        self.handle_link_event(ev).await;
                    }
                }
                rd = data.recv() => {
                    if let Ok(rd) = rd {
                        self.handle_received_data(rd).await;
                    }
                }
            }
        }
    }

    /// Transmit a packed LXMF message to a peer.
    async fn send_packed(&self, destination_hash: [u8; 16], packed: Vec<u8>) -> Result<(), LxmfError> {
        let peer = AddressHash::new(destination_hash);
        let link_id = self.ensure_link(peer).await?;

        // Fast path: the link is already active.
        if let Some(link) = self.transport.find_out_link(&link_id).await {
            if link.lock().await.status() == LinkStatus::Active {
                let packet = link.lock().await.data_packet(&packed)?;
                self.transport.send_packet(packet).await;
                trace!("lxmf: sent {} bytes over active link {}", packed.len(), link_id);
                return Ok(());
            }
        }

        // Otherwise wait for the link to activate (or close/time out).
        let transport = self.transport.clone();
        timeout(LINK_ACTIVATION_TIMEOUT, async move {
            let mut out_links = transport.out_link_events();
            loop {
                let ev = out_links.recv().await.map_err(|_| LxmfError::LinkClosed)?;
                if ev.id != link_id {
                    continue;
                }
                match ev.event {
                    LinkEvent::Activated => {
                        let link = transport
                            .find_out_link(&link_id)
                            .await
                            .ok_or(LxmfError::LinkLost)?;
                        let packet = link.lock().await.data_packet(&packed)?;
                        transport.send_packet(packet).await;
                        trace!("lxmf: sent {} bytes over newly active link {}", packed.len(), link_id);
                        return Ok(());
                    }
                    LinkEvent::Closed => return Err(LxmfError::LinkClosed),
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| LxmfError::SendTimeout)??;

        Ok(())
    }

    /// Ensure an outbound link to `peer` exists; create one if needed.
    async fn ensure_link(&self, peer: AddressHash) -> Result<LinkId, LxmfError> {
        if let Some(&link_id) = self.links.lock().await.get(&peer) {
            return Ok(link_id);
        }

        let desc = self
            .transport
            .get_out_destination(&peer)
            .await
            .ok_or(LxmfError::NoPathToDestination(peer.as_slice().try_into().unwrap()))?
            .lock()
            .await
            .desc;

        let link = self.transport.link(desc).await;
        let link_id = *link.lock().await.id();

        // Request a path in case announce propagation needs a nudge.
        self.transport.request_path(&peer, None, None).await;

        self.links.lock().await.insert(peer, link_id);
        debug!("lxmf: established outbound link {} to {}", link_id, peer);
        Ok(link_id)
    }

    /// Handle an incoming link event (inbound or outbound).
    async fn handle_link_event(&self, ev: LinkEventData) {
        match ev.event {
            LinkEvent::Data(payload) => {
                self.handle_link_data(ev.id, payload.as_slice()).await;
            }
            LinkEvent::Activated => {
                if let Some(peer) = self.peer_for_link(ev.id).await {
                    debug!("lxmf: peer connected: {}", peer);
                    let _ = self.events.send(LxmfEvent::PeerConnected(peer));
                }
            }
            LinkEvent::Closed => {
                let peer = self.peer_for_link(ev.id).await;
                self.links.lock().await.retain(|_, id| *id != ev.id);
                if let Some(peer) = peer {
                    let _ = self.events.send(LxmfEvent::PeerDisconnected(peer));
                }
            }
            _ => {}
        }
    }

    /// Attempt to parse link payload bytes as an LXMF message.
    async fn handle_link_data(&self, _link_id: LinkId, payload: &[u8]) {
        let Some(message) = self.unpack_message(payload).await else {
            return;
        };

        // Only accept messages addressed to this client.
        if message.destination_hash != self.identity.address_hash().as_slice()[..] {
            trace!("lxmf: dropping message not addressed to us");
            return;
        }

        if !message.signature_validated {
            warn!("lxmf: message signature could not be validated");
        }

        let stored = self.store.lock().unwrap().push(message, Direction::Inbound);
        let _ = self.events.send(LxmfEvent::MessageReceived(stored));
    }

    /// Handle a direct (opportunistic) packet received outside a link.
    async fn handle_received_data(&self, rd: ReceivedData) {
        // The reference implementation re-prepends the packet's destination
        // hash for opportunistic deliveries.
        let mut data = Vec::with_capacity(16 + rd.data.len());
        data.extend_from_slice(rd.destination.as_slice());
        data.extend_from_slice(rd.data.as_slice());

        let Some(message) = self.unpack_message(&data).await else {
            return;
        };

        if message.destination_hash != self.identity.address_hash().as_slice()[..] {
            return;
        }

        let stored = self.store.lock().unwrap().push(message, Direction::Inbound);
        let _ = self.events.send(LxmfEvent::MessageReceived(stored));
    }

    /// Unpack message bytes, recalling the source identity when possible.
    async fn unpack_message(&self, data: &[u8]) -> Option<LxmfMessage> {
        let source_hash: [u8; 16] = data.get(16..32)?.try_into().ok()?;
        let recall = match self
            .transport
            .get_out_destination(&AddressHash::new(source_hash))
            .await
        {
            Some(dest) => Some(dest.lock().await.desc.identity),
            None => {
                trace!(
                    "lxmf: source identity {} unknown",
                    AddressHash::new(source_hash)
                );
                None
            }
        };

        match LxmfMessage::unpack(data, &|_| recall) {
            Ok(message) => Some(message),
            Err(e) => {
                trace!("lxmf: dropped undecodable data: {e}");
                None
            }
        }
    }

    /// Resolve the peer address for a link.
    async fn peer_for_link(&self, link_id: LinkId) -> Option<AddressHash> {
        if let Some(link) = self.transport.find_out_link(&link_id).await {
            return Some(link.lock().await.destination().address_hash);
        }
        if let Some(link) = self.transport.find_in_link(&link_id).await {
            return Some(link.lock().await.destination().address_hash);
        }
        None
    }
}

/// Announce app-data for a delivery identity: a msgpack list of
/// `[display_name, stamp_cost, [supported_functionality]]`.
fn delivery_app_data(display_name: Option<&str>) -> Vec<u8> {
    let name = display_name
        .map(|n| Value::Binary(n.as_bytes().to_vec()))
        .unwrap_or(Value::Nil);
    // Supported functionality: SF_COMPRESSION = 0x00 (not enabled in MVP).
    let data = vec![name, Value::Nil, Value::Array(vec![Value::Integer(0.into())])];
    rmp_serde::to_vec(&data).unwrap_or_default()
}

/// Display name from an LXMF delivery announce's app data (a msgpack
/// `[name, stamp_cost, functionality]` list). Returns `None` when unnamed.
fn delivery_name(announce: &AnnounceEvent) -> Option<String> {
    let value: Option<Value> =
        rmpv::decode::read_value(&mut announce.app_data.as_slice()).ok();
    let first = match value {
        Some(Value::Array(items)) => items.first().cloned(),
        _ => None,
    };
    let name = match first {
        Some(Value::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
        Some(Value::String(s)) => s.to_string(),
        _ => return None,
    };
    if name.trim().is_empty() {
        None
    } else {
        Some(name)
    }
}