//! The LXMF client: announces a delivery identity, receives messages over
//! links and packets, and sends messages by establishing links to peers.

use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, trace, warn};
use rmpv::Value;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};


use reticulum_sdk::destination::link::{LinkEvent, LinkEventData, LinkId, LinkStatus};
use reticulum_sdk::destination::SingleInputDestination;
use reticulum_sdk::hash::AddressHash;
use reticulum_sdk::identity::PrivateIdentity;
use reticulum_sdk::transport::{ReceivedData, Transport};

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