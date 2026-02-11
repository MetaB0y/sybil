//! Witness types consumed by the verifier.
//!
//! The sequencer builds a [`BlockWitness`] after each block. The verifier
//! (and future ZK circuit) takes it as input and checks every constraint.

use std::collections::HashMap;

use matching_engine::{Fill, MarketGroup, MarketId, MmConstraint, Nanos, Order};

/// Everything the verifier needs to check a single block.
///
/// Built by the sequencer, consumed by the verifier. A future ZK circuit
/// takes this as its public/private input.
pub struct BlockWitness {
    /// Block header being verified.
    pub header: WitnessBlockHeader,
    /// Previous block header (`None` for genesis).
    pub previous_header: Option<WitnessBlockHeader>,

    // -- Orders --
    /// Orders accepted into this batch (with account mapping).
    pub orders: Vec<WitnessOrder>,
    /// Orders rejected (with reasons).
    pub rejections: Vec<WitnessRejection>,

    // -- Solver output --
    pub fills: Vec<Fill>,
    /// Clearing prices per market: `market_id -> [price_outcome_0, price_outcome_1, ...]`.
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub total_welfare: i64,
    /// Minting cost not captured in fill-level welfare (MILP only, 0 for heuristics).
    pub minting_cost: i64,

    // -- Constraints --
    pub mm_constraints: Vec<MmConstraint>,
    pub market_groups: Vec<MarketGroup>,

    // -- Account state --
    /// Account snapshots *before* settlement, sorted by id.
    pub pre_state: Vec<AccountSnapshot>,
    /// Account snapshots *after* settlement, sorted by id.
    pub post_state: Vec<AccountSnapshot>,

    /// Markets that are resolved/voided — orders/fills must not reference these.
    pub resolved_markets: Vec<MarketId>,
}

/// Minimal block header stored in the witness.
#[derive(Clone, Debug)]
pub struct WitnessBlockHeader {
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
}

/// An order together with the account that placed it.
pub struct WitnessOrder {
    pub order: Order,
    pub account_id: u64,
    /// Whether this is a market-maker order (skip balance validation).
    pub is_mm: bool,
}

/// A rejected order together with a reason.
pub struct WitnessRejection {
    pub order: Order,
    pub account_id: u64,
    pub reason: RejectionReason,
}

/// Reason an order was rejected (mirrors sequencer's `RejectionReason`).
#[derive(Clone, Debug)]
pub enum RejectionReason {
    InsufficientBalance {
        required: i64,
        available: i64,
    },
    InsufficientPosition {
        market: MarketId,
        outcome: u8,
        required: i64,
        available: i64,
    },
    AccountNotFound,
}

/// Snapshot of a single account's state at a point in time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub id: u64,
    pub balance: i64,
    /// Sorted by `(market, outcome)`.
    pub positions: Vec<(MarketId, u8, i64)>,
}
