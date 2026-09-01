//! Prints a deterministically packed LXMF message for cross-checking against
//! the reference implementation (see tools/check-lxmf-interop.py).

use reticulum_sdk::identity::PrivateIdentity;

use reticula_lxmf::LxmfMessage;

fn main() {
    // Deterministic identity derived from a name (matches the reference
    // `RNS.Identity.fromname` derivation).
    let sender = PrivateIdentity::new_from_name("interop-test-sender");
    let recipient = PrivateIdentity::new_from_name("interop-test-recipient");

    let sender_hash: [u8; 16] = sender.address_hash().as_slice().try_into().unwrap();
    let recipient_hash: [u8; 16] = recipient.address_hash().as_slice().try_into().unwrap();

    let mut msg = LxmfMessage::new(
        recipient_hash,
        sender_hash,
        "Hello, mesh".as_bytes(),
        "This is a cross-implementation test of the LXMF wire format."
            .as_bytes(),
    );
    // Fix the timestamp so both sides hash the same bytes.
    msg.timestamp = 1_750_000_000.0;

    let packed = msg.pack(&sender).unwrap();

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(sender.as_identity().public_key_bytes());

    println!("sender_identity_hex={}", sender.to_hex_string());
    println!("recipient_identity_hex={}", recipient.to_hex_string());
    println!("sender_public={}", hex(&pubkey));
    println!("recipient_hash={}", hex(&recipient_hash));
    println!("packed={}", hex(&packed));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}