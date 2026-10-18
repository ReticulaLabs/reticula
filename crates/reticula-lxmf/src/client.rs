//! The LXMF client: announces a delivery identity, receives messages over
//! links and packets, and sends messages by establishing links to peers.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use log::{debug, info, trace, warn};
use rmpv::Value;
use sha2::Digest;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

use reticulum_sdk::destination::link::{Link, LinkEvent, LinkEventData, LinkId, LinkStatus};
use reticulum_sdk::destination::{DestinationName, SingleInputDestination};
use reticulum_sdk::hash::{AddressHash, Hash};
use reticulum_sdk::identity::{HashIdentity, PrivateIdentity};
use reticulum_sdk::transport::{AnnounceEvent, ReceivedData, Transport};

use crate::message::{Delivery, LxmfMessage};
use crate::store::{Direction, MessageStore};
use crate::LxmfError;

/// LXMF application name, as used in the delivery destination name.
pub const APP_NAME: &str = "lxmf";
/// Aspect used for the delivery destination (`lxmf/delivery`).
pub const DELIVERY_ASPECT: &str = "delivery";
/// How long to wait for a link to a peer to activate before giving up.
pub const LINK_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the link status is re-read while waiting for a link to activate.
///
/// The wait must not rely on the `Activated` broadcast alone: the transport
/// can activate the link in the window between `ensure_link` returning and
/// this waiter subscribing, and a broadcast receiver only ever sees messages
/// sent after it subscribed. Polling mirrors the Nomad client's
/// `wait_for_link_active` and closes that race.
pub const LINK_ACTIVATION_POLL: Duration = Duration::from_millis(100);
/// How long to wait for a path request to re-install a destination that the
/// transport pruned (the `embedded` build expires destinations 120s after
/// their last announce) before giving up on the send.
pub const PATH_REDISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// How many times a single user-visible send is attempted before it is
/// reported as failed.
///
/// A message is only meaningful once it reaches the mesh, and the common
/// failure modes (a link that lost a proof, a path that expired mid-flight)
/// are all resolved by simply establishing a fresh link, so retrying is far
/// better than dropping the message on the floor.
pub const SEND_ATTEMPTS: usize = 3;
/// Pause between send attempts.
pub const SEND_RETRY_DELAY: Duration = Duration::from_secs(5);
/// How long to wait for the peer's acknowledgement before treating the link
/// that carried the message as having silently lost it.
///
/// This is what makes "the transport accepted the bytes" mean something: a
/// packet written to a link whose far end has gone away produces no error, so
/// the acknowledgement is the only signal that the message actually crossed.
pub const ACK_TIMEOUT: Duration = Duration::from_secs(12);
/// How often the acknowledgement is polled while waiting.
const ACK_POLL: Duration = Duration::from_millis(100);
/// How long a sent message is tracked waiting for its delivery proof before
/// the pending entry is discarded.
///
/// Bounds the pending-proof map for a peer that never answers: a message can
/// only be in this state for as long as its link survived anyway.
pub const PENDING_PROOF_TTL: Duration = Duration::from_secs(300);
/// How long a delivery proof waits for the sending task to finish registering
/// its message before being given up on.
pub const PENDING_PROOF_RETRY_DELAY: Duration = Duration::from_millis(50);
const PENDING_PROOF_RETRIES: usize = 10;

/// How a peer was observed to be reachable.
#[derive(Debug, Clone, Copy)]
pub struct PeerReachability {
    /// Interface the peer was seen on.
    pub iface: AddressHash,
    /// Link the observation came from, so it can be invalidated when that
    /// link closes.
    pub link_id: LinkId,
}

/// Record how `peer` was seen reachable over `iface`.
///
/// `own_address` is this client's own delivery address. A link activation whose
/// address is that one is an *in*-link: the SDK fills the event's address from
/// the link's destination, which for an in-link is our own destination rather
/// than the remote. Recording it would file this client as its own peer, so it
/// is ignored — an in-link's peer is not known until a message arrives on it,
/// and [`record_reachability`] is then called with the message source hash.
fn record_reachability(
    peers: &mut HashMap<AddressHash, PeerReachability>,
    peer: AddressHash,
    own_address: &AddressHash,
    iface: AddressHash,
    link_id: LinkId,
) {
    if &peer == own_address {
        return;
    }
    peers.insert(peer, PeerReachability { iface, link_id });
}

/// Forget every peer whose reachability was observed through `link_id`.
///
/// Without this the map is a latch: an entry written once would outlive the
/// link that produced it, and the UI would keep reporting the peer as
/// reachable over an interface it is no longer on.
fn forget_link(peers: &mut HashMap<AddressHash, PeerReachability>, link_id: LinkId) {
    peers.retain(|_, reach| reach.link_id != link_id);
}

/// Events the LXMF client emits to the rest of the application.
#[derive(Debug, Clone)]
pub enum LxmfEvent {
    /// A new inbound message was received, verified and stored.
    MessageReceived(Arc<LxmfMessage>),
    /// An outbound message was handed to the transport for delivery.
    MessageSent(Arc<LxmfMessage>),
    /// A peer announced its `lxmf/delivery` destination (a chat contact).
    ///
    /// `stamp_cost` is the stamp cost the peer requires from senders, as
    /// announced. `None` means the peer accepts unstamped messages.
    ContactDiscovered {
        address: [u8; 16],
        name: Option<String>,
        hops: u8,
        stamp_cost: Option<u64>,
    },
    /// A link to or from a peer became active.
    PeerConnected(AddressHash),
    /// A link to or from a peer closed.
    PeerDisconnected(AddressHash),
    /// A previously sent message was acknowledged by the peer's link layer.
    ///
    /// This confirms the peer's delivery destination received and decrypted
    /// the packet. It does *not* confirm the peer parsed, accepted, or
    /// displayed the message — the proof is issued before the receiving
    /// application sees the payload. A peer configured to require LXMF stamps
    /// issues this proof and then discards the message anyway.
    Acknowledged([u8; 32]),
    /// A sent message changed delivery state.
    ///
    /// Emitted for every transition the user can observe, including the ones
    /// that are not failures — previously `Sent` and `Failed` were applied to
    /// the store silently, so anything rendering from the store kept showing
    /// the message as pending forever.
    DeliveryStateChanged { message_id: [u8; 32], state: Delivery },
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
    display_name: std::sync::Mutex<String>,
    delivery: Arc<Mutex<SingleInputDestination>>,
    /// The delivery destination's address hash — the LXMF address peers use
    /// to send us messages.
    delivery_address: AddressHash,
    store: Arc<std::sync::Mutex<MessageStore>>,
    events: broadcast::Sender<LxmfEvent>,
    /// Outbound links we established per peer, keyed by peer address hash.
    links: Mutex<HashMap<AddressHash, LinkId>>,
    /// In-links a peer opened to *our* delivery destination, keyed by that
    /// peer's own address hash (learned from the messages it sends).
    ///
    /// These are the "backchannel" links the reference LXMF router keeps: a
    /// peer that can reach us has necessarily proved it holds a route to us,
    /// but it need never have announced its own delivery destination to us. On
    /// a link such as LoRa, where announces frequently do not propagate, that
    /// is the normal case — replying over the link the peer already opened is
    /// then the only way back.
    in_links: Mutex<HashMap<AddressHash, LinkId>>,
    /// The interface each peer was last seen reachable over, keyed by peer
    /// address hash. Populated from the interface id carried on link events,
    /// together with the link the observation came from, so the mapping can be
    /// dropped when that link goes away. Without the link id this map is a
    /// latch: an entry recorded once would survive every disconnect and still
    /// claim the peer is reachable over an interface it is no longer on.
    peer_ifaces: std::sync::Mutex<HashMap<AddressHash, PeerReachability>>,
    /// LXMF delivery destinations discovered from announces, in discovery order.
    discovered: Mutex<Vec<AddressHash>>,
    /// Stamp cost each peer requires from senders, learned from its announce.
    ///
    /// Only peers that demand a non-zero cost are recorded, so an absent entry
    /// means "accepts unstamped messages" — the common case, and the one that
    /// needs no bookkeeping.
    stamp_costs: Mutex<HashMap<AddressHash, u64>>,
    /// Messages that were handed to the transport but not yet confirmed,
    /// keyed by the transport packet hash whose proof will confirm them, with
    /// the time the entry was created so unanswered entries can be pruned.
    pending_proofs: Mutex<HashMap<Hash, ([u8; 32], Instant)>>,
    /// Hashes of recently received messages. The reference implementation keeps
    /// a "locally delivered" cache and ignores already-received messages; a
    /// sender that retransmits an unconfirmed message must not surface as a
    /// duplicate. Bounded to a small FIFO window.
    seen: Mutex<VecDeque<[u8; 32]>>,
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
        let delivery_address = delivery_address_for(&identity);
        Self {
            transport,
            identity,
            display_name: std::sync::Mutex::new(display_name.into()),
            delivery,
            delivery_address,
            store,
            events,
            links: Mutex::new(HashMap::new()),
            in_links: Mutex::new(HashMap::new()),
            peer_ifaces: std::sync::Mutex::new(HashMap::new()),
            discovered: Mutex::new(Vec::new()),
            stamp_costs: Mutex::new(HashMap::new()),
            pending_proofs: Mutex::new(HashMap::new()),
            seen: Mutex::new(VecDeque::new()),
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
        self.delivery_address
    }

    /// Our display name, as announced to peers.
    pub fn display_name(&self) -> String {
        self.display_name.lock().unwrap().clone()
    }

    /// Update the display name announced to peers. Takes effect on the next
    /// [`LxmfClient::announce`].
    pub fn set_display_name(&self, name: impl Into<String>) {
        *self.display_name.lock().unwrap() = name.into();
    }

    /// Announce the delivery identity so peers can discover a path to us.
    pub async fn announce(&self) {
        let app_data = delivery_app_data(Some(&self.display_name.lock().unwrap()));
        let address = self.delivery.lock().await.desc.address_hash;
        info!("lxmf: announcing delivery destination {address}");
        self.transport
            .send_announce(&self.delivery, Some(&app_data))
            .await;
        debug!(
            "lxmf: announced delivery destination {}",
            self.identity.address_hash()
        );
    }

    /// The interface `peer` was last seen reachable over, if known.
    ///
    /// `None` means there is no live link to that peer right now; the entry is
    /// removed when the link it was observed on closes.
    pub fn peer_interface(&self, peer: AddressHash) -> Option<AddressHash> {
        self.peer_ifaces.lock().unwrap().get(&peer).map(|r| r.iface)
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

    /// The stamp cost `peer` requires from senders, if it announced one.
    ///
    /// `None` means the peer accepts unstamped messages, or that no announce
    /// from it has been seen yet.
    pub async fn stamp_cost(&self, peer: AddressHash) -> Option<u64> {
        self.stamp_costs.lock().await.get(&peer).copied()
    }

    /// Record the stamp cost from a peer announce, dropping any stale entry.
    ///
    /// Re-announced on a timer, so this is also how a peer that *lowers* its
    /// stamp cost is noticed.
    async fn record_stamp_cost(&self, peer: AddressHash, cost: Option<u64>) {
        let mut costs = self.stamp_costs.lock().await;
        match cost {
            Some(cost) if cost > 0 => {
                costs.insert(peer, cost);
            }
            _ => {
                costs.remove(&peer);
            }
        }
    }

    /// The long-running discovery loop. Watches for `lxmf/delivery` announces
    /// so chat contacts show up without requiring an inbound message first.
    pub async fn run_discovery(&self) -> Result<(), LxmfError> {
        let mut announces = self.transport.recv_announces().await;
        while let Ok(announce) = announces.recv().await {
            if Self::is_lxmf_delivery(&announce).await {
                let address = announce.destination.lock().await.desc.address_hash;
                let fields = delivery_announce_fields(announce.app_data.as_slice());
                let name = fields.name;
                let stamp_cost = fields.stamp_cost;
                self.record_stamp_cost(address, stamp_cost).await;
                let mut seen = self.discovered.lock().await;
                if !seen.contains(&address) {
                    seen.push(address);
                    info!("lxmf: discovered contact {address} ({name:?}) at {} hops, stamp cost {stamp_cost:?}", announce.hops);
                    let _ = self.events.send(LxmfEvent::ContactDiscovered {
                        address: address.as_slice().try_into().unwrap(),
                        name,
                        hops: announce.hops,
                        stamp_cost,
                    });
                } else {
                    // Already known, but its advertised requirements may have
                    // changed since the last announce.
                    drop(seen);
                    let _ = self.events.send(LxmfEvent::ContactDiscovered {
                        address: address.as_slice().try_into().unwrap(),
                        name: None,
                        hops: announce.hops,
                        stamp_cost,
                    });
                }
            }
        }
        Ok(())
    }

    /// Send a message to a peer by LXMF address hash.
    ///
    /// Transmits over a direct link to the recipient — one we established
    /// earlier, one the peer opened to us, or a freshly established one.
    /// Returns the message hash once the peer has **acknowledged** it; see
    /// [`Delivery::Acknowledged`] for exactly what that does and does not
    /// prove.
    ///
    /// The peer acknowledges each message it accepts over a link, and that
    /// acknowledgement clears the message's pending state. It is advisory, not
    /// a delivery guarantee, and it is not treated as one: proof matching does
    /// not reliably interoperate with the reference implementation, so the
    /// absence of a proof never causes a retry or a failure. Only a genuine
    /// transport error (a link that closed or was lost, or no route at all)
    /// retries, up to [`SEND_ATTEMPTS`] times.
    ///
    /// A peer that announced a non-zero [`stamp_cost`](Self::stamp_cost) is
    /// refused with [`LxmfError::StampsRequired`]: this client does not
    /// implement LXMF stamping, and such a peer drops unstamped messages at
    /// the application layer — after the link layer has already proved them.
    /// Sending anyway would burn a link establishment, report success, and
    /// deliver nothing.
    pub async fn send(
        &self,
        destination_hash: [u8; 16],
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<[u8; 32], LxmfError> {
        let mut message = LxmfMessage::new(
            destination_hash,
            // The source must be our name-derived delivery address (the LXMF
            // address), not the identity hash: recipients recall the sender's
            // identity via `get_out_destination(source_hash)`, which only
            // knows name-derived destination hashes.
            self.delivery_address.as_slice().try_into().unwrap(),
            title.into().into_bytes(),
            content.into().into_bytes(),
        );
        let packed = message.pack(&self.identity)?;
        let hash = message.hash;
        let peer = AddressHash::new(destination_hash);

        // Store the message once, before any transmission attempt. Every
        // attempt re-sends *this* message, so pushing inside the loop would
        // show the user one copy of their message per retry — and because the
        // copies share a hash, only one of them would ever be updated when
        // the delivery state changed, leaving the rest stuck as pending.
        let stored = self.store.lock().unwrap().push(message, Direction::Outbound);
        let _ = self.events.send(LxmfEvent::MessageSent(stored));

        if let Some(cost) = self.stamp_cost(peer).await {
            if cost > 0 {
                let reason = LxmfError::StampsRequired(cost).to_string();
                warn!("lxmf: refusing to send to {peer}: {reason}");
                self.store.lock().unwrap().set_delivery(hash, Delivery::Failed);
                let _ = self.events.send(LxmfEvent::DeliveryStateChanged {
                    message_id: hash,
                    state: Delivery::Failed,
                });
                let _ = self.events.send(LxmfEvent::Undeliverable {
                    message_id: hash,
                    reason: reason.clone(),
                });
                return Err(LxmfError::StampsRequired(cost));
            }
        }

        let mut last_error = None;
        for attempt in 1..=SEND_ATTEMPTS {
            match self.send_packed(destination_hash, packed.clone()).await {
                Ok(packet_hash) => {
                    self.track_pending_proof(packet_hash, hash).await;

                    if self.await_acknowledgement(hash).await {
                        return Ok(hash);
                    }

                    // The message was written to an established link and the link
                    // did not report an error, so it was sent.
                    //
                    // A missing acknowledgement is deliberately *not* treated
                    // as failure. Proof matching does not interoperate with
                    // the reference implementation, and a peer that does
                    // acknowledge can have the proof delayed well past any
                    // timeout on a slow interface. Retrying on that basis
                    // would multiply deliveries and report failure for
                    // messages that actually arrived.
                    self.store.lock().unwrap().set_delivery(hash, Delivery::Sent);
                    let _ = self.events.send(LxmfEvent::DeliveryStateChanged {
                        message_id: hash,
                        state: Delivery::Sent,
                    });
                    info!("lxmf: sent to {peer}, no acknowledgement seen");
                    return Ok(hash);
                }
                Err(e) => {
                    warn!("lxmf: send to {peer} failed (attempt {attempt}/{SEND_ATTEMPTS}): {e}");
                    last_error = Some(e);
                    // The link for this peer is suspect (closed, stale, or
                    // never established); drop it so the next attempt builds a
                    // fresh one instead of waiting on a dead link.
                    self.drop_links_to(peer).await;
                    if attempt < SEND_ATTEMPTS {
                        tokio::time::sleep(SEND_RETRY_DELAY).await;
                    }
                }
            }
        }

        info!("lxmf: giving up sending to {peer} after {SEND_ATTEMPTS} attempts");
        // A message we have given up on must stop reading as "still sending".
        let error = last_error.expect("at least one attempt was made");
        self.store.lock().unwrap().set_delivery(hash, Delivery::Failed);
        let _ = self.events.send(LxmfEvent::DeliveryStateChanged {
            message_id: hash,
            state: Delivery::Failed,
        });
        let _ = self.events.send(LxmfEvent::Undeliverable {
            message_id: hash,
            reason: error.to_string(),
        });
        Err(error)
    }

    /// Wait for the peer's acknowledgement of `message_hash`.
    ///
    /// Returns `true` once it arrives. A link that silently dropped the packet
    /// produces no error elsewhere in the stack, so this is the only way to
    /// find out that it did.
    async fn await_acknowledgement(&self, message_hash: [u8; 32]) -> bool {
        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let state = self.store.lock().unwrap().delivery_of(message_hash);
            if state == Some(Delivery::Acknowledged) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(ACK_POLL).await;
        }
    }

    /// Forget every cached link to `peer`, so the next send re-establishes one.
    async fn drop_links_to(&self, peer: AddressHash) {
        self.links.lock().await.remove(&peer);
        self.in_links.lock().await.remove(&peer);
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
                        debug!("lxmf: in-link event");
                        self.handle_link_event(ev).await;
                    }
                }
                ev = out_links.recv() => {
                    if let Ok(ev) = ev {
                        debug!("lxmf: out-link event");
                        self.handle_link_event(ev).await;
                    }
                }
                rd = data.recv() => {
                    if let Ok(rd) = rd {
                        info!("lxmf: received data packet len={}", rd.data.len());
                        self.handle_received_data(rd).await;
                    }
                }
            }
        }
    }

    /// Transmit a packed LXMF message to a peer.
    ///
    /// Returns the transport packet hash, which is what the recipient's
    /// link-layer proof will be issued against — the handle used to tell
    /// "handed to the transport" apart from "confirmed delivered".
    async fn send_packed(
        &self,
        destination_hash: [u8; 16],
        packed: Vec<u8>,
    ) -> Result<Hash, LxmfError> {
        let peer = AddressHash::new(destination_hash);
        let link_id = self.ensure_link(peer).await?;

        let transport = self.transport.clone();
        timeout(LINK_ACTIVATION_TIMEOUT, async move {
            let mut out_links = transport.out_link_events();
            loop {
                // Re-read the shared link state on every pass rather than
                // relying on the `Activated` broadcast alone. The transport
                // updates the `Link` object as it processes the proof, and it
                // can do so in the window between `ensure_link` returning and
                // this task subscribing to `out_link_events()` — a receiver
                // created after an event was broadcast never sees that event,
                // so without this check a link that came up quickly would sit
                // here until the timeout and the message would be lost.
                //
                // The status is read into a local first: temporaries in a
                // scrutinee expression live until the end of the whole
                // `match`, so locking the link again inside an arm would
                // deadlock on the link's own mutex and wedge every task that
                // needs that link.
                let status = match Self::find_any_link(&transport, &link_id).await {
                    Some(link) => link.lock().await.status(),
                    None => return Err(LxmfError::LinkLost),
                };
                match status {
                    LinkStatus::Active => {
                        let link = Self::find_any_link(&transport, &link_id).await
                            .ok_or(LxmfError::LinkLost)?;
                        let packet = link.lock().await.data_packet(&packed)?;
                        let packet_hash = packet.hash();
                        transport.send_packet(packet).await;
                        trace!(
                            "lxmf: sent {} bytes over link {link_id} as {packet_hash}",
                            packed.len()
                        );
                        return Ok(packet_hash);
                    }
                    LinkStatus::Closed => return Err(LxmfError::LinkClosed),
                    // Pending/Handshake/Stale: keep waiting for the proof.
                    _ => {}
                }

                tokio::select! {
                    ev = out_links.recv() => match ev {
                        Ok(ev) => {
                            if ev.id == link_id && matches!(ev.event, LinkEvent::Closed) {
                                return Err(LxmfError::LinkClosed);
                            }
                        }
                        // Events were dropped, not lost: the link is still the
                        // authority on its state. Aborting here would turn a
                        // momentarily busy event channel into a lost message.
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => return Err(LxmfError::LinkClosed),
                    },
                    _ = tokio::time::sleep(LINK_ACTIVATION_POLL) => {}
                }
            }
        })
        .await
        .map_err(|_| LxmfError::SendTimeout)?
    }

    /// Ensure an outbound link to `peer` exists; create one if needed.
    async fn ensure_link(&self, peer: AddressHash) -> Result<LinkId, LxmfError> {
        // Reuse a cached link only while it is still alive. A link that
        // closed (or whose `Closed` event was missed) must not be reused:
        // `send_packed` would otherwise wait out `LINK_ACTIVATION_TIMEOUT` on
        // a dead link for every message instead of establishing a fresh one.
        //
        // The cached id is copied out and the guard dropped before anything
        // else runs: the temporary guard in an `if let` scrutinee lives until
        // the end of the whole `if let`, so locking again inside the body
        // would deadlock on this non-reentrant mutex — permanently wedging
        // every send and link-event handler for this peer.
        let cached = { self.links.lock().await.get(&peer).copied() };
        if let Some(link_id) = cached {
            if self.link_is_active(&link_id).await {
                return Ok(link_id);
            }
            self.links.lock().await.remove(&peer);
            debug!("lxmf: cached link {link_id} to {peer} is dead, re-establishing");
        }

        // A peer that has opened a link to our delivery destination has proved it
        // holds a route to us, and that link is a way back to it.
        //
        // It is a *last* resort, though. A receiver only installs its delivery
        // callback on links it did not initiate: the reference router wires an
        // out-link's established-callback to its own bookkeeping, not to the
        // handler that installs the packet callback, and that handler only
        // ever fires on the responder side. Data written to a link the peer
        // opened can therefore be dropped in silence — no error, no proof —
        // which looks exactly like a lost packet and is impossible to tell
        // apart from one.
        //
        // So: always prefer establishing our own link when we know where the
        // peer is, and only fall back to its link when we have no route at all.
        if self.transport.get_out_destination(&peer).await.is_none() {
            let backchannel = { self.in_links.lock().await.get(&peer).copied() };
            if let Some(link_id) = backchannel {
                if self.link_is_active(&link_id).await {
                    debug!("lxmf: no known destination for {peer}, replying over peer-initiated link {link_id}");
                    return Ok(link_id);
                }
                self.in_links.lock().await.remove(&peer);
                debug!("lxmf: peer-initiated link {link_id} from {peer} is dead");
            }
        }

        // The transport prunes its destination cache aggressively on embedded
        // targets (120s without a re-announce), so a destination we saw
        // earlier can be gone by the time the user sends. When that happens,
        // ask the mesh for a fresh path and wait for the path response /
        // announce to re-install the destination, instead of failing the send
        // outright. `request_path` is rate-limited per destination (20s), so
        // repeated sends cannot spam the network.
        let desc = match self.transport.get_out_destination(&peer).await {
            Some(dest) => dest.lock().await.desc,
            None => {
                info!("lxmf: no known path to {peer}, requesting one");
                self.transport.request_path(&peer, None, None).await;
                let deadline = Instant::now() + PATH_REDISCOVERY_TIMEOUT;
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if let Some(dest) = self.transport.get_out_destination(&peer).await {
                        info!("lxmf: path to {peer} rediscovered");
                        break dest.lock().await.desc;
                    }
                    if Instant::now() >= deadline {
                        info!("lxmf: still no path to {peer} after {PATH_REDISCOVERY_TIMEOUT:?}");
                        return Err(LxmfError::NoPathToDestination(
                            peer.as_slice().try_into().unwrap(),
                        ));
                    }
                }
            }
        };

        // Request a path so the link request below can be routed directly
        // rather than broadcast (fresh path, faster establishment).
        self.transport.request_path(&peer, None, None).await;

        let link = self.transport.link(desc).await;
        let link_id = *link.lock().await.id();

        self.links.lock().await.insert(peer, link_id);
        debug!("lxmf: established outbound link {} to {}", link_id, peer);
        Ok(link_id)
    }

    /// Resolve a link id to its transport link, whether it was opened by us or by
    /// the peer.
///
/// Replies may travel over a link the peer opened to us, so every lookup has
    /// to consider both tables rather than just the out-link one.
    async fn find_any_link(
        transport: &Transport,
        link_id: &LinkId,
    ) -> Option<Arc<tokio::sync::Mutex<Link>>> {
        match transport.find_out_link(link_id).await {
            Some(link) => Some(link),
            None => transport.find_in_link(link_id).await,
        }
    }

    /// True if the link with `link_id` is still Active.
    ///
    /// Only `Active` counts. A `Stale` link is one the far end has stopped
    /// answering on, and writing to it succeeds locally while the packet goes
    /// nowhere — exactly the failure this client must not mistake for a send.
    async fn link_is_active(&self, link_id: &LinkId) -> bool {
        match self.find_link(link_id).await {
            Some(link) => link.lock().await.status() == LinkStatus::Active,
            None => false,
        }
    }

    /// Resolve a link id to its transport link, whether it was opened by us or
    /// by the peer.
    async fn find_link(&self, link_id: &LinkId) -> Option<Arc<tokio::sync::Mutex<Link>>> {
        Self::find_any_link(&self.transport, link_id).await
    }

    /// Remember that `packet_hash` is awaiting a delivery proof for
    /// `message_hash`.
    ///
    /// Entries for proofs that never arrive are pruned after
    /// [`PENDING_PROOF_TTL`], so a peer that silently drops messages cannot
    /// grow this map without bound.
    async fn track_pending_proof(&self, packet_hash: Hash, message_hash: [u8; 32]) {
        let now = Instant::now();
        let mut pending = self.pending_proofs.lock().await;
        pending.retain(|_, (_, tracked)| now.duration_since(*tracked) < PENDING_PROOF_TTL);
        pending.insert(packet_hash, (message_hash, now));
    }

    /// Mark a message acknowledged once its link-layer proof arrives.
    ///
    /// A recipient proves every packet it accepts over a link, so this is the
    /// first point at which "the bytes were handed to the transport" becomes
    /// "the peer's delivery destination took delivery of the bytes". It is
    /// still not evidence that the peer parsed, accepted, or displayed the
    /// message — see [`Delivery::Acknowledged`].
    async fn handle_link_proof(&self, packet_hash: Hash) {
        // The message is registered as pending just after `send_packed`
        // returns, so on a fast link the proof can come back before that
        // bookkeeping has run. Retry briefly rather than dropping the proof
        // on the floor, which would leave the message pending forever.
        for _ in 0..PENDING_PROOF_RETRIES {
            let Some((message_hash, _)) =
                self.pending_proofs.lock().await.get(&packet_hash).copied()
            else {
                // Proofs also cover keepalives and other traffic; only the
                // ones we are waiting on are interesting.
                return;
            };
            if self.store.lock().unwrap().set_delivery(message_hash, Delivery::Acknowledged) {
                self.pending_proofs.lock().await.remove(&packet_hash);
                debug!("lxmf: acknowledged by peer: {message_hash:02x?}");
                let _ = self.events.send(LxmfEvent::Acknowledged(message_hash));
                return;
            }
            tokio::time::sleep(PENDING_PROOF_RETRY_DELAY).await;
        }
        self.pending_proofs.lock().await.remove(&packet_hash);
    }

    /// Handle an incoming link event (inbound or outbound).
    async fn handle_link_event(&self, ev: LinkEventData) {
        match ev.event {
            LinkEvent::Data(payload) => {
                self.handle_link_data(ev.id, ev.iface, payload.as_slice()).await;
            }
            LinkEvent::Activated => {
                // The link event carries the interface the link is carried on,
                // so record how the peer is reachable for the UI.
                //
                // `address_hash` is the *destination* of the link. For an
                // out-link that is the remote peer, but for an in-link it is
                // our own delivery destination — recording that would list us
                // as our own peer and report a bogus reachability. An in-link
                // has no peer address until a message arrives on it, so it is
                // recorded from the message source hash in `handle_link_data`.
                if let Some(iface) = ev.iface {
                    let mut peers = self.peer_ifaces.lock().unwrap();
                    record_reachability(
                        &mut peers,
                        ev.address_hash,
                        &self.delivery_address,
                        iface,
                        ev.id,
                    );
                }
                if let Some(peer) = self.peer_for_link(ev.id).await {
                    debug!("lxmf: peer connected: {}", peer);
                    let _ = self.events.send(LxmfEvent::PeerConnected(peer));
                }
            }
            LinkEvent::Proof(packet_hash) => {
                self.handle_link_proof(packet_hash).await;
            }
            LinkEvent::Closed => {
                let peer = self.peer_for_link(ev.id).await;
                self.links.lock().await.retain(|_, id| *id != ev.id);
                self.in_links.lock().await.retain(|_, id| *id != ev.id);
                // Drop reachability recorded through this link. For an in-link
                // the mapping was filed under the remote's address (which the
                // link event does not carry), so match on the recorded link id
                // rather than on a peer address.
                forget_link(&mut self.peer_ifaces.lock().unwrap(), ev.id);
                if let Some(peer) = peer {
                    let _ = self.events.send(LxmfEvent::PeerDisconnected(peer));
                }
            }
            _ => {}
        }
    }

    /// Attempt to parse link payload bytes as an LXMF message.
    async fn handle_link_data(&self, link_id: LinkId, iface: Option<AddressHash>, payload: &[u8]) {
        debug!("lxmf: link data arrived ({len} bytes)", len = payload.len());
        let Some(message) = self.unpack_message(payload).await else {
            debug!("lxmf: link data could not be unpacked");
            return;
        };

        // Record the interface the sender is reachable over (for in-links the
        // link's destination is our own delivery address, so the peer address
        // comes from the message source).
        if let Some(iface) = iface {
            let mut peers = self.peer_ifaces.lock().unwrap();
            record_reachability(
                &mut peers,
                AddressHash::new(message.source_hash),
                &self.delivery_address,
                iface,
                link_id,
            );
        }

        // Remember the link this peer reached us on, so a reply can go back
        // over it instead of needing a path to the peer's own destination.
        self.in_links
            .lock()
            .await
            .insert(AddressHash::new(message.source_hash), link_id);

        // Only accept messages addressed to this client's LXMF address.
        let is_ours = message.destination_hash == self.delivery_address.as_slice()[..];
        if !is_ours {
            info!(
                "lxmf: dropping message not addressed to us (dst={:02x?} ours={:02x?})",
                message.destination_hash,
                self.delivery_address.as_slice(),
            );
            return;
        }

        if !message.signature_validated {
            warn!("lxmf: message signature could not be validated");
        }

        if !self.record_seen(message.hash).await {
            debug!("lxmf: ignoring already-received message (dup={:02x?})", message.hash);
            return;
        }

        let stored = self.store.lock().unwrap().push(message, Direction::Inbound);
        let _ = self.events.send(LxmfEvent::MessageReceived(stored));
    }

    /// Handle a direct (opportunistic) packet received outside a link.
    async fn handle_received_data(&self, rd: ReceivedData) {
        debug!("lxmf: opportunistic data from {:02x?} len={}", rd.destination.as_slice(), rd.data.len());
        // The reference implementation re-prepends the packet's destination
        // hash for opportunistic deliveries.
        let mut data = Vec::with_capacity(16 + rd.data.len());
        data.extend_from_slice(rd.destination.as_slice());
        data.extend_from_slice(rd.data.as_slice());

        let Some(message) = self.unpack_message(&data).await else {
            debug!("lxmf: opportunistic data could not be unpacked");
            return;
        };

        if message.destination_hash != self.delivery_address.as_slice()[..] {
            debug!("lxmf: opportunistic data not addressed to us");
            return;
        }

        if !self.record_seen(message.hash).await {
            debug!("lxmf: ignoring already-received message (dup={:02x?})", message.hash);
            return;
        }

        let stored = self.store.lock().unwrap().push(message, Direction::Inbound);
        let _ = self.events.send(LxmfEvent::MessageReceived(stored));
    }

    /// Record a received message hash, returning `false` (and not re-recording)
    /// when the hash was already seen. The cache is a small bounded FIFO so it
    /// cannot grow without limit.
    async fn record_seen(&self, hash: [u8; 32]) -> bool {
        const MAX_SEEN: usize = 256;
        let mut seen = self.seen.lock().await;
        if seen.contains(&hash) {
            return false;
        }
        if seen.len() >= MAX_SEEN {
            seen.pop_front();
        }
        seen.push_back(hash);
        true
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

/// The address of the `lxmf/delivery` destination for `identity` (our LXMF
/// address), mirroring how the SDK derives a named destination's address.
///
/// This is the address peers use to send us messages. Used by the client and
/// by the application to show/refresh the address after an identity change.
pub fn delivery_address_for(identity: &PrivateIdentity) -> AddressHash {
    let name = DestinationName::new(APP_NAME, DELIVERY_ASPECT);
    AddressHash::new_from_hash(&Hash::new(
        Hash::generator()
            .chain_update(name.as_name_hash_slice())
            .chain_update(identity.as_address_hash_slice())
            .finalize()
            .into(),
    ))
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

/// The parts of an `lxmf/delivery` announce app-data blob this client uses.
struct DeliveryAnnounce {
    /// Announced display name, if the peer set a non-empty one.
    name: Option<String>,
    /// Stamp cost the peer requires from senders. `None` = accepts unstamped.
    stamp_cost: Option<u64>,
}

/// Decode an `lxmf/delivery` announce app-data blob.
///
/// Modern announces are a msgpack list `[display_name, stamp_cost,
/// [supported_functionality]]`; the pre-0.5.0 format was the bare display name
/// as raw bytes. Both are handled, mirroring the reference decoder, because the
/// stamp cost is the only way to know in advance that a peer will silently
/// discard everything we send it.
fn delivery_announce_fields(app_data: &[u8]) -> DeliveryAnnounce {
    let empty = DeliveryAnnounce { name: None, stamp_cost: None };
    if app_data.is_empty() {
        return empty;
    }

    // msgpack fixarray (0x90..=0x9f) or array16 (0xdc).
    let is_list = matches!(app_data[0], 0x90..=0x9f | 0xdc);
    if !is_list {
        // Legacy format: the display name as raw bytes.
        let name = String::from_utf8_lossy(app_data).trim().replace('\0', "").trim().to_string();
        return DeliveryAnnounce {
            name: (!name.is_empty()).then_some(name),
            stamp_cost: None,
        };
    }

    let Ok(Value::Array(items)) = rmpv::decode::read_value(&mut &app_data[..]) else {
        return empty;
    };

    let name = match items.first() {
        Some(Value::Binary(b)) => {
            let n = String::from_utf8_lossy(b).replace('\0', "").trim().to_string();
            (!n.is_empty()).then_some(n)
        }
        Some(Value::String(s)) => {
            let n = s.as_str().unwrap_or_default().replace('\0', "").trim().to_string();
            (!n.is_empty()).then_some(n)
        }
        _ => None,
    };

    // A cost of 0 (or a negative/oversized value) means "no stamps required",
    // which is the same as not announcing one at all.
    let stamp_cost = match items.get(1) {
        // Costs are 1..=254; 0, negatives and anything oversized are treated
        // as "no stamps required", matching the reference decoder.
        Some(Value::Integer(v)) => match v.as_u64() {
            Some(cost) if (1..255).contains(&cost) => Some(cost),
            _ => None,
        },
        _ => None,
    };

    DeliveryAnnounce { name, stamp_cost }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(b: u8) -> AddressHash {
        AddressHash::new([b; 16])
    }

    /// Announce app data as the reference encodes it: a msgpack list
    /// `[display_name, stamp_cost, [supported_functionality]]`.
    fn announce_bytes(name: Option<&str>, stamp_cost: Option<u64>) -> Vec<u8> {
        let name = name
            .map(|n| Value::Binary(n.as_bytes().to_vec()))
            .unwrap_or(Value::Nil);
        let cost = match stamp_cost {
            Some(c) => Value::Integer(c.into()),
            None => Value::Nil,
        };
        rmp_serde::to_vec(&vec![name, cost, Value::Array(vec![])]).expect("app data")
    }

    #[test]
    fn an_unstamped_announce_reports_no_stamp_cost() {
        // What this client itself announces: no stamps required of senders.
        let data = announce_bytes(Some("Reticula"), None);
        let fields = delivery_announce_fields(&data);
        assert_eq!(fields.name.as_deref(), Some("Reticula"));
        assert_eq!(fields.stamp_cost, None);
    }

    #[test]
    fn a_stamping_announce_reports_its_cost() {
        // Byte-for-byte what the reference emits for a peer with stamp_cost=8;
        // cross-checked against `LXMF.LXMF.stamp_cost_from_app_data`.
        let data = announce_bytes(Some("rust-interop-peer"), Some(8));
        assert_eq!(data[0], 0x93, "must be a msgpack fixarray of 3");
        let fields = delivery_announce_fields(&data);
        assert_eq!(fields.name.as_deref(), Some("rust-interop-peer"));
        assert_eq!(fields.stamp_cost, Some(8));
    }

    #[test]
    fn a_zero_stamp_cost_means_no_stamps_required() {
        let fields = delivery_announce_fields(&announce_bytes(Some("peer"), Some(0)));
        assert_eq!(fields.stamp_cost, None);
    }

    #[test]
    fn an_out_of_range_stamp_cost_is_ignored() {
        // The reference clamps: costs below 1 or above 254 are treated as "no
        // stamp required" rather than trusted.
        for cost in [255u64, 1000, u64::from(u32::MAX)] {
            let fields = delivery_announce_fields(&announce_bytes(Some("peer"), Some(cost)));
            assert_eq!(fields.stamp_cost, None, "cost {cost} must not be honoured");
        }
    }

    #[test]
    fn the_legacy_announce_format_yields_a_name_but_no_cost() {
        // Pre-0.5.0 announces carried the bare display name as raw bytes.
        let fields = delivery_announce_fields(b"legacy-node");
        assert_eq!(fields.name.as_deref(), Some("legacy-node"));
        assert_eq!(fields.stamp_cost, None);
    }

    #[test]
    fn an_unusable_announce_yields_nothing_rather_than_panicking() {
        for data in [
            &b""[..],
            &b"\x92"[..],                    // truncated fixarray
            &b"\xdc\x00"[..],                // truncated array16
            &b"\x93\xc0\xc0\xc0"[..],         // array of three nils
            &b"\x93\xa1a\xa1b\xa1c"[..],     // array of three strings
        ] {
            let fields = delivery_announce_fields(data);
            assert_eq!(fields.stamp_cost, None, "{data:?} must not yield a cost");
        }
    }

    #[test]
    fn out_link_activation_records_the_peer() {
        let me = addr(1);
        let peer = addr(2);
        let (iface, link) = (addr(9), addr(8));

        let mut peers = HashMap::new();
        record_reachability(&mut peers, peer, &me, iface, link);

        assert_eq!(peers.get(&peer).unwrap().iface, iface);
        assert_eq!(peers.get(&peer).unwrap().link_id, link);
    }

    #[test]
    fn in_link_activation_does_not_record_us_as_our_own_peer() {
        // An in-link event carries *our* delivery address as the link
        // destination. Recording it would make the client list itself as a
        // reachable peer and show a bogus interface indicator for it.
        let me = addr(1);
        let (iface, link) = (addr(9), addr(8));

        let mut peers = HashMap::new();
        record_reachability(&mut peers, me, &me, iface, link);

        assert!(peers.is_empty(), "in-link activation must not be recorded");
    }

    #[test]
    fn closing_a_link_clears_its_reachability() {
        let me = addr(1);
        let peer = addr(2);
        let (iface, link) = (addr(9), addr(8));

        let mut peers = HashMap::new();
        record_reachability(&mut peers, peer, &me, iface, link);
        assert!(peers.contains_key(&peer));

        forget_link(&mut peers, link);
        assert!(!peers.contains_key(&peer), "closed link must not latch");
    }

    #[test]
    fn closing_one_link_keeps_other_peers() {
        let me = addr(1);
        let a = addr(2);
        let b = addr(3);
        let link_a = addr(8);
        let link_b = addr(7);

        let mut peers = HashMap::new();
        record_reachability(&mut peers, a, &me, addr(9), link_a);
        record_reachability(&mut peers, b, &me, addr(10), link_b);

        forget_link(&mut peers, link_a);
        assert!(!peers.contains_key(&a));
        assert!(peers.contains_key(&b), "unrelated peer must survive");
    }

    #[test]
    fn in_link_peer_is_recorded_from_the_message_source() {
        // The in-link event itself cannot name the peer; the first message on
        // it supplies the source hash. That mapping must be attributed to the
        // in-link so closing the in-link clears it.
        let me = addr(1);
        let remote = addr(2);
        let (iface, link) = (addr(9), addr(8));

        let mut peers = HashMap::new();
        // Activation on the in-link: address is us, so nothing recorded.
        record_reachability(&mut peers, me, &me, iface, link);
        assert!(peers.is_empty());

        // A message arrives: source hash names the real peer.
        record_reachability(&mut peers, remote, &me, iface, link);
        assert_eq!(peers.get(&remote).unwrap().link_id, link);

        forget_link(&mut peers, link);
        assert!(peers.is_empty(), "in-link close must clear its peer too");
    }
}
