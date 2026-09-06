//! `reticula-ui` — the Reticula user interface.
//!
//! A small, self-contained UI for handheld devices with a keyboard and a
//! low-resolution LCD. It is built directly on `embedded-graphics` and
//! renders through the generic [`DrawTarget`] of the connected display, so it
//! works identically on the ESP32-S3 T-Deck and in the desktop terminal
//! simulator.
//!
//! The UI is intentionally decoupled from the network layer:
//!
//! * [`context::ViewContext`] is a per-frame snapshot of application state the
//!   app hands to the active screen for rendering.
//! * [`command::Command`]s are returned by screens in response to key presses;
//!   the app executes them asynchronously (send message, fetch page, ...).
//! * [`screens::Screen`] is the set of screens the app can show.

pub mod command;
pub mod context;
pub mod screens;
pub mod theme;
pub mod widgets;

pub use command::{Command, LoraSettings, ScreenId};
pub use context::{ChatMessage, Conversation, NetworkState, NodeEntry, ViewContext};
pub use screens::Screen;
pub use theme::Theme;