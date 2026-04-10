use matching_engine::{MarketId, Nanos};

/// Metadata for a market (sequencer-layer, not in matching-engine).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MarketMetadata {
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub resolution_criteria: String,
    /// 0 = no expiry
    pub expiry_timestamp_ms: u64,
    pub created_at_ms: u64,
}

/// A single price observation for a market at a given block.
#[derive(Clone, Debug)]
pub struct PricePoint {
    pub height: u64,
    pub timestamp_ms: u64,
    pub yes_price: Nanos,
    pub no_price: Nanos,
    /// Per-market volume for this block.
    pub volume_nanos: u64,
}

/// Record of a fill attributed to an account.
#[derive(Clone, Debug)]
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
