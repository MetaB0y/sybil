//! In-memory registry of [`DataFeed`]s, indexed by id and by pubkey.
//!
//! Persistence lives in `matching-sequencer/src/store.rs` (the `DATA_FEEDS`
//! redb table); this registry is rebuilt from that table on startup.

use std::collections::HashMap;

use crate::feed::{DataFeed, FeedId, FeedPubkey};

#[derive(Clone, Debug, Default)]
pub struct FeedRegistry {
    feeds: HashMap<FeedId, DataFeed>,
    by_pubkey: HashMap<FeedPubkey, FeedId>,
    next_id: u64,
}

impl FeedRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new feed. Returns the newly-allocated [`FeedId`].
    ///
    /// If `pubkey` is already registered, returns the existing id without
    /// modification — same-pubkey-different-name is rejected at the API layer,
    /// so here we treat a collision as idempotent re-registration.
    pub fn register(&mut self, pubkey: FeedPubkey, name: String, now_ms: u64) -> FeedId {
        if let Some(&existing) = self.by_pubkey.get(&pubkey) {
            return existing;
        }
        let id = FeedId(self.next_id);
        self.next_id += 1;
        let feed = DataFeed {
            id,
            pubkey: pubkey.clone(),
            name,
            created_at_ms: now_ms,
        };
        self.feeds.insert(id, feed);
        self.by_pubkey.insert(pubkey, id);
        id
    }

    /// Re-insert a feed exactly as persisted. Only used by the store loader.
    pub fn restore(&mut self, feed: DataFeed) {
        let FeedId(n) = feed.id;
        if n + 1 > self.next_id {
            self.next_id = n + 1;
        }
        self.by_pubkey.insert(feed.pubkey.clone(), feed.id);
        self.feeds.insert(feed.id, feed);
    }

    pub fn get(&self, id: FeedId) -> Option<&DataFeed> {
        self.feeds.get(&id)
    }

    pub fn resolve_pubkey(&self, pubkey: &FeedPubkey) -> Option<&DataFeed> {
        self.by_pubkey.get(pubkey).and_then(|id| self.feeds.get(id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &DataFeed> {
        self.feeds.values()
    }

    pub fn len(&self) -> usize {
        self.feeds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.feeds.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_is_idempotent_for_same_pubkey() {
        let mut reg = FeedRegistry::new();
        let pk = FeedPubkey(vec![1u8; 33]);
        let a = reg.register(pk.clone(), "admin".into(), 1000);
        let b = reg.register(pk, "admin".into(), 2000);
        assert_eq!(a, b);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn resolve_by_pubkey() {
        let mut reg = FeedRegistry::new();
        let pk = FeedPubkey(vec![2u8; 33]);
        let id = reg.register(pk.clone(), "polymarket_mirror".into(), 1000);
        let feed = reg.resolve_pubkey(&pk).unwrap();
        assert_eq!(feed.id, id);
        assert_eq!(feed.name, "polymarket_mirror");
    }

    #[test]
    fn restore_preserves_ids_and_next_id() {
        let mut reg = FeedRegistry::new();
        reg.restore(DataFeed {
            id: FeedId(7),
            pubkey: FeedPubkey(vec![3u8; 33]),
            name: "admin".into(),
            created_at_ms: 100,
        });
        // Next registered must come out as FeedId(8).
        let id = reg.register(FeedPubkey(vec![4u8; 33]), "other".into(), 200);
        assert_eq!(id, FeedId(8));
    }
}
