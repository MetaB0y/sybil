//! Dependency-light contract between the sequencer product-history outbox, the
//! private history projector, and the API proxy.
//!
//! These are committed *facts*, not sequencer storage rows and not validity
//! inputs. Keep the schema versioned and additive. A history projection may be
//! deleted and rebuilt without changing exchange state.

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};

pub const COMMITTED_HISTORY_SCHEMA_V1: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct PositionDeltaFact {
    pub market_id: u32,
    pub outcome: u8,
    pub delta: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct AccountFillFact {
    pub account_id: u64,
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price_nanos: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub position_deltas: Vec<PositionDeltaFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct AccountEquityFact {
    pub account_id: u64,
    pub height: u64,
    pub timestamp_ms: u64,
    pub portfolio_value_nanos: i64,
    pub deposited_nanos: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountEventKind {
    Created,
    Placed,
    PartialFill,
    Filled,
    Cancelled,
    Expired,
    Deposit,
    Withdrawal,
    Resolved,
    Rejected,
}

impl AccountEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Placed => "placed",
            Self::PartialFill => "partial_fill",
            Self::Filled => "filled",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Resolved => "resolved",
            Self::Rejected => "rejected",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::Created | Self::Deposit | Self::Withdrawal => "funding",
            Self::Resolved => "settlement",
            _ => "trades",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct AccountEventFact {
    pub account_id: u64,
    pub seq: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub kind: AccountEventKind,
    pub market_id: Option<u32>,
    pub order_id: Option<u64>,
    pub side: Option<String>,
    pub outcome: Option<String>,
    pub qty: Option<u64>,
    pub price_nanos: Option<u64>,
    pub amount_nanos: Option<i64>,
    pub realized_pnl_nanos: Option<i64>,
    pub payout_outcome: Option<String>,
    pub reason: Option<String>,
    pub required_nanos: Option<i64>,
    pub available_nanos: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct MarketPriceFact {
    pub market_id: u32,
    pub height: u64,
    pub timestamp_ms: u64,
    pub yes_price_nanos: u64,
    pub no_price_nanos: u64,
    pub volume_nanos: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceCandle {
    pub bucket_start_ms: u64,
    pub bucket_end_ms: u64,
    pub first_height: u64,
    pub last_height: u64,
    pub open_yes_price_nanos: u64,
    pub high_yes_price_nanos: u64,
    pub low_yes_price_nanos: u64,
    pub close_yes_price_nanos: u64,
    pub open_no_price_nanos: u64,
    pub high_no_price_nanos: u64,
    pub low_no_price_nanos: u64,
    pub close_no_price_nanos: u64,
    pub volume_nanos: u64,
    pub point_count: u64,
}

impl PriceCandle {
    pub fn from_point(resolution_secs: u32, point: MarketPriceFact) -> Self {
        let resolution_ms = u64::from(resolution_secs.max(1)).saturating_mul(1_000);
        let bucket_start_ms = point.timestamp_ms - point.timestamp_ms % resolution_ms;
        Self {
            bucket_start_ms,
            bucket_end_ms: bucket_start_ms.saturating_add(resolution_ms),
            first_height: point.height,
            last_height: point.height,
            open_yes_price_nanos: point.yes_price_nanos,
            high_yes_price_nanos: point.yes_price_nanos,
            low_yes_price_nanos: point.yes_price_nanos,
            close_yes_price_nanos: point.yes_price_nanos,
            open_no_price_nanos: point.no_price_nanos,
            high_no_price_nanos: point.no_price_nanos,
            low_no_price_nanos: point.no_price_nanos,
            close_no_price_nanos: point.no_price_nanos,
            volume_nanos: point.volume_nanos,
            point_count: 1,
        }
    }

    pub fn merge_point(&mut self, point: MarketPriceFact) {
        if point.height < self.first_height {
            self.first_height = point.height;
            self.open_yes_price_nanos = point.yes_price_nanos;
            self.open_no_price_nanos = point.no_price_nanos;
        }
        if point.height >= self.last_height {
            self.last_height = point.height;
            self.close_yes_price_nanos = point.yes_price_nanos;
            self.close_no_price_nanos = point.no_price_nanos;
        }
        self.high_yes_price_nanos = self.high_yes_price_nanos.max(point.yes_price_nanos);
        self.low_yes_price_nanos = self.low_yes_price_nanos.min(point.yes_price_nanos);
        self.high_no_price_nanos = self.high_no_price_nanos.max(point.no_price_nanos);
        self.low_no_price_nanos = self.low_no_price_nanos.min(point.no_price_nanos);
        self.volume_nanos = self.volume_nanos.saturating_add(point.volume_nanos);
        self.point_count = self.point_count.saturating_add(1);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedHistoryBatchV1 {
    pub schema_version: u16,
    pub genesis_hash: [u8; 32],
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub committed_at_ms: u64,
    pub fills: Vec<AccountFillFact>,
    pub equity: Vec<AccountEquityFact>,
    pub events: Vec<AccountEventFact>,
    pub prices: Vec<MarketPriceFact>,
    pub payload_hash: [u8; 32],
}

#[derive(BorshSerialize)]
struct HashPayload<'a> {
    schema_version: u16,
    genesis_hash: &'a [u8; 32],
    height: u64,
    parent_hash: &'a [u8; 32],
    block_hash: &'a [u8; 32],
    state_root: &'a [u8; 32],
    committed_at_ms: u64,
    fills: &'a [AccountFillFact],
    equity: &'a [AccountEquityFact],
    events: &'a [AccountEventFact],
    prices: &'a [MarketPriceFact],
}

#[derive(Debug, thiserror::Error)]
pub enum BatchValidationError {
    #[error("unsupported committed-history schema version {0}")]
    UnsupportedSchema(u16),
    #[error("failed to encode committed-history batch: {0}")]
    Encode(String),
    #[error("committed-history payload hash mismatch")]
    PayloadHashMismatch,
    #[error("fact height is inconsistent with batch height")]
    FactHeightMismatch,
    #[error("committed-history facts are not in canonical identity order")]
    NonCanonicalFactOrder,
}

impl CommittedHistoryBatchV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        genesis_hash: [u8; 32],
        height: u64,
        parent_hash: [u8; 32],
        block_hash: [u8; 32],
        state_root: [u8; 32],
        committed_at_ms: u64,
        fills: Vec<AccountFillFact>,
        equity: Vec<AccountEquityFact>,
        events: Vec<AccountEventFact>,
        prices: Vec<MarketPriceFact>,
    ) -> Result<Self, BatchValidationError> {
        let mut batch = Self {
            schema_version: COMMITTED_HISTORY_SCHEMA_V1,
            genesis_hash,
            height,
            parent_hash,
            block_hash,
            state_root,
            committed_at_ms,
            fills,
            equity,
            events,
            prices,
            payload_hash: [0; 32],
        };
        for fill in &mut batch.fills {
            fill.position_deltas
                .sort_by_key(|delta| (delta.market_id, delta.outcome));
        }
        batch
            .fills
            .sort_by_key(|fact| (fact.account_id, fact.block_height, fact.order_id));
        batch
            .equity
            .sort_by_key(|fact| (fact.account_id, fact.height));
        batch
            .events
            .sort_by_key(|fact| (fact.account_id, fact.block_height, fact.seq));
        batch
            .prices
            .sort_by_key(|fact| (fact.market_id, fact.height));
        batch.payload_hash = batch.compute_payload_hash()?;
        batch.validate()?;
        Ok(batch)
    }

    pub fn compute_payload_hash(&self) -> Result<[u8; 32], BatchValidationError> {
        let payload = HashPayload {
            schema_version: self.schema_version,
            genesis_hash: &self.genesis_hash,
            height: self.height,
            parent_hash: &self.parent_hash,
            block_hash: &self.block_hash,
            state_root: &self.state_root,
            committed_at_ms: self.committed_at_ms,
            fills: &self.fills,
            equity: &self.equity,
            events: &self.events,
            prices: &self.prices,
        };
        let bytes = borsh::to_vec(&payload)
            .map_err(|error| BatchValidationError::Encode(error.to_string()))?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    pub fn validate(&self) -> Result<(), BatchValidationError> {
        if self.schema_version != COMMITTED_HISTORY_SCHEMA_V1 {
            return Err(BatchValidationError::UnsupportedSchema(self.schema_version));
        }
        if self.fills.iter().any(|fact| fact.block_height != self.height)
            || self.equity.iter().any(|fact| fact.height != self.height)
            // Account lifecycle events may be staged between blocks (for
            // example a cancellation at the current committed height) and are
            // durably exported by the next block. They may therefore precede,
            // but must never be ahead of, the enclosing batch.
            || self.events.iter().any(|fact| fact.block_height > self.height)
            || self.prices.iter().any(|fact| fact.height != self.height)
        {
            return Err(BatchValidationError::FactHeightMismatch);
        }
        let fills_canonical = self.fills.windows(2).all(|pair| {
            (pair[0].account_id, pair[0].block_height, pair[0].order_id)
                < (pair[1].account_id, pair[1].block_height, pair[1].order_id)
        }) && self.fills.iter().all(|fill| {
            fill.position_deltas.windows(2).all(|pair| {
                (pair[0].market_id, pair[0].outcome) < (pair[1].market_id, pair[1].outcome)
            })
        });
        let equity_canonical = self.equity.windows(2).all(|pair| {
            (pair[0].account_id, pair[0].height) < (pair[1].account_id, pair[1].height)
        });
        let events_canonical = self.events.windows(2).all(|pair| {
            (pair[0].account_id, pair[0].block_height, pair[0].seq)
                < (pair[1].account_id, pair[1].block_height, pair[1].seq)
        });
        let prices_canonical = self
            .prices
            .windows(2)
            .all(|pair| (pair[0].market_id, pair[0].height) < (pair[1].market_id, pair[1].height));
        if !(fills_canonical && equity_canonical && events_canonical && prices_canonical) {
            return Err(BatchValidationError::NonCanonicalFactOrder);
        }
        if self.payload_hash != self.compute_payload_hash()? {
            return Err(BatchValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillCursor {
    pub block_height: u64,
    pub order_id: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillQuery {
    pub account_id: u64,
    pub market_id: Option<u32>,
    pub after: Option<FillCursor>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountEventQuery {
    pub account_id: u64,
    pub limit: usize,
    pub before: Option<(u64, u64)>,
    pub category: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct EquityQuery {
    pub account_id: u64,
    pub since_ms: u64,
}

/// Opening equity anchors for a windowed cross-account calculation such as
/// the public leaderboard. Accounts without a sample at or before the cutoff
/// are omitted from the response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EquityBaselinesQuery {
    pub account_ids: Vec<u64>,
    pub at_or_before_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EquityBaselines {
    pub baselines: Vec<AccountEquityFact>,
    pub status: ProjectionStatus,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PriceHistoryQuery {
    pub market_id: u32,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub before_height: Option<u64>,
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PriceCandleQuery {
    pub market_id: u32,
    pub resolution_secs: u32,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub before_ms: Option<u64>,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionStatus {
    pub genesis_hash: Option<[u8; 32]>,
    pub first_height: Option<u64>,
    pub first_timestamp_ms: Option<u64>,
    pub indexed_through_height: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryPage<T> {
    pub items: Vec<T>,
    pub status: ProjectionStatus,
    pub source_points: usize,
    pub downsampled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceHistoryPage {
    pub points: Vec<MarketPriceFact>,
    pub next_before_height: Option<u64>,
    pub status: ProjectionStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceCandlePage {
    pub resolution_secs: u32,
    pub candles: Vec<PriceCandle>,
    pub next_before_ms: Option<u64>,
    pub status: ProjectionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyBatchOutcome {
    Applied,
    AlreadyApplied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyBatchResponse {
    pub outcome: ApplyBatchOutcome,
    pub indexed_through_height: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> CommittedHistoryBatchV1 {
        CommittedHistoryBatchV1::new(
            [1; 32],
            7,
            [0; 32],
            [2; 32],
            [3; 32],
            99,
            vec![AccountFillFact {
                account_id: 4,
                order_id: 5,
                fill_qty: 6,
                fill_price_nanos: 7,
                block_height: 7,
                timestamp_ms: 99,
                position_deltas: vec![],
            }],
            vec![],
            vec![],
            vec![],
        )
        .expect("valid batch")
    }

    #[test]
    fn payload_hash_detects_mutation() {
        let mut batch = batch();
        assert!(batch.validate().is_ok());
        batch.fills[0].fill_qty += 1;
        assert!(matches!(
            batch.validate(),
            Err(BatchValidationError::PayloadHashMismatch)
        ));
    }

    #[test]
    fn fact_height_must_match_batch() {
        let mut batch = batch();
        batch.fills[0].block_height += 1;
        batch.payload_hash = batch.compute_payload_hash().expect("hash");
        assert!(matches!(
            batch.validate(),
            Err(BatchValidationError::FactHeightMismatch)
        ));
    }

    #[test]
    fn constructor_canonicalizes_facts_and_position_deltas() {
        let fill = |account_id, order_id, deltas| AccountFillFact {
            account_id,
            order_id,
            fill_qty: 1,
            fill_price_nanos: 2,
            block_height: 7,
            timestamp_ms: 99,
            position_deltas: deltas,
        };
        let batch = CommittedHistoryBatchV1::new(
            [1; 32],
            7,
            [0; 32],
            [2; 32],
            [3; 32],
            99,
            vec![
                fill(9, 2, vec![]),
                fill(
                    4,
                    1,
                    vec![
                        PositionDeltaFact {
                            market_id: 5,
                            outcome: 1,
                            delta: 1,
                        },
                        PositionDeltaFact {
                            market_id: 2,
                            outcome: 0,
                            delta: -1,
                        },
                    ],
                ),
            ],
            vec![],
            vec![],
            vec![],
        )
        .expect("canonical batch");
        assert_eq!(batch.fills[0].account_id, 4);
        assert_eq!(batch.fills[0].position_deltas[0].market_id, 2);
        assert!(batch.validate().is_ok());
    }

    #[test]
    fn duplicate_fact_identity_is_rejected() {
        let fill = batch().fills[0].clone();
        let error = CommittedHistoryBatchV1::new(
            [1; 32],
            7,
            [0; 32],
            [2; 32],
            [3; 32],
            99,
            vec![fill.clone(), fill],
            vec![],
            vec![],
            vec![],
        )
        .expect_err("duplicate identity");
        assert!(matches!(error, BatchValidationError::NonCanonicalFactOrder));
    }
}
