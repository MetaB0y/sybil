pub const DEFAULT_ORDER_TTL_BLOCKS: u64 = 63_072_000;
/// Default capital floor per resting order: one tenth of a cent. At this floor, live
/// order-state growth is backed by at least $0.001 of cash notional or position
/// value instead of zero/dust reservation.
pub const DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnalyticsMemoryStats {
    pub equity_known_accounts: usize,
    pub equity_cached_accounts: usize,
    pub equity_cached_points: usize,
    pub equity_pending_points: usize,
    pub equity_points_per_account_capacity: usize,
    pub history_cached_accounts: usize,
    pub history_cached_events: usize,
    pub history_pending_events: usize,
    pub history_events_per_account_capacity: usize,
    pub history_event_next_seq: u64,
}

/// All tunable parameters for a [`BlockSequencer`] and its surrounding actor.
///
/// Construct via [`SequencerConfig::default()`] for sensible defaults, then
/// override individual fields as needed.
#[derive(Clone, Debug)]
pub struct SequencerConfig {
    /// Order time-to-live in blocks. Orders not filled within this many blocks
    /// are expired from the order book. Default is ~1 year (GTC behaviour).
    pub order_ttl_blocks: u64,
    /// Block production interval. Drives the actor tick loop.
    pub block_interval: std::time::Duration,
    /// Cap on buffered MM / multi-market submissions waiting for the next
    /// block. A runaway client hits backpressure before exhausting memory.
    pub max_pending_bundles: usize,
    /// Maximum number of orders accepted in one submission. Bounds request
    /// amplification before the solver ever sees the payload.
    pub max_orders_per_submission: usize,
    /// Per-account sustained submission rate. Set generously: this is a guard
    /// rail for runaway agents, not a normal trading throttle.
    pub max_submissions_per_account_per_second: u32,
    /// Per-account burst allowance for the submission rate limiter.
    pub submission_burst_per_account: u32,
    /// Global sustained order/cancel submission rate. This bounds coordinated
    /// many-account floods and invalid signed traffic before account lookup.
    pub max_global_submissions_per_second: u32,
    /// Global burst allowance for the submission rate limiter.
    pub global_submission_burst: u32,
    /// Maximum resting non-MM orders per account, including non-MM orders
    /// already staged in pending bundles.
    pub max_open_orders_per_account: usize,
    /// Minimum `ceil(limit_price * quantity / SHARE_SCALE)` for a non-MM order
    /// that may enter the resting book. Expressed in nanodollars.
    pub min_resting_order_notional_nanos: u64,
    /// Maximum deferred MM / multi-market submissions per account.
    pub max_pending_bundles_per_account: usize,
    /// In-memory ring buffer size for recent blocks (served by the `/blocks`
    /// history endpoint). Bounds memory use per sequencer.
    pub block_history_capacity: usize,
    /// Maximum in-memory chart points retained per market. This is a serving
    /// cache, not canonical state.
    pub max_price_history_points_per_market: usize,
    /// Durable full-block history rows retained by bounded pruning. 0 means
    /// do not prune this stream.
    pub block_history_retention_blocks: u64,
    /// Durable raw price-point rows retained by bounded pruning. 0 means do
    /// not prune this stream.
    pub raw_price_retention_blocks: u64,
    /// Block cadence for retention maintenance. 0 disables scheduled pruning.
    pub history_prune_interval_blocks: u64,
    /// Maximum durable history rows deleted in one maintenance pass.
    pub history_prune_max_rows: usize,
    /// Candle resolutions maintained from committed raw price points.
    pub price_candle_resolutions_secs: Vec<u32>,
    /// Per-resolution durable candle retention, aligned by index with
    /// `price_candle_resolutions_secs`. 0 means unbounded for that resolution.
    pub price_candle_retention_secs: Vec<u64>,
    /// Maximum in-memory fill records retained per account for API queries.
    /// Persistent storage may retain more rows.
    pub max_fill_history_per_account: usize,
    /// In-memory equity points retained per account (serving fallback only;
    /// full series lives in redb). Set to 0 in prod.
    pub max_equity_points_per_account: usize,
    /// In-memory history events retained per account (serving fallback only).
    /// Set to 0 in prod.
    pub max_history_events_per_account: usize,
    /// Queue depth where actor mailbox pressure should be logged as a warning.
    /// Set to 0 to disable warning logs.
    pub actor_queue_warn_depth: usize,
    /// Queue depth where actor mailbox pressure should be logged as an error.
    /// Set to 0 to disable error logs.
    pub actor_queue_error_depth: usize,
    /// Width of the ±band around each market's midprice used by the
    /// off-block `LiquidityTracker` to score "near-the-money" depth. Default
    /// 50_000_000 nanos = $0.05. Ships on the wire alongside the average so
    /// FE labels can read "(±$0.05)".
    pub liquidity_band_nanos: u64,
    /// Explicit devnet escape hatch for old log-and-commit behavior when
    /// hard block verification fails. Production defaults to fail-closed.
    pub verification_fail_open: bool,
    /// Run the native full verifier inline as a debug/prover-adjacent check.
    /// Production keeps this off; unit tests and scenario simulations enable
    /// it so verifier drift is caught outside the hot block path.
    pub debug_verify_full: bool,
}

impl Default for SequencerConfig {
    fn default() -> Self {
        Self {
            order_ttl_blocks: DEFAULT_ORDER_TTL_BLOCKS,
            block_interval: std::time::Duration::from_secs(1),
            max_pending_bundles: 10_000,
            max_orders_per_submission: 64,
            max_submissions_per_account_per_second: 50,
            submission_burst_per_account: 200,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 3_000,
            max_open_orders_per_account: 1_000,
            min_resting_order_notional_nanos: DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS,
            max_pending_bundles_per_account: 100,
            block_history_capacity: 100,
            max_price_history_points_per_market:
                crate::price_tracker::DEFAULT_MAX_PRICE_HISTORY_POINTS_PER_MARKET,
            block_history_retention_blocks: 0,
            raw_price_retention_blocks: 0,
            history_prune_interval_blocks: 1_000,
            history_prune_max_rows: 10_000,
            price_candle_resolutions_secs: vec![60, 300, 3_600],
            price_candle_retention_secs: vec![2_592_000, 15_552_000, 0],
            max_fill_history_per_account:
                crate::fill_recorder::DEFAULT_MAX_FILL_HISTORY_PER_ACCOUNT,
            max_equity_points_per_account: crate::aggregates::MAX_EQUITY_POINTS,
            max_history_events_per_account: crate::aggregates::MAX_HISTORY_EVENTS_PER_ACCOUNT,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
            liquidity_band_nanos: 50_000_000,
            verification_fail_open: false,
            debug_verify_full: cfg!(test),
        }
    }
}
