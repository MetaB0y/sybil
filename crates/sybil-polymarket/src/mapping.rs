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

    /// native market key -> sybil market_id
    #[serde(default)]
    native_to_sybil: HashMap<String, u32>,
    /// native template id -> sybil group info
    #[serde(default)]
    native_template_to_group: HashMap<String, GroupInfo>,

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

    /// Register a native catalog child market -> Sybil market mapping.
    pub fn register_native_market(&mut self, native_market_key: String, sybil_market_id: u32) {
        self.native_to_sybil
            .insert(native_market_key, sybil_market_id);
    }

    /// Look up Sybil market_id from a native catalog child-market key.
    pub fn native_market_id(&self, native_market_key: &str) -> Option<u32> {
        self.native_to_sybil.get(native_market_key).copied()
    }

    /// Register a NegRisk event → Sybil market group.
    pub fn register_event(&mut self, event_id: String, group_info: GroupInfo) {
        self.synced_events.insert(event_id.clone());
        self.event_to_group.insert(event_id, group_info);
    }

    /// Register a native categorical template → Sybil market group.
    pub fn register_native_group(&mut self, template_id: String, group_info: GroupInfo) {
        self.native_template_to_group
            .insert(template_id, group_info);
    }

    /// Look up the Sybil market group registered for a native categorical template.
    pub fn native_group(&self, template_id: &str) -> Option<GroupInfo> {
        self.native_template_to_group.get(template_id).cloned()
    }

    /// Add newly observed Sybil markets to an existing NegRisk event mapping.
    ///
    /// The server-side group extension is idempotent; this local mapping is too,
    /// so re-observing the same Polymarket event does not inflate group size.
    pub fn extend_event_group(&mut self, event_id: &str, market_ids: &[u32]) {
        if let Some(group) = self.event_to_group.get_mut(event_id) {
            for &market_id in market_ids {
                if !group.sybil_market_ids.contains(&market_id) {
                    group.sybil_market_ids.push(market_id);
                }
            }
            self.synced_events.insert(event_id.to_string());
        }
    }

    /// Add newly observed Sybil markets to an existing native group mapping.
    pub fn extend_native_group(&mut self, template_id: &str, market_ids: &[u32]) {
        if let Some(group) = self.native_template_to_group.get_mut(template_id) {
            for &market_id in market_ids {
                if !group.sybil_market_ids.contains(&market_id) {
                    group.sybil_market_ids.push(market_id);
                }
            }
        }
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

    /// Number of native child markets mapped from the checked-in catalog.
    pub fn native_market_count(&self) -> usize {
        self.native_to_sybil.len()
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
        self.native_to_sybil.clear();
        self.native_template_to_group.clear();
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

    /// All mapped native catalog child markets.
    pub fn all_native_mappings(&self) -> Vec<(String, u32)> {
        self.native_to_sybil
            .iter()
            .map(|(key, id)| (key.clone(), *id))
            .collect()
    }

    /// All Sybil market ids referenced by this mapping store.
    pub fn all_sybil_market_ids(&self) -> Vec<u32> {
        self.condition_to_sybil
            .values()
            .chain(self.native_to_sybil.values())
            .copied()
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
    fn extend_event_group_is_idempotent() {
        let mut store = MappingStore::new(None);
        store.register_event(
            "event1".into(),
            GroupInfo {
                group_name: "Event".into(),
                sybil_market_ids: vec![1, 2],
                neg_risk: true,
            },
        );

        store.extend_event_group("event1", &[2, 3, 3]);
        let group = store.event_group("event1").unwrap();
        assert_eq!(group.sybil_market_ids, vec![1, 2, 3]);
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
        assert_eq!(loaded.native_market_count(), 0);
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
        assert_eq!(store.native_market_count(), 0);
    }

    #[test]
    fn native_markets_and_groups_roundtrip() {
        let mut store = MappingStore::new(None);
        store.register_native_market("native:event:a".into(), 10);
        store.register_native_market("native:event:b".into(), 11);
        store.register_native_group(
            "event".into(),
            GroupInfo {
                group_name: "Native event".into(),
                sybil_market_ids: vec![10, 11],
                neg_risk: true,
            },
        );

        assert_eq!(store.native_market_id("native:event:a"), Some(10));
        assert_eq!(store.native_market_count(), 2);
        assert_eq!(store.all_sybil_market_ids().len(), 2);

        let json = serde_json::to_string(&store).unwrap();
        let mut loaded: MappingStore = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.native_market_id("native:event:b"), Some(11));
        let group = loaded.native_group("event").unwrap();
        assert_eq!(group.sybil_market_ids, vec![10, 11]);

        loaded.extend_native_group("event", &[11, 12, 12]);
        assert_eq!(
            loaded.native_group("event").unwrap().sybil_market_ids,
            vec![10, 11, 12]
        );
    }
}
