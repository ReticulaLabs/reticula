//! A small in-memory store of LXMF messages.
//!
//! Sized for a constrained device: the store is bounded and evicts oldest
//! messages first. It is meant to be replaced by a durable store (e.g. SPIFFS
//! on the T-Deck) without changing the rest of the application.

use std::collections::HashMap;
use std::sync::Arc;

use crate::LxmfMessage;
use crate::message::Delivery;

/// Whether a stored message was sent by us or received from a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outbound,
    Inbound,
}

/// A bounded store of messages, indexed per peer.
#[derive(Debug, Default)]
pub struct MessageStore {
    messages: Vec<Arc<LxmfMessage>>,
    direction: Vec<Direction>,
    /// Peer hash → indexes into `messages`.
    by_peer: HashMap<[u8; 16], Vec<usize>>,
    max_messages: usize,
}

impl MessageStore {
    /// Create an empty store holding at most `max_messages` messages.
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            direction: Vec::new(),
            by_peer: HashMap::new(),
            max_messages,
        }
    }

    /// Insert a message, evicting the oldest message if over capacity.
    pub fn push(&mut self, message: LxmfMessage, direction: Direction) -> Arc<LxmfMessage> {
        if self.messages.len() >= self.max_messages {
            self.evict_oldest();
        }

        let peer = match direction {
            // The "peer" of a conversation is the other party: the sender of
            // an inbound message, or the recipient of an outbound one.
            Direction::Inbound => message.source_hash,
            Direction::Outbound => message.destination_hash,
        };
        let index = self.messages.len();
        self.by_peer.entry(peer).or_default().push(index);
        self.messages.push(Arc::new(message));
        self.direction.push(direction);
        self.messages[index].clone()
    }

    /// All stored messages, oldest first.
    pub fn all(&self) -> &[Arc<LxmfMessage>] {
        &self.messages
    }

    /// The direction of the message at `index`.
    pub fn direction_of(&self, index: usize) -> Option<Direction> {
        self.direction.get(index).copied()
    }

    /// All messages to/from a specific peer hash, oldest first.
    pub fn for_peer(&self, peer: &[u8; 16]) -> Vec<Arc<LxmfMessage>> {
        let Some(indexes) = self.by_peer.get(peer) else {
            return Vec::new();
        };
        indexes.iter().filter_map(|&i| self.messages.get(i).cloned()).collect()
    }

    /// Update the delivery state of a stored message, by message hash.
    ///
    /// Returns `true` if a matching message was found *and* its state actually
    /// changed, so callers can tell a real transition from a duplicate
    /// confirmation. Messages are addressed by hash rather than by index
    /// because eviction shifts every index after the evicted one.
    pub fn set_delivery(&mut self, message_hash: [u8; 32], delivery: Delivery) -> bool {
        let Some(index) = self.messages.iter().position(|m| m.hash == message_hash) else {
            return false;
        };
        // The stored messages are `Arc`s that may still be shared with whoever
        // received the MessageReceived/MessageSent event, so they cannot be
        // mutated in place; the message is copied and re-shared instead.
        let Some(mut message) = self.messages.get(index).map(|m| LxmfMessage { ..(**m).clone() })
        else {
            return false;
        };
        if message.delivery == delivery {
            return false;
        }
        message.delivery = delivery;
        self.messages[index] = Arc::new(message);
        true
    }

    /// Total number of stored messages.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// True if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    fn evict_oldest(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        self.messages.remove(0);
        self.direction.remove(0);
        // Every remaining message shifted down one index. The evicted message
        // was index 0, so drop that index from its peer's list before shifting
        // the rest, then prune any peer that no longer has messages.
        for indexes in self.by_peer.values_mut() {
            indexes.retain(|&i| i != 0);
            for i in indexes.iter_mut() {
                *i -= 1;
            }
        }
        self.by_peer.retain(|_, indexes| !indexes.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::UnwrapErr;
    use reticulum_sdk::identity::PrivateIdentity;

    fn msg(source: [u8; 16]) -> LxmfMessage {
        LxmfMessage::new(source, source, "", "")
    }

    #[test]
    fn stores_and_groups_by_peer() {
        let mut store = MessageStore::new(100);
        let a = [1u8; 16];
        let b = [2u8; 16];

        store.push(msg(a), Direction::Inbound);
        store.push(msg(b), Direction::Inbound);
        store.push(msg(a), Direction::Inbound);

        assert_eq!(store.len(), 3);
        assert_eq!(store.for_peer(&a).len(), 2);
        assert_eq!(store.for_peer(&b).len(), 1);
    }

    #[test]
    fn groups_outbound_by_recipient() {
        let mut store = MessageStore::new(100);
        let me = [9u8; 16];
        let peer = [7u8; 16];

        // We sent to `peer`: the source is us, the destination is the peer.
        let sent = LxmfMessage::new(peer, me, "", "hello");
        store.push(sent, Direction::Outbound);

        // Inbound: the peer sent to us.
        let received = LxmfMessage::new(me, peer, "", "hi");
        store.push(received, Direction::Inbound);

        // Both belong to the conversation with `peer`, and are found under it.
        assert_eq!(store.for_peer(&peer).len(), 2);
        assert_eq!(store.for_peer(&me).len(), 0);
    }

    /// Build an outbound message with a real hash, as one that has actually
    /// been sent and stored would have. `set_delivery` matches on the hash, so
    /// an unpacked message (whose hash is still all zeroes) would not be a
    /// meaningful subject.
    fn sent_msg(content: &str) -> LxmfMessage {
        let identity =
            PrivateIdentity::new_from_rand(&mut UnwrapErr(getrandom::SysRng));
        let mut message = LxmfMessage::new([7u8; 16], [9u8; 16], "", content);
        message.pack(&identity).expect("packing a message must succeed");
        message
    }

    #[test]
    fn set_delivery_marks_the_message_acknowledged() {
        let mut store = MessageStore::new(100);
        let peer = [7u8; 16];
        let message = sent_msg("hello");
        let hash = message.hash;
        assert_eq!(message.delivery, Delivery::Pending, "outbound starts unacknowledged");
        store.push(message, Direction::Outbound);

        assert!(store.set_delivery(hash, Delivery::Acknowledged));
        assert_eq!(store.for_peer(&peer)[0].delivery, Delivery::Acknowledged);
    }

    #[test]
    fn set_delivery_reports_no_change_when_already_acknowledged() {
        let mut store = MessageStore::new(100);
        let message = sent_msg("hello");
        let hash = message.hash;
        store.push(message, Direction::Outbound);

        assert!(store.set_delivery(hash, Delivery::Acknowledged));
        // A duplicate proof must not report a second transition.
        assert!(!store.set_delivery(hash, Delivery::Acknowledged));
    }

    #[test]
    fn set_delivery_ignores_unknown_hash() {
        let mut store = MessageStore::new(100);
        store.push(sent_msg("hi"), Direction::Outbound);
        assert!(!store.set_delivery([0xAAu8; 32], Delivery::Acknowledged));
    }

    #[test]
    fn set_delivery_works_while_the_message_arc_is_shared() {
        // The MessageSent event hands the same Arc to the UI, so the store
        // cannot mutate it in place: it copies and re-shares instead.
        let mut store = MessageStore::new(100);
        let peer = [7u8; 16];
        let message = sent_msg("hello");
        let hash = message.hash;
        let shared = store.push(message, Direction::Outbound);
        let held_by_ui = shared.clone();
        assert!(Arc::strong_count(&shared) >= 2);

        assert!(store.set_delivery(hash, Delivery::Acknowledged));
        assert_eq!(store.for_peer(&peer)[0].delivery, Delivery::Acknowledged);
        // The copy the UI was already holding is deliberately left alone.
        assert_eq!(held_by_ui.delivery, Delivery::Pending);
    }

    #[test]
    fn set_delivery_survives_eviction_of_older_messages() {
        // Messages are addressed by hash, not index: eviction shifts indices.
        let mut store = MessageStore::new(2);
        let peer = [7u8; 16];
        let first = sent_msg("one");
        let first_hash = first.hash;
        store.push(first, Direction::Outbound);
        store.push(sent_msg("two"), Direction::Outbound);
        store.push(sent_msg("three"), Direction::Outbound);

        // "one" was evicted; its late proof must not land on a replacement.
        assert!(!store.set_delivery(first_hash, Delivery::Acknowledged));
        assert!(store
            .for_peer(&peer)
            .iter()
            .all(|m| m.delivery == Delivery::Pending));
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let mut store = MessageStore::new(2);
        store.push(msg([1u8; 16]), Direction::Inbound);
        store.push(msg([2u8; 16]), Direction::Inbound);
        store.push(msg([3u8; 16]), Direction::Inbound);

        assert_eq!(store.len(), 2);
        assert_eq!(store.all()[0].source_hash, [2u8; 16]);
        assert_eq!(store.all()[1].source_hash, [3u8; 16]);
    }

    #[test]
    fn eviction_drops_evicted_peer_index() {
        let mut store = MessageStore::new(2);
        let a = [1u8; 16];
        let b = [2u8; 16];
        let c = [3u8; 16];

        store.push(msg(a), Direction::Inbound);
        store.push(msg(b), Direction::Inbound);
        store.push(msg(c), Direction::Inbound);

        // `a` was evicted: it must no longer resolve to any message, and the
        // surviving peers must still map to their own messages.
        assert_eq!(store.for_peer(&a).len(), 0);
        assert_eq!(store.for_peer(&b).len(), 1);
        assert_eq!(store.for_peer(&c).len(), 1);
        assert_eq!(store.for_peer(&b)[0].source_hash, b);
        assert_eq!(store.for_peer(&c)[0].source_hash, c);
    }
}