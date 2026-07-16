pub const DEFAULT_ORDER_TTL_BLOCKS: u64 = 63_072_000;
/// Default capital floor per resting order: one tenth of a cent. At this floor, live
/// order-state growth is backed by at least $0.001 of cash notional or position
/// value instead of zero/dust reservation.
pub const DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS: u64 = 1_000_000;

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
    /// Maximum number of orders accepted in one submission. The service-only
    /// bulk route needs enough room for one two-sided quote across the current
    /// market catalog; this still bounds amplification before solver work.
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
    /// In-memory cache of recent canonical blocks. This is distinct from both
    /// the bounded replay archive and the product-history service.
    pub recent_block_cache_capacity: usize,
    /// Canonical replay heights (and paired DA artifacts) retained locally.
    /// Zero disables archive maintenance.
    pub canonical_archive_retention_blocks: u64,
    /// Block cadence for canonical archive maintenance.
    pub canonical_archive_maintenance_interval_blocks: u64,
    /// Maximum replay-block or DA-artifact rows deleted in one pass.
    pub canonical_archive_max_rows_per_pass: usize,
    /// Acknowledged proof-job heights retained in the sequencer after the
    /// standalone prover durably ingests them. Zero disables pruning.
    pub acknowledged_proof_job_retention_blocks: u64,
    /// Block cadence for acknowledged proof-job maintenance.
    pub acknowledged_proof_job_maintenance_interval_blocks: u64,
    /// Maximum old proof-job rows examined in one maintenance pass.
    pub acknowledged_proof_job_max_rows_per_pass: usize,
    /// Retain portable proof jobs and DA serving artifacts for an attached
    /// validity stack. Disable this for a product-only chain that makes no
    /// ZK/DA validity claim. Native verification, the latest recovery witness,
    /// canonical replay blocks, and product history remain enabled.
    ///
    /// This is chain identity, not a live tuning knob: persistent API stores
    /// bind the value before the first block and refuse later changes.
    pub retain_validity_artifacts: bool,
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
            max_orders_per_submission: 512,
            max_submissions_per_account_per_second: 50,
            submission_burst_per_account: 200,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 3_000,
            max_open_orders_per_account: 1_000,
            min_resting_order_notional_nanos: DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS,
            max_pending_bundles_per_account: 100,
            recent_block_cache_capacity: 100,
            canonical_archive_retention_blocks: 0,
            canonical_archive_maintenance_interval_blocks: 1_000,
            canonical_archive_max_rows_per_pass: 10_000,
            acknowledged_proof_job_retention_blocks: 0,
            acknowledged_proof_job_maintenance_interval_blocks: 1_000,
            acknowledged_proof_job_max_rows_per_pass: 10_000,
            retain_validity_artifacts: true,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
            liquidity_band_nanos: 50_000_000,
            verification_fail_open: false,
            debug_verify_full: cfg!(test),
        }
    }
}
