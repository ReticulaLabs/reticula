//! The Reticula application: board + UI + network clients.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};

use getrandom::SysRng;
use rand_core::UnwrapErr;

use tokio::sync::Mutex as AsyncMutex;

use reticulum_sdk::destination::{DestinationName, SingleInputDestination};
use reticulum_sdk::hash::AddressHash;
use reticulum_sdk::identity::PrivateIdentity;
use reticulum_sdk::iface::tcp_client::TcpClient;
use reticulum_sdk::iface::udp::UdpInterface;
use reticulum_sdk::iface::InterfaceMode;
use reticulum_sdk::transport::{Transport, TransportConfig};

use reticula_hal::{Board, Display, Keyboard, KeyCode, KeyEvent, KeyState};
use reticula_lxmf::client::{delivery_address_for, APP_NAME, DELIVERY_ASPECT};
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
use reticula_ui::screens::settings_identity::SettingsIdentityScreen;
use reticula_ui::screens::settings_lora::SettingsLoraScreen;
use reticula_ui::screens::settings_wifi::SettingsWifiScreen;
use reticula_ui::{Command, LoraSettings, Screen, ScreenId, Theme};

use crate::config::NetConfig;

/// Frame pacing of the UI loop.
/// Frame/input poll interval. Kept short so the trackball (polled here) picks
/// up quick movements; the render is cheap on static screens (the framebuffer
/// is only pushed to the panel when it changes).
pub const FRAME_MS: u64 = 25;
/// Maximum number of messages kept in memory.
pub const MAX_MESSAGES: usize = 512;

/// Persists a freshly generated identity (NVS on device, file on the sim).
/// Called before the app requests a restart so the new identity survives it.
pub type PersistIdentity = Box<dyn Fn(&PrivateIdentity) + Send + 'static>;
/// Persists new WiFi credentials (NVS on device). No-op on the simulator.
pub type PersistWifi = Box<dyn Fn(&str, &str) + Send + 'static>;
/// Persists new LoRa radio settings (NVS on device). No-op on the simulator.
pub type PersistLora = Box<dyn Fn(&LoraSettings) + Send + 'static>;

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
    /// LXMF contacts discovered from `lxmf/delivery` announces, even without
    /// any messages yet. Merged into `conversations` at render refresh time.
    contacts: std::sync::Mutex<Vec<Conversation>>,
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
    transport: Arc<Transport>,
    store: Arc<std::sync::Mutex<MessageStore>>,
    identity: PrivateIdentity,
    display_name: String,
    identity_hex: String,
    shared: Arc<SharedState>,
    screen: Screen,
    back_stack: Vec<Screen>,
    quit_on_root_back: bool,
    quit: bool,
    /// Persist a regenerated identity before restarting.
    persist_identity: Option<PersistIdentity>,
    /// Persist new WiFi credentials before restarting.
    persist_wifi: Option<PersistWifi>,
    /// Persist new LoRa radio settings before restarting.
    persist_lora: Option<PersistLora>,
    /// The currently configured WiFi SSID (for display in the WiFi sub-menu).
    wifi_ssid: String,
    /// The currently configured LoRa radio settings.
    lora_settings: LoraSettings,
    /// Transient notice shown on the settings screens (e.g. "restarting…").
    notice: String,
    /// Set when the app must restart (identity regenerated / WiFi changed).
    restart_requested: bool,
    /// Whether a LoRa radio interface is active (`None` = not configured).
    lora_online: Option<bool>,
}

impl<B: Board> ReticulaApp<B> {
    /// Create the application, bring up the Reticulum transport and register
    /// the LXMF delivery identity.
    ///
    /// `persist_identity` is called with a freshly generated identity before a
    /// restart (so regeneration survives the reboot); `persist_wifi` is called
    /// with new SSID/password. Either may be `None` (e.g. the simulator has no
    /// WiFi persistence).
    pub async fn new(
        board: B,
        identity: PrivateIdentity,
        display_name: String,
        net: NetConfig,
        persist_identity: Option<PersistIdentity>,
        persist_wifi: Option<PersistWifi>,
        persist_lora: Option<PersistLora>,
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
        // Show the LXMF delivery address (what peers use to message us), not
        // the raw identity key hex.
        let identity_hex = lxmf.delivery_address().to_hex_string();
        let wifi_ssid = board.wifi_ssid().unwrap_or_default();
        let lora_settings = {
            #[cfg(feature = "lora")]
            {
                net.lora.as_ref().map(|c| LoraSettings {
                    enabled: true,
                    frequency_hz: c.frequency,
                    bandwidth_hz: c.bandwidth as u64,
                    spreading_factor: c.spreading_factor,
                    coding_rate: c.coding_rate,
                    tx_power_dbm: c.tx_power,
                })
                .unwrap_or_default()
            }
            #[cfg(not(feature = "lora"))]
            {
                LoraSettings::default()
            }
        };

        Ok(Self {
            board,
            theme: Theme::default(),
            lxmf,
            nomad,
            transport,
            store,
            identity,
            display_name,
            identity_hex,
            shared: Arc::new(SharedState::default()),
            screen: Screen::Home(HomeScreen::new()),
            back_stack: Vec::new(),
            quit_on_root_back: net.quit_on_root_back,
            quit: false,
            persist_identity,
            persist_wifi,
            persist_lora,
            wifi_ssid,
            lora_settings,
            notice: String::new(),
            restart_requested: false,
            lora_online: {
                #[cfg(feature = "lora")]
                {
                    net.lora.as_ref().map(|_| true)
                }
                #[cfg(not(feature = "lora"))]
                {
                    None
                }
            },
        })
    }

    /// Our LXMF identity.
    pub fn identity(&self) -> &PrivateIdentity {
        &self.identity
    }

    /// Whether the app asked to restart (identity regenerated / WiFi changed).
    /// The platform should reboot/reload so the change takes effect.
    pub fn restart_requested(&self) -> bool {
        self.restart_requested
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
        // LXMF contact discovery (watch for `lxmf/delivery` announces).
        tokio::spawn({
            let lxmf = self.lxmf.clone();
            async move {
                if let Err(e) = lxmf.run_discovery().await {
                    warn!("lxmf discovery stopped: {e}");
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

        // Periodic announce task. Wait a few seconds for the network interfaces
        // (the TCP peer) to come up before the first announce, otherwise it is
        // broadcast before the connection exists and never reaches the mesh.
        tokio::spawn({
            let lxmf = self.lxmf.clone();
            let interval = self.announce_interval();
            async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
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
        // Ctrl/Alt are held between their press and release; Backspace pressed
        // while either is held means "back" (Ctrl+Backspace / Alt+Backspace).
        let mut ctrl_held = false;
        let mut alt_held = false;
        while !self.quit {
            // Input: keyboard plus any trackball/pointer device.
            let mut n = self.board.keyboard().read(&mut key_events);
            if let Some(trackball) = self.board.trackball() {
                n += trackball.read(&mut key_events[n..]);
            }
            for ev in key_events.iter().take(n) {
                if ev.state == KeyState::Pressed {
                    match ev.code {
                        KeyCode::Ctrl => ctrl_held = true,
                        KeyCode::Alt => alt_held = true,
                        KeyCode::Backspace if ctrl_held || alt_held => {
                            // Modifier+Backspace goes back on any screen.
                            info!("input: back (Ctrl/Alt+Backspace)");
                            let cmd = self.screen.handle_key(KeyCode::Esc);
                            self.execute(cmd);
                        }
                        code => {
                            let cmd = self.screen.handle_key(code);
                            self.execute(cmd);
                        }
                    }
                } else if ev.state == KeyState::Released {
                    match ev.code {
                        KeyCode::Ctrl => ctrl_held = false,
                        KeyCode::Alt => alt_held = false,
                        _ => {}
                    }
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

            // A settings change requested a restart: show the notice briefly,
            // then return so the platform can reboot/reload.
            if self.restart_requested {
                tokio::time::sleep(Duration::from_secs(2)).await;
                return Ok(());
            }

            // Periodic heartbeat so a connected serial monitor shows the app
            // is alive and healthy.
            if last_heartbeat.elapsed() >= Duration::from_secs(10) {
                let m = self.transport.metrics().await;
                log::info!(
                    "reticula: alive uptime={}s links={} msgs={} sdk[path={} dst={} ann={} anncache={}]",
                    self.board.uptime_ms() / 1000,
                    self.shared.peer_links.load(Ordering::Relaxed),
                    self.store.lock().unwrap().len(),
                    m.path_table_entries,
                    m.single_out_destinations_entries,
                    m.announce_table_entries,
                    m.announce_cache_entries,
                );
                last_heartbeat = Instant::now();
            }

            tokio::time::sleep(Duration::from_millis(FRAME_MS)).await;
        }

        Ok(())
    }

    fn announce_interval(&self) -> Duration {
        // Re-announce every minute so peers that just connected (or whose
        // contact cache expired) can still discover us.
        Duration::from_secs(60)
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
                self.display_name = name.clone();
                let lxmf = self.lxmf.clone();
                tokio::spawn(async move {
                    lxmf.set_display_name(name);
                    lxmf.announce().await;
                });
            }
            Command::RegenerateIdentity => {
                let identity = PrivateIdentity::new_from_rand(&mut UnwrapErr(SysRng));
                if let Some(persist) = &self.persist_identity {
                    persist(&identity);
                }
                // Refresh the displayed address for the brief moment before
                // the restart; the transport still uses the old identity.
                self.identity = identity;
                self.identity_hex = delivery_address_for(&self.identity).to_hex_string();
                self.notice = "New identity saved. Restarting…".to_string();
                self.restart_requested = true;
            }
            Command::SaveWifi { ssid, password } => {
                if let Some(persist) = &self.persist_wifi {
                    persist(&ssid, &password);
                }
                self.wifi_ssid = ssid;
                self.notice = "WiFi saved. Restarting…".to_string();
                self.restart_requested = true;
            }
            Command::SaveLora(settings) => {
                if let Some(persist) = &self.persist_lora {
                    persist(&settings);
                }
                self.lora_settings = settings;
                self.notice = "LoRa settings saved. Restarting…".to_string();
                self.restart_requested = true;
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
            ScreenId::SettingsIdentity => Screen::SettingsIdentity(SettingsIdentityScreen::new()),
            ScreenId::SettingsWifi => Screen::SettingsWifi(SettingsWifiScreen::new()),
            ScreenId::SettingsLora => Screen::SettingsLora(SettingsLoraScreen::new()),
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
LxmfEvent::ContactDiscovered { address, name } => {
                let mut contacts = self.shared.contacts.lock().unwrap();
                if !contacts.iter().any(|c| c.peer == address) {
                    let name = name.unwrap_or_default();
                    contacts.push(Conversation {
                        peer: address,
                        peer_hex: AddressHash::new(address).to_hex_string(),
                        peer_name: name.clone(),
                        last_title: name,
                        last_content: String::new(),
                        unread: 0,
                        last_ts: 0.0,
                    });
                    info!("reticula: discovered LXMF contact {address:02x?}");
                }
                drop(contacts);
                self.refresh_conversations();
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

        // Include LXMF contacts discovered from announces that have no
        // messages yet, so announced peers show up in the chat list. Also
        // build a peer→name map so conversations can show real names.
        let mut name_by_peer: HashMap<[u8; 16], String> = HashMap::new();
        {
            let contacts = self.shared.contacts.lock().unwrap();
            for c in contacts.iter() {
                if !c.last_title.is_empty() {
                    name_by_peer.entry(c.peer).or_insert_with(|| c.last_title.clone());
                }
                by_peer.entry(c.peer).or_insert_with(|| {
                    (
                        c.last_content.clone(),
                        c.last_title.clone(),
                        c.last_ts,
                        0,
                    )
                });
            }
        }

        let mut conversations: Vec<Conversation> = by_peer
            .into_iter()
            .map(|(peer, (last_content, last_title, last_ts, unread))| Conversation {
                peer,
                peer_hex: AddressHash::new(peer).to_hex_string(),
                peer_name: name_by_peer.get(&peer).cloned().unwrap_or_default(),
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
            wifi_ssid,
            lora_settings,
            notice,
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
            notice: notice.as_str(),
            wifi_ssid: wifi_ssid.as_str(),
            lora_settings: Some(lora_settings),
            network: NetworkState {
                connected: shared.connected.load(Ordering::Relaxed),
                uptime_ms: board.uptime_ms(),
                peer_links: shared.peer_links.load(Ordering::Relaxed),
                wifi_connected: board.wifi_status().map(|w| w.0).unwrap_or(false),
                wifi_rssi: board.wifi_status().map(|w| w.1),
                lora_online: self.lora_online,
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
    // Prove received link data messages (per-message acknowledgements sent
    // back over the same link). The reference LXMF delivery destination always
    // proves link-delivered messages, and senders rely on those proofs to
    // confirm delivery — without them a peer retransmits and we receive
    // duplicates, while it never learns the message arrived.
    config.set_prove_link_messages(true);

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
            // Roaming mode: allows our own announces to propagate (AccessPoint
            // mode blocks *all* announces on the interface, including local
            // ones, so peers could never discover us or reply). Paths still
            // expire quickly, keeping memory bounded on the busy network.
            let iface = TcpClient::new(addr.to_string())
                .with_interface_mode(InterfaceMode::Roaming);
            transport
                .iface_manager()
                .lock()
                .await
                .spawn(iface, TcpClient::spawn);
            info!("reticulum: TCP client interface to {addr} (roaming mode)");
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
    // Prove opportunistic (link-less) packets addressed to the delivery
    // destination too, not just messages arriving over links. The reference
    // LXMF delivery destination always proves received packets (its packet
    // callback calls `packet.prove()`); senders wait for these proofs to
    // confirm delivery, and without them they retransmit.
    delivery.lock().await.set_prove_packets(true);
    Ok((Arc::new(transport), delivery))
}

fn now_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}