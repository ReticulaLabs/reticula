//! Reticula firmware for the LILYGO T-Deck.
//!
//! Boots the board, connects to WiFi, brings up the Reticulum transport as a
//! pure end client, and runs the full application.
//!
//! Configuration is provided at build time via environment variables:
//!
//! * `WIFI_SSID` / `WIFI_PASS` — WiFi credentials (optional; the device runs
//!   offline otherwise).
//! * `RNS_PEER` — `host:port` of a reachable Reticulum node to peer with
//!   (optional).

#![allow(clippy::missing_safety_doc)]

use std::time::Duration;

use esp_idf_sys as _;

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, WifiDriver, WifiStaDriver};

use reticula_app::{NetConfig, ReticulaApp, TransportKind};
use reticula_tdeck::TDeckBoard;

const WIFI_SSID: Option<&str> = option_env!("WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("WIFI_PASS");
const RNS_PEER: Option<&str> = option_env!("RNS_PEER");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    esp_idf_sys::link_patches();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    log::info!("reticula-firmware starting");

    // WiFi (best-effort; the device still runs offline).
    let _wifi = match connect_wifi() {
        Ok(wifi) => {
            log::info!("WiFi connected");
            Some(wifi)
        }
        Err(e) => {
            log::warn!("WiFi not connected: {e}");
            None
        }
    };

    let peripherals = Peripherals::take().unwrap();
    let board = TDeckBoard::new(peripherals)?;

    // TODO: persist the identity (NVS / SPIFFS). A fresh identity is
    // generated on every boot until storage is wired up.
    let identity = reticula_app::identity::load_or_create(None);

    let net = NetConfig {
        transport: TransportKind::Udp {
            bind: "0.0.0.0:5238".to_string(),
            forward: RNS_PEER.map(str::to_string),
        },
        quit_on_root_back: false,
        announce_interval: Duration::from_secs(300),
    };

    log::info!("identity: {}", identity.to_hex_string());

    let mut app = ReticulaApp::new(board, identity, "Reticula".to_string(), net).await?;
    app.run().await?;

    Ok(())
}

/// Connect to WiFi as a station, if credentials are configured.
fn connect_wifi() -> Result<WifiStaDriver<'static>, esp_idf_svc::wifi::WifiError> {
    let Some(ssid) = WIFI_SSID else {
        return Err(esp_idf_svc::wifi::WifiError::InvalidArgument);
    };

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let wifi = WifiDriver::new(esp_idf_svc::hal::peripherals::Peripherals::take().unwrap(), sys_loop, nvs)?;
    let mut sta = WifiStaDriver::new(wifi)?;

    sta.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.into(),
        password: WIFI_PASS.unwrap_or("").into(),
        ..Default::default()
    }))?;
    sta.start()?;
    sta.connect()?;

    log::info!("connecting to WiFi \"{ssid}\"…");
    for _ in 0..20 {
        if sta.is_connected()? {
            return Ok(sta);
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    Err(esp_idf_svc::wifi::WifiError::NotConnected)
}