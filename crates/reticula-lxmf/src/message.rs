//! The LXMF message wire format.
//!
//! Format (matching the reference LXMF implementation):
//!
//! ```text
//! packed message = destination_hash (16)
//!                | source_hash (16)
//!                | ed25519_signature (64)
//!                | msgpack([timestamp, title, content, fields])
//! ```
//!
//! Where
//! ```text
//! hashed_part = destination_hash ‖ source_hash ‖ msgpack([timestamp, title, content, fields])
//! hash         = SHA256(hashed_part)
//! signed_part  = hashed_part ‖ hash
//! signature    = ed25519(source_identity.sign_key, signed_part)
//! ```
//!
//! The `fields` entry is always a msgpack map (empty if unset).

use rmpv::Value;
use sha2::{Digest, Sha256};

use ed25519_dalek::Signature;
use reticulum_sdk::identity::{Identity, PrivateIdentity};

use crate::LxmfError;

/// Length of a truncated Reticulum address hash (LXMF destination/source hash).
pub const DESTINATION_HASH_LENGTH: usize = 16;
/// Length of an Ed25519 signature.
pub const SIGNATURE_LENGTH: usize = 64;
/// Length of a full SHA-256 hash.
pub const HASH_LENGTH: usize = 32;

/// An LXMF message.
///
/// Either created locally with [`LxmfMessage::new`] and then [`LxmfMessage::pack`]ed,
/// or received via [`LxmfMessage::unpack`].
#[derive(Debug, Clone)]
pub struct LxmfMessage {
    /// Truncated hash of the destination identity (recipient).
    pub destination_hash: [u8; DESTINATION_HASH_LENGTH],
    /// Truncated hash of the source identity (sender).
    pub source_hash: [u8; DESTINATION_HASH_LENGTH],
    /// Unix timestamp (seconds, with fractional part) of creation.
    pub timestamp: f64,
    /// Message title, as raw bytes (usually UTF-8).
    pub title: Vec<u8>,
    /// Message content, as raw bytes (usually UTF-8).
    pub content: Vec<u8>,
    /// Extra structured fields (msgpack map). Usually empty.
    pub fields: Value,
    /// Full SHA-256 hash over the hashed part of the message.
    pub hash: [u8; HASH_LENGTH],
    /// Ed25519 signature over `hashed_part ‖ hash`.
    pub signature: [u8; SIGNATURE_LENGTH],
    /// Whether the signature was validated against a known source identity.
    pub signature_validated: bool,
    /// Whether this message was received (as opposed to locally created).
    pub incoming: bool,
}

impl LxmfMessage {
    /// Create a new outbound message.
    pub fn new(
        destination_hash: [u8; DESTINATION_HASH_LENGTH],
        source_hash: [u8; DESTINATION_HASH_LENGTH],
        title: impl Into<Vec<u8>>,
        content: impl Into<Vec<u8>>,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        Self {
            destination_hash,
            source_hash,
            timestamp: now,
            title: title.into(),
            content: content.into(),
            fields: Value::Map(Vec::new()),
            hash: [0u8; HASH_LENGTH],
            signature: [0u8; SIGNATURE_LENGTH],
            signature_validated: false,
            incoming: false,
        }
    }

    /// Set arbitrary structured fields on this message.
    pub fn with_fields(mut self, fields: Value) -> Self {
        self.fields = fields;
        self
    }

    /// Pack the message into its serialised form, signing with `source`.
    ///
    /// This computes the message hash and Ed25519 signature. The returned
    /// bytes are ready to transmit over a link or packet.
    pub fn pack(&mut self, source: &PrivateIdentity) -> Result<Vec<u8>, LxmfError> {
        let payload = vec![
            Value::F64(self.timestamp),
            Value::Binary(self.title.clone()),
            Value::Binary(self.content.clone()),
            self.fields.clone(),
        ];
        let packed_payload = rmp_serde::to_vec(&payload)?;

        let mut hashed_part = Vec::with_capacity(2 * DESTINATION_HASH_LENGTH + packed_payload.len());
        hashed_part.extend_from_slice(&self.destination_hash);
        hashed_part.extend_from_slice(&self.source_hash);
        hashed_part.extend_from_slice(&packed_payload);

        let digest: [u8; HASH_LENGTH] = Sha256::digest(&hashed_part).into();
        self.hash = digest;

        let mut signed_part = hashed_part;
        signed_part.extend_from_slice(&digest);
        let signature = source.sign(&signed_part).to_bytes();
        self.signature = signature;
        self.signature_validated = true;

        let mut packed = Vec::with_capacity(
            2 * DESTINATION_HASH_LENGTH + SIGNATURE_LENGTH + packed_payload.len(),
        );
        packed.extend_from_slice(&self.destination_hash);
        packed.extend_from_slice(&self.source_hash);
        packed.extend_from_slice(&self.signature);
        packed.extend_from_slice(&packed_payload);

        Ok(packed)
    }

    /// Unpack a serialised LXMF message.
    ///
    /// `recall_identity` is called with the message source hash and should
    /// return the source [`Identity`] if known (e.g. recalled from announces).
    /// If the identity is unknown, the signature is left unvalidated.
    pub fn unpack(
        data: &[u8],
        recall_identity: &dyn Fn(&[u8; DESTINATION_HASH_LENGTH]) -> Option<Identity>,
    ) -> Result<LxmfMessage, LxmfError> {
        let fixed = 2 * DESTINATION_HASH_LENGTH + SIGNATURE_LENGTH;
        if data.len() < fixed {
            return Err(LxmfError::InsufficientData);
        }

        let destination_hash: [u8; DESTINATION_HASH_LENGTH] = data[..16].try_into().unwrap();
        let source_hash: [u8; DESTINATION_HASH_LENGTH] = data[16..32].try_into().unwrap();
        let signature: [u8; SIGNATURE_LENGTH] = data[32..96].try_into().unwrap();
        let packed_payload = &data[fixed..];

        let mut payload: Vec<Value> = rmp_serde::from_slice(packed_payload)?;
        if payload.len() < 4 {
            return Err(LxmfError::MalformedPayload);
        }
        // Optional stamp appended as a 5th element; not used by this client.
        if payload.len() > 4 {
            payload.truncate(4);
        }

        let timestamp = payload[0]
            .as_f64()
            .or_else(|| payload[0].as_i64().map(|v| v as f64))
            .ok_or(LxmfError::MalformedPayload)?;

        let title = bytes_from_value(&payload[1]);
        let content = bytes_from_value(&payload[2]);
        let fields = payload[3].clone();

        // The hash is computed over the 4-element payload as packed by the
        // sender, so re-encode exactly what we just decoded.
        let packed_payload_4 = rmp_serde::to_vec(&payload)?;

        let mut hashed_part = Vec::with_capacity(2 * DESTINATION_HASH_LENGTH + packed_payload_4.len());
        hashed_part.extend_from_slice(&destination_hash);
        hashed_part.extend_from_slice(&source_hash);
        hashed_part.extend_from_slice(&packed_payload_4);

        let hash: [u8; HASH_LENGTH] = Sha256::digest(&hashed_part).into();

        let mut signature_validated = false;
        if let Some(src_identity) = recall_identity(&source_hash) {
            let mut signed_part = hashed_part;
            signed_part.extend_from_slice(&hash);
            let sig = Signature::from_bytes(&signature);
            signature_validated = src_identity.verify(&signed_part, &sig).is_ok();
        }

        Ok(LxmfMessage {
            destination_hash,
            source_hash,
            timestamp,
            title,
            content,
            fields,
            hash,
            signature,
            signature_validated,
            incoming: true,
        })
    }

    /// Title decoded as lossy UTF-8.
    pub fn title_string(&self) -> String {
        String::from_utf8_lossy(&self.title).into_owned()
    }

    /// Content decoded as lossy UTF-8.
    pub fn content_string(&self) -> String {
        String::from_utf8_lossy(&self.content).into_owned()
    }
}

fn bytes_from_value(v: &Value) -> Vec<u8> {
    match v {
        Value::Binary(b) => b.clone(),
        Value::String(s) => s.as_str().map(str::as_bytes).unwrap_or_default().to_vec(),
        Value::Nil => Vec::new(),
        _ => Vec::new(),
    }
}

/// Encrypt `plaintext` so that only `recipient` can read it, using the
/// Reticulum "encryptor" scheme (ephemeral X25519 + AES/HMAC Fernet).
///
/// This is the same construction the reference LXMF uses for propagated and
/// paper messages (`destination.encrypt()`). Direct link delivery does not
/// need it, because the link itself provides transport encryption.
pub fn encrypt_for(
    recipient: &Identity,
    plaintext: &[u8],
    out: &mut [u8],
) -> Result<usize, LxmfError> {
    let mut rng = rand_core::UnwrapErr(getrandom::SysRng);
    let slice = recipient.encrypt_packet(&mut rng, plaintext, None, out)?;
    Ok(slice.len())
}

/// Decrypt data produced by [`encrypt_for`] with a [`PrivateIdentity`].
pub fn decrypt_with(
    identity: &PrivateIdentity,
    data: &[u8],
    out: &mut [u8],
) -> Result<usize, LxmfError> {
    let mut rng = rand_core::UnwrapErr(getrandom::SysRng);
    let slice = identity.decrypt_packet(&mut rng, data, None, out)?;
    Ok(slice.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::UnwrapErr;

    fn identity() -> PrivateIdentity {
        PrivateIdentity::new_from_rand(&mut UnwrapErr(getrandom::SysRng))
    }

    fn address_hash(id: &PrivateIdentity) -> [u8; 16] {
        id.address_hash().as_slice().try_into().unwrap()
    }

    #[test]
    fn round_trip() {
        let sender = identity();
        let recipient = identity();

        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            "hello".as_bytes(),
            "world".as_bytes(),
        );
        let packed = msg.pack(&sender).unwrap();

        let recall = |_h: &[u8; 16]| Some(*sender.as_identity());
        let unpacked = LxmfMessage::unpack(&packed, &recall).unwrap();

        assert_eq!(unpacked.destination_hash, address_hash(&recipient));
        assert_eq!(unpacked.source_hash, address_hash(&sender));
        assert_eq!(unpacked.title_string(), "hello");
        assert_eq!(unpacked.content_string(), "world");
        assert_eq!(unpacked.hash, msg.hash);
        assert!(unpacked.signature_validated);
        assert!(unpacked.incoming);
    }

    #[test]
    fn signature_detects_tampering() {
        let sender = identity();
        let recipient = identity();

        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            "t".as_bytes(),
            "c".as_bytes(),
        );
        let mut packed = msg.pack(&sender).unwrap();

        // Corrupt the content payload (last byte region, before the fixed
        // header). Tampering must invalidate the recomputed hash/signature.
        let n = packed.len();
        packed[n - 1] ^= 0xff;

        let recall = |_h: &[u8; 16]| Some(*sender.as_identity());
        let unpacked = LxmfMessage::unpack(&packed, &recall).unwrap();
        assert!(!unpacked.signature_validated);
    }

    #[test]
    fn stamp_payload_still_parses() {
        // A payload with a 5th (stamp) element must still unpack; the stamp
        // is ignored.
        let sender = identity();
        let recipient = identity();

        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            "t".as_bytes(),
            "c".as_bytes(),
        );
        let packed = msg.pack(&sender).unwrap();

        // Insert a stamp into the payload by re-packing with 5 elements.
        let fixed = 2 * DESTINATION_HASH_LENGTH + SIGNATURE_LENGTH;
        let mut payload: Vec<Value> = rmp_serde::from_slice(&packed[fixed..]).unwrap();
        payload.push(Value::Binary(vec![0xAB; 16]));

        let mut stamped = packed[..fixed].to_vec();
        stamped.extend_from_slice(&rmp_serde::to_vec(&payload).unwrap());

        let recall = |_h: &[u8; 16]| Some(*sender.as_identity());
        let unpacked = LxmfMessage::unpack(&stamped, &recall).unwrap();
        assert_eq!(unpacked.content_string(), "c");
        // Hash still validates because the stamp is excluded.
        assert!(unpacked.signature_validated);
    }
}