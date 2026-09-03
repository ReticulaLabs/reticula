//! Reticula firmware for the LILYGO T-Deck.
//!
//! Boots the board, connects to WiFi, brings up the Reticulum transport as a
//! pure end client, and runs the full application.
//!
//! Configuration is provided at build time via environment variables:
//!
//! * `WIFI_SSID` / `WIFI_PASS` — WiFi credentials (optional; the device runs
//!   offline otherwise).
//! * `RNS_PEER` — `host:port` of a reachable Reticulum node to connect to
//!   over TCP (optional). Without it the device listens on UDP.

#![allow(clippy::missing_safety_doc)]

use core::convert::TryInto;
use std::time::Duration;

use esp_idf_sys as _;

use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

use reticula_app::{NetConfig, ReticulaApp, TransportKind};
use reticula_tdeck::TDeckBoard;

const WIFI_SSID: Option<&str> = option_env!("WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("WIFI_PASS");
const RNS_PEER: Option<&str> = option_env!("RNS_PEER");

// LoRa (SX1262) configuration, supplied at build time:
//   LORA_FREQ   frequency in Hz, e.g. 868000000 (setting this enables LoRa)
//   LORA_BW     bandwidth in Hz (default 125000)
//   LORA_TXPOWER tx power in dBm (default 14)
//   LORA_SF     spreading factor (default 7)
//   LORA_CR     coding rate (default 5)
const LORA_FREQ: Option<&str> = option_env!("LORA_FREQ");
const LORA_BANDWIDTH: f64 = 125_000.0;
const LORA_TX_POWER: i8 = 14;
const LORA_SPREADING_FACTOR: u8 = 7;
const LORA_CODING_RATE: u8 = 5;

fn parse_freq_hz(s: &str) -> Option<u64> {
    s.parse().ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    esp_idf_sys::link_patches();

    // Start the lwIP/tcpip stack so sockets work even without WiFi (the UDP
    // interface peers over Ethernet/TCP). Without this, `esp_netif_init()` is
    // only reached via WiFi init and `tcpip_mbox` stays uninitialized.
    let _ = unsafe { esp_idf_svc::sys::esp_netif_init() };

    // The tokio IO driver calls `eventfd()`, which ESP-IDF provides via a VFS
    // that must be registered first (otherwise `eventfd()` returns EACCES and
    // the runtime build fails). Do this before creating the runtime.
    let eventfd_cfg = esp_idf_sys::esp_vfs_eventfd_config_t { max_fds: 8 };
    let _ = unsafe { esp_idf_sys::esp_vfs_eventfd_register(&eventfd_cfg) };

    // Route `log` output to the ESP-IDF console (UART0/USB-serial).
    esp_idf_svc::log::EspLogger::initialize_default();

    // The SDK is designed for a multi-thread runtime. Worker threads get a
    // generous stack: the SDK's transport task does crypto and buffer work
    // that overflows the ESP-IDF pthread default (8 KB).
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .thread_stack_size(65536)
        .build()
        .map_err(|e| format!("tokio runtime build: {e}"))?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    log::info!("reticula-firmware starting");

    // Take the peripherals once; WiFi needs the modem, the board needs the
    // rest. (A second `Peripherals::take()` would fail with
    // ESP_ERR_INVALID_STATE once the modem is taken.)
    let Peripherals {
        spi2,
        i2c0,
        modem,
        pins,
        ..
    } = Peripherals::take().map_err(|e| format!("Peripherals::take: {e}"))?;

    // WiFi (best-effort; the device still runs offline).
    let _wifi = match connect_wifi(modem) {
        Ok(wifi) => {
            log::info!("WiFi connected");
            Some(wifi)
        }
        Err(e) => {
            log::warn!("WiFi not connected: {e}");
            None
        }
    };

    let board = TDeckBoard::new(spi2, i2c0, pins)
        .map_err(|e| format!("TDeckBoard::new: {e}"))?;

    // TODO: persist the identity (NVS / SPIFFS). A fresh identity is
    // generated on every boot until storage is wired up.
    let identity = reticula_app::identity::load_or_create(None);

    // Optional LoRa radio (SX1262). Configured via build-time env vars; when
    // absent the radio is left unused.
    let lora = match LORA_FREQ.and_then(parse_freq_hz) {
        Some(frequency) => {
            let hw = board
                .lora_hw()
                .ok_or("board has no LoRa hardware")?;
            Some(
                reticulum_sdk::iface::lora::LoRaConfig::new(
                    "", // hardware provider supplies the bus
                    frequency,
                    LORA_BANDWIDTH,
                    LORA_TX_POWER,
                    LORA_SPREADING_FACTOR,
                    LORA_CODING_RATE,
                )
                .with_embedded_hw(hw),
            )
        }
        None => None,
    };

    let net = NetConfig {
        transport: match RNS_PEER {
            // Default to an outbound TCP connection to a reachable Reticulum
            // node (`host:port`).
            Some(peer) => TransportKind::TcpPeer {
                addr: peer.to_string(),
            },
            // No peer configured: listen on UDP so the device can still be
            // reached on the local network.
            None => TransportKind::Udp {
                bind: "0.0.0.0:5238".to_string(),
                forward: None,
            },
        },
        quit_on_root_back: false,
        announce_interval: Duration::from_secs(300),
        lora,
    };

    log::info!("identity: {}", identity.to_hex_string());

    let mut app = ReticulaApp::new(board, identity, "Reticula".to_string(), net)
        .await
        .map_err(|e| format!("ReticulaApp::new: {e}"))?;
    app.run().await.map_err(|e| format!("app.run: {e}"))?;

    Ok(())
}

/// Connect to WiFi as a station, if credentials are configured.
///
/// Returns the blocking WiFi handle so it stays alive for the app's lifetime.
fn connect_wifi(
    modem: esp_idf_hal::modem::Modem,
) -> Result<BlockingWifi<EspWifi<'static>>, Box<dyn std::error::Error>> {
    let Some(ssid) = WIFI_SSID else {
        return Err("WIFI_SSID not configured".into());
    };

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: WIFI_PASS.unwrap_or("").try_into().unwrap(),
        channel: None,
        ..Default::default()
    }))?;

    wifi.start()?;
    log::info!("connecting to WiFi \"{ssid}\"…");
    wifi.connect()?;
    wifi.wait_netif_up()?;

    Ok(wifi)
}