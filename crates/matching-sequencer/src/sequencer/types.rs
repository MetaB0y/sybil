use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OrderSubmission {
    pub account_id: AccountId,
    pub orders: Vec<Order>,
    pub mm_constraint: Option<MmConstraint>,
}

/// Result of [`BlockSequencer::try_admit_direct`].
///
/// A submission that targets a single market with a single non-MM order can
/// be inserted into the resting order book immediately, becoming visible to
/// clients and to the next block's solver without a mempool wait. MM bundles
/// and multi-market / multi-order submissions still need the block-time
/// solver path (STP, flash liquidity, bundle atomicity), so the caller is
/// asked to defer them via its existing buffering path.
#[derive(Debug)]
pub enum AdmitOutcome {
    /// Submission was fully admitted into the resting book. `resting_order`
    /// is a clone of the row that was pushed — the actor serializes it into
    /// the admit-log WAL so the admit survives a crash before the next block.
    Admitted {
        order_id: u64,
        resting_order: crate::order_book::RestingOrder,
    },
    /// Submission is not eligible for direct admission; caller should route
    /// it through the existing pre-block buffer.
    Deferred {
        order_ids: Vec<u64>,
        submission: OrderSubmission,
    },
    /// Submission was rejected synchronously (bad market, missing account,
    /// insufficient balance, ...).
    Rejected(SequencerError),
}

/// Result of a single batch — thin view over a Block for simulation compatibility.
pub struct BatchResult {
    pub pipeline_result: PipelineResult,
    pub fills: Vec<Fill>,
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub total_welfare: i64,
    pub total_volume: u64,
    pub rejections: Vec<Rejection>,
    pub orders_submitted: usize,
    pub orders_filled: usize,
}

/// Public view of a pending order for API exposure.
#[derive(Clone, Debug)]
pub struct PendingOrderInfo {
    pub order_id: u64,
    pub account_id: AccountId,
    pub market_ids: Vec<MarketId>,
    pub side: &'static str,
    pub limit_price: Nanos,
    pub remaining_qty: u64,
    pub created_at_block: u64,
    pub expires_at_block: u64,
    /// Quantity at admit time (B5/B8). `remaining_qty` shrinks as the
    /// order is partially filled; this stays constant. Used by the FE to
    /// draw partial-fill progress.
    pub original_quantity: u64,
    /// Wall-clock admit time, ms since epoch. `0` for pre-existing orders.
    pub created_at_ms: u64,
}

pub struct PreparedBlock {
    pub(crate) next_sequencer: BlockSequencer,
    pub(crate) production: BlockProduction,
}

pub(crate) struct SolvedBatch {
    pub(crate) pipeline_result: PipelineResult,
    pub(crate) fills: Vec<Fill>,
    pub(crate) clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub(crate) total_welfare: i64,
    pub(crate) total_volume: u64,
    pub(crate) orders_filled: usize,
    /// Per-market welfare from this batch's fills. A multi-market order
    /// credits each active market with its full welfare contribution; the
    /// platform `total_welfare` counts each fill once (so sum-of-per-market
    /// over-counts for bundles, just like `placers_by_market`).
    pub(crate) welfare_by_market: HashMap<MarketId, i64>,
}

pub(crate) struct FinalizedBlockState {
    pub(crate) post_state: CanonicalState,
    /// Per-market volume split for this block, returned by
    /// `PriceTracker::record_block` and plumbed onto the Block so wire
    /// consumers see `BlockMarketStats.volume_nanos`.
    pub(crate) volume_by_market: HashMap<MarketId, u64>,
    pub(crate) mark_prices: HashMap<MarketId, Vec<Nanos>>,
    pub(crate) minting_cost: i64,
    pub(crate) invariant_failures: Vec<BlockInvariantFailure>,
}

pub(crate) struct WitnessArtifacts {
    pub(crate) header: BlockHeader,
    pub(crate) witness: BlockWitness,
}

pub(crate) struct WitnessAssemblyInput<'a> {
    pub(crate) post_state: CanonicalState,
    pub(crate) order_count: u32,
    pub(crate) timestamp_ms: u64,
    pub(crate) previous_header: Option<WitnessBlockHeader>,
    pub(crate) witness_orders: Vec<WitnessOrder>,
    pub(crate) witness_rejections: Vec<WitnessRejection>,
    pub(crate) system_events: &'a [SystemEvent],
    pub(crate) fills: &'a [Fill],
    pub(crate) clearing_prices: &'a HashMap<MarketId, Vec<Nanos>>,
    pub(crate) total_welfare: i64,
    pub(crate) minting_cost: i64,
    pub(crate) problem: &'a Problem,
    pub(crate) pre_state: Vec<AccountSnapshot>,
    pub(crate) pre_state_sidecar: sybil_verifier::StateSidecarSnapshot,
    pub(crate) pre_deposit_frontier: sybil_l1_protocol::DepositFrontier,
    pub(crate) post_system_state: Vec<AccountSnapshot>,
    pub(crate) resolved_markets: Vec<MarketId>,
}

impl PreparedBlock {
    pub fn production(&self) -> &BlockProduction {
        &self.production
    }

    pub fn next_sequencer(&self) -> &BlockSequencer {
        &self.next_sequencer
    }
}

impl PendingOrderInfo {
    pub(super) fn from_resting(
        order: &Order,
        account_id: AccountId,
        created_at: u64,
        expires_at_block: u64,
        original_max_fill: u64,
        created_at_ms: u64,
    ) -> Self {
        let market_ids: Vec<_> = order.active_markets().collect();
        let side = super::production::witness::classify_order_side(order);
        Self {
            order_id: order.id,
            account_id,
            market_ids,
            side,
            limit_price: order.limit_price,
            remaining_qty: order.max_fill.0,
            created_at_block: created_at,
            expires_at_block,
            // `original_max_fill` is `0` on pre-B5 snapshots (#[serde(default)]).
            // Fall back to the current `max_fill` so the FE progress bar
            // still renders sensibly during the rolling transition.
            original_quantity: if original_max_fill == 0 {
                order.max_fill.0
            } else {
                original_max_fill
            },
            created_at_ms,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LeaderboardBase {
    pub account_id: AccountId,
    /// All-time net PnL (portfolio value − total deposited), nanos.
    pub pnl_nanos: i64,
    /// Current portfolio equity (balance + marked positions), nanos.
    pub equity_nanos: i64,
    /// Total deposited capital, nanos — the all-time ROI basis.
    pub deposited_nanos: i64,
    /// Distinct markets with a currently open position.
    pub markets_traded: u32,
}

/// One ranked leaderboard entry over a window (SYB-59). `pnl_nanos` and
/// `roi_bps` are already windowed; the actor assigns the final ordering.
#[derive(Clone, Copy, Debug)]
pub struct LeaderboardRow {
    pub account_id: AccountId,
    /// Windowed net PnL, nanos.
    pub pnl_nanos: i64,
    /// Windowed return on capital, basis points (100 = 1%).
    pub roi_bps: i64,
    /// Distinct markets with a currently open position.
    pub markets_traded: u32,
    /// Current portfolio equity, nanos.
    pub equity_nanos: i64,
}

pub fn batch_result_from_block(
    block: &Block,
    analytics: &BlockAnalytics,
    pipeline_result: PipelineResult,
) -> BatchResult {
    BatchResult {
        pipeline_result,
        fills: block.fills.clone(),
        clearing_prices: block.clearing_prices.clone(),
        total_welfare: analytics.total_welfare,
        total_volume: analytics.total_volume,
        rejections: block.rejections.clone(),
        orders_submitted: block.header.order_count as usize,
        orders_filled: analytics.orders_filled,
    }
}
