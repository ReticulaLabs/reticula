#!/usr/bin/env python3
"""Cross-check Reticula's LXMF packing against the reference algorithm.

Reproduces the reference `LXMessage.pack()` from the raw identity hex that the
Rust example printed, using the same msgpack/hashing/signing rules, then
compares byte-for-byte with the Rust-produced packed message.
"""
import hashlib
import msgpack
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

def load(path):
    vals = {}
    for line in open(path):
        k, _, v = line.strip().partition("=")
        vals[k] = v
    return vals

def main():
    vals = load(sys.argv[1])

    sender_identity_hex = vals["sender_identity_hex"]
    recipient_identity_hex = vals["recipient_identity_hex"]
    recipient_hash = bytes.fromhex(vals["recipient_hash"])
    rust_packed = bytes.fromhex(vals["packed"])

    # The identity hex is the X25519 private key followed by the Ed25519
    # signing key seed (RNS's serialised identity format).
    sign_key_seed = bytes.fromhex(sender_identity_hex[64:128])

    # sender_hash = first 16 bytes of SHA-256(public_key || verifying_key)
    sender_pub = bytes.fromhex(vals["sender_public"])
    verifying_key = Ed25519PrivateKey.from_private_bytes(sign_key_seed).public_key()
    sender_hash = hashlib.sha256(sender_pub + verifying_key.public_bytes_raw()[:32]).digest()[:16]

    # --- Reference pack() -------------------------------------------------
    payload = [
        1_750_000_000.0,
        b"Hello, mesh",
        b"This is a cross-implementation test of the LXMF wire format.",
        {},
    ]
    packed_payload = msgpack.packb(payload, use_bin_type=True)

    hashed_part = recipient_hash + sender_hash + packed_payload
    message_hash = hashlib.sha256(hashed_part).digest()
    signed_part = hashed_part + message_hash
    signature = Ed25519PrivateKey.from_private_bytes(sign_key_seed).sign(signed_part)

    reference_packed = recipient_hash + sender_hash + signature + packed_payload

    ok = reference_packed == rust_packed
    print(f"sender_hash      match: {sender_hash.hex() == rust_packed[16:32].hex()}")
    print(f"msgpack payload  match: {packed_payload.hex() == rust_packed[96:].hex()}")
    print(f"full packed      match: {ok}")
    if not ok:
        print(f"reference: {reference_packed.hex()}")
        print(f"rust:      {rust_packed.hex()}")
        sys.exit(1)
    print("OK: Rust and reference LXMF packing are byte-identical.")

if __name__ == "__main__":
    main()