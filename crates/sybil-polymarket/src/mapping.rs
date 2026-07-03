use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Bidirectional mapping between Polymarket and Sybil identifiers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MappingStore {
    /// polymarket condition_id -> sybil market_id
    condition_to_sybil: HashMap<String, u32>,
    /// sybil market_id -> polymarket condition_id
    sybil_to_condition: HashMap<u32, String>,
    /// polymarket token_id -> (sybil_market_id, outcome_index: 0=YES, 1=NO)
    token_to_sybil: HashMap<String, (u32, u8)>,
    /// polymarket event_id -> sybil group info
    event_to_group: HashMap<String, GroupInfo>,
    /// Set of polymarket event IDs already synced
    synced_events: HashSet<String>,

    /// Persisted MM account id (PM-7). The mirror runs a single MM identity, so
    /// this is keyed implicitly by "the mirror's MM". Reattached on restart when
    /// the Sybil server still knows the account; otherwise a fresh account is
    /// minted and stored here. `None` means "never minted / no durable account".
    #[serde(default)]
    mm_account_id: Option<u64>,

    /// Persistence path (not serialized).
    #[serde(skip)]
    persist_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    pub group_name: String,
    pub sybil_market_ids: Vec<u32>,
    pub neg_risk: bool,
}

impl MappingStore {
    pub fn new(persist_path: Option<PathBuf>) -> Self {
        Self {
            persist_path,
            ..Default::default()
        }
    }

    /// Load from a JSON file, or create empty if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Self, Error> {
        if path.exists() {
            let data = std::fs::read_to_string(path)?;
            let mut store: Self = serde_json::from_str(&data)?;
            store.persist_path = Some(path.to_path_buf());
            Ok(store)
        } else {
            Ok(Self::new(Some(path.to_path_buf())))
        }
    }

    /// Save to disk if a persistence path is configured.
    pub fn save(&self) -> Result<(), Error> {
        if let Some(ref path) = self.persist_path {
            let data = serde_json::to_string_pretty(self)?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }

    /// Register a Polymarket market → Sybil market mapping.
    pub fn register_market(
        &mut self,
        condition_id: String,
        token_ids: Vec<String>,
        sybil_market_id: u32,
    ) {
        self.condition_to_sybil
            .insert(condition_id.clone(), sybil_market_id);
        self.sybil_to_condition
            .insert(sybil_market_id, condition_id);

        // token_ids[0] = YES token, token_ids[1] = NO token
        for (i, token_id) in token_ids.into_iter().enumerate() {
            let outcome = i as u8; // 0 = YES, 1 = NO
            self.token_to_sybil
                .insert(token_id, (sybil_market_id, outcome));
        }
    }

    /// Register a NegRisk event → Sybil market group.
    pub fn register_event(&mut self, event_id: String, group_info: GroupInfo) {
        self.synced_events.insert(event_id.clone());
        self.event_to_group.insert(event_id, group_info);
    }

    /// Look up the Sybil market group registered for a Polymarket event.
    pub fn event_group(&self, event_id: &str) -> Option<GroupInfo> {
        self.event_to_group.get(event_id).cloned()
    }

    /// Mark a simple (non-NegRisk) event as synced.
    pub fn mark_event_synced(&mut self, event_id: &str) {
        self.synced_events.insert(event_id.to_string());
    }

    /// Check if an event has been synced.
    pub fn is_event_synced(&self, event_id: &str) -> bool {
        self.synced_events.contains(event_id)
    }

    /// Look up Sybil market_id from a Polymarket condition_id.
    pub fn sybil_market_id(&self, condition_id: &str) -> Option<u32> {
        self.condition_to_sybil.get(condition_id).copied()
    }

    /// Look up (sybil_market_id, outcome_index) from a Polymarket token_id.
    pub fn sybil_from_token(&self, token_id: &str) -> Option<(u32, u8)> {
        self.token_to_sybil.get(token_id).copied()
    }

    /// Get all registered YES token IDs (for WebSocket subscription).
    pub fn all_yes_token_ids(&self) -> Vec<String> {
        self.token_to_sybil
            .iter()
            .filter(|(_, (_, outcome))| *outcome == 0)
            .map(|(token_id, _)| token_id.clone())
            .collect()
    }

    /// Get all registered token IDs (both YES and NO).
    pub fn all_token_ids(&self) -> Vec<String> {
        self.token_to_sybil.keys().cloned().collect()
    }

    /// Number of synced events.
    pub fn event_count(&self) -> usize {
        self.synced_events.len()
    }

    /// Number of mapped markets.
    pub fn market_count(&self) -> usize {
        self.condition_to_sybil.len()
    }

    /// Clear all persisted Sybil mappings while preserving the persistence path.
    ///
    /// The MM account id is cleared too: `clear()` is only invoked when the
    /// Sybil chain has been rebuilt from scratch (mapped markets no longer
    /// exist server-side), which means the old MM account is gone as well.
    /// Reattachment would fail its `get_account` probe anyway; dropping it here
    /// keeps the persisted state internally consistent.
    pub fn clear(&mut self) {
        self.condition_to_sybil.clear();
        self.sybil_to_condition.clear();
        self.token_to_sybil.clear();
        self.event_to_group.clear();
        self.synced_events.clear();
        self.mm_account_id = None;
    }

    /// Persisted MM account id, if one has been minted and stored.
    pub fn mm_account_id(&self) -> Option<u64> {
        self.mm_account_id
    }

    /// Persist the MM account id so the next restart reattaches to it.
    pub fn set_mm_account_id(&mut self, account_id: u64) {
        self.mm_account_id = Some(account_id);
    }

    /// All (condition_id, sybil_market_id) pairs — used by the resolution
    /// actor to reconcile settled Polymarket conditions against locally
    /// mirrored markets.
    pub fn all_condition_mappings(&self) -> Vec<(String, u32)> {
        self.condition_to_sybil
            .iter()
            .map(|(c, id)| (c.clone(), *id))
            .collect()
    }

    /// Iterate all mapped markets: yields (sybil_market_id, yes_token_id, group_key, group_size).
    pub fn all_markets(&self) -> Vec<(u32, String, Option<String>, usize)> {
        self.all_markets_matching(|_| true)
    }

    /// Iterate mapped markets whose Polymarket condition is in `conditions`.
    pub fn all_markets_for_conditions(
        &self,
        conditions: &HashSet<String>,
    ) -> Vec<(u32, String, Option<String>, usize)> {
        self.all_markets_matching(|condition| conditions.contains(condition))
    }

    fn all_markets_matching(
        &self,
        mut condition_matches: impl FnMut(&str) -> bool,
    ) -> Vec<(u32, String, Option<String>, usize)> {
        let mut group_by_market = HashMap::new();
        for (event_id, group) in &self.event_to_group {
            if !group.neg_risk {
                continue;
            }
            let group_size = group.sybil_market_ids.len();
            for &market_id in &group.sybil_market_ids {
                group_by_market.insert(market_id, (event_id.clone(), group_size));
            }
        }

        self.token_to_sybil
            .iter()
            .filter(|(_, (_, outcome))| *outcome == 0) // YES tokens only
            .filter(|(_, (sybil_id, _))| {
                self.sybil_to_condition
                    .get(sybil_id)
                    .is_some_and(|condition| condition_matches(condition))
            })
            .map(|(token_id, (sybil_id, _))| {
                let group = group_by_market.get(sybil_id).cloned();
                (
                    *sybil_id,
                    token_id.clone(),
                    group.as_ref().map(|(group_key, _)| group_key.clone()),
                    group.map(|(_, group_size)| group_size).unwrap_or(0),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut store = MappingStore::new(None);

        store.register_market(
            "0xabc".into(),
            vec!["token_yes".into(), "token_no".into()],
            42,
        );

        assert_eq!(store.sybil_market_id("0xabc"), Some(42));
        assert_eq!(store.sybil_from_token("token_yes"), Some((42, 0)));
        assert_eq!(store.sybil_from_token("token_no"), Some((42, 1)));
        assert_eq!(store.sybil_from_token("unknown"), None);
    }

    #[test]
    fn event_sync_tracking() {
        let mut store = MappingStore::new(None);
        assert!(!store.is_event_synced("event1"));

        store.mark_event_synced("event1");
        assert!(store.is_event_synced("event1"));
    }

    #[test]
    fn serialize_roundtrip() {
        let mut store = MappingStore::new(None);
        store.register_market("cond1".into(), vec!["t1".into(), "t2".into()], 0);
        store.register_event(
            "ev1".into(),
            GroupInfo {
                group_name: "Test".into(),
                sybil_market_ids: vec![0, 1],
                neg_risk: true,
            },
        );

        let json = serde_json::to_string(&store).unwrap();
        let loaded: MappingStore = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.sybil_market_id("cond1"), Some(0));
        assert!(loaded.is_event_synced("ev1"));
    }

    #[test]
    fn mm_account_id_roundtrips_and_clears() {
        let mut store = MappingStore::new(None);
        assert_eq!(store.mm_account_id(), None);

        store.set_mm_account_id(777);
        assert_eq!(store.mm_account_id(), Some(777));

        let json = serde_json::to_string(&store).unwrap();
        let loaded: MappingStore = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.mm_account_id(), Some(777));

        // A fresh-chain clear drops the account so reattach mints a new one.
        let mut loaded = loaded;
        loaded.clear();
        assert_eq!(loaded.mm_account_id(), None);
    }

    #[test]
    fn mm_account_id_defaults_when_absent_from_json() {
        // Older on-disk stores predate the field; they must load cleanly.
        let json = r#"{
            "condition_to_sybil": {},
            "sybil_to_condition": {},
            "token_to_sybil": {},
            "event_to_group": {},
            "synced_events": []
        }"#;
        let store: MappingStore = serde_json::from_str(json).unwrap();
        assert_eq!(store.mm_account_id(), None);
    }
}
