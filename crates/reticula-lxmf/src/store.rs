//! A small in-memory store of LXMF messages.
//!
//! Sized for a constrained device: the store is bounded and evicts oldest
//! messages first. It is meant to be replaced by a durable store (e.g. SPIFFS
//! on the T-Deck) without changing the rest of the application.

use std::collections::HashMap;
use std::sync::Arc;

use crate::LxmfMessage;

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