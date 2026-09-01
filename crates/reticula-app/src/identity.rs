//! Identity persistence.
//!
//! A Reticulum identity is just a private X25519 key and an Ed25519 signing
//! key. We store it as the concatenated hex of both keys, which is exactly
//! what the SDK's `to_hex_string` / `new_from_hex_string` round-trip.

use std::path::Path;

use getrandom::SysRng;
use rand_core::UnwrapErr;
use reticulum_sdk::identity::PrivateIdentity;

/// Load the identity from `path`, or create a new one and persist it.
///
/// Passing `None` skips persistence and always generates a fresh identity
/// (used on devices without a filesystem yet).
pub fn load_or_create(path: Option<&Path>) -> PrivateIdentity {
    if let Some(path) = path {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(identity) = PrivateIdentity::new_from_hex_string(data.trim()) {
                log::info!("identity loaded from {}", path.display());
                return identity;
            }
            log::warn!("ignoring unparsable identity file {}", path.display());
        }
    }

    let identity = PrivateIdentity::new_from_rand(&mut UnwrapErr(SysRng));

    if let Some(path) = path {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(path, identity.to_hex_string()) {
            Ok(()) => log::info!("identity saved to {}", path.display()),
            Err(e) => log::warn!("could not save identity to {}: {e}", path.display()),
        }
    }

    identity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join("reticula-identity-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("identity.key");

        let a = load_or_create(Some(&path));
        let b = load_or_create(Some(&path));
        assert_eq!(a.to_hex_string(), b.to_hex_string());
        let _ = std::fs::remove_dir_all(&dir);
    }
}