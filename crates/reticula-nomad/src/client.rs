//! The NomadNet browser client.

use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, trace};
use rmpv::Value;
use sha2::Digest;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

use reticulum_sdk::destination::link::{LinkEvent, LinkId, LinkStatus};
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

/// Events the NomadNet browser emits to the rest of the application.
#[derive(Debug, Clone)]
pub enum NomadEvent {
    /// A `nomadnetwork/node` destination announced on the network.
    NodeDiscovered { address: AddressHash, name: Option<String> },
}

/// A NomadNet browser.
pub struct NomadClient {
    transport: Arc<Transport>,
    events: broadcast::Sender<NomadEvent>,
    /// Outbound links per node, keyed by node address.
    links: Mutex<HashMap<AddressHash, LinkId>>,
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
                    debug!("nomad: discovered node {address} ({name:?})");
                    let _ = self
                        .events
                        .send(NomadEvent::NodeDiscovered { address, name });
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

        let request_id = self
            .transport
            .link_request(link_id, path, Value::Nil)
            .await?;
        trace!("nomad: requested {path} from {node} (request {request_id})");

        let transport = self.transport.clone();
        let data = timeout(PAGE_REQUEST_TIMEOUT, async move {
            let mut out_links = transport.out_link_events();
            loop {
                let ev = out_links.recv().await.map_err(|_| NomadError::LinkClosed)?;
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
                        // The link closed without answering. This happens when
                        // the node is unreachable or tears the link down after
                        // a failed request.
                        return Err(NomadError::LinkClosed);
                    }
                    _ => {}
                }
            }
        })
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
        if let Some(&link_id) = self.links.lock().await.get(&node) {
            let still_good = self.link_is_active(&link_id).await;
            if still_good {
                return Ok(link_id);
            }
            // The cached link is gone/closed; drop it and create a fresh one.
            self.links.lock().await.remove(&node);
        }

        let desc = self
            .transport
            .get_out_destination(&node)
            .await
            .ok_or_else(|| NomadError::NoLink(node.to_hex_string()))?
            .lock()
            .await
            .desc;

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