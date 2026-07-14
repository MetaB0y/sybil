//! Curated Polymarket mirror seed set (SYB-150).
//!
//! The default mirror path ([`GammaClient::fetch_active_events`]) ranks *all*
//! active Polymarket events by volume and mirrors the top N that pass the
//! category filters. That is right for a broad mirror but wrong for a
//! hand-picked launch set: we want a specific, reviewed list of AI / company /
//! tech questions, addressed by Polymarket **event id** so the selection is
//! deterministic and auditable across redeploys.
//!
//! This module loads that curated list from a JSON file (see
//! `crates/sybil-polymarket/curated_markets.json`). When
//! `--curated-markets-path` / `CURATED_MARKETS_PATH` is set, [`SyncActor`]
//! fetches exactly these events by id (via
//! [`GammaClient::fetch_curated_events`]) instead of the volume scan, and the
//! MM bootstrap in `main.rs` scopes its allowed-condition set to them.
//!
//! Provenance for the resulting Sybil markets is unchanged and needs no new
//! field: each mirrored market still carries `polymarket_condition_id`,
//! `event_id`, and the Polymarket `external_url` (the "view on Polymarket"
//! resolution link) through the existing `set_market_metadata` path, which is
//! exactly what marks it as a mirror-with-source end to end.
//!
//! [`GammaClient::fetch_active_events`]: crate::polymarket::gamma::GammaClient::fetch_active_events
//! [`GammaClient::fetch_curated_events`]: crate::polymarket::gamma::GammaClient::fetch_curated_events
//! [`SyncActor`]: crate::sync::SyncActor

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Parsed curated seed set.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CuratedMarkets {
    /// Free-form provenance note for humans editing the file. Ignored by code.
    #[serde(default)]
    pub description: String,
    /// Curated events, in listing order.
    #[serde(default)]
    pub events: Vec<CuratedEvent>,
}

/// One curated Polymarket event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CuratedEvent {
    /// Polymarket Gamma event id (stable numeric id, carried as a string).
    /// Required — this is the key the mirror fetches by.
    pub event_id: String,
    /// Polymarket event slug. Documentation / cross-check only.
    #[serde(default)]
    pub slug: String,
    /// Human-readable title. Documentation only.
    #[serde(default)]
    pub title: String,
    /// Why this event is in the seed set, plus any threshold notes.
    /// Documentation only.
    #[serde(default)]
    pub note: String,
}

impl CuratedMarkets {
    /// Load and validate a curated set from a JSON file on disk.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        Self::parse_json(&data)
    }

    /// Parse and validate a curated set from a JSON string.
    pub fn parse_json(data: &str) -> Result<Self, Error> {
        let parsed: Self = serde_json::from_str(data)?;
        parsed.validate()?;
        Ok(parsed)
    }

    fn validate(&self) -> Result<(), Error> {
        for (i, event) in self.events.iter().enumerate() {
            if event.event_id.trim().is_empty() {
                return Err(Error::PolymarketApi(format!(
                    "curated event #{i} (slug={:?}) has an empty event_id",
                    event.slug
                )));
            }
        }
        Ok(())
    }

    /// De-duplicated, order-preserving list of Polymarket event ids to fetch.
    pub fn event_ids(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.events
            .iter()
            .map(|event| event.event_id.trim().to_string())
            .filter(|id| !id.is_empty() && seen.insert(id.clone()))
            .collect()
    }

    /// Number of curated events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the curated set is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let json = r#"{ "events": [ { "event_id": "123" } ] }"#;
        let curated = CuratedMarkets::parse_json(json).unwrap();
        assert_eq!(curated.len(), 1);
        assert_eq!(curated.event_ids(), vec!["123"]);
        assert_eq!(curated.events[0].slug, "");
    }

    #[test]
    fn parses_full_config_with_docs_fields() {
        let json = r#"{
            "description": "seed set",
            "events": [
                { "event_id": "556382", "slug": "best-ai", "title": "Best AI?", "note": "negrisk" },
                { "event_id": "500753", "slug": "anthropic-val", "title": "Anthropic", "note": "$1.75T" }
            ]
        }"#;
        let curated = CuratedMarkets::parse_json(json).unwrap();
        assert_eq!(curated.len(), 2);
        assert_eq!(curated.event_ids(), vec!["556382", "500753"]);
        assert_eq!(curated.events[1].note, "$1.75T");
    }

    #[test]
    fn dedups_event_ids_preserving_order() {
        let json = r#"{ "events": [
            { "event_id": "1" }, { "event_id": "2" }, { "event_id": "1" }, { "event_id": " 2 " }
        ] }"#;
        let curated = CuratedMarkets::parse_json(json).unwrap();
        // Trimmed and de-duplicated; first-seen order preserved.
        assert_eq!(curated.event_ids(), vec!["1", "2"]);
    }

    #[test]
    fn empty_event_id_is_rejected() {
        let json = r#"{ "events": [ { "event_id": "  " } ] }"#;
        let err = CuratedMarkets::parse_json(json).unwrap_err();
        assert!(err.to_string().contains("empty event_id"), "{err}");
    }

    #[test]
    fn empty_config_is_valid_and_empty() {
        let curated = CuratedMarkets::parse_json("{}").unwrap();
        assert!(curated.is_empty());
        assert!(curated.event_ids().is_empty());
    }

    #[test]
    fn checked_in_seed_set_parses_and_is_nonempty() {
        // The file the deploy actually ships. Keep the small reviewed set
        // honest against the parser and accidental catalog expansion.
        let data = include_str!("../curated_markets.json");
        let curated = CuratedMarkets::parse_json(data).unwrap();
        assert_eq!(curated.len(), 4, "expected the four reviewed seed events");
        // Every id is a non-empty numeric string.
        for id in curated.event_ids() {
            assert!(
                id.chars().all(|c| c.is_ascii_digit()),
                "non-numeric id {id}"
            );
        }
        assert_eq!(
            curated.event_ids(),
            ["333737", "79075", "85299", "96557"].map(str::to_string)
        );
    }
}
