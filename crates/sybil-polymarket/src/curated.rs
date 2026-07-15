//! Curated Polymarket mirror seed set (SYB-150).
//!
//! The default mirror path ([`GammaClient::fetch_active_events`]) ranks *all*
//! active Polymarket events by volume and mirrors the top N that pass the
//! category filters. That is right for a broad mirror but wrong for a
//! hand-picked launch set: we want a specific, reviewed list of AI / company /
//! tech questions, addressed by stable Polymarket **condition id** so the
//! selection is deterministic and parent events cannot silently add children.
//!
//! This module loads that curated list from a JSON file (see
//! `crates/sybil-polymarket/curated_markets.json`). When
//! `--curated-markets-path` / `CURATED_MARKETS_PATH` is set, [`SyncActor`]
//! fetches their parent events by id (via
//! [`GammaClient::fetch_curated_events`]) instead of the volume scan, and the
//! mirror and MM bootstrap retain only the configured condition ids.
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
    /// Exact reviewed child markets. When non-empty, this is the authoritative
    /// child allow-list; event ids are used only to fetch parent metadata.
    #[serde(default)]
    pub conditions: Vec<CuratedCondition>,
}

/// One exact Polymarket child market.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CuratedCondition {
    /// Stable 32-byte Polymarket condition id.
    pub condition_id: String,
    /// Parent Gamma event id used to fetch the child.
    pub event_id: String,
    /// Human-readable cross-check only.
    #[serde(default)]
    pub title: String,
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
        let mut seen = HashSet::new();
        for (i, market) in self.conditions.iter().enumerate() {
            let condition_id = market.condition_id.trim();
            if condition_id.len() != 66
                || !condition_id.starts_with("0x")
                || !condition_id[2..].chars().all(|ch| ch.is_ascii_hexdigit())
            {
                return Err(Error::PolymarketApi(format!(
                    "curated condition #{i} is not a 32-byte 0x condition id"
                )));
            }
            if market.event_id.trim().is_empty() || !seen.insert(condition_id.to_ascii_lowercase())
            {
                return Err(Error::PolymarketApi(format!(
                    "curated condition #{i} has an empty event id or duplicate condition id"
                )));
            }
        }
        Ok(())
    }

    /// De-duplicated, order-preserving list of Polymarket event ids to fetch.
    pub fn event_ids(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.conditions
            .iter()
            .map(|market| market.event_id.as_str())
            .chain(self.events.iter().map(|event| event.event_id.as_str()))
            .map(str::trim)
            .map(str::to_string)
            .filter(|id| !id.is_empty() && seen.insert(id.clone()))
            .collect()
    }

    /// Exact, normalized condition ids to retain after fetching parent events.
    pub fn condition_ids(&self) -> Vec<String> {
        self.conditions
            .iter()
            .map(|market| market.condition_id.trim().to_ascii_lowercase())
            .collect()
    }

    /// Number of curated events.
    pub fn len(&self) -> usize {
        if self.conditions.is_empty() {
            self.events.len()
        } else {
            self.conditions.len()
        }
    }

    /// Whether the curated set is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.conditions.is_empty()
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
        assert!(curated.condition_ids().is_empty());
    }

    #[test]
    fn exact_conditions_drive_parent_fetch_and_reject_duplicates() {
        let id = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let json = format!(
            r#"{{ "conditions": [
                {{ "condition_id": "{id}", "event_id": "7" }},
                {{ "condition_id": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "event_id": "7" }}
            ] }}"#
        );
        let curated = CuratedMarkets::parse_json(&json).unwrap();
        assert_eq!(curated.event_ids(), vec!["7"]);
        assert_eq!(curated.condition_ids().len(), 2);

        let duplicate = format!(
            r#"{{ "conditions": [
                {{ "condition_id": "{id}", "event_id": "7" }},
                {{ "condition_id": "{id}", "event_id": "8" }}
            ] }}"#
        );
        assert!(CuratedMarkets::parse_json(&duplicate).is_err());
    }

    #[test]
    fn checked_in_seed_set_parses_and_is_nonempty() {
        // The file the deploy actually ships. Keep the reviewed child set
        // honest against the parser and accidental catalog expansion.
        let data = include_str!("../curated_markets.json");
        let curated = CuratedMarkets::parse_json(data).unwrap();
        assert_eq!(curated.len(), 72, "expected exact reviewed mirror children");
        assert_eq!(curated.condition_ids().len(), 72);
        assert_eq!(curated.event_ids().len(), 10);
    }
}
