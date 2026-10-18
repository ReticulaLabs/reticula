//! The LXMF message wire format.
//!
//! Format (matching the reference LXMF implementation):
//!
//! ```text
//! packed message = destination_hash (16)
//!                | source_hash (16)
//!                | ed25519_signature (64)
//!                | msgpack([timestamp, title, content, fields, stamp?])
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
//!
//! An optional 5th element carries the stamp. It is deliberately *outside*
//! `hashed_part`: a stamp is derived from the message hash, so including it in
//! what gets hashed would be circular. The reference strips it for the same
//! reason and re-packs the 4-element form — which is exactly what
//! [`LxmfMessage::unpack`] does, so both implementations agree on the hash.

use rmpv::Value;
use sha2::{Digest, Sha256};

use ed25519_dalek::Signature;
use reticulum_sdk::identity::{Identity, PrivateIdentity};

use crate::stamper::{STAMP_LEN as STAMP_LENGTH};
use crate::LxmfError;

/// Length of a truncated Reticulum address hash (LXMF destination/source hash).
pub const DESTINATION_HASH_LENGTH: usize = 16;
/// Length of an Ed25519 signature.
pub const SIGNATURE_LENGTH: usize = 64;
/// Length of a full SHA-256 hash.
pub const HASH_LENGTH: usize = 32;

/// Acknowledgement state of a message.
///
/// Handing bytes to the transport is not the same as the message having
/// arrived, and neither is the same as the recipient having *displayed* it.
/// A recipient proves every packet it accepts over a link, but that proof is
/// issued by the link layer as soon as the packet decrypts — before, and
/// independently of, the receiving application's own handling of the payload.
/// So an acknowledgement here means "the peer's delivery destination took
/// delivery of these bytes", and nothing more.
///
/// Acknowledgement is also not reliable across implementations: the reference
/// hashes the packet as it arrived on the wire while the SDK hashes the packet
/// as it was built, so a peer's proof frequently cannot be matched to the
/// message that caused it. [`Delivery::Sent`] therefore exists as the ordinary
/// resting state for an outgoing message, and the absence of a proof is not
/// evidence that anything went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Written to the transport; waiting to see whether it is acknowledged.
    Pending,
    /// Written to the transport. No acknowledgement arrived, which may simply
    /// mean the peer's proofs cannot be correlated with ours.
    Sent,
    /// The peer's link layer returned a proof for the packet.
    Acknowledged,
    /// Delivery was given up on: the transport reported an error (a link that
    /// closed or was lost, or no route to the peer at all).
    Failed,
}

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
    /// Stamp proving this message was not free for the sender to produce.
    ///
    /// Absent unless the sender's peer required one. See
    /// [`crate::stamper`] for what a stamp actually is.
    pub stamp: Option<[u8; STAMP_LENGTH]>,
    /// Value of the stamp, in leading zero bits, once checked.
    ///
    /// `None` until [`LxmfMessage::check_stamp`] has been called.
    pub stamp_value: Option<u64>,
    /// Outcome of stamp verification: `Some(true)` valid, `Some(false)`
    /// invalid, `None` not checked.
    ///
    /// Checking costs a 768 KB workblock, so it is never done implicitly.
    pub stamp_valid: Option<bool>,
    /// Whether this message was received (as opposed to locally created).
    pub incoming: bool,
    /// Whether the message has been acknowledged by its recipient's link
    /// layer. Only meaningful for outgoing messages.
    pub delivery: Delivery,
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
            stamp: None,
            stamp_value: None,
            stamp_valid: None,
            incoming: false,
            // Locally created: nothing has acknowledged it yet.
            delivery: Delivery::Pending,
        }
    }

    /// Set arbitrary structured fields on this message.
    pub fn with_fields(mut self, fields: Value) -> Self {
        self.fields = fields;
        self
    }

    /// Attach a stamp to this message.
    ///
    /// Must be called after [`LxmfMessage::pack`] has computed [`Self::hash`],
    /// since the stamp is derived from it. Re-pack afterwards to put the stamp
    /// on the wire; the message hash is unchanged by the stamp.
    pub fn set_stamp(&mut self, stamp: [u8; STAMP_LENGTH], value: u64) {
        self.stamp = Some(stamp);
        self.stamp_value = Some(value);
        self.stamp_valid = Some(true);
    }

    /// Verify this message's stamp against a cost the sender had to meet.
    ///
    /// Costs a 768 KB workblock to build, so it is never done implicitly: a
    /// device receiving messages should only pay this when it actually intends
    /// to act on the result. Returns `None` when the message carries no stamp.
    pub fn check_stamp(&mut self, target_cost: u8) -> Option<bool> {
        let stamp = self.stamp?;
        let digest = crate::stamper::workblock_digest(&self.hash, &stamp);
        let valid = crate::stamper::leading_zero_bits(&digest) >= target_cost as u64;
        self.stamp_valid = Some(valid);
        if valid {
            self.stamp_value = Some(crate::stamper::leading_zero_bits(&digest));
        }
        Some(valid)
    }

    /// The payload this message hashes over: four elements, no stamp.
    fn payload_values(&self) -> Vec<Value> {
        vec![
            Value::F64(self.timestamp),
            Value::Binary(self.title.clone()),
            Value::Binary(self.content.clone()),
            self.fields.clone(),
        ]
    }

    /// Pack the message into its serialised form, signing with `source`.
    ///
    /// This computes the message hash and Ed25519 signature. If a stamp has
    /// been attached with [`LxmfMessage::set_stamp`] it is appended as a fifth
    /// payload element; it is not part of what is hashed or signed.
    pub fn pack(&mut self, source: &PrivateIdentity) -> Result<Vec<u8>, LxmfError> {
        let payload = self.payload_values();
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

        // The wire form carries the stamp as a fifth element, but the hash and
        // signature above are taken over the four-element form only.
        let packed_payload = match self.stamp {
            Some(stamp) => {
                let mut with_stamp = payload;
                with_stamp.push(Value::Binary(stamp.to_vec()));
                rmp_serde::to_vec(&with_stamp)?
            }
            None => packed_payload,
        };

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
        // An optional stamp arrives as a 5th element. It is read and kept, then
        // dropped from the payload so the hash below is taken over the
        // four-element form — which is what the sender hashed and what the
        // reference does, so both sides agree.
        let stamp = match payload.get(4) {
            Some(Value::Binary(b)) if b.len() == crate::stamper::STAMP_LEN => {
                let mut stamp = [0u8; crate::stamper::STAMP_LEN];
                stamp.copy_from_slice(b);
                Some(stamp)
            }
            _ => None,
        };
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
            // A received stamp is recorded but not verified: checking costs a
            // 768 KB workblock, and a client that does not enforce stamps gains
            // nothing from checking them.
            stamp,
            stamp_value: None,
            stamp_valid: None,
            incoming: true,
            // Received, so it demonstrably arrived; there is nothing pending.
            delivery: Delivery::Acknowledged,
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

    async fn stampper_work(message_id: &[u8; 32], cost: u8) -> (Vec<u8>, u64) {
        let mut rng = rand_core::UnwrapErr(getrandom::SysRng);
        let work = crate::stamper::StampWork::new(message_id).await;
        let stamp = work
            .generate(cost, &mut rng, 0, |_| false)
            .await
            .expect("stamp found");
        (stamp.bytes().to_vec(), stamp.value)
    }

    #[tokio::test]
    async fn a_stamp_survives_a_round_trip() {
        let sender = identity();
        let recipient = identity();

        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            b"t".as_slice(),
            b"c".as_slice(),
        );
        // First pack computes the hash the stamp is derived from.
        msg.pack(&sender).unwrap();
        let hash_before = msg.hash;

        let (stamp, value) = stampper_work(&msg.hash, 6).await;
        let stamp: [u8; crate::stamper::STAMP_LEN] = stamp.try_into().unwrap();
        msg.set_stamp(stamp, value);
        let packed = msg.pack(&sender).unwrap();

        // The stamp must not change what is hashed, or the receiver would
        // recompute a different hash and reject the signature.
        assert_eq!(msg.hash, hash_before, "a stamp must not alter the hash");

        let recall = |_h: &[u8; 16]| Some(*sender.as_identity());
        let mut unpacked = LxmfMessage::unpack(&packed, &recall).unwrap();

        assert_eq!(unpacked.hash, hash_before);
        assert!(unpacked.signature_validated, "signature must still validate");
        assert_eq!(unpacked.stamp, Some(stamp));
        assert_eq!(unpacked.stamp_value, None, "not verified on receipt");
        assert_eq!(unpacked.check_stamp(6), Some(true));
        assert_eq!(unpacked.check_stamp(64), Some(false));
    }

    #[test]
    fn an_unstamped_message_reports_no_stamp() {
        let sender = identity();
        let recipient = identity();
        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            b"t".as_slice(),
            b"c".as_slice(),
        );
        let packed = msg.pack(&sender).unwrap();
        let mut unpacked = LxmfMessage::unpack(&packed, &|_| None).unwrap();
        assert_eq!(unpacked.stamp, None);
        assert_eq!(unpacked.check_stamp(8), None);
    }

    #[test]
    fn a_malformed_stamp_is_ignored_rather_than_fatal() {
        // A wrong-length 5th element must not stop the message being read.
        let sender = identity();
        let recipient = identity();
        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            b"t".as_slice(),
            b"c".as_slice(),
        );
        let packed = msg.pack(&sender).unwrap();

        let fixed = 2 * DESTINATION_HASH_LENGTH + SIGNATURE_LENGTH;
        let mut payload: Vec<Value> = rmp_serde::from_slice(&packed[fixed..]).unwrap();
        payload.push(Value::Binary(vec![0xAA; 7]));
        let mut tampered = packed[..fixed].to_vec();
        tampered.extend_from_slice(&rmp_serde::to_vec(&payload).unwrap());

        let unpacked = LxmfMessage::unpack(&tampered, &|_| None).unwrap();
        assert_eq!(unpacked.stamp, None, "a 7-byte stamp is not a stamp");
        assert_eq!(unpacked.content_string(), "c");
    }

    #[tokio::test]
    async fn the_stamp_is_outside_the_signed_region() {
        // Same signature either way: the stamp is transport metadata, not
        // something the sender commits to.
        let sender = identity();
        let recipient = identity();
        let mut msg = LxmfMessage::new(
            address_hash(&recipient),
            address_hash(&sender),
            b"t".as_slice(),
            b"c".as_slice(),
        );
        let plain = msg.pack(&sender).unwrap();
        let signature_without_stamp = msg.signature;

        let (stamp, value) = stampper_work(&msg.hash, 5).await;
        msg.set_stamp(stamp.try_into().unwrap(), value);
        let stamped = msg.pack(&sender).unwrap();

        assert_eq!(msg.signature, signature_without_stamp);
        assert_ne!(plain, stamped, "the stamped form must be larger on the wire");
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