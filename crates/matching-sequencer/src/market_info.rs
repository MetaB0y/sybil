use std::fmt;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceHistoryPage {
    pub points: Vec<PricePoint>,
    pub next_before_height: Option<u64>,
    pub retention_min_height: Option<u64>,
}

/// Downsampled committed-batch price history for one market and resolution.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PriceCandle {
    pub bucket_start_ms: u64,
    pub bucket_end_ms: u64,
    pub first_height: u64,
    pub last_height: u64,
    pub open_yes_price: Nanos,
    pub high_yes_price: Nanos,
    pub low_yes_price: Nanos,
    pub close_yes_price: Nanos,
    pub open_no_price: Nanos,
    pub high_no_price: Nanos,
    pub low_no_price: Nanos,
    pub close_no_price: Nanos,
    pub volume_nanos: u64,
    pub point_count: u64,
}

impl PriceCandle {
    pub fn from_point(resolution_secs: u32, point: &PricePoint) -> Self {
        let resolution_ms = u64::from(resolution_secs.max(1)).saturating_mul(1000);
        let bucket_start_ms = point.timestamp_ms - (point.timestamp_ms % resolution_ms);
        Self {
            bucket_start_ms,
            bucket_end_ms: bucket_start_ms.saturating_add(resolution_ms),
            first_height: point.height,
            last_height: point.height,
            open_yes_price: point.yes_price,
            high_yes_price: point.yes_price,
            low_yes_price: point.yes_price,
            close_yes_price: point.yes_price,
            open_no_price: point.no_price,
            high_no_price: point.no_price,
            low_no_price: point.no_price,
            close_no_price: point.no_price,
            volume_nanos: point.volume_nanos,
            point_count: 1,
        }
    }

    pub fn merge_point(&mut self, point: &PricePoint) {
        if point.height < self.first_height {
            self.first_height = point.height;
            self.open_yes_price = point.yes_price;
            self.open_no_price = point.no_price;
        }
        if point.height >= self.last_height {
            self.last_height = point.height;
            self.close_yes_price = point.yes_price;
            self.close_no_price = point.no_price;
        }
        self.high_yes_price = self.high_yes_price.max(point.yes_price);
        self.low_yes_price = self.low_yes_price.min(point.yes_price);
        self.high_no_price = self.high_no_price.max(point.no_price);
        self.low_no_price = self.low_no_price.min(point.no_price);
        self.volume_nanos = self.volume_nanos.saturating_add(point.volume_nanos);
        self.point_count = self.point_count.saturating_add(1);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceCandlePage {
    pub resolution_secs: u32,
    pub candles: Vec<PriceCandle>,
    pub next_before_ms: Option<u64>,
    pub retention_min_bucket_ms: Option<u64>,
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

/// A durable derived-history page plus the oldest timestamp for which the
/// server can claim complete retention. `None` means retention is disabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedHistoryPage<T> {
    pub items: Vec<T>,
    pub retention_min_timestamp_ms: Option<u64>,
    /// Fill-history only: this account's high-water block removed by retention.
    pub pruned_through_height: Option<u64>,
    pub durable: bool,
    pub source_points: usize,
    pub downsampled: bool,
}

/// Stable per-account fill cursor.
///
/// The HTTP representation is `"<block_height>.<order_id>"`. `order_id` is
/// sequencer-assigned and globally monotonic, so `(block_height, order_id)` is
/// stable across restarts and already matches the durable fill-history key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountFillCursor {
    pub block_height: u64,
    pub order_id: u64,
}

impl AccountFillCursor {
    pub const MIN: Self = Self {
        block_height: 0,
        order_id: 0,
    };

    pub fn new(block_height: u64, order_id: u64) -> Self {
        Self {
            block_height,
            order_id,
        }
    }

    pub fn from_record(record: &AccountFillRecord) -> Self {
        Self::new(record.block_height, record.order_id)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let (block_height, order_id) = value.split_once('.')?;
        Some(Self {
            block_height: block_height.parse().ok()?,
            order_id: order_id.parse().ok()?,
        })
    }
}

impl fmt::Display for AccountFillCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.block_height, self.order_id)
    }
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
