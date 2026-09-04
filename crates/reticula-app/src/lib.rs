//! `reticula-app` — application layer.
//!
//! This crate wires the pieces of Reticula together:
//!
//! * a [`Board`] (display + keyboard) from a BSP crate such as
//!   `reticula-host` or `reticula-tdeck`;
//! * the [`reticula_ui::Screen`] interface;
//! * the [`reticula_lxmf::LxmfClient`] chat client and the
//!   [`reticula_nomad::NomadClient`] browser, both running on a single shared
//!   `reticulum-sdk` transport configured as a pure *end client*;
//! * a small view model (conversations, nodes, page) that bridges the async
//!   network layer and the synchronous render loop.
//!
//! The two binaries — the desktop simulator and the ESP32 firmware — only
//! select a board and a network configuration, then call
//! [`app::ReticulaApp`].

pub mod app;
pub mod config;
pub mod identity;

pub use app::{AppError, PersistIdentity, PersistWifi, ReticulaApp};
pub use config::{NetConfig, TransportKind};