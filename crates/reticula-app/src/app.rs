//! The Reticula application: board + UI + network clients.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};

use tokio::sync::Mutex as AsyncMutex;

use reticulum_sdk::destination::{DestinationName, SingleInputDestination};
use reticulum_sdk::hash::AddressHash;
use reticulum_sdk::identity::PrivateIdentity;
use reticulum_sdk::iface::tcp_client::TcpClient;
use reticulum_sdk::iface::udp::UdpInterface;
use reticulum_sdk::transport::{Transport, TransportConfig};

use reticula_hal::{Board, Display, Keyboard, KeyCode, KeyEvent, KeyState};
use reticula_lxmf::client::{APP_NAME, DELIVERY_ASPECT};
use reticula_lxmf::{LxmfClient, LxmfEvent, MessageStore};
use reticula_nomad::client::DEFAULT_PAGE_PATH;
use reticula_nomad::{NomadClient, NomadEvent, Page};
use reticula_ui::context::{
    ChatMessage, Conversation, NetworkState, NodeEntry, ViewContext,
};
use reticula_ui::screens::chat::ChatScreen;
use reticula_ui::screens::chat_list::ChatListScreen;
use reticula_ui::screens::home::HomeScreen;
use reticula_ui::screens::new_chat::NewChatScreen;
use reticula_ui::screens::nomad_list::NomadListScreen;
use reticula_ui::screens::nomad_view::NomadViewScreen;
use reticula_ui::screens::settings::SettingsScreen;
use reticula_ui::{Command, Screen, ScreenId, Theme};

use crate::config::NetConfig;

/// Frame pacing of the UI loop.
pub const FRAME_MS: u64 = 50;
/// Maximum number of messages kept in memory.
pub const MAX_MESSAGES: usize = 512;

/// Errors produced by the application.
#[derive(Debug)]
pub enum AppError {
    /// Reticulum transport setup failed.
    Transport(String),
}

impl core::fmt::Display for AppError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AppError::Transport(e) => write!(f, "transport error: {e}"),
        }
    }
}

impl core::error::Error for AppError {}

/// Shared, render-visible application state.
///
/// Locked briefly by the render loop (synchronous) and by the async network
/// event handlers. No lock is ever held across an `await`.
#[derive(Default)]
struct SharedState {
    conversations: std::sync::Mutex<Vec<Conversation>>,
    messages: std::sync::Mutex<Vec<ChatMessage>>,
    nodes: std::sync::Mutex<Vec<NodeEntry>>,
    page: std::sync::Mutex<Option<Page>>,
    page_node: std::sync::Mutex<Option<NodeEntry>>,
    page_notice: std::sync::Mutex<String>,
    active_peer: std::sync::Mutex<Option<[u8; 16]>>,
    last_seen: std::sync::Mutex<HashMap<[u8; 16], f64>>,
    connected: AtomicBool,
    peer_links: AtomicU32,
}

/// The Reticula application.
///
/// Generic over the board so the same application runs on any supported
/// device (the desktop simulator and the T-Deck firmware are two instances).
pub struct ReticulaApp<B: Board> {
    pub board: B,
    pub theme: Theme,
    lxmf: Arc<LxmfClient>,
    nomad: Arc<NomadClient>,
    store: Arc<std::sync::Mutex<MessageStore>>,
    identity: PrivateIdentity,
    display_name: String,
    identity_hex: String,
    shared: Arc<SharedState>,
    screen: Screen,
    back_stack: Vec<Screen>,
    quit_on_root_back: bool,
    quit: bool,
}

impl<B: Board> ReticulaApp<B> {
    /// Create the application, bring up the Reticulum transport and register
    /// the LXMF delivery identity.
    pub async fn new(
        board: B,
        identity: PrivateIdentity,
        display_name: String,
        net: NetConfig,
    ) -> Result<Self, AppError> {
        let (transport, delivery) = build_transport(&identity, &net).await?;

        let store = Arc::new(std::sync::Mutex::new(MessageStore::new(MAX_MESSAGES)));
        let lxmf = Arc::new(LxmfClient::new(
            transport.clone(),
            identity.clone(),
            delivery,
            display_name.clone(),
            store.clone(),
            64,
        ));
        let nomad = Arc::new(NomadClient::new(transport.clone(), 64));
        let identity_hex = identity.to_hex_string();

        Ok(Self {
            board,
            theme: Theme::default(),
            lxmf,
            nomad,
            store,
            identity,
            display_name,
            identity_hex,
            shared: Arc::new(SharedState::default()),
            screen: Screen::Home(HomeScreen::new()),
            back_stack: Vec::new(),
            quit_on_root_back: net.quit_on_root_back,
            quit: false,
        })
    }

    /// Our LXMF identity.
    pub fn identity(&self) -> &PrivateIdentity {
        &self.identity
    }

    /// The main application loop.
    pub async fn run(&mut self) -> Result<(), AppError> {
        let mut lxmf_events = self.lxmf.events();
        let mut nomad_events = self.nomad.events();

        // Network event loops.
        tokio::spawn({
            let lxmf = self.lxmf.clone();
            async move {
                if let Err(e) = lxmf.run().await {
                    warn!("lxmf client stopped: {e}");
                }
            }
        });
        tokio::spawn({
            let nomad = self.nomad.clone();
            async move {
                if let Err(e) = nomad.run().await {
                    warn!("nomad client stopped: {e}");
                }
            }
        });

        // Periodic announce task.
        tokio::spawn({
            let lxmf = self.lxmf.clone();
            let interval = self.announce_interval();
            async move {
                lxmf.announce().await;
                loop {
                    tokio::time::sleep(interval).await;
                    lxmf.announce().await;
                }
            }
        });

        self.shared.connected.store(true, Ordering::Relaxed);

        let mut key_events = [KeyEvent::pressed(KeyCode::Unknown(0)); 16];
        let mut last_heartbeat = Instant::now();
        while !self.quit {
            // Input: keyboard plus any trackball/pointer device.
            let mut n = self.board.keyboard().read(&mut key_events);
            if let Some(trackball) = self.board.trackball() {
                n += trackball.read(&mut key_events[n..]);
            }
            for ev in key_events.iter().take(n) {
                if ev.state == KeyState::Pressed {
                    let cmd = self.screen.handle_key(ev.code);
                    self.execute(cmd);
                }
            }

            // Network events.
            while let Ok(ev) = lxmf_events.try_recv() {
                self.on_lxmf_event(ev);
            }
            while let Ok(ev) = nomad_events.try_recv() {
                self.on_nomad_event(ev);
            }

            // Render.
            self.render();

            // Periodic heartbeat so a connected serial monitor shows the app
            // is alive and healthy.
            if last_heartbeat.elapsed() >= Duration::from_secs(10) {
                log::info!(
                    "reticula: alive uptime={}s links={} msgs={}",
                    self.board.uptime_ms() / 1000,
                    self.shared.peer_links.load(Ordering::Relaxed),
                    self.store.lock().unwrap().len(),
                );
                last_heartbeat = Instant::now();
            }

            tokio::time::sleep(Duration::from_millis(FRAME_MS)).await;
        }

        Ok(())
    }

    fn announce_interval(&self) -> Duration {
        // Fixed 5-minute cadence; could come from NetConfig later.
        Duration::from_secs(300)
    }

    fn execute(&mut self, cmd: Command) {
        match cmd {
            Command::None => {}
            Command::Back => match self.back_stack.pop() {
                Some(prev) => {
                    self.screen = prev;
                    if !matches!(self.screen, Screen::Chat(_)) {
                        *self.shared.active_peer.lock().unwrap() = None;
                    }
                }
                None => {
                    // At the root (home): quit only if the platform allows.
                    self.quit = self.quit_on_root_back;
                }
            },
            Command::Quit => self.quit = true,
            Command::ShowScreen(id) => self.push_screen(id),
            Command::StartChat(peer) => {
                self.shared
                    .last_seen
                    .lock()
                    .unwrap()
                    .insert(peer, now_f64());
                *self.shared.active_peer.lock().unwrap() = Some(peer);
                self.refresh_messages();
                self.push(Screen::Chat(ChatScreen::new(peer)));
            }
            Command::SendMessage { peer, text } => {
                let lxmf = self.lxmf.clone();
                tokio::spawn(async move {
                    if let Err(e) = lxmf.send(peer, "", text).await {
                        warn!("lxmf send failed: {e}");
                    }
                });
            }
            Command::FetchPage { node, path } => self.fetch_page(node, path),
            Command::OpenNode(node) => {
                self.push(Screen::NomadView(NomadViewScreen::new(node)));
                self.fetch_page(node, DEFAULT_PAGE_PATH.to_string());
            }
            Command::Announce => {
                let lxmf = self.lxmf.clone();
                tokio::spawn(async move { lxmf.announce().await });
            }
            Command::SetDisplayName(name) => {
                self.display_name = name;
                let lxmf = self.lxmf.clone();
                tokio::spawn(async move { lxmf.announce().await });
            }
        }
    }

    fn push_screen(&mut self, id: ScreenId) {
        let screen = match id {
            ScreenId::Home => {
                self.back_stack.clear();
                self.screen = Screen::Home(HomeScreen::new());
                return;
            }
            ScreenId::ChatList => Screen::ChatList(ChatListScreen::new()),
            ScreenId::NewChat => Screen::NewChat(NewChatScreen::new()),
            ScreenId::NomadList => Screen::NomadList(NomadListScreen::new()),
            ScreenId::Settings => Screen::Settings(SettingsScreen::new()),
            ScreenId::Chat | ScreenId::NomadView => return, // opened with context
        };
        self.push(screen);
    }

    fn push(&mut self, screen: Screen) {
        // Always save the current screen so that `Back` can restore it,
        // including returning to the home menu from any sub-screen.
        self.back_stack
            .push(std::mem::replace(&mut self.screen, screen));
    }

    /// Kick off an async page fetch; results land in the shared state.
    fn fetch_page(&self, node: [u8; 16], path: String) {
        let nomad = self.nomad.clone();
        let shared = self.shared.clone();
        *shared.page.lock().unwrap() = None;
        *shared.page_notice.lock().unwrap() = format!("Fetching {path}…");
        tokio::spawn(async move {
            match nomad.fetch_page(AddressHash::new(node), &path).await {
                Ok(page) => {
                    *shared.page.lock().unwrap() = Some(page);
                    *shared.page_node.lock().unwrap() = Some(NodeEntry {
                        address: node,
                        hex: AddressHash::new(node).to_hex_string(),
                        name: String::new(),
                    });
                    *shared.page_notice.lock().unwrap() = String::new();
                }
                Err(e) => {
                    *shared.page_notice.lock().unwrap() = format!("Page error: {e}");
                }
            }
        });
    }

    fn on_lxmf_event(&mut self, ev: LxmfEvent) {
        match ev {
            LxmfEvent::MessageReceived(_) | LxmfEvent::MessageSent(_) => {
                self.refresh_conversations();
                self.refresh_messages();
            }
            LxmfEvent::PeerConnected(_) => {
                self.shared.peer_links.fetch_add(1, Ordering::Relaxed);
            }
            LxmfEvent::PeerDisconnected(_) => {
                self.shared.peer_links.fetch_sub(1, Ordering::Relaxed);
            }
            LxmfEvent::Delivered(_) => {}
            LxmfEvent::Undeliverable { message_id, reason } => {
                warn!("message {message_id:02x?} undeliverable: {reason}");
            }
        }
    }

    fn on_nomad_event(&mut self, ev: NomadEvent) {
        let NomadEvent::NodeDiscovered { address, name } = ev;
        let peer: [u8; 16] = address.as_slice().try_into().unwrap();
        let mut nodes = self.shared.nodes.lock().unwrap();
        if !nodes.iter().any(|n| n.address == peer) {
            nodes.push(NodeEntry {
                address: peer,
                hex: address.to_hex_string(),
                name: name.unwrap_or_default(),
            });
        }
    }

    /// Rebuild the conversation list from the message store.
    fn refresh_conversations(&self) {
        let store = self.store.lock().unwrap();
        let last_seen = self.shared.last_seen.lock().unwrap();

        let mut by_peer: HashMap<[u8; 16], (String, String, f64, u32)> = HashMap::new();
        for m in store.all() {
            let peer = if m.incoming { m.source_hash } else { m.destination_hash };
            let entry = by_peer.entry(peer).or_default();
            if m.timestamp >= entry.2 {
                entry.0 = m.content_string();
                entry.1 = m.title_string();
                entry.2 = m.timestamp;
            }
            if m.incoming {
                let seen = last_seen.get(&peer).copied().unwrap_or(0.0);
                if m.timestamp > seen + 0.001 {
                    entry.3 += 1;
                }
            }
        }
        drop(last_seen);

        let mut conversations: Vec<Conversation> = by_peer
            .into_iter()
            .map(|(peer, (last_content, last_title, last_ts, unread))| Conversation {
                peer,
                peer_hex: AddressHash::new(peer).to_hex_string(),
                last_content,
                last_title,
                unread,
                last_ts,
            })
            .collect();
        conversations.sort_by(|a, b| {
            b.last_ts
                .partial_cmp(&a.last_ts)
                .unwrap_or(core::cmp::Ordering::Equal)
        });

        *self.shared.conversations.lock().unwrap() = conversations;
    }

    /// Rebuild the open conversation's message list.
    fn refresh_messages(&self) {
        let peer = self.shared.active_peer.lock().unwrap();
        let peer = peer.unwrap_or([0u8; 16]);
        let store = self.store.lock().unwrap();

        let messages: Vec<ChatMessage> = store
            .for_peer(&peer)
            .iter()
            .map(|m| ChatMessage {
                incoming: m.incoming,
                title: m.title_string(),
                content: m.content_string(),
                ts: m.timestamp,
            })
            .collect();

        *self.shared.messages.lock().unwrap() = messages;
    }

    /// Render one frame and flush it to the display.
    fn render(&mut self) {
        let Self {
            board,
            theme,
            screen,
            shared,
            identity_hex,
            display_name,
            ..
        } = self;

        let conversations = shared.conversations.lock().unwrap();
        let messages = shared.messages.lock().unwrap();
        let nodes = shared.nodes.lock().unwrap();
        let page = shared.page.lock().unwrap();
        let page_node = shared.page_node.lock().unwrap();
        let page_notice = shared.page_notice.lock().unwrap();

        let ctx = ViewContext {
            conversations: &conversations,
            messages: &messages,
            nodes: &nodes,
            page: page.as_ref(),
            page_node: page_node.as_ref(),
            page_notice: &page_notice,
            identity_hex: identity_hex.as_str(),
            display_name: display_name.as_str(),
            network: NetworkState {
                connected: shared.connected.load(Ordering::Relaxed),
                uptime_ms: board.uptime_ms(),
                peer_links: shared.peer_links.load(Ordering::Relaxed),
            },
        };

        if let Some(display) = board.display() {
            screen.render(display.target(), &ctx, theme);
            display.flush();
        }
    }
}

/// Set up the Reticulum transport as a pure end client and register the LXMF
/// delivery destination.
async fn build_transport(
    identity: &PrivateIdentity,
    net: &NetConfig,
) -> Result<(Arc<Transport>, Arc<AsyncMutex<SingleInputDestination>>), AppError> {
    let mut config = TransportConfig::new("reticula", identity, false);
    // End-client only: never retransmit/forward for others.
    config.set_retransmit(false);
    config.set_reroute_eager(false);
    config.set_respond_to_probes(false);
    config.set_announce_forever(true);
    // The SDK default of 16384 pre-allocates ~8.6 MB per broadcast channel
    // (7 channels), which exceeds the T-Deck's 8 MB PSRAM. The end-client is
    // low-throughput, so a small ring is plenty.
    config.set_event_channel_capacity(512);

    let mut transport = Transport::new(config);

    match &net.transport {
        crate::TransportKind::Udp { bind, forward } => {
            let iface = UdpInterface::new(bind.to_string(), forward.clone());
            transport
                .iface_manager()
                .lock()
                .await
                .spawn(iface, UdpInterface::spawn);
            info!("reticulum: UDP interface bound to {bind}");
        }
        crate::TransportKind::TcpPeer { addr } => {
            let iface = TcpClient::new(addr.to_string());
            transport
                .iface_manager()
                .lock()
                .await
                .spawn(iface, TcpClient::spawn);
            info!("reticulum: TCP client interface to {addr}");
        }
        crate::TransportKind::None => {
            info!("reticulum: no network interface configured (offline)");
        }
    }

    // Optional LoRa radio interface (e.g. an SX1262 on the T-Deck). The config
    // carries its embedded-hal hardware provider.
    #[cfg(feature = "lora")]
    {
        use reticulum_sdk::iface::lora::LoRaInterface;
        use reticulum_sdk::iface::lora::sx1262::SX1262;
        if let Some(lora) = &net.lora {
            let iface = LoRaInterface::<SX1262>::new(lora.clone());
            transport
                .iface_manager()
                .lock()
                .await
                .spawn(iface, LoRaInterface::spawn);
            info!("reticulum: LoRa interface configured");
        }
    }

    let delivery = transport
        .add_destination(
            identity.clone(),
            DestinationName::new(APP_NAME, DELIVERY_ASPECT),
        )
        .await;
    Ok((Arc::new(transport), delivery))
}

fn now_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}