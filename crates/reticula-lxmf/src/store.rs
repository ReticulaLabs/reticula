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

        let peer = message.source_hash;
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
        let peer = self.messages[0].source_hash;
        self.messages.remove(0);
        self.direction.remove(0);
        // Every remaining message shifted down one index.
        for indexes in self.by_peer.values_mut() {
            for i in indexes.iter_mut() {
                *i = i.saturating_sub(1);
            }
        }
        // Drop the entry that referred to the removed message.
        if let Some(indexes) = self.by_peer.get_mut(&peer) {
            indexes.retain(|&i| i != usize::MAX);
        }
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
    fn evicts_oldest_at_capacity() {
        let mut store = MessageStore::new(2);
        store.push(msg([1u8; 16]), Direction::Inbound);
        store.push(msg([2u8; 16]), Direction::Inbound);
        store.push(msg([3u8; 16]), Direction::Inbound);

        assert_eq!(store.len(), 2);
        assert_eq!(store.all()[0].source_hash, [2u8; 16]);
        assert_eq!(store.all()[1].source_hash, [3u8; 16]);
    }
}