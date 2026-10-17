//! The NomadNet browser client.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use log::{debug, trace};
use rmpv::Value;
use sha2::Digest;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

use reticulum_sdk::destination::link::{LinkEvent, LinkEventData, LinkId, LinkStatus};
use reticulum_sdk::destination::DestinationName;
use reticulum_sdk::hash::{AddressHash, Hash};
use reticulum_sdk::transport::{AnnounceEvent, Transport};

use crate::page::Page;
use crate::NomadError;

/// NomadNet application name (`nomadnetwork/node`).
pub const APP_NAME: &str = "nomadnetwork";
pub const NODE_ASPECT: &str = "node";
/// Default page path on a node.
pub const DEFAULT_PAGE_PATH: &str = "/page/index.mu";
/// How long to wait for a node to answer a page request.
pub const PAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How long to wait for an outbound link to a node to activate.
pub const LINK_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for a path request to re-install a node destination that
/// the transport pruned (the `embedded` build expires destinations 120s after
/// their last announce) before giving up on the fetch.
pub const PATH_REDISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Events the NomadNet browser emits to the rest of the application.
#[derive(Debug, Clone)]
pub enum NomadEvent {
    /// A `nomadnetwork/node` destination announced on the network.
    NodeDiscovered { address: AddressHash, name: Option<String>, hops: u8 },
}

/// A NomadNet browser.
pub struct NomadClient {
    transport: Arc<Transport>,
    events: broadcast::Sender<NomadEvent>,
    /// Outbound links per node, keyed by node address.
    links: Mutex<HashMap<AddressHash, LinkId>>,
    /// The interface each node was last seen reachable over, keyed by node
    /// address. Populated from the interface a page-fetch link is carried on.
    node_ifaces: std::sync::Mutex<HashMap<AddressHash, AddressHash>>,
    /// Discovered node addresses, in discovery order.
    discovered: Mutex<Vec<AddressHash>>,
}

impl NomadClient {
    pub fn new(transport: Arc<Transport>, event_capacity: usize) -> Self {
        let (events, _) = broadcast::channel(event_capacity);
        Self {
            transport,
            events,
            links: Mutex::new(HashMap::new()),
            node_ifaces: std::sync::Mutex::new(HashMap::new()),
            discovered: Mutex::new(Vec::new()),
        }
    }

    /// Subscribe to browser events.
    pub fn events(&self) -> broadcast::Receiver<NomadEvent> {
        self.events.subscribe()
    }

    /// The address hash of the `nomadnetwork/node` destination that `identity`
    /// would announce. Used to recognise NomadNet node announces.
    pub fn node_address_for(identity: &[u8; 16]) -> AddressHash {
        let name = DestinationName::new(APP_NAME, NODE_ASPECT);
        let full: [u8; 32] = Hash::generator()
            .chain_update(name.as_name_hash_slice())
            .chain_update(identity)
            .finalize()
            .into();
        AddressHash::new_from_hash(&Hash::new(full))
    }

    /// Whether an announce event is for a `nomadnetwork/node` destination.
    pub async fn is_nomad_node(announce: &AnnounceEvent) -> bool {
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
        let expected = Self::node_address_for(&identity_hash);
        dest.desc.address_hash == expected
    }

    /// Addresses of nodes discovered so far.
    pub async fn discovered(&self) -> Vec<AddressHash> {
        self.discovered.lock().await.clone()
    }

    /// The long-running discovery loop. Watches for node announces.
    pub async fn run(&self) -> Result<(), NomadError> {
        let mut announces = self.transport.recv_announces().await;
        while let Ok(announce) = announces.recv().await {
            if Self::is_nomad_node(&announce).await {
                let address = announce.destination.lock().await.desc.address_hash;
                let name = announce_name(&announce);
                let mut seen = self.discovered.lock().await;
                if !seen.contains(&address) {
                    seen.push(address);
                    debug!("nomad: discovered node {address} ({name:?}) at {} hops", announce.hops);
                    let _ = self
                        .events
                        .send(NomadEvent::NodeDiscovered { address, name, hops: announce.hops });
                }
            }
        }
        Ok(())
    }

    /// Fetch a page from a node.
    ///
    /// `path` defaults to [`DEFAULT_PAGE_PATH`] when empty.
    pub async fn fetch_page(&self, node: AddressHash, path: &str) -> Result<Page, NomadError> {
        let path = if path.is_empty() {
            DEFAULT_PAGE_PATH
        } else {
            path
        };

        let link_id = self.ensure_link(node).await?;
        trace!("nomad: using link {link_id} to node {node}");

        // Wait for the link to become active before sending a request. A
        // freshly created link is `Pending`; sending on it before activation
        // will fail or be dropped. If the link never activates (no path /
        // node unreachable) it will close and we surface that cleanly.
        self.wait_for_link_active(link_id).await?;

        // The active link is bound to the interface it is carried on; record
        // it so the UI can show how the node is reachable.
        if let Some(iface) = self.link_interface(&link_id).await {
            self.node_ifaces.lock().unwrap().insert(node, iface);
        }

        // Subscribe to link events *before* the request goes out.
        //
        // These are a `tokio::sync::broadcast` channel, and a receiver created
        // after a send only ever sees events sent after it subscribed. But
        // `link_request` awaits `send_packet`, which yields to the executor for
        // the network write, so a node that answers inside that window has its
        // `LinkEvent::Response` broadcast before we would have subscribed. That
        // response is then lost permanently: unlike link activation there is no
        // shared state to re-read, so the loop below would sit waiting for an
        // event that already happened and report a `Timeout` after the full
        // `PAGE_REQUEST_TIMEOUT`, on a link that delivered the page instantly.
        //
        // Subscribing first closes the window. Events for other links, and for
        // other requests on this one, are buffered in the receiver's ring and
        // filtered by the `link_id` / `request_id` checks in the loop.
        let out_links = self.transport.out_link_events();

        let request_id = self
            .transport
            .link_request(link_id, path, Value::Nil)
            .await?;
        trace!("nomad: requested {path} from {node} (request {request_id})");

        let data = timeout(
            PAGE_REQUEST_TIMEOUT,
            wait_for_response(out_links, link_id, request_id),
        )
        .await
        .map_err(|_| NomadError::Timeout)??;

        match data {
            Value::Binary(bytes) => Ok(Page::from_bytes(&bytes, node.to_hex_string())),
            Value::String(s) => Ok(Page::from_bytes(
                s.as_str().unwrap_or("").as_bytes(),
                node.to_hex_string(),
            )),
            _ => Err(NomadError::InvalidResponse),
        }
    }

    /// Wait for an outbound link to become active.
    ///
    /// If the link is already active this returns immediately. Otherwise it
    /// subscribes to link events *and* polls the link status (the transport
    /// updates the shared `Link` object as it processes the link proof), so
    /// an `Activated` event emitted before we subscribed is still caught.
    async fn wait_for_link_active(&self, link_id: LinkId) -> Result<(), NomadError> {
        let transport = self.transport.clone();
        timeout(LINK_ACTIVATION_TIMEOUT, async move {
            let mut out_links = transport.out_link_events();
            loop {
                // Fast-path / poll: is the link active yet?
                if let Some(link) = transport.find_out_link(&link_id).await {
                    match link.lock().await.status() {
                        LinkStatus::Active => return Ok(()),
                        LinkStatus::Closed => return Err(NomadError::LinkClosed),
                        _ => {}
                    }
                }

                // Also react to link events.
                tokio::select! {
                    ev = out_links.recv() => {
                        match ev {
                            Ok(ev) if ev.id == link_id => {
                                match ev.event {
                                    LinkEvent::Activated => return Ok(()),
                                    LinkEvent::Closed => return Err(NomadError::LinkClosed),
                                    _ => {}
                                }
                            }
                            Err(_) => return Err(NomadError::LinkClosed),
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Polling interval; loop back to the status check.
                    }
                }
            }
        })
        .await
        .map_err(|_| NomadError::Timeout)?
    }

    /// Ensure an outbound link to `node` exists.
    async fn ensure_link(&self, node: AddressHash) -> Result<LinkId, NomadError> {
        // Only reuse a link that is not closed.
        //
        // The cached id is copied out and the guard dropped first: the
        // temporary guard in an `if let` scrutinee lives until the end of the
        // whole `if let`, so the `remove()` below would deadlock on this
        // non-reentrant mutex and wedge every fetch and link-event handler for
        // this node.
        let cached = { self.links.lock().await.get(&node).copied() };
        if let Some(link_id) = cached {
            let still_good = self.link_is_active(&link_id).await;
            if still_good {
                return Ok(link_id);
            }
            // The cached link is gone/closed; drop it and create a fresh one.
            self.links.lock().await.remove(&node);
        }

        // The transport prunes its destination cache aggressively on embedded
        // targets (120s without a re-announce), so a node we discovered can
        // be gone by the time the user browses. When that happens, ask the
        // mesh for a fresh path and wait for the path response / announce to
        // re-install the destination, instead of failing the fetch outright.
        // `request_path` is rate-limited per destination (20s), so repeated
        // fetches cannot spam the network.
        let desc = match self.transport.get_out_destination(&node).await {
            Some(dest) => dest.lock().await.desc,
            None => {
                self.transport.request_path(&node, None, None).await;
                let deadline = Instant::now() + PATH_REDISCOVERY_TIMEOUT;
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if let Some(dest) = self.transport.get_out_destination(&node).await {
                        break dest.lock().await.desc;
                    }
                    if Instant::now() >= deadline {
                        return Err(NomadError::NoLink(node.to_hex_string()));
                    }
                }
            }
        };

        // Request a path so the link request below can be routed directly
        // rather than broadcast (fresh path, faster establishment).
        self.transport.request_path(&node, None, None).await;

        let link = self.transport.link(desc).await;
        let link_id = *link.lock().await.id();
        self.links.lock().await.insert(node, link_id);
        debug!("nomad: established link {link_id} to node {node}");
        Ok(link_id)
    }

    /// True if the outbound link with `link_id` exists and is not closed.
    async fn link_is_active(&self, link_id: &LinkId) -> bool {
        match self.transport.find_out_link(link_id).await {
            Some(link) => link.lock().await.status() != LinkStatus::Closed,
            None => false,
        }
    }

    /// The interface the active outbound link with `link_id` is bound to, if
    /// the link exists.
    async fn link_interface(&self, link_id: &LinkId) -> Option<AddressHash> {
        match self.transport.find_out_link(link_id).await {
            Some(link) => link.lock().await.attached_interface(),
            None => None,
        }
    }

    /// The interface `node` was last seen reachable over, if known.
    pub fn node_interface(&self, node: AddressHash) -> Option<AddressHash> {
        self.node_ifaces.lock().unwrap().get(&node).copied()
    }
}

/// Wait for the node's response to `request_id` on `link_id`.
///
/// `events` must have been subscribed **before** the request was sent: the
/// channel is a `tokio::sync::broadcast`, so a receiver created afterwards
/// never sees a response the node already sent, and because a response leaves
/// no other trace to re-read, that event is simply gone — the caller would sit
/// here until `PAGE_REQUEST_TIMEOUT` expired and report a `Timeout` for a page
/// that had already arrived.
///
/// Events already buffered at entry are drained first, so a node that answered
/// while the request was still being written is served from the receiver's
/// ring rather than lost. Everything not addressed to this link or this
/// request is skipped rather than treated as an error.
async fn wait_for_response(
    mut events: broadcast::Receiver<LinkEventData>,
    link_id: LinkId,
    request_id: AddressHash,
) -> Result<Value, NomadError> {
    loop {
        let ev = events.recv().await.map_err(|_| NomadError::LinkClosed)?;
        if ev.id != link_id {
            continue;
        }
        match ev.event {
            LinkEvent::Response(response) => {
                if response.request_id == request_id {
                    return Ok(response.data);
                }
                trace!(
                    "nomad: ignored response with mismatched request id {}",
                    response.request_id
                );
            }
            LinkEvent::Closed => {
                // The link closed without answering. This happens when the node
                // is unreachable or tears the link down after a failed request.
                return Err(NomadError::LinkClosed);
            }
            _ => {}
        }
    }
}

/// Node name from an announce's app data (raw UTF-8 in the reference client).
fn announce_name(announce: &AnnounceEvent) -> Option<String> {
    let name = String::from_utf8_lossy(announce.app_data.as_slice()).into_owned();
    if name.trim().is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reticulum_sdk::destination::link::{LinkEventData, LinkResponse};

    fn addr(b: u8) -> AddressHash {
        AddressHash::new([b; 16])
    }

    fn event(link: LinkId, event: LinkEvent) -> LinkEventData {
        LinkEventData {
            id: link,
            address_hash: addr(1),
            iface: None,
            event,
        }
    }

    /// Send an event. `LinkEventData` is not `Debug`, so `send(..).unwrap()`
    /// cannot be used here.
    fn send(tx: &broadcast::Sender<LinkEventData>, ev: LinkEventData) {
        assert!(
            tx.send(ev).is_ok(),
            "no receivers left; test setup is wrong"
        );
    }

    fn response(request_id: AddressHash, payload: &str) -> LinkEvent {
        LinkEvent::Response(LinkResponse {
            request_id,
            data: Value::String(payload.into()),
        })
    }

    #[tokio::test]
    async fn a_response_that_arrived_before_the_wait_starts_is_not_lost() {
        // This is the race `fetch_page` used to lose: the node answered while
        // the request was still being written, so the response was broadcast
        // before anything was waiting for it. The only reason it survives is
        // that the caller subscribed before sending — a broadcast receiver made
        // afterwards would never see this event, and the fetch would time out
        // on a page that had already been delivered.
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        send(&tx, event(link, response(request_id, "index.mu")));

        let data = wait_for_response(rx, link, request_id).await.unwrap();
        assert_eq!(data, Value::String("index.mu".into()));
    }

    #[tokio::test]
    async fn a_response_sent_after_the_wait_starts_is_received() {
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        let waiter = tokio::spawn(async move { wait_for_response(rx, link, request_id).await });

        send(&tx, event(link, response(request_id, "late.mu")));

        let data = waiter.await.unwrap().unwrap();
        assert_eq!(data, Value::String("late.mu".into()));
    }

    #[tokio::test]
    async fn a_response_for_another_request_is_skipped() {
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        // Buffered before the wait begins, as a stale answer would be.
        send(&tx, event(link, response(addr(99), "wrong-request.mu")));
        send(&tx, event(link, response(request_id, "right.mu")));

        let data = wait_for_response(rx, link, request_id).await.unwrap();
        assert_eq!(data, Value::String("right.mu".into()));
    }

    #[tokio::test]
    async fn a_response_on_another_link_is_skipped() {
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        send(&tx, event(addr(8), response(request_id, "other-link.mu")));
        send(&tx, event(link, response(request_id, "our-link.mu")));

        let data = wait_for_response(rx, link, request_id).await.unwrap();
        assert_eq!(data, Value::String("our-link.mu".into()));
    }

    #[tokio::test]
    async fn unrelated_link_events_are_skipped() {
        // Activations, proofs and data all share the out-link channel; none of
        // them may be mistaken for the response.
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        send(&tx, event(link, LinkEvent::Activated));
        send(&tx, event(link, LinkEvent::Proof(Hash::new([4u8; 32]))));
        send(&tx, event(addr(8), LinkEvent::Activated));
        send(&tx, event(link, response(request_id, "page.mu")));

        let data = wait_for_response(rx, link, request_id).await.unwrap();
        assert_eq!(data, Value::String("page.mu".into()));
    }

    #[tokio::test]
    async fn a_closed_link_before_the_response_is_reported_as_closed() {
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel(16);

        send(&tx, event(link, LinkEvent::Closed));
        send(&tx, event(link, response(request_id, "too-late.mu")));

        assert!(matches!(
            wait_for_response(rx, link, request_id).await,
            Err(NomadError::LinkClosed)
        ));
    }

    #[tokio::test]
    async fn a_dropped_channel_is_reported_as_closed() {
        let link = addr(7);
        let request_id = addr(3);
        let (tx, rx) = broadcast::channel::<LinkEventData>(16);
        drop(tx);

        assert!(matches!(
            wait_for_response(rx, link, request_id).await,
            Err(NomadError::LinkClosed)
        ));
    }
}
