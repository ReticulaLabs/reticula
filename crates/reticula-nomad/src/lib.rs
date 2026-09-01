//! NomadNet browsing for Reticula.
//!
//! NomadNet serves *pages* from *nodes* — Reticulum destinations named
//! `nomadnetwork/node`. A browser connects a link to a node and requests a
//! page path (e.g. `/page/index.mu`); the node answers with the page bytes.
//!
//! This crate provides:
//!
//! * [`page::Page`] — a fetched page, with a small Micron
//!   ([`page::render_micron`]) renderer that produces displayable lines and
//!   clickable links.
//! * [`client::NomadClient`] — discovers nodes from announces, connects to
//!   them and fetches pages over Reticulum links.

pub mod client;
pub mod page;

pub use client::{NomadClient, NomadEvent};
pub use page::{Link, Page, PageLine, PageStyle, render_micron};

use reticulum_sdk::error::RnsError;

/// Errors that can occur while browsing NomadNet.
#[derive(Debug)]
pub enum NomadError {
    /// No link could be established to the requested node.
    NoLink(String),
    /// The node did not answer in time.
    Timeout,
    /// The link closed while waiting for a response.
    LinkClosed,
    /// The node returned data that could not be interpreted as a page.
    InvalidResponse,
    /// A Reticulum layer error.
    Reticulum(RnsError),
}

impl core::fmt::Display for NomadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NomadError::NoLink(h) => write!(f, "no link to node {h}"),
            NomadError::Timeout => write!(f, "node did not answer in time"),
            NomadError::LinkClosed => write!(f, "link to node closed"),
            NomadError::InvalidResponse => write!(f, "invalid page response"),
            NomadError::Reticulum(e) => write!(f, "reticulum error: {e}"),
        }
    }
}

impl core::error::Error for NomadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            NomadError::Reticulum(e) => Some(e),
            _ => None,
        }
    }
}

impl From<RnsError> for NomadError {
    fn from(e: RnsError) -> Self {
        NomadError::Reticulum(e)
    }
}