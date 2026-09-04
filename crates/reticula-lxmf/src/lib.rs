//! LXMF (Lightweight eXchange Message Format) messaging for Reticula.
//!
//! This crate implements the LXMF message wire format plus a [`client::LxmfClient`]
//! that delivers and receives messages over the Reticulum transport provided by
//! [`reticulum_sdk`]. It is wire-compatible with the reference Python LXMF
//! implementation:
//!
//! * [`message::LxmfMessage`] — the packed message format
//!   `destination_hash ‖ source_hash ‖ ed25519_signature ‖ msgpack([ts, title, content, fields])`,
//!   hashing and signing rules matching the reference implementation.
//! * [`client::LxmfClient`] — announces a `lxmf/delivery` destination, receives
//!   messages over links and packets, and sends messages by establishing a
//!   direct link to the recipient.
//! * [`store::MessageStore`] — a small, bounded in-memory store of messages,
//!   grouped per peer, suitable for a device with limited RAM.
//!
//! The Reticulum transport instance is owned by the application and shared with
//! other clients (such as the NomadNet browser); this crate never creates or
//! announces a transport and is therefore strictly an *end client*.

pub mod client;
pub mod message;
pub mod store;

pub use client::{LxmfClient, LxmfEvent, delivery_address_for};
pub use message::LxmfMessage;
pub use store::{Direction, MessageStore};

use reticulum_sdk::error::RnsError;

/// Errors that can occur in LXMF message handling.
#[derive(Debug)]
pub enum LxmfError {
    /// Incoming data was shorter than the fixed LXMF header.
    InsufficientData,
    /// The msgpack payload was not a valid LXMF payload array.
    MalformedPayload,
    /// The message header / payload was structurally invalid.
    InvalidArgument,
    /// The message signature could not be validated (and the source identity
    /// was known).
    SignatureInvalid,
    /// The message destination is not this client.
    NotForUs,
    /// A Reticulum layer error.
    Reticulum(RnsError),
    /// Outgoing message send timed out waiting for a link to activate.
    SendTimeout,
    /// No path is known to the destination (it has not announced recently).
    NoPathToDestination([u8; 16]),
    /// A link we relied on closed.
    LinkClosed,
    /// The link could not be found in the transport.
    LinkLost,
    /// Msgpack encoding failed.
    Encoding(rmp_serde::encode::Error),
    /// Msgpack decoding failed.
    Decoding(rmp_serde::decode::Error),
}

impl core::fmt::Display for LxmfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LxmfError::InsufficientData => write!(f, "LXMF data shorter than fixed header"),
            LxmfError::MalformedPayload => write!(f, "LXMF msgpack payload malformed"),
            LxmfError::InvalidArgument => write!(f, "invalid LXMF argument"),
            LxmfError::SignatureInvalid => write!(f, "LXMF signature validation failed"),
            LxmfError::NotForUs => write!(f, "LXMF message not addressed to this client"),
            LxmfError::Reticulum(e) => write!(f, "reticulum error: {e}"),
            LxmfError::SendTimeout => write!(f, "LXMF send timed out"),
            LxmfError::NoPathToDestination(h) => {
                write!(f, "no path to LXMF destination {}", hex16(h))
            }
            LxmfError::LinkClosed => write!(f, "LXMF link closed"),
            LxmfError::LinkLost => write!(f, "LXMF link lost"),
            LxmfError::Encoding(e) => write!(f, "LXMF msgpack encoding error: {e}"),
            LxmfError::Decoding(e) => write!(f, "LXMF msgpack decoding error: {e}"),
        }
    }
}

impl core::error::Error for LxmfError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            LxmfError::Reticulum(e) => Some(e),
            LxmfError::Encoding(e) => Some(e),
            LxmfError::Decoding(e) => Some(e),
            _ => None,
        }
    }
}

impl From<RnsError> for LxmfError {
    fn from(e: RnsError) -> Self {
        LxmfError::Reticulum(e)
    }
}

impl From<rmp_serde::encode::Error> for LxmfError {
    fn from(e: rmp_serde::encode::Error) -> Self {
        LxmfError::Encoding(e)
    }
}

impl From<rmp_serde::decode::Error> for LxmfError {
    fn from(e: rmp_serde::decode::Error) -> Self {
        LxmfError::Decoding(e)
    }
}

fn hex16(h: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in h {
        s.push_str(&format!("{b:02x}"));
    }
    s
}