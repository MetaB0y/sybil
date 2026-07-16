use matching_engine::{MarketId, Nanos};

/// Per-market resolution configuration.
///
/// References a template by name. The template's policy then drives how
/// attestations are evaluated. Stored inside `MarketMetadata` so it persists
/// via the existing `MARKET_META` table without a layout bump.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolutionConfig {
    pub template: String,
}

/// Metadata for a market (sequencer-layer, not in matching-engine).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarketMetadata {
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub resolution_criteria: String,
    /// 0 = no expiry
    pub expiry_timestamp_ms: u64,
    pub created_at_ms: u64,
    /// Which resolution template this market uses. `None` = default
    /// (`admin_immediate`) — keeps legacy markets resolvable without
    /// migration.
    #[serde(default)]
    pub resolution_config: Option<ResolutionConfig>,
    /// Stable operator-supplied identity for idempotent market creation.
    /// `None` preserves the ordinary allocate-on-every-call behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_key: Option<String>,
    /// Recovery-only override for DA-imported markets where the witness proves
    /// the metadata digest but does not carry the raw metadata fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_metadata_digest: Option<[u8; 32]>,
}

impl MarketMetadata {
    /// Template this market resolves under; falls back to the default admin
    /// template when no config is set.
    pub fn effective_template(&self) -> &str {
        self.resolution_config
            .as_ref()
            .map(|c| c.template.as_str())
            .unwrap_or("admin_immediate")
    }

    pub(crate) fn same_creation_fields(&self, other: &Self) -> bool {
        let mut self_tags = self.tags.clone();
        let mut other_tags = other.tags.clone();
        self_tags.sort();
        other_tags.sort();
        self.description == other.description
            && self.category == other.category
            && self_tags == other_tags
            && self.resolution_criteria == other.resolution_criteria
            && self.expiry_timestamp_ms == other.expiry_timestamp_ms
            && self.resolution_config == other.resolution_config
            && self.creation_key == other.creation_key
    }
}

/// A single price observation for a market at a given block.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PricePoint {
    pub height: u64,
    pub timestamp_ms: u64,
    pub yes_price: Nanos,
    pub no_price: Nanos,
    /// Per-market volume for this block.
    pub volume_nanos: u64,
}

/// Record of a fill attributed to an account.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountFillRecord {
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price: Nanos,
    pub block_height: u64,
    pub timestamp_ms: u64,
    /// Position changes from this fill: (market_id, outcome, signed delta).
    pub position_deltas: Vec<(MarketId, u8, i64)>,
}

/// Query parameters for searching markets.
#[derive(Clone, Debug, Default)]
pub struct MarketSearchQuery {
    /// Searches name + description (case-insensitive substring).
    pub text: Option<String>,
    /// Any tag matches.
    pub tags: Option<Vec<String>>,
    /// Exact category match.
    pub category: Option<String>,
    /// "active" or "resolved".
    pub status: Option<String>,
    pub min_yes_price: Option<Nanos>,
    pub max_yes_price: Option<Nanos>,
    pub min_volume: Option<u64>,
    pub sort_by: Option<MarketSortField>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Fields by which markets can be sorted.
#[derive(Clone, Debug)]
pub enum MarketSortField {
    Volume,
    CreatedAt,
    Name,
    Price,
}
