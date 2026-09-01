//! Reticula desktop simulator.
//!
//! Runs the full Reticula application on a desktop machine, rendering the UI
//! to the terminal and joining a real Reticulum mesh over UDP or TCP. This is
//! the fastest way to develop the UI and chat without flashing a device.
//!
//! ```
//! # join a local mesh over UDP
//! cargo run -p reticula-sim -- --udp-bind 0.0.0.0:5238
//!
//! # peer with a specific node
//! cargo run -p reticula-sim -- --udp-bind 0.0.0.0:5238 --udp-forward 192.168.1.10:5238
//!
//! # connect to a Reticulum TCP server
//! cargo run -p reticula-sim -- --tcp-peer example.net:5242
//! ```

use std::path::PathBuf;

use log::info;

use reticula_app::{NetConfig, ReticulaApp, TransportKind};
use reticula_host::HostBoard;

#[derive(Debug, Default)]
struct Args {
    identity_path: Option<PathBuf>,
    name: Option<String>,
    udp_bind: Option<String>,
    udp_forward: Option<String>,
    tcp_peer: Option<String>,
    size: Option<(u32, u32)>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = parse_args();

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(args))?;

    // The board restores the terminal (leaves the alternate screen buffer)
    // when it is dropped at the end of `run`.
    Ok(())
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let identity_path = args.identity_path.unwrap_or_else(default_identity_path);
    let identity = reticula_app::identity::load_or_create(Some(&identity_path));

    let transport = if let Some(addr) = args.tcp_peer {
        TransportKind::TcpPeer { addr }
    } else {
        TransportKind::Udp {
            bind: args.udp_bind.clone().unwrap_or_else(|| "0.0.0.0:5238".into()),
            forward: args.udp_forward.clone(),
        }
    };

    let net = NetConfig {
        transport,
        quit_on_root_back: true,
        ..NetConfig::default()
    };

    let board = match args.size {
        Some((w, h)) => HostBoard::with_size(w, h),
        None => HostBoard::new(),
    };

    let display_name = args.name.unwrap_or_else(|| "Reticula Sim".to_string());

    info!("identity: {}", identity.to_hex_string());
    info!("display name: {display_name}");
    let mut app = ReticulaApp::new(board, identity, display_name, net).await?;
    app.run().await?;
    Ok(())
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--identity" => args.identity_path = it.next().map(PathBuf::from),
            "--name" => args.name = it.next(),
            "--udp-bind" => args.udp_bind = it.next(),
            "--udp-forward" => args.udp_forward = it.next(),
            "--tcp-peer" => args.tcp_peer = it.next(),
            "--size" => {
                if let Some(s) = it.next() {
                    if let Some((w, h)) = s.split_once('x') {
                        if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                            args.size = Some((w, h));
                        }
                    }
                }
            }
            "--help" | "-h" => {
                println!(
                    "reticula-sim — Reticula desktop simulator\n\n\
                     USAGE:\n    reticula [OPTIONS]\n\n\
                     OPTIONS:\n    \
                     --identity <path>          identity key file (default ~/.config/reticula/identity.key)\n    \
                     --name <name>              display name announced to peers\n    \
                     --udp-bind <addr:port>     UDP bind address (default 0.0.0.0:5238)\n    \
                     --udp-forward <addr:port>  optional remote UDP node to peer with\n    \
                     --tcp-peer <addr:port>     connect to a Reticulum TCP server instead\n    \
                     --size <WxH>               simulated framebuffer size (default 320x240)"
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }
    args
}

fn default_identity_path() -> PathBuf {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".config"));
    dir.join("reticula").join("identity.key")
}