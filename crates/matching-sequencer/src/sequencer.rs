use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use matching_engine::{
    Fill, MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem,
};
use matching_solver::{PipelineResult, Solver};
use sybil_oracle::{MarketStatus, Oracle, ResolutionRecord};
use sybil_verifier::{
    AccountSnapshot, BlockWitness, SystemEventWitness, WitnessBlockHeader, WitnessOrder,
    WitnessRejection,
};
use tracing::{debug, error};

use crate::account::{Account, AccountId, AccountStore};
use crate::block::{
    hash_header, state_sidecar_snapshot, Block, BlockFlowMetrics, BlockHeader, BlockProduction,
};
use crate::bridge::{
    account_key, amount_token_units_to_i64_nanos, amount_token_units_to_nanos, BridgeBlockData,
    BridgeError, BridgeState, BridgeWithdrawalRequest, L1Deposit, WithdrawalLeaf,
    DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS,
};
use crate::canonical_state::{snapshot_account, CanonicalState};
use crate::error::{Rejection, RejectionReason, SequencerError};
use crate::market_info::{AccountFillRecord, MarketMetadata, PricePoint};
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::OrderBook;
use crate::settlement;
use crate::store::{RestoredState, SequencerSnapshot};
use crate::system_event::SystemEvent;

/// Default order TTL in blocks. At 500ms block intervals this is ~1 year (GTC).
pub const DEFAULT_ORDER_TTL_BLOCKS: u64 = 63_072_000;

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
    /// Maximum deferred MM / multi-market submissions per account.
    pub max_pending_bundles_per_account: usize,
    /// In-memory ring buffer size for recent blocks (served by the `/blocks`
    /// history endpoint). Bounds memory use per sequencer.
    pub block_history_capacity: usize,
    /// Queue depth where actor mailbox pressure should be logged as a warning.
    /// Set to 0 to disable warning logs.
    pub actor_queue_warn_depth: usize,
    /// Queue depth where actor mailbox pressure should be logged as an error.
    /// Set to 0 to disable error logs.
    pub actor_queue_error_depth: usize,
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
            max_pending_bundles_per_account: 100,
            block_history_capacity: 100,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
        }
    }
}

/// An order submission from a participant.
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
    Deferred(OrderSubmission),
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
}

pub struct PreparedBlock {
    next_sequencer: BlockSequencer,
    production: BlockProduction,
}

struct SolvedBatch {
    pipeline_result: PipelineResult,
    fills: Vec<Fill>,
    clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    total_welfare: i64,
    total_volume: u64,
    orders_filled: usize,
}

struct FinalizedBlockState {
    post_state: CanonicalState,
}

struct WitnessArtifacts {
    header: BlockHeader,
    witness: BlockWitness,
}

struct WitnessAssemblyInput<'a> {
    post_state: CanonicalState,
    order_count: u32,
    timestamp_ms: u64,
    previous_header: Option<WitnessBlockHeader>,
    witness_orders: Vec<WitnessOrder>,
    witness_rejections: Vec<WitnessRejection>,
    system_events: &'a [SystemEvent],
    fills: &'a [Fill],
    clearing_prices: &'a HashMap<MarketId, Vec<Nanos>>,
    total_welfare: i64,
    problem: &'a Problem,
    pre_state: Vec<AccountSnapshot>,
    post_system_state: Vec<AccountSnapshot>,
    resolved_markets: Vec<MarketId>,
}

fn bridge_block_data(system_events: &[SystemEvent], bridge_state: &BridgeState) -> BridgeBlockData {
    let mut consumed_deposits = Vec::new();
    let mut withdrawal_leaves = Vec::new();
    for event in system_events {
        match event {
            SystemEvent::L1Deposit { deposit, .. } => consumed_deposits.push(deposit.clone()),
            SystemEvent::WithdrawalCreated { withdrawal, .. } => {
                withdrawal_leaves.push(withdrawal.clone());
            }
            SystemEvent::CreateAccount { .. }
            | SystemEvent::Deposit { .. }
            | SystemEvent::MarketResolved { .. } => {}
        }
    }
    BridgeBlockData {
        deposit_count: bridge_state.deposit_cursor,
        deposit_root: bridge_state.deposit_root,
        consumed_deposits,
        withdrawal_leaves,
    }
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
    fn from_resting(
        order: &Order,
        account_id: AccountId,
        created_at: u64,
        expires_at_block: u64,
    ) -> Self {
        let market_ids: Vec<_> = order.active_markets().collect();
        let side = classify_order_side(order);
        Self {
            order_id: order.id,
            account_id,
            market_ids,
            side,
            limit_price: order.limit_price,
            remaining_qty: order.max_fill,
            created_at_block: created_at,
            expires_at_block,
        }
    }
}

/// Classify an order's side from its payoff structure.
fn classify_order_side(order: &Order) -> &'static str {
    if order.num_markets != 1 || order.num_states != 2 {
        return if order.is_seller() { "Sell" } else { "Custom" };
    }
    // Binary market: state 0 = YES wins, state 1 = NO wins
    let p0 = order.payoffs[0]; // payoff when YES
    let p1 = order.payoffs[1]; // payoff when NO
    match (p0, p1) {
        (1, 0) => "BuyYes",
        (0, 1) => "BuyNo",
        (-1, 0) => "SellYes",
        (0, -1) => "SellNo",
        _ if order.is_seller() => "Sell",
        _ => "Custom",
    }
}

fn expected_balance_delta_from_fills(fills: &[Fill], order_map: &HashMap<u64, &Order>) -> i64 {
    fills.iter().fold(0, |net_delta, fill| {
        if fill.fill_qty == 0 {
            return net_delta;
        }

        let Some(order) = order_map.get(&fill.order_id) else {
            return net_delta;
        };

        let has_positive = order.payoffs[..order.num_states as usize]
            .iter()
            .any(|&p| p > 0);
        let has_negative = order.payoffs[..order.num_states as usize]
            .iter()
            .any(|&p| p < 0);
        let cost = (fill.fill_price as i128 * fill.fill_qty as i128) as i64;

        if has_positive && !has_negative {
            net_delta - cost
        } else if has_negative && !has_positive {
            net_delta + cost
        } else {
            net_delta
        }
    })
}

/// Build the witness state snapshots around the system-event boundary.
///
/// `pre_state` represents block-start state, so accounts touched by pending
/// system events use their captured baseline. Created accounts are omitted.
/// `post_system_state` is the live account store after system events.
fn build_witness_phase_snapshots(
    accounts: &AccountStore,
    system_account_baselines: &HashMap<AccountId, Option<Account>>,
) -> (Vec<AccountSnapshot>, Vec<AccountSnapshot>) {
    let pre_state =
        CanonicalState::from_snapshot_iter(accounts.iter().filter_map(|(account_id, account)| {
            match system_account_baselines.get(account_id) {
                Some(Some(baseline)) => Some(snapshot_account(baseline)),
                Some(None) => None,
                None => Some(snapshot_account(account)),
            }
        }))
        .into_snapshots();

    let post_system_state = CanonicalState::from_accounts(accounts).into_snapshots();
    (pre_state, post_system_state)
}

/// Convert sequencer `RejectionReason` to verifier `RejectionReason`.
fn convert_rejection_reason(r: &RejectionReason) -> sybil_verifier::RejectionReason {
    match r {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => sybil_verifier::RejectionReason::InsufficientBalance {
            required: *required,
            available: *available,
        },
        RejectionReason::InsufficientPosition {
            market,
            outcome,
            required,
            available,
        } => sybil_verifier::RejectionReason::InsufficientPosition {
            market: *market,
            outcome: *outcome,
            required: *required,
            available: *available,
        },
        RejectionReason::AccountNotFound => sybil_verifier::RejectionReason::AccountNotFound,
        RejectionReason::CompleteSetFormation => {
            sybil_verifier::RejectionReason::CompleteSetFormation
        }
        RejectionReason::Expired {
            current_block,
            expires_at_block,
        } => sybil_verifier::RejectionReason::Expired {
            current_block: *current_block,
            expires_at_block: *expires_at_block,
        },
    }
}

fn convert_system_event(event: &SystemEvent) -> SystemEventWitness {
    match event {
        SystemEvent::CreateAccount {
            account_id,
            initial_balance,
        } => SystemEventWitness::CreateAccount {
            account_id: account_id.0,
            initial_balance: *initial_balance,
        },
        SystemEvent::Deposit { account_id, amount } => SystemEventWitness::Deposit {
            account_id: account_id.0,
            amount: *amount,
        },
        SystemEvent::L1Deposit {
            account_id,
            amount,
            deposit,
        } => SystemEventWitness::L1Deposit {
            account_id: account_id.0,
            amount: *amount,
            deposit_id: deposit.deposit_id,
            deposit_root: deposit.deposit_root,
            sybil_account_key: deposit.sybil_account_key,
        },
        SystemEvent::WithdrawalCreated {
            account_id,
            amount,
            withdrawal,
        } => SystemEventWitness::WithdrawalCreated {
            account_id: account_id.0,
            amount: *amount,
            withdrawal_id: withdrawal.withdrawal_id,
            recipient: withdrawal.recipient,
            token: withdrawal.token_address,
            amount_token_units: withdrawal.amount_token_units,
            expiry_height: withdrawal.expiry_height,
            nullifier: withdrawal.nullifier,
        },
        SystemEvent::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => SystemEventWitness::MarketResolved {
            market_id: *market_id,
            payout_nanos: *payout_nanos,
            affected_accounts: affected_accounts.iter().map(|id| id.0).collect(),
        },
    }
}

/// Per-order self-trade prevention (STP) for market groups.
///
/// Tracks buy-side outcome coverage per account across a batch. When an order
/// would complete coverage of all N outcomes in a group (enabling minting
/// self-trade), that specific order is rejected. Earlier orders are kept.
///
/// Applied to ALL accounts, not just MMs — same principle as traditional
/// exchange STP (CME, Nasdaq, etc.) but adapted for batch auctions.
///
/// Coverage rules:
/// - BuyYes on market_i → covers outcome i
/// - BuyNo on market_i → covers all outcomes EXCEPT i (in the group)
/// - SellYes/SellNo → does NOT contribute (reduces exposure)
struct GroupCoverageTracker {
    /// market_id → (group_index, group_size)
    market_to_group: HashMap<MarketId, (usize, usize)>,
    /// (account_id, group_index) → set of covered outcome market_ids
    coverage: HashMap<(AccountId, usize), HashSet<MarketId>>,
    /// group_index → list of market_ids in the group
    group_markets: Vec<Vec<MarketId>>,
}

impl GroupCoverageTracker {
    fn new(market_groups: &[MarketGroup]) -> Self {
        let mut market_to_group = HashMap::new();
        let mut group_markets = Vec::with_capacity(market_groups.len());
        for (gi, group) in market_groups.iter().enumerate() {
            let markets: Vec<MarketId> = group.markets.clone();
            let n = markets.len();
            for &mid in &markets {
                market_to_group.insert(mid, (gi, n));
            }
            group_markets.push(markets);
        }
        Self {
            market_to_group,
            coverage: HashMap::new(),
            group_markets,
        }
    }

    /// Check if accepting this order would complete a group set for the account.
    /// Returns true if the order should be REJECTED (would complete self-trade).
    fn would_complete_set(&self, account_id: AccountId, order: &Order) -> bool {
        if order.num_markets != 1 || order.num_states != 2 {
            return false;
        }
        let market = order.markets[0];
        let Some(&(gi, n)) = self.market_to_group.get(&market) else {
            return false;
        };

        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);

        // Compute what this order would add to coverage
        let mut new_coverage: HashSet<MarketId> = HashSet::new();
        if yes_pay > 0 && no_pay == 0 {
            new_coverage.insert(market);
        } else if yes_pay == 0 && no_pay > 0 {
            for &gm in &self.group_markets[gi] {
                if gm != market {
                    new_coverage.insert(gm);
                }
            }
        } else {
            return false; // Sell or mixed — not a coverage concern
        }

        let key = (account_id, gi);
        let existing = self.coverage.get(&key);
        let total = match existing {
            Some(set) => set.union(&new_coverage).count(),
            None => new_coverage.len(),
        };

        total >= n
    }

    /// Record that this order was accepted — update coverage for the account.
    fn record(&mut self, account_id: AccountId, order: &Order) {
        if order.num_markets != 1 || order.num_states != 2 {
            return;
        }
        let market = order.markets[0];
        let Some(&(gi, _)) = self.market_to_group.get(&market) else {
            return;
        };

        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);
        let key = (account_id, gi);
        let set = self.coverage.entry(key).or_default();

        if yes_pay > 0 && no_pay == 0 {
            set.insert(market);
        } else if yes_pay == 0 && no_pay > 0 {
            for &gm in &self.group_markets[gi] {
                if gm != market {
                    set.insert(gm);
                }
            }
        }
    }
}

/// Block-producing sequencer. Core sync layer.
///
/// Manages accounts, assigns order IDs, validates, solves, settles, and
/// produces blocks. The actor layer calls `produce_block()` on each timer tick.
/// Simulations can use this directly without the actor.
#[derive(Clone)]
pub struct BlockSequencer {
    pub accounts: AccountStore,
    /// Pluggable solver for matching optimization.
    solver: Arc<dyn Solver>,
    next_order_id: u64,
    /// Resting orders with tracked balance/position reservations.
    order_book: OrderBook,
    /// Current block height.
    height: u64,
    /// Markets available for trading.
    markets: MarketSet,
    /// Market groups (multi-outcome event constraints).
    market_groups: Vec<MarketGroup>,
    /// Last block header for hash chaining.
    last_header: Option<BlockHeader>,
    /// Price tracking: clearing prices, history, volume.
    pub price_tracker: crate::price_tracker::PriceTracker,
    /// Fill recording: per-account fill history.
    pub fill_recorder: crate::fill_recorder::FillRecorder,
    /// Market lifecycle: statuses, oracle, metadata.
    pub lifecycle: crate::market_lifecycle::MarketLifecycle,
    /// P256 public key to account mapping.
    pubkey_registry: HashMap<crate::crypto::PublicKey, AccountId>,
    /// L1 bridge sidecar state: consumed deposits and normal withdrawal leaves.
    bridge: BridgeState,
    /// Administrative state changes that should be included in the next block.
    pending_system_events: Vec<SystemEvent>,
    /// Block-start baselines for accounts touched by pending system events.
    /// `None` means the account did not exist before the first system event.
    pending_system_account_baselines: HashMap<AccountId, Option<Account>>,
    /// Buffered submissions that couldn't be admitted into the resting book
    /// at submit time (MM-constrained, multi-order, multi-market). Drained
    /// by the clone inside `prepare_block` and consumed by the solver. The
    /// durable counterpart lives in the `PENDING_BUNDLES` redb table so a
    /// crash between admit and the next block commit doesn't drop them.
    pending_bundles: Vec<OrderSubmission>,
    /// Runtime configuration for this sequencer and its surrounding actor.
    pub config: SequencerConfig,
}

impl BlockSequencer {
    pub fn new(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        oracle: Arc<dyn Oracle>,
        solver: Arc<dyn Solver>,
        config: SequencerConfig,
    ) -> Self {
        let order_book = OrderBook::new(config.order_ttl_blocks);
        Self {
            accounts,
            solver,
            next_order_id: 1,
            order_book,
            height: 0,
            markets,
            market_groups,
            last_header: None,
            price_tracker: crate::price_tracker::PriceTracker::new(),
            fill_recorder: crate::fill_recorder::FillRecorder::new(),
            lifecycle: crate::market_lifecycle::MarketLifecycle::new(oracle),
            pubkey_registry: HashMap::new(),
            bridge: BridgeState::default(),
            pending_system_events: Vec::new(),
            pending_system_account_baselines: HashMap::new(),
            pending_bundles: Vec::new(),
            config,
        }
    }

    /// Create with the default LP solver.
    pub fn with_default_solver(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        oracle: Arc<dyn Oracle>,
        config: SequencerConfig,
    ) -> Self {
        Self::new(
            accounts,
            markets,
            market_groups,
            oracle,
            Arc::new(matching_solver::LpSolver::new()),
            config,
        )
    }

    /// Restore from persisted state.
    pub fn restore(state: RestoredState, oracle: Arc<dyn Oracle>, config: SequencerConfig) -> Self {
        let solver: Arc<dyn Solver> = Arc::new(matching_solver::LpSolver::new());
        let mut lifecycle = MarketLifecycle::new(oracle);
        for (market_id, status) in state.market_statuses {
            lifecycle.set_market_status(market_id, status);
        }
        for (market_id, meta) in state.market_metadata {
            lifecycle.set_market_metadata(market_id, meta);
        }
        for feed in state.data_feeds {
            lifecycle.restore_feed(feed);
        }
        let mut order_book = OrderBook::restore(state.resting_orders, config.order_ttl_blocks);
        // Replay the admit-log WAL on top of the snapshot: every non-MM
        // admit since the last committed block is durable on its own row
        // and must be re-inserted before the sequencer starts taking new
        // traffic, so nothing acknowledged with a 200 OK is dropped by a
        // crash.
        for resting in state.admit_log {
            order_book.reinsert_for_replay(resting);
        }
        let mut restored = Self {
            accounts: state.accounts,
            solver,
            next_order_id: state.next_order_id,
            order_book,
            height: state.height,
            markets: state.markets,
            market_groups: state.market_groups,
            last_header: state.last_header,
            price_tracker: crate::price_tracker::PriceTracker::with_state(
                state.last_clearing_prices,
                state.market_volumes,
            ),
            fill_recorder: crate::fill_recorder::FillRecorder::restore(state.account_fills),
            lifecycle,
            pubkey_registry: state.pubkey_registry,
            bridge: state.bridge_state,
            pending_system_events: Vec::new(),
            pending_system_account_baselines: HashMap::new(),
            pending_bundles: state.pending_bundles,
            config,
        };
        for deposit in state.pending_l1_deposits {
            restored
                .ingest_l1_deposit(deposit)
                .expect("pending l1 deposit replay should be valid");
        }
        for request in state.pending_bridge_withdrawals {
            restored
                .request_bridge_withdrawal(request)
                .expect("pending bridge withdrawal replay should be valid");
        }
        restored
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn order_ttl_blocks(&self) -> u64 {
        self.config.order_ttl_blocks
    }

    /// Snapshot of all state needed to persist the most recently produced block.
    ///
    /// Call this on the `next_sequencer` returned by `prepare_block` (or on a
    /// live `BlockSequencer` after `produce_block`) — it panics if no block has
    /// been produced yet, since there's no header to associate the snapshot with.
    pub fn snapshot(&self) -> SequencerSnapshot<'_> {
        let header = self
            .last_header
            .as_ref()
            .expect("snapshot called before any block was produced");
        SequencerSnapshot {
            accounts: &self.accounts,
            markets: &self.markets,
            market_groups: &self.market_groups,
            lifecycle: &self.lifecycle,
            header,
            next_order_id: self.next_order_id,
            pubkey_registry: &self.pubkey_registry,
            last_clearing_prices: self.price_tracker.last_clearing_prices(),
            market_volumes: self.price_tracker.market_volumes(),
            account_fills: self.fill_recorder.snapshot(),
            resting_orders: self.order_book.snapshot(),
            bridge_state: &self.bridge,
        }
    }

    pub fn markets(&self) -> &MarketSet {
        &self.markets
    }

    pub fn markets_mut(&mut self) -> &mut MarketSet {
        &mut self.markets
    }

    pub fn market_groups(&self) -> &[MarketGroup] {
        &self.market_groups
    }

    pub fn market_lifecycle(&self) -> &MarketLifecycle {
        &self.lifecycle
    }

    pub fn market_groups_mut(&mut self) -> &mut Vec<MarketGroup> {
        &mut self.market_groups
    }

    pub fn last_header(&self) -> Option<&BlockHeader> {
        self.last_header.as_ref()
    }

    pub fn next_order_id(&self) -> u64 {
        self.next_order_id
    }

    pub fn pubkey_registry(&self) -> &HashMap<crate::crypto::PublicKey, AccountId> {
        &self.pubkey_registry
    }

    /// Get the oracle-tracked status for a market. Returns `Active` if not explicitly set.
    pub fn market_status(&self, id: MarketId) -> MarketStatus {
        self.lifecycle.market_status(id)
    }

    pub fn market_statuses(&self) -> &HashMap<MarketId, MarketStatus> {
        self.lifecycle.market_statuses()
    }

    pub fn oracle(&self) -> Arc<dyn Oracle> {
        self.lifecycle.oracle()
    }

    pub fn set_market_metadata(&mut self, market_id: MarketId, metadata: MarketMetadata) {
        self.lifecycle.set_market_metadata(market_id, metadata);
    }

    pub fn market_metadata(&self, market_id: MarketId) -> Option<&MarketMetadata> {
        self.lifecycle.market_metadata(market_id)
    }

    pub fn market_metadata_all(&self) -> &HashMap<MarketId, MarketMetadata> {
        self.lifecycle.market_metadata_all()
    }

    pub fn last_clearing_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        self.price_tracker.last_clearing_prices()
    }

    pub fn price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Vec<PricePoint> {
        self.price_tracker.price_history(market_id, from_ms, to_ms)
    }

    pub fn market_volume(&self, market_id: MarketId) -> u64 {
        self.price_tracker.market_volume(market_id)
    }

    pub fn market_volumes(&self) -> &HashMap<MarketId, u64> {
        self.price_tracker.market_volumes()
    }

    pub fn open_orders_for_account(&self, account_id: AccountId) -> usize {
        self.order_book.orders_for_account(account_id)
    }

    pub fn pending_bundles_for_account(&self, account_id: AccountId) -> usize {
        self.pending_bundles
            .iter()
            .filter(|submission| submission.account_id == account_id)
            .count()
    }

    pub fn pending_non_mm_orders_for_account(&self, account_id: AccountId) -> usize {
        self.pending_bundles
            .iter()
            .filter(|submission| {
                submission.account_id == account_id && submission.mm_constraint.is_none()
            })
            .map(|submission| submission.orders.len())
            .sum()
    }

    pub fn account_fills(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Vec<AccountFillRecord> {
        self.fill_recorder
            .account_fills(account_id, market_id_filter, limit, offset)
    }

    // --- Public key registry ---

    pub fn register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
    ) -> Result<(), SequencerError> {
        if self.accounts.get(account_id).is_none() {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        }
        if self.pubkey_registry.contains_key(&pubkey) {
            return Err(SequencerError::AccountAlreadyRegistered);
        }
        self.pubkey_registry.insert(pubkey, account_id);
        Ok(())
    }

    pub fn lookup_pubkey(&self, pubkey: &crate::crypto::PublicKey) -> Option<AccountId> {
        self.pubkey_registry.get(pubkey).copied()
    }

    fn capture_system_account_baseline(&mut self, account_id: AccountId) {
        if self
            .pending_system_account_baselines
            .contains_key(&account_id)
        {
            return;
        }
        self.pending_system_account_baselines
            .insert(account_id, self.accounts.get(account_id).cloned());
    }

    fn capture_missing_system_account(&mut self, account_id: AccountId) {
        self.pending_system_account_baselines
            .entry(account_id)
            .or_insert(None);
    }

    pub fn create_account(&mut self, initial_balance: i64) -> AccountId {
        let account_id = self.accounts.create_account(initial_balance);
        self.capture_missing_system_account(account_id);
        self.record_system_event(SystemEvent::CreateAccount {
            account_id,
            initial_balance,
        });
        account_id
    }

    pub fn fund_account(
        &mut self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        self.capture_system_account_baseline(account_id);
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;

        account.balance += amount;
        account.total_deposited += amount;
        let updated = account.clone();
        self.record_system_event(SystemEvent::Deposit { account_id, amount });
        Ok(updated)
    }

    pub fn bridge_state(&self) -> &BridgeState {
        &self.bridge
    }

    pub fn order_book(&self) -> &OrderBook {
        &self.order_book
    }

    pub fn bridge_account_key(&self, account_id: AccountId) -> Option<[u8; 32]> {
        self.accounts
            .get(account_id)
            .map(|_| account_key(account_id))
    }

    pub fn default_bridge_withdrawal_expiry_height(&self) -> u64 {
        self.height
            .saturating_add(1)
            .saturating_add(DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS)
    }

    pub fn bridge_withdrawal(&self, withdrawal_id: u64) -> Option<&WithdrawalLeaf> {
        self.bridge.withdrawals.get(&withdrawal_id)
    }

    pub fn validate_l1_deposit(&self, deposit: &L1Deposit) -> Result<i64, SequencerError> {
        if self.accounts.get(deposit.account_id).is_none() {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: deposit.account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        }
        let expected_id = self.bridge.deposit_cursor.saturating_add(1);
        if deposit.deposit_id != expected_id {
            return Err(SequencerError::Bridge(
                BridgeError::NonSequentialDeposit {
                    expected: expected_id,
                    actual: deposit.deposit_id,
                }
                .to_string(),
            ));
        }
        if deposit.sybil_account_key != account_key(deposit.account_id) {
            return Err(SequencerError::Bridge(
                BridgeError::AccountKeyMismatch.to_string(),
            ));
        }
        amount_token_units_to_i64_nanos(deposit.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))
    }

    pub fn ingest_l1_deposit(&mut self, deposit: L1Deposit) -> Result<Account, SequencerError> {
        let amount = self.validate_l1_deposit(&deposit)?;
        let account_id = deposit.account_id;
        self.capture_system_account_baseline(account_id);
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;

        account.balance += amount;
        account.total_deposited += amount;
        let updated = account.clone();
        self.bridge.deposit_cursor = deposit.deposit_id;
        self.bridge.deposit_root = deposit.deposit_root;
        self.record_system_event(SystemEvent::L1Deposit {
            account_id,
            amount,
            deposit,
        });
        Ok(updated)
    }

    pub fn validate_bridge_withdrawal(
        &self,
        request: &BridgeWithdrawalRequest,
    ) -> Result<i64, SequencerError> {
        let account = self.accounts.get(request.account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: request.account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        let amount = amount_token_units_to_i64_nanos(request.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))?;
        let next_height = self.height.saturating_add(1);
        if request.expiry_height < next_height {
            return Err(SequencerError::Bridge(
                BridgeError::WithdrawalExpired {
                    expiry_height: request.expiry_height,
                    next_height,
                }
                .to_string(),
            ));
        }
        let available = account.balance - self.order_book.reserved_balance(request.account_id);
        if amount > available {
            return Err(SequencerError::Bridge(
                BridgeError::InsufficientAvailableBalance {
                    required: amount,
                    available,
                }
                .to_string(),
            ));
        }
        Ok(amount)
    }

    pub fn request_bridge_withdrawal(
        &mut self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        let amount_i64 = self.validate_bridge_withdrawal(&request)?;
        let amount_nanos = amount_token_units_to_nanos(request.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))?;
        let withdrawal_id = self.bridge.next_withdrawal_id;
        let nullifier = crate::bridge::withdrawal_nullifier(
            request.chain_id,
            request.vault_address,
            withdrawal_id,
            request.account_id,
            request.recipient,
            request.token_address,
            request.amount_token_units,
        );
        let withdrawal = WithdrawalLeaf {
            withdrawal_id,
            account_id: request.account_id,
            recipient: request.recipient,
            token_address: request.token_address,
            amount_token_units: request.amount_token_units,
            amount_nanos,
            expiry_height: request.expiry_height,
            nullifier,
            created_at_height: self.height.saturating_add(1),
        };

        self.capture_system_account_baseline(request.account_id);
        let account = self.accounts.get_mut(request.account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: request.account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        account.balance -= amount_i64;
        self.bridge.next_withdrawal_id = withdrawal_id.saturating_add(1);
        self.bridge
            .withdrawals
            .insert(withdrawal_id, withdrawal.clone());
        self.record_system_event(SystemEvent::WithdrawalCreated {
            account_id: request.account_id,
            amount: amount_i64,
            withdrawal: withdrawal.clone(),
        });
        Ok(withdrawal)
    }

    pub fn record_system_event(&mut self, event: SystemEvent) {
        self.pending_system_events.push(event);
    }

    /// Try to admit a submission directly into the resting order book so it
    /// is visible to clients and participates in the next block's solve,
    /// bypassing the pre-block buffer.
    ///
    /// Eligible submissions are single-order, single-market, non-MM ones:
    /// the resting book's `accept` path already performs the full validation
    /// and reservation dance that `prepare_block` would do at block time, so
    /// we can run it now and return an HTTP-level Accept/Reject synchronously.
    ///
    /// MM-constrained, multi-order, and multi-market submissions are returned
    /// as `Deferred` — the caller still has to buffer them and feed them to
    /// `prepare_block` at block time, because they rely on batch-local state
    /// (STP across the whole bundle, MM flash liquidity) that the resting
    /// book doesn't model.
    pub fn try_admit_direct(&mut self, submission: OrderSubmission) -> AdmitOutcome {
        for order in &submission.orders {
            for market_id in order.active_markets() {
                if self.markets.get(market_id).is_none() {
                    return AdmitOutcome::Rejected(SequencerError::MarketNotFound);
                }
                let status = self.market_status(market_id);
                if !status.is_tradeable() {
                    return AdmitOutcome::Rejected(SequencerError::InvalidMarketState(format!(
                        "market {} is {}",
                        market_id.0,
                        status.as_str()
                    )));
                }
            }
        }

        let eligible = submission.mm_constraint.is_none()
            && submission.orders.len() == 1
            && submission.orders[0].num_markets == 1;
        if !eligible {
            return AdmitOutcome::Deferred(submission);
        }

        let account_id = submission.account_id;
        let Some(account) = self.accounts.get(account_id) else {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        };

        let mut order = submission.orders.into_iter().next().expect("len == 1");
        let order_id = self.next_order_id;
        self.next_order_id += 1;
        order.id = order_id;
        let next_batch_height = self.height.saturating_add(1);
        let expires_at_block = order.effective_expires_at_block(self.height, self.order_book.ttl());
        if next_batch_height > expires_at_block {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason: RejectionReason::Expired {
                    current_block: next_batch_height,
                    expires_at_block,
                },
            }));
        }

        let mut stp = GroupCoverageTracker::new(&self.market_groups);
        self.seed_group_coverage_for_account(&mut stp, account_id);
        if stp.would_complete_set(account_id, &order) {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason: RejectionReason::CompleteSetFormation,
            }));
        }

        match self
            .order_book
            .accept(order, account_id, account, self.height)
        {
            Ok(accepted) => AdmitOutcome::Admitted {
                order_id: accepted.order.id,
                resting_order: accepted.resting_order,
            },
            Err(reason) => AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason,
            })),
        }
    }

    /// Seed an STP tracker with every resting/pending-bundle order belonging
    /// to `account_id`. Used at admit time so a single order can't complete a
    /// coverage set against the account's prior-block resting orders or against
    /// bundles still staged in `pending_bundles`.
    fn seed_group_coverage_for_account(
        &self,
        stp: &mut GroupCoverageTracker,
        account_id: AccountId,
    ) {
        for (order, aid) in self.order_book.resting_orders() {
            if aid == account_id {
                stp.record(aid, order);
            }
        }
        for bundle in &self.pending_bundles {
            if bundle.account_id == account_id {
                for order in &bundle.orders {
                    stp.record(account_id, order);
                }
            }
        }
    }

    /// Seed an STP tracker with every account's resting coverage. Used inside
    /// `prepare_block` before the submission loop so cross-block coverage
    /// participates in the same check the loop applies to fresh orders.
    fn seed_group_coverage_from_all_resting(&self, stp: &mut GroupCoverageTracker) {
        for (order, aid) in self.order_book.resting_orders() {
            stp.record(aid, order);
        }
    }

    /// Get pending orders, optionally filtered by account.
    pub fn pending_orders_info(
        &self,
        account_id_filter: Option<AccountId>,
    ) -> Vec<PendingOrderInfo> {
        self.order_book
            .resting_orders_full()
            .filter(|(_, aid, _, _)| account_id_filter.is_none_or(|filter| *aid == filter))
            .map(|(order, aid, created_at, expires_at_block)| {
                PendingOrderInfo::from_resting(order, aid, created_at, expires_at_block)
            })
            .collect()
    }

    /// Get pending orders for a specific market.
    pub fn market_orderbook(&self, market_id: MarketId) -> Vec<PendingOrderInfo> {
        self.order_book
            .resting_orders_full()
            .filter(|(order, _, _, _)| order.active_markets().any(|m| m == market_id))
            .map(|(order, aid, created_at, expires_at_block)| {
                PendingOrderInfo::from_resting(order, aid, created_at, expires_at_block)
            })
            .collect()
    }

    /// Cancel a resting order owned by `account_id`.
    pub fn cancel_pending_order(
        &mut self,
        account_id: AccountId,
        order_id: u64,
    ) -> Result<(), SequencerError> {
        match self.order_book.cancel(account_id, order_id) {
            Ok(()) => Ok(()),
            Err(crate::order_book::CancelError::NotFound) => Err(SequencerError::OrderNotFound),
            Err(crate::order_book::CancelError::WrongOwner) => {
                Err(SequencerError::OrderOwnershipMismatch)
            }
        }
    }

    /// Resolve a market through the oracle.
    ///
    /// `payout_nanos`: YES payout per share in nanos (0 to NANOS_PER_DOLLAR).
    ///
    /// On `SettleNow`: calls settlement, removes from market groups, updates status.
    /// On `Propose`: stores the pending proposal (future L0 path).
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        // Lifecycle decides (consults oracle, updates status)
        let action = self
            .lifecycle
            .resolve_market(market_id, payout_nanos, timestamp_ms)?;

        // Sequencer executes the side effects
        match action {
            sybil_oracle::ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            } => {
                let affected_accounts: Vec<AccountId> = self
                    .accounts
                    .iter()
                    .filter_map(|(&account_id, account)| {
                        let yes_pos = account.position(market_id, 0);
                        let no_pos = account.position(market_id, 1);
                        (yes_pos != 0 || no_pos != 0).then_some(account_id)
                    })
                    .collect();
                for account_id in &affected_accounts {
                    self.capture_system_account_baseline(*account_id);
                }
                let affected_accounts =
                    settlement::resolve_market(&mut self.accounts, market_id, payout_nanos);
                self.record_system_event(SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                });
                self.market_groups
                    .retain(|g| !g.markets.contains(&market_id));
                Ok(record)
            }
            sybil_oracle::ResolutionAction::Propose { .. } => Err(SequencerError::OracleError(
                "resolution proposed but not yet settled".to_string(),
            )),
            sybil_oracle::ResolutionAction::Reject { reason } => {
                Err(SequencerError::OracleError(reason))
            }
        }
    }

    /// Resolve a market from a signed attestation via the market's template
    /// policy. Signature verification is done by the caller (the sequencer
    /// actor) before this is called; here the lifecycle re-checks that the
    /// signer is the template's expected feed and then settles.
    pub fn resolve_market_attested(
        &mut self,
        market_id: MarketId,
        signed: &sybil_oracle::SignedAttestation,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        let action = self
            .lifecycle
            .resolve_from_attestation(market_id, signed, timestamp_ms)?;

        match action {
            sybil_oracle::ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            } => {
                let affected_accounts: Vec<AccountId> = self
                    .accounts
                    .iter()
                    .filter_map(|(&account_id, account)| {
                        let yes_pos = account.position(market_id, 0);
                        let no_pos = account.position(market_id, 1);
                        (yes_pos != 0 || no_pos != 0).then_some(account_id)
                    })
                    .collect();
                for account_id in &affected_accounts {
                    self.capture_system_account_baseline(*account_id);
                }
                let affected_accounts =
                    settlement::resolve_market(&mut self.accounts, market_id, payout_nanos);
                self.record_system_event(SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                });
                self.market_groups
                    .retain(|g| !g.markets.contains(&market_id));
                Ok(record)
            }
            sybil_oracle::ResolutionAction::Propose { .. } => Err(SequencerError::OracleError(
                "resolution proposed but not yet settled".to_string(),
            )),
            sybil_oracle::ResolutionAction::Reject { reason } => {
                Err(SequencerError::OracleError(reason))
            }
        }
    }

    /// Register a data feed (e.g. admin key, Polymarket mirror signer). Returns
    /// the assigned [`sybil_oracle::FeedId`]. Idempotent on pubkey.
    pub fn register_feed(
        &mut self,
        pubkey: sybil_oracle::FeedPubkey,
        name: String,
        now_ms: u64,
    ) -> sybil_oracle::FeedId {
        self.lifecycle.register_feed(pubkey, name, now_ms)
    }

    pub fn feed_by_id(&self, id: sybil_oracle::FeedId) -> Option<&sybil_oracle::DataFeed> {
        self.lifecycle.feed_by_id(id)
    }

    pub fn feed_by_pubkey(
        &self,
        pubkey: &sybil_oracle::FeedPubkey,
    ) -> Option<&sybil_oracle::DataFeed> {
        self.lifecycle.feed_by_pubkey(pubkey)
    }

    pub fn install_template(&mut self, template: sybil_oracle::ResolutionTemplate) {
        self.lifecycle.install_template(template);
    }

    /// Whether a template with this id has been installed. Used by the API
    /// layer to reject market-creation requests that reference a missing
    /// template, instead of deferring the error until resolve time.
    pub fn template_exists(&self, id: &str) -> bool {
        self.lifecycle.templates().get_str(id).is_some()
    }

    /// Prepare one block from the given submissions without mutating live sequencer state.
    ///
    /// Any submissions buffered on `self.pending_bundles` (from the admit
    /// path for MM / multi-market orders) are drained into the same solver
    /// input ahead of the caller-supplied batch. The drain happens on the
    /// clone, so if the prepared block is discarded the live sequencer
    /// still holds the bundles and the next tick retries them.
    ///
    /// The returned [`PreparedBlock`] can either be committed atomically or discarded.
    #[tracing::instrument(skip_all, fields(height))]
    pub fn prepare_block(
        &self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> PreparedBlock {
        let mut next_sequencer = self.clone();
        let mut all_submissions = std::mem::take(&mut next_sequencer.pending_bundles);
        all_submissions.extend(submissions);
        let production = next_sequencer.produce_block_in_place(all_submissions, timestamp_ms);
        PreparedBlock {
            next_sequencer,
            production,
        }
    }

    /// Buffer a submission that couldn't be admitted directly into the
    /// resting book. The bundle is added to `self.pending_bundles` and will
    /// be handed to the solver on the next `prepare_block` call.
    ///
    /// Persistence of this bundle is the caller's responsibility (usually
    /// the actor, via `Store::append_pending_bundle`) so durability decisions
    /// stay out of the sync core.
    pub fn push_pending_bundle(&mut self, submission: OrderSubmission) {
        self.pending_bundles.push(submission);
    }

    pub fn pending_bundles_len(&self) -> usize {
        self.pending_bundles.len()
    }

    #[tracing::instrument(
        skip_all,
        fields(height = prepared.production().block.header.height)
    )]
    pub fn commit_prepared_block(&mut self, prepared: PreparedBlock) -> BlockProduction {
        let PreparedBlock {
            next_sequencer,
            production,
        } = prepared;
        *self = next_sequencer;
        production
    }

    /// Core sync method: prepare + immediately commit a block in-memory.
    ///
    /// Direct callers such as simulations keep the previous semantics. The actor
    /// can instead call [`Self::prepare_block`] and commit only after persistence succeeds.
    #[tracing::instrument(skip_all, fields(height))]
    pub fn produce_block(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> BlockProduction {
        let prepared = self.prepare_block(submissions, timestamp_ms);
        self.commit_prepared_block(prepared)
    }

    #[tracing::instrument(
        skip_all,
        fields(
            height = self.height,
            orders = problem.orders.len(),
            active_markets = active_markets.len()
        )
    )]
    fn solve_batch_phase(
        &mut self,
        problem: &Problem,
        order_account_map: &HashMap<u64, AccountId>,
        active_markets: &HashSet<MarketId>,
    ) -> SolvedBatch {
        let pipeline_result = self.solver.solve(problem);

        let markets_with_fills: HashSet<MarketId> = {
            let order_map: HashMap<u64, &Order> =
                problem.orders.iter().map(|o| (o.id, o)).collect();
            pipeline_result
                .result
                .fills
                .iter()
                .filter(|f| f.fill_qty > 0)
                .filter_map(|f| order_map.get(&f.order_id))
                .flat_map(|o| o.active_markets())
                .collect()
        };

        let position_markets = CanonicalState::from_accounts(&self.accounts)
            .market_position_totals()
            .markets();
        let clearing_prices = self.price_tracker.merge_prices(
            &pipeline_result.price_discovery,
            &markets_with_fills,
            active_markets,
            &position_markets,
        );

        let mut fills = pipeline_result.result.fills.clone();
        for fill in &mut fills {
            if let Some(&aid) = order_account_map.get(&fill.order_id) {
                fill.account_id = aid.0;
            }
        }

        let total_welfare = pipeline_result.result.total_welfare;
        let total_volume = fills
            .iter()
            .map(|f| f.fill_price.saturating_mul(f.fill_qty))
            .fold(0u64, |acc, v| acc.saturating_add(v));
        let orders_filled = pipeline_result.result.orders_filled;

        SolvedBatch {
            pipeline_result,
            fills,
            clearing_prices,
            total_welfare,
            total_volume,
            orders_filled,
        }
    }

    #[tracing::instrument(
        skip_all,
        fields(height = self.height, fills = fills.len())
    )]
    fn finalize_block_state_phase(
        &mut self,
        fills: &[Fill],
        problem: &Problem,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        timestamp_ms: u64,
    ) -> FinalizedBlockState {
        let pre_total_balance: i64 = self.accounts.iter().map(|(_, a)| a.balance).sum();

        settlement::settle_batch(&mut self.accounts, fills, &problem.orders, self.height);

        let market_totals = CanonicalState::from_accounts(&self.accounts)
            .market_position_totals()
            .minting_inputs();
        let mint_adjustments = matching_engine::derive_minting(&market_totals, clearing_prices);
        if !mint_adjustments.is_empty() {
            let mint = self
                .accounts
                .get_mut(crate::account::AccountId::MINT)
                .expect("mint account must exist");
            settlement::apply_minting(mint, &mint_adjustments, self.height);
        }

        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        self.price_tracker.record_block(
            fills,
            &order_map,
            clearing_prices,
            self.height,
            timestamp_ms,
        );
        self.fill_recorder
            .record_fills(fills, &order_map, self.height, timestamp_ms);

        let post_total_balance: i64 = self.accounts.iter().map(|(_, a)| a.balance).sum();
        let balance_delta = post_total_balance - pre_total_balance;
        if balance_delta != 0 {
            let expected_balance_delta = expected_balance_delta_from_fills(fills, &order_map);
            if balance_delta != expected_balance_delta {
                error!(
                    height = self.height,
                    balance_delta,
                    expected_balance_delta,
                    diff = balance_delta - expected_balance_delta,
                    "post-settlement balance delta mismatch"
                );
            }
        }

        let post_state = CanonicalState::from_accounts(&self.accounts);
        let post_position_totals = post_state.market_position_totals();
        for market in self.markets.iter() {
            let (total_yes, total_no) = post_position_totals.totals_for(market.id);
            if total_yes != total_no {
                error!(
                    height = self.height,
                    market = ?market.id,
                    total_yes,
                    total_no,
                    diff = total_yes - total_no,
                    "post-settlement position imbalance"
                );
            }
        }

        FinalizedBlockState { post_state }
    }

    fn assemble_witness_artifacts(&self, input: WitnessAssemblyInput<'_>) -> WitnessArtifacts {
        let WitnessAssemblyInput {
            post_state,
            order_count,
            timestamp_ms,
            previous_header,
            witness_orders,
            witness_rejections,
            system_events,
            fills,
            clearing_prices,
            total_welfare,
            problem,
            pre_state,
            post_system_state,
            resolved_markets,
        } = input;

        let state_sidecar = state_sidecar_snapshot(
            &self.bridge,
            &self.order_book,
            &self.markets,
            &self.market_groups,
            &self.lifecycle,
        );
        let header = BlockHeader {
            height: self.height,
            parent_hash: self
                .last_header
                .as_ref()
                .map(hash_header)
                .unwrap_or([0u8; 32]),
            state_root: sybil_verifier::block::compute_state_root_with_sidecar(
                post_state.as_snapshots(),
                &state_sidecar,
            ),
            order_count,
            fill_count: fills.len() as u32,
            timestamp_ms,
        };

        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: header.height,
                parent_hash: header.parent_hash,
                state_root: header.state_root,
                order_count: header.order_count,
                fill_count: header.fill_count,
                timestamp_ms: header.timestamp_ms,
            },
            previous_header,
            orders: witness_orders,
            rejections: witness_rejections,
            system_events: system_events.iter().map(convert_system_event).collect(),
            fills: fills.to_vec(),
            clearing_prices: clearing_prices.clone(),
            total_welfare,
            minting_cost: 0,
            mm_constraints: problem.mm_constraints.clone(),
            market_groups: problem.market_groups.clone(),
            pre_state,
            post_system_state,
            post_state: post_state.into_snapshots(),
            state_sidecar,
            resolved_markets,
        };

        WitnessArtifacts { header, witness }
    }

    fn produce_block_in_place(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> BlockProduction {
        self.height += 1;
        tracing::Span::current().record("height", self.height);
        let system_events = std::mem::take(&mut self.pending_system_events);
        let system_account_baselines = std::mem::take(&mut self.pending_system_account_baselines);

        for event in &system_events {
            match event {
                SystemEvent::CreateAccount {
                    account_id,
                    initial_balance,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_create_account_event(
                            *initial_balance,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::Deposit { account_id, amount } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_deposit_event(*amount, self.height);
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::L1Deposit {
                    account_id,
                    amount,
                    deposit,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_l1_deposit_event(
                            deposit.deposit_id,
                            *amount,
                            &deposit.deposit_root,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::WithdrawalCreated {
                    account_id,
                    amount,
                    withdrawal,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_withdrawal_created_event(
                            withdrawal.withdrawal_id,
                            *amount,
                            &withdrawal.nullifier,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                } => {
                    let encoded = crate::digest::encode_resolution_event(
                        *market_id,
                        *payout_nanos,
                        self.height,
                    );
                    for account_id in affected_accounts {
                        if let Some(account) = self.accounts.get_mut(*account_id) {
                            account.events_digest =
                                crate::digest::update_digest(&account.events_digest, &encoded);
                        }
                    }
                }
            }
        }
        let bridge = bridge_block_data(&system_events, &self.bridge);

        let fresh_submissions = submissions.len();
        let fresh_orders_received: usize = submissions
            .iter()
            .map(|submission| submission.orders.len())
            .sum();

        let mut all_orders: Vec<Order> = Vec::new();
        let mut all_mm_constraints: Vec<MmConstraint> = Vec::new();
        let mut rejections: Vec<Rejection> = Vec::new();

        // Track witness data alongside normal processing
        let mut witness_orders: Vec<WitnessOrder> = Vec::new();
        let mut witness_rejections: Vec<WitnessRejection> = Vec::new();
        let mut mm_order_ids_set: HashSet<u64> = HashSet::new();

        // Collect tradeable market IDs (active markets that aren't in a non-tradeable state)
        let active_markets: HashSet<MarketId> = self
            .markets
            .iter()
            .filter(|m| self.market_status(m.id).is_tradeable())
            .map(|m| m.id)
            .collect();

        // Collect resolved market IDs for witness
        let resolved_markets: Vec<MarketId> = self
            .lifecycle
            .market_statuses()
            .iter()
            .filter(|(_, status)| matches!(status, MarketStatus::Resolved { .. }))
            .map(|(&id, _)| id)
            .collect();

        // ── Order Book: expire stale, remove orders for resolved markets ──
        self.order_book.expire(self.height);
        self.order_book.revalidate(&self.accounts, &active_markets);

        // Build batch-local account map from resting orders
        let mut order_account_map: HashMap<u64, AccountId> = HashMap::new();
        for (order, account_id) in self.order_book.resting_orders() {
            order_account_map.insert(order.id, account_id);
            witness_orders.push(WitnessOrder {
                order: order.clone(),
                account_id: account_id.0,
                is_mm: false,
            });
            all_orders.push(order.clone());
        }
        let carried_resting_orders = all_orders.len();

        // ── Process new submissions ──
        // Seed STP from existing resting orders across all accounts so
        // cross-block coverage participates in the same complete-set check
        // the loop applies to fresh orders.
        let mut stp = GroupCoverageTracker::new(&self.market_groups);
        self.seed_group_coverage_from_all_resting(&mut stp);

        for mut sub in submissions {
            let account_id = sub.account_id;

            let Some(account) = self.accounts.get(account_id) else {
                for order in &sub.orders {
                    witness_rejections.push(WitnessRejection {
                        order: order.clone(),
                        account_id: account_id.0,
                        reason: sybil_verifier::RejectionReason::AccountNotFound,
                    });
                    rejections.push(Rejection {
                        order_id: order.id,
                        account_id,
                        reason: RejectionReason::AccountNotFound,
                    });
                }
                continue;
            };

            let is_mm = sub.mm_constraint.is_some();

            // Cap MM budget to account balance — prevents cumulative overdraft.
            if let Some(ref mut mm_c) = sub.mm_constraint {
                if account.balance <= 0 {
                    continue;
                }
                mm_c.max_capital = mm_c.max_capital.min(account.balance as u64);
            }

            let mut accepted_orders: Vec<Order> = Vec::new();
            let mut submission_idx_to_order_id: HashMap<usize, u64> = HashMap::new();

            for (sub_idx, mut order) in sub.orders.into_iter().enumerate() {
                let order_markets_active =
                    order.active_markets().all(|m| active_markets.contains(&m));
                if !order_markets_active {
                    continue;
                }

                let order_id = self.next_order_id;
                self.next_order_id += 1;
                order.id = order_id;
                let expires_at_block =
                    order.effective_expires_at_block(self.height, self.order_book.ttl());
                if self.height > expires_at_block {
                    witness_rejections.push(WitnessRejection {
                        order: order.clone(),
                        account_id: account_id.0,
                        reason: sybil_verifier::RejectionReason::Expired {
                            current_block: self.height,
                            expires_at_block,
                        },
                    });
                    rejections.push(Rejection {
                        order_id,
                        account_id,
                        reason: RejectionReason::Expired {
                            current_block: self.height,
                            expires_at_block,
                        },
                    });
                    continue;
                }

                if is_mm {
                    // MM orders: STP check, flash liquidity (skip balance validation)
                    if stp.would_complete_set(account_id, &order) {
                        witness_rejections.push(WitnessRejection {
                            order: order.clone(),
                            account_id: account_id.0,
                            reason: sybil_verifier::RejectionReason::CompleteSetFormation,
                        });
                        rejections.push(Rejection {
                            order_id,
                            account_id,
                            reason: RejectionReason::CompleteSetFormation,
                        });
                        continue;
                    }
                    stp.record(account_id, &order);
                    submission_idx_to_order_id.insert(sub_idx, order_id);
                    order_account_map.insert(order_id, account_id);
                    mm_order_ids_set.insert(order_id);
                    witness_orders.push(WitnessOrder {
                        order: order.clone(),
                        account_id: account_id.0,
                        is_mm: true,
                    });
                    accepted_orders.push(order);
                } else {
                    // Non-MM orders: validate + reserve via OrderBook
                    match self
                        .order_book
                        .accept(order.clone(), account_id, account, self.height)
                    {
                        Ok(accepted) => {
                            if stp.would_complete_set(account_id, &accepted.order) {
                                // Undo the book acceptance — release reservations
                                // (settle with a "fully filled" phantom to release)
                                let phantom_fill =
                                    Fill::new(accepted.order.id, accepted.order.max_fill, 0);
                                self.order_book.settle(
                                    &[phantom_fill],
                                    &HashSet::new(),
                                    self.height,
                                );
                                witness_rejections.push(WitnessRejection {
                                    order: accepted.order.clone(),
                                    account_id: account_id.0,
                                    reason: sybil_verifier::RejectionReason::CompleteSetFormation,
                                });
                                rejections.push(Rejection {
                                    order_id: accepted.order.id,
                                    account_id,
                                    reason: RejectionReason::CompleteSetFormation,
                                });
                                continue;
                            }
                            stp.record(account_id, &accepted.order);
                            order_account_map.insert(accepted.order.id, account_id);
                            witness_orders.push(WitnessOrder {
                                order: accepted.order.clone(),
                                account_id: account_id.0,
                                is_mm: false,
                            });
                            accepted_orders.push(accepted.order);
                        }
                        Err(reason) => {
                            witness_rejections.push(WitnessRejection {
                                order: order.clone(),
                                account_id: account_id.0,
                                reason: convert_rejection_reason(&reason),
                            });
                            rejections.push(Rejection {
                                order_id,
                                account_id,
                                reason,
                            });
                        }
                    }
                }
            }

            // Rebuild MmConstraint with assigned IDs
            if let Some(mm_constraint) = sub.mm_constraint {
                let old_order_ids = &mm_constraint.order_ids;
                let old_sides = &mm_constraint.order_sides;

                let mut new_constraint =
                    MmConstraint::new(mm_constraint.mm_id, mm_constraint.max_capital);

                for (sub_idx, old_id) in old_order_ids.iter().enumerate() {
                    if let (Some(&new_id), Some(&side)) = (
                        submission_idx_to_order_id.get(&sub_idx),
                        old_sides.get(old_id),
                    ) {
                        new_constraint.add_order(new_id, side);
                    }
                }

                if new_constraint.num_orders() > 0 {
                    all_mm_constraints.push(new_constraint);
                }
            }

            all_orders.extend(accepted_orders);
        }

        let fresh_orders_accepted = all_orders.len().saturating_sub(carried_resting_orders);
        let rejected_orders = rejections.len();
        let order_ids: Vec<u64> = all_orders.iter().map(|o| o.id).collect();
        let orders_submitted = all_orders.len() + rejections.len();

        // Debug: log order and rejection counts per block
        if !all_orders.is_empty() || !rejections.is_empty() {
            let mut buy_yes = 0u32;
            let mut sell_yes = 0u32;
            let mut buy_no = 0u32;
            let mut sell_no = 0u32;
            for o in &all_orders {
                if o.num_markets == 1 && o.num_states == 2 {
                    let is_buy = o.payoffs[0] > 0 || o.payoffs[1] > 0;
                    let is_yes = o.payoffs[0] != 0;
                    match (is_buy, is_yes) {
                        (true, true) => buy_yes += 1,
                        (true, false) => buy_no += 1,
                        (false, true) => sell_yes += 1,
                        (false, false) => sell_no += 1,
                    }
                }
            }
            debug!(
                accepted = all_orders.len(),
                rejected = rejections.len(),
                buy_yes,
                sell_yes,
                buy_no,
                sell_no,
                "block order summary"
            );
            for rej in &rejections {
                debug!(
                    order_id = rej.order_id,
                    account = rej.account_id.0,
                    reason = ?rej.reason,
                    "order rejected"
                );
            }
        }

        // Build Problem
        let mut problem = Problem::new("block");
        problem.markets = self.markets.clone();
        problem.orders = all_orders;
        problem.mm_constraints = all_mm_constraints;
        problem.market_groups = self.market_groups.clone();

        // Phase 1: solve the batch and derive fill/cross-market pricing outputs.
        let SolvedBatch {
            pipeline_result,
            fills,
            clearing_prices,
            total_welfare,
            total_volume,
            orders_filled,
        } = self.solve_batch_phase(&problem, &order_account_map, &active_markets);

        let (pre_state, post_system_state) =
            build_witness_phase_snapshots(&self.accounts, &system_account_baselines);

        // Phase 2: apply fills, derive minting, and validate the finalized account state.
        let FinalizedBlockState { post_state } =
            self.finalize_block_state_phase(&fills, &problem, &clearing_prices, timestamp_ms);

        // Update order book: release filled orders' reservations, adjust partial fills
        self.order_book
            .settle(&fills, &mm_order_ids_set, self.height);
        let pending_orders_after = self.order_book.len();

        let previous_header = self.last_header.as_ref().map(|h| WitnessBlockHeader {
            height: h.height,
            parent_hash: h.parent_hash,
            state_root: h.state_root,
            order_count: h.order_count,
            fill_count: h.fill_count,
            timestamp_ms: h.timestamp_ms,
        });

        let WitnessArtifacts { header, witness } =
            self.assemble_witness_artifacts(WitnessAssemblyInput {
                post_state,
                order_count: orders_submitted as u32,
                timestamp_ms,
                previous_header,
                witness_orders,
                witness_rejections,
                system_events: &system_events,
                fills: &fills,
                clearing_prices: &clearing_prices,
                total_welfare,
                problem: &problem,
                pre_state,
                post_system_state,
                resolved_markets,
            });

        self.last_header = Some(header.clone());

        debug!(
            orders_submitted,
            accepted = order_ids.len(),
            rejected = rejections.len(),
            fills = fills.len(),
            orders_filled,
            total_welfare,
            total_volume,
            "block produced"
        );

        let block = Block {
            header,
            order_ids,
            system_events,
            bridge,
            fills,
            clearing_prices,
            rejections,
            total_welfare,
            total_volume,
            orders_filled,
        };

        // Verify the block using all 4 verification layers.
        // TODO: This runs inline for now. Eventually a separate prover node
        // will consume the BlockWitness and generate ZK proofs asynchronously.
        let verification = sybil_verifier::verify_full(&witness, /* diagnostics */ false);
        if !verification.valid {
            error!(
                violations = verification.violations.len(),
                "block #{} FAILED verification", self.height
            );
            for v in &verification.violations {
                error!(kind = ?v.kind, details = %v.details, "verification violation");
            }
        }

        BlockProduction {
            block,
            pipeline: pipeline_result,
            witness,
            flow_metrics: BlockFlowMetrics {
                fresh_submissions,
                fresh_orders_received,
                carried_resting_orders,
                fresh_orders_accepted,
                rejected_orders,
                pending_orders_after,
            },
        }
    }
}

/// Convert a Block + PipelineResult into a BatchResult for simulation compatibility.
pub fn batch_result_from_block(block: &Block, pipeline_result: PipelineResult) -> BatchResult {
    BatchResult {
        pipeline_result,
        fills: block.fills.clone(),
        clearing_prices: block.clearing_prices.clone(),
        total_welfare: block.total_welfare,
        total_volume: block.total_volume,
        rejections: block.rejections.clone(),
        orders_submitted: block.header.order_count as usize,
        orders_filled: block.orders_filled,
    }
}

/// Backwards-compatible alias.
pub type BatchSequencer = BlockSequencer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::error::RejectionReason;
    use crate::validation::{validate_order, validate_order_with_reservation};
    use matching_engine::{outcome_buy, outcome_sell, MarketId, MarketSet, MmId, NANOS_PER_DOLLAR};
    use proptest::prelude::*;
    use sybil_oracle::AdminOracle;

    fn setup() -> (MarketSet, MarketId) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        (markets, m0)
    }

    fn make_sequencer(balance: i64) -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(balance);
        let markets = MarketSet::new();
        let oracle = Arc::new(AdminOracle::new());
        (
            BlockSequencer::with_default_solver(
                accounts,
                markets,
                vec![],
                oracle,
                SequencerConfig::default(),
            ),
            aid,
        )
    }

    fn eth_address(byte: u8) -> [u8; 20] {
        [byte; 20]
    }

    fn l1_deposit(account_id: AccountId, deposit_id: u64, amount_token_units: u64) -> L1Deposit {
        L1Deposit {
            deposit_id,
            account_id,
            chain_id: 1,
            vault_address: eth_address(0x10),
            token_address: eth_address(0x20),
            sender: eth_address(0x30),
            sybil_account_key: account_key(account_id),
            amount_token_units,
            deposit_root: [deposit_id as u8; 32],
        }
    }

    #[test]
    fn bridge_deposit_and_withdrawal_emit_block_sidecar() {
        let (mut seq, aid) = make_sequencer(0);

        let account = seq.ingest_l1_deposit(l1_deposit(aid, 1, 10_000)).unwrap();
        assert_eq!(account.balance, 10_000_000);

        let withdrawal = seq
            .request_bridge_withdrawal(BridgeWithdrawalRequest {
                account_id: aid,
                chain_id: 1,
                vault_address: eth_address(0x10),
                recipient: eth_address(0x40),
                token_address: eth_address(0x20),
                amount_token_units: 4_000,
                expiry_height: 10,
            })
            .unwrap();
        assert_eq!(withdrawal.amount_nanos, 4_000_000);

        let block = seq.produce_block(vec![], 1_000).block;
        assert_eq!(block.bridge.deposit_count, 1);
        assert_eq!(block.bridge.deposit_root, [1u8; 32]);
        assert_eq!(block.bridge.consumed_deposits.len(), 1);
        assert_eq!(block.bridge.withdrawal_leaves, vec![withdrawal]);
        assert_eq!(seq.accounts.get(aid).unwrap().balance, 6_000_000);
    }

    #[test]
    fn bridge_deposit_requires_next_l1_cursor() {
        let (mut seq, aid) = make_sequencer(0);
        match seq.ingest_l1_deposit(l1_deposit(aid, 2, 10_000)) {
            Err(SequencerError::Bridge(_)) => {}
            other => panic!(
                "expected bridge error, got {:?}",
                other.map(|account| account.id)
            ),
        }
    }

    #[test]
    fn test_market_position_totals_sums_all_accounts() {
        let (mut markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid0 = accounts.create_account(0);
        let aid1 = accounts.create_account(0);

        accounts.get_mut(aid0).unwrap().positions.insert((m0, 0), 7);
        accounts.get_mut(aid0).unwrap().positions.insert((m0, 1), 2);
        accounts
            .get_mut(aid1)
            .unwrap()
            .positions
            .insert((m0, 0), -3);
        accounts.get_mut(aid1).unwrap().positions.insert((m0, 1), 5);

        let totals = CanonicalState::from_accounts(&accounts)
            .market_position_totals()
            .totals_for(m0);
        assert_eq!(totals, (4, 7));

        let m1 = markets.add_binary("Unused");
        let unused_totals = CanonicalState::from_accounts(&accounts)
            .market_position_totals()
            .totals_for(m1);
        assert_eq!(unused_totals, (0, 0));
    }

    #[test]
    fn test_expected_balance_delta_from_fills_respects_order_side() {
        let (markets, m0) = setup();
        let buy = outcome_buy(&markets, 1, m0, 0, 300_000_000, 4);
        let sell = outcome_sell(&markets, 2, m0, 0, 700_000_000, 2);
        let order_map = HashMap::from([(buy.id, &buy), (sell.id, &sell)]);

        let fills = vec![
            Fill::new(buy.id, 4, 300_000_000),
            Fill::new(sell.id, 2, 700_000_000),
        ];

        let expected_delta = expected_balance_delta_from_fills(&fills, &order_map);
        assert_eq!(expected_delta, -(300_000_000i64 * 4) + (700_000_000i64 * 2));
    }

    #[test]
    fn test_minting_market_totals_include_markets_only_present_in_positions() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(0);
        let orphaned_market = MarketId::new(777);

        accounts
            .get_mut(aid)
            .expect("account should exist")
            .positions
            .insert((orphaned_market, 1), 9);

        let totals = CanonicalState::from_accounts(&accounts)
            .market_position_totals()
            .minting_inputs();

        assert_eq!(totals, vec![(orphaned_market, 0, 9)]);
    }

    #[test]
    fn test_block_minting_uses_position_markets_outside_catalog() {
        let mut markets = MarketSet::new();
        let active_market = markets.add_binary("Active");
        let orphaned_market = MarketId::new(active_market.0 + 1);

        let mut accounts = AccountStore::new();
        let holder = accounts.create_account(0);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            oracle,
            SequencerConfig::default(),
        );
        seq.price_tracker = crate::price_tracker::PriceTracker::with_state(
            HashMap::from([(orphaned_market, vec![400_000_000, 600_000_000])]),
            HashMap::new(),
        );

        seq.accounts
            .get_mut(holder)
            .expect("holder should exist")
            .positions
            .insert((orphaned_market, 1), 7);

        let bp = seq.produce_block(vec![], 1_000);

        let mint = seq
            .accounts
            .get(crate::account::AccountId::MINT)
            .expect("mint should exist");
        assert_eq!(mint.position(orphaned_market, 1), -7);
        assert_eq!(
            bp.block.clearing_prices.get(&orphaned_market),
            Some(&vec![400_000_000, 600_000_000])
        );

        let verification = sybil_verifier::verify_full(&bp.witness, false);
        assert!(
            verification.valid,
            "Violations: {:?}",
            verification.violations
        );
    }

    /// Helper: run a batch through the block sequencer, returning BatchResult.
    fn run_batch(
        seq: &mut BlockSequencer,
        submissions: Vec<OrderSubmission>,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
    ) -> BatchResult {
        // Temporarily swap markets/groups for this batch
        let old_markets = std::mem::replace(&mut seq.markets, markets.clone());
        let old_groups = std::mem::replace(&mut seq.market_groups, market_groups.to_vec());
        let bp = seq.produce_block(submissions, 0);
        seq.markets = old_markets;
        seq.market_groups = old_groups;
        batch_result_from_block(&bp.block, bp.pipeline)
    }

    fn snapshot_by_id(
        snapshots: &[AccountSnapshot],
        account_id: AccountId,
    ) -> Option<&AccountSnapshot> {
        snapshots
            .iter()
            .find(|snapshot| snapshot.id == account_id.0)
    }

    // --- Validation tests ---

    #[test]
    fn test_validate_buy_sufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        assert!(validate_order(&order, account, &HashMap::new()).is_ok());
    }

    #[test]
    fn test_validate_buy_insufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(3 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let result = validate_order(&order, account, &HashMap::new());
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientBalance {
                required,
                available,
            } => {
                assert_eq!(required, 5_000_000_000);
                assert_eq!(available, 3_000_000_000);
            }
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_sell_sufficient_position() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10);

        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 5);
        assert!(validate_order(&order, account, &HashMap::new()).is_ok());
    }

    #[test]
    fn test_validate_sell_insufficient_position() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 3);

        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 5);
        let result = validate_order(&order, account, &HashMap::new());
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientPosition {
                market,
                outcome,
                required,
                available,
            } => {
                assert_eq!(market, m0);
                assert_eq!(outcome, 0);
                assert_eq!(required, 5);
                assert_eq!(available, 3);
            }
            other => panic!("Expected InsufficientPosition, got {:?}", other),
        }
    }

    // --- Balance reservation tests ---

    #[test]
    fn test_balance_reservation_returns_cost() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 600_000_000, 5);
        let cost = validate_order_with_reservation(&order, account, 0, &HashMap::new()).unwrap();
        assert_eq!(cost, 600_000_000i64 * 5);
    }

    #[test]
    fn test_balance_reservation_blocks_double_spend() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(8 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let cost1 = validate_order_with_reservation(&order1, account, 0, &HashMap::new()).unwrap();
        assert_eq!(cost1, 5_000_000_000);

        let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, 10);
        let result = validate_order_with_reservation(&order2, account, cost1, &HashMap::new());
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientBalance {
                required,
                available,
            } => {
                assert_eq!(required, 5_000_000_000);
                assert_eq!(available, 3_000_000_000);
            }
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_balance_reservation_in_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(8 * NANOS_PER_DOLLAR as i64);

        let order1 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order1, order2],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);

        assert_eq!(result.rejections.len(), 1);
        match &result.rejections[0].reason {
            RejectionReason::InsufficientBalance { .. } => {}
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_sell_order_does_not_reserve_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(5 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 100);

        let sell = outcome_sell(&markets, 1, m0, 0, 500_000_000, 10);
        let cost = validate_order_with_reservation(&sell, account, 0, &HashMap::new()).unwrap();
        assert_eq!(cost, 0);
    }

    // --- Account not found ---

    #[test]
    fn test_account_not_found_rejection() {
        let (markets, m0) = setup();
        let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

        let bogus_id = AccountId(999);
        let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 1);
        let sub = OrderSubmission {
            account_id: bogus_id,
            orders: vec![order],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 1);
        assert_eq!(result.rejections[0].account_id, bogus_id);
        match &result.rejections[0].reason {
            RejectionReason::AccountNotFound => {}
            other => panic!("Expected AccountNotFound, got {:?}", other),
        }
    }

    // --- MM validation skip ---

    #[test]
    fn test_mm_orders_skip_validation() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(0);

        let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 100);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 0);
    }

    // --- Order ID assignment ---

    #[test]
    fn test_order_ids_are_unique() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub1], &markets, &[]);

        let sub2 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub2], &markets, &[]);

        assert_eq!(seq.next_order_id, 5);
    }

    // --- Order persistence tests ---

    #[test]
    fn test_unfilled_orders_persist() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 0);

        assert_eq!(seq.order_book.len(), 1);
        let (_, resting_aid, resting_created, _) =
            seq.order_book.resting_orders_full().next().unwrap();
        assert_eq!(resting_aid, aid);
        assert_eq!(resting_created, 1);
    }

    #[test]
    fn test_pending_orders_included_in_next_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub1], &markets, &[]);
        assert_eq!(seq.order_book.len(), 1);

        let result = run_batch(&mut seq, vec![], &markets, &[]);
        assert!(result.orders_submitted >= 1);
    }

    #[test]
    fn test_resting_orders_survive_restart_and_match() {
        let (markets, m0) = setup();

        let mut accounts = AccountStore::new();
        let aid_a = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let aid_b = accounts.create_account(0);
        accounts
            .get_mut(aid_b)
            .unwrap()
            .positions
            .insert((m0, 0), 10);

        let oracle: Arc<dyn Oracle> = Arc::new(AdminOracle::new());
        let mut seq_a = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle.clone(),
            SequencerConfig::default(),
        );

        let sub = OrderSubmission {
            account_id: aid_a,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 700_000_000, 5)],
            mm_constraint: None,
        };
        seq_a.produce_block(vec![sub], 1_000);
        assert_eq!(
            seq_a.order_book.len(),
            1,
            "expected unfilled buy to rest in book"
        );
        let reserved_before = seq_a.order_book.reserved_balance(aid_a);
        assert!(reserved_before > 0);

        // Build a RestoredState as the store would, then restore into seq_b.
        let state = RestoredState {
            accounts: seq_a.accounts.clone(),
            markets: markets.clone(),
            market_groups: vec![],
            market_statuses: HashMap::new(),
            market_metadata: HashMap::new(),
            height: seq_a.height(),
            last_header: seq_a.last_header().cloned(),
            next_order_id: seq_a.next_order_id(),
            pubkey_registry: seq_a.pubkey_registry().clone(),
            last_clearing_prices: seq_a.last_clearing_prices().clone(),
            market_volumes: seq_a.market_volumes().clone(),
            resting_orders: seq_a.order_book.snapshot(),
            data_feeds: Vec::new(),
            pending_bundles: Vec::new(),
            pending_l1_deposits: Vec::new(),
            pending_bridge_withdrawals: Vec::new(),
            bridge_state: BridgeState::default(),
            admit_log: Vec::new(),
            account_fills: Vec::new(),
        };

        let mut seq_b = BlockSequencer::restore(state, oracle, SequencerConfig::default());
        assert_eq!(
            seq_b.order_book.len(),
            1,
            "restored order book should contain A's resting buy"
        );
        assert_eq!(
            seq_b.order_book.reserved_balance(aid_a),
            reserved_before,
            "balance reservation should be reconstructed"
        );

        // A matching sell from B should clear A's resting buy in the next batch.
        let sell = outcome_sell(&markets, 1_000, m0, 0, 300_000_000, 5);
        let sub_b = OrderSubmission {
            account_id: aid_b,
            orders: vec![sell],
            mm_constraint: None,
        };
        let bp = seq_b.produce_block(vec![sub_b], 2_000);

        let total_fill_qty: u64 = bp.block.fills.iter().map(|f| f.fill_qty).sum();
        assert!(
            total_fill_qty > 0,
            "expected restored resting buy to match the new sell, got fills={:?}",
            bp.block.fills
        );
        assert_eq!(
            seq_b.order_book.reserved_balance(aid_a),
            0,
            "A's reservation should be released after the fill"
        );
    }

    #[test]
    fn test_expired_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
        seq.order_book.set_ttl(2);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.order_book.len(), 1);

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.order_book.len(), 1);

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.order_book.len(), 0);
    }

    #[test]
    fn test_orders_for_resolved_markets_removed() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Market A");
        let m1 = markets.add_binary("Market B");

        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 5),
                outcome_buy(&markets, 0, m1, 0, 100_000_000, 5),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.order_book.len(), 2);

        let mut reduced_markets = MarketSet::new();
        reduced_markets.add_binary("Market B");

        run_batch(&mut seq, vec![], &reduced_markets, &[]);
        assert_eq!(seq.order_book.len(), 1);
    }

    #[test]
    fn test_bankrupt_account_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.order_book.len(), 1);

        let account = seq.accounts.get_mut(aid).unwrap();
        account.balance = 0;

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.order_book.len(), 0);
    }

    #[test]
    fn test_mm_orders_not_persisted() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let order = outcome_buy(&markets, 0, m0, 0, 100_000_000, 5);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.order_book.len(), 0);
    }

    // --- Fill settlement integration ---

    #[test]
    fn test_matching_buy_and_sell_settles_correctly() {
        let (markets, m0) = setup();

        let mut accounts = AccountStore::new();
        let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        accounts
            .get_mut(seller_id)
            .unwrap()
            .positions
            .insert((m0, 0), 50);

        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            MarketSet::new(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let buy_sub = OrderSubmission {
            account_id: buyer_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
            mm_constraint: None,
        };
        let sell_sub = OrderSubmission {
            account_id: seller_id,
            orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![buy_sub, sell_sub], &markets, &[]);

        if result.orders_filled > 0 {
            let buyer = seq.accounts.get(buyer_id).unwrap();
            let seller = seq.accounts.get(seller_id).unwrap();

            assert!(buyer.balance < 100 * NANOS_PER_DOLLAR as i64);
            assert!(buyer.position(m0, 0) > 0);

            assert!(seller.balance > 10 * NANOS_PER_DOLLAR as i64);
            assert!(seller.position(m0, 0) < 50);
        }
    }

    #[test]
    fn test_fill_updates_only_participating_account_digests() {
        let (markets, m0) = setup();

        let mut accounts = AccountStore::new();
        let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let untouched_id = accounts.create_account(50 * NANOS_PER_DOLLAR as i64);
        accounts
            .get_mut(seller_id)
            .unwrap()
            .positions
            .insert((m0, 0), 50);

        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let buy_sub = OrderSubmission {
            account_id: buyer_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
            mm_constraint: None,
        };
        let sell_sub = OrderSubmission {
            account_id: seller_id,
            orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
            mm_constraint: None,
        };

        seq.produce_block(vec![buy_sub, sell_sub], 1000);

        assert_ne!(seq.accounts.get(buyer_id).unwrap().events_digest, [0u8; 32]);
        assert_ne!(
            seq.accounts.get(seller_id).unwrap().events_digest,
            [0u8; 32]
        );
        assert_eq!(
            seq.accounts.get(untouched_id).unwrap().events_digest,
            [0u8; 32]
        );
    }

    // --- Block height counter ---

    #[test]
    fn test_batch_counter_increments() {
        let (markets, _) = setup();
        let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

        assert_eq!(seq.height, 0);
        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.height, 1);
        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.height, 2);
    }

    // --- Block-specific tests ---

    #[test]
    fn test_produce_block_returns_valid_header() {
        let (markets, _) = setup();
        let accounts = AccountStore::new();
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let bp = seq.produce_block(vec![], 1000);
        assert_eq!(bp.block.header.height, 1);
        assert_eq!(bp.block.header.parent_hash, [0u8; 32]); // genesis
        assert_eq!(bp.block.header.timestamp_ms, 1000);
    }

    #[test]
    fn test_block_chain_parent_hash() {
        let (markets, _) = setup();
        let accounts = AccountStore::new();
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let bp1 = seq.produce_block(vec![], 1000);
        let expected_parent = hash_header(&bp1.block.header);

        let bp2 = seq.produce_block(vec![], 2000);
        assert_eq!(bp2.block.header.parent_hash, expected_parent);
        assert_eq!(bp2.block.header.height, 2);
    }

    #[test]
    fn test_create_account_uses_post_system_state_for_orders() {
        let (markets, m0) = setup();
        let accounts = AccountStore::new();
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let aid = seq.create_account(10 * NANOS_PER_DOLLAR as i64);
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        let bp = seq.produce_block(vec![sub], 0);

        assert!(bp
            .witness
            .pre_state
            .iter()
            .all(|snapshot| snapshot.id != aid.0));
        let post_system = bp
            .witness
            .post_system_state
            .iter()
            .find(|snapshot| snapshot.id == aid.0)
            .expect("created account should exist after system events");
        assert_eq!(post_system.balance, 10 * NANOS_PER_DOLLAR as i64);

        let verification = sybil_verifier::verify_full(&bp.witness, false);
        assert!(
            verification.valid,
            "Violations: {:?}",
            verification.violations
        );
    }

    #[test]
    fn test_deposit_keeps_block_start_pre_state() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(0);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        seq.fund_account(aid, 10 * NANOS_PER_DOLLAR as i64).unwrap();
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        let bp = seq.produce_block(vec![sub], 0);

        let pre_state = bp
            .witness
            .pre_state
            .iter()
            .find(|snapshot| snapshot.id == aid.0)
            .expect("funded account should exist at block start");
        assert_eq!(pre_state.balance, 0);

        let post_system = bp
            .witness
            .post_system_state
            .iter()
            .find(|snapshot| snapshot.id == aid.0)
            .expect("funded account should exist after system events");
        assert_eq!(post_system.balance, 10 * NANOS_PER_DOLLAR as i64);

        let verification = sybil_verifier::verify_full(&bp.witness, false);
        assert!(
            verification.valid,
            "Violations: {:?}",
            verification.violations
        );
    }

    proptest! {
        #[test]
        fn prop_phase_builder_is_identity_without_system_baselines(
            balances in prop::collection::vec(0i64..=10_000i64, 0..6)
        ) {
            let mut accounts = AccountStore::new();
            for balance in balances {
                accounts.create_account(balance);
            }

            let (pre_state, post_system_state) =
                build_witness_phase_snapshots(&accounts, &HashMap::new());

            prop_assert_eq!(pre_state, post_system_state);
        }

        #[test]
        fn prop_created_account_is_only_in_post_system_state(
            initial_balances in prop::collection::vec(0i64..=10_000i64, 0..5),
            created_balance in 0i64..=10_000i64,
        ) {
            let mut accounts = AccountStore::new();
            for balance in initial_balances {
                accounts.create_account(balance);
            }
            let created_account = accounts.create_account(created_balance);

            let mut baselines = HashMap::new();
            baselines.insert(created_account, None);

            let (pre_state, post_system_state) =
                build_witness_phase_snapshots(&accounts, &baselines);

            prop_assert!(snapshot_by_id(&pre_state, created_account).is_none());
            let created_snapshot = snapshot_by_id(&post_system_state, created_account)
                .expect("created account must exist after system events");
            prop_assert_eq!(created_snapshot.balance, created_balance);
        }

        #[test]
        fn prop_baselined_account_uses_block_start_snapshot(
            initial_balance in 0i64..=10_000i64,
            funded_balance in 0i64..=20_000i64,
            initial_position in 0i64..=20,
            final_position in 0i64..=20,
        ) {
            let mut accounts = AccountStore::new();
            let account_id = accounts.create_account(initial_balance);
            {
                let account = accounts.get_mut(account_id).unwrap();
                account.positions.insert((MarketId::new(0), 0), initial_position);
            }

            let baseline = accounts.get(account_id).unwrap().clone();
            {
                let account = accounts.get_mut(account_id).unwrap();
                account.balance = funded_balance;
                account.total_deposited = baseline.total_deposited + 5;
                account.positions.insert((MarketId::new(0), 0), final_position);
            }

            let mut baselines = HashMap::new();
            baselines.insert(account_id, Some(baseline.clone()));

            let (pre_state, post_system_state) =
                build_witness_phase_snapshots(&accounts, &baselines);

            let pre_snapshot =
                snapshot_by_id(&pre_state, account_id).expect("baseline should appear in pre-state");
            let post_snapshot = snapshot_by_id(&post_system_state, account_id)
                .expect("live account should appear in post-system state");

            prop_assert_eq!(pre_snapshot.balance, baseline.balance);
            prop_assert_eq!(pre_snapshot.total_deposited, baseline.total_deposited);
            prop_assert_eq!(
                pre_snapshot.positions.iter().find(|&&(market, outcome, _)| market == MarketId::new(0) && outcome == 0).map(|&(_, _, qty)| qty).unwrap_or(0),
                initial_position
            );
            prop_assert_eq!(post_snapshot.balance, funded_balance);
            prop_assert_eq!(post_snapshot.total_deposited, baseline.total_deposited + 5);
            prop_assert_eq!(
                post_snapshot.positions.iter().find(|&&(market, outcome, _)| market == MarketId::new(0) && outcome == 0).map(|&(_, _, qty)| qty).unwrap_or(0),
                final_position
            );
        }

        #[test]
        fn prop_baseline_insertion_order_does_not_change_phase_snapshots(
            balance_a in 0i64..=10_000i64,
            balance_b in 0i64..=10_000i64,
            created_balance in 0i64..=10_000i64,
        ) {
            let mut accounts = AccountStore::new();
            let account_a = accounts.create_account(balance_a);
            let account_b = accounts.create_account(balance_b);
            let baseline_b = accounts.get(account_b).unwrap().clone();
            let created_account = accounts.create_account(created_balance);

            let mut baselines_ab = HashMap::new();
            baselines_ab.insert(created_account, None);
            baselines_ab.insert(account_b, Some(baseline_b.clone()));

            let mut baselines_ba = HashMap::new();
            baselines_ba.insert(account_b, Some(baseline_b));
            baselines_ba.insert(created_account, None);

            let (pre_ab, post_ab) = build_witness_phase_snapshots(&accounts, &baselines_ab);
            let (pre_ba, post_ba) = build_witness_phase_snapshots(&accounts, &baselines_ba);

            prop_assert!(snapshot_by_id(&pre_ab, account_a).is_some());
            prop_assert_eq!(pre_ab, pre_ba);
            prop_assert_eq!(post_ab, post_ba);
        }
    }

    #[test]
    fn test_state_root_in_block() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let bp1 = seq.produce_block(vec![], 0);

        // Submit an unfilled order that rests in the committed order book.
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };
        let bp2 = seq.produce_block(vec![sub], 0);

        // State root matches the witness post-state (what verifier will check)
        let expected_root = sybil_verifier::block::compute_state_root_with_sidecar(
            &bp2.witness.post_state,
            &bp2.witness.state_sidecar,
        );
        assert_eq!(bp2.block.header.state_root, expected_root);

        // It does not change account balances/positions, but it does change
        // committed order/reservation leaves.
        assert_ne!(bp1.block.header.state_root, bp2.block.header.state_root);
    }

    #[test]
    fn test_resolution_followed_by_empty_block_still_verifies() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let yes_buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let no_buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let opening_block = seq.produce_block(
            vec![
                OrderSubmission {
                    account_id: yes_buyer,
                    orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 1)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: no_buyer,
                    orders: vec![outcome_buy(&markets, 0, m0, 1, 500_000_000, 1)],
                    mm_constraint: None,
                },
            ],
            1_000,
        );
        let opening_verification = sybil_verifier::verify_full(&opening_block.witness, false);
        assert!(
            opening_verification.valid,
            "Violations: {:?}",
            opening_verification.violations
        );

        assert_ne!(seq.accounts.get(yes_buyer).unwrap().position(m0, 0), 0);
        assert_ne!(seq.accounts.get(no_buyer).unwrap().position(m0, 1), 0);

        seq.resolve_market(m0, NANOS_PER_DOLLAR, 2_000)
            .expect("resolution should succeed");

        assert_eq!(seq.accounts.get(yes_buyer).unwrap().position(m0, 0), 0);
        assert_eq!(seq.accounts.get(no_buyer).unwrap().position(m0, 1), 0);

        let resolution_block = seq.produce_block(vec![], 3_000);
        let resolution_verification = sybil_verifier::verify_full(&resolution_block.witness, false);
        assert!(
            resolution_verification.valid,
            "Violations: {:?}",
            resolution_verification.violations
        );
        assert_eq!(
            resolution_block.block.header.state_root,
            sybil_verifier::block::compute_state_root_with_sidecar(
                &resolution_block.witness.post_state,
                &resolution_block.witness.state_sidecar,
            )
        );
    }

    #[test]
    fn test_witness_includes_untouched_accounts() {
        let (markets, _) = setup();
        let mut accounts = AccountStore::new();
        accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        accounts.create_account(200 * NANOS_PER_DOLLAR as i64);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );

        let bp = seq.produce_block(vec![], 0);

        assert_eq!(bp.witness.pre_state.len(), 3);
        assert_eq!(bp.witness.post_system_state.len(), 3);
        assert_eq!(bp.witness.post_state.len(), 3);
        assert_eq!(
            bp.block.header.state_root,
            crate::block::compute_state_root_v2(
                &seq.accounts,
                seq.bridge_state(),
                seq.order_book(),
                seq.markets(),
                seq.market_groups(),
                seq.market_lifecycle(),
            )
        );
    }

    // --- Complete-set self-trade prevention ---

    fn setup_group() -> (MarketSet, MarketId, MarketId, MarketId, MarketGroup) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let m2 = markets.add_binary("C");
        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        (markets, m0, m1, m2, group)
    }

    #[test]
    fn test_mm_complete_set_buyyes_rejected() {
        let (markets, m0, m1, m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        constraint.add_order(1, matching_engine::MmSide::BuyYes);
        constraint.add_order(2, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
                outcome_buy(&markets, 0, m1, 0, 350_000_000, 10),
                outcome_buy(&markets, 0, m2, 0, 300_000_000, 10),
            ],
            mm_constraint: Some(constraint),
        };

        let bp = seq.produce_block(vec![sub], 1000);
        // Per-order STP: only the 3rd order (completing the set) is rejected
        assert_eq!(bp.block.rejections.len(), 1);
        assert!(bp.block.fills.is_empty());
    }

    #[test]
    fn test_mm_partial_group_accepted() {
        let (markets, m0, m1, _m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );

        // Only quote 2 of 3 outcomes — not a complete set
        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        constraint.add_order(1, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
                outcome_buy(&markets, 0, m1, 0, 350_000_000, 10),
            ],
            mm_constraint: Some(constraint),
        };

        let bp = seq.produce_block(vec![sub], 1000);
        assert_eq!(
            bp.block.rejections.len(),
            0,
            "Partial group should be accepted"
        );
    }

    #[test]
    fn test_mm_same_market_both_sides_accepted() {
        // BuyYes + BuyNo on same market (not in a group) — legitimate MM behavior
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        constraint.add_order(1, matching_engine::MmSide::BuyNo);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
                outcome_buy(&markets, 0, m0, 1, 400_000_000, 10),
            ],
            mm_constraint: Some(constraint),
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(
            result.rejections.len(),
            0,
            "Same-market BuyYes+BuyNo should be accepted"
        );
    }

    #[test]
    fn test_mm_buyno_complete_set_rejected() {
        // 3-market group: BuyNo on M0 covers {M1,M2}, BuyNo on M1 covers {M0,M2}
        // Union = {M0,M1,M2} = complete set — 2nd order completes it
        let (markets, m0, m1, _m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyNo);
        constraint.add_order(1, matching_engine::MmSide::BuyNo);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 1, 800_000_000, 10), // BuyNo M0 → covers {M1,M2}
                outcome_buy(&markets, 0, m1, 1, 800_000_000, 10), // BuyNo M1 → would cover {M0,M2}, completing set
            ],
            mm_constraint: Some(constraint),
        };

        let bp = seq.produce_block(vec![sub], 1000);
        assert_eq!(
            bp.block.rejections.len(),
            1,
            "Per-order STP: only the completing BuyNo rejected"
        );
    }

    // --- MM budget capping ---

    #[test]
    fn test_mm_budget_clamped_to_balance() {
        // MM has $10 balance but requests $50 budget
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let counter = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle,
            SequencerConfig::default(),
        );

        // Give counterparty YES positions to sell
        seq.accounts
            .get_mut(counter)
            .unwrap()
            .positions
            .insert((m0, 0), 1000);

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let mm_sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 100)],
            mm_constraint: Some(constraint),
        };
        let sell_sub = OrderSubmission {
            account_id: counter,
            orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 100)],
            mm_constraint: None,
        };

        let _result = run_batch(&mut seq, vec![mm_sub, sell_sub], &markets, &[]);

        // MM balance should never go below 0
        let mm_acct = seq.accounts.get(aid).unwrap();
        assert!(
            mm_acct.balance >= 0,
            "MM balance negative: {}",
            mm_acct.balance
        );
    }

    #[test]
    fn test_bankrupt_mm_skipped() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(0); // zero balance
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            MarketSet::new(),
            vec![],
            oracle,
            SequencerConfig::default(),
        );

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 100)],
            mm_constraint: Some(constraint),
        };

        let bp = seq.produce_block(vec![sub], 1000);
        assert!(
            bp.block.fills.is_empty(),
            "Bankrupt MM should not generate fills"
        );
    }

    /// Verify that group minting maintains position balance across multiple blocks.
    ///
    /// This is the key test for the MINT account mechanism: when the MM buys
    /// YES on all markets in a group, group minting creates YES without NO
    /// counterparties. The sequencer must derive the minting and adjust MINT
    /// so that total_yes == total_no for every market, every block.
    #[test]
    fn test_group_minting_position_balance_multi_block() {
        use matching_engine::{simple_yes_buy, MarketGroup};

        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let m2 = markets.add_binary("C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);

        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(1_000_000 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group.clone()],
            oracle,
            SequencerConfig::default(),
        );

        // Run 5 blocks, each with BuyYes on all 3 group markets.
        // Group minting will fire each time. MINT must stay balanced.
        for block_num in 0..5 {
            let sub = OrderSubmission {
                account_id: buyer,
                orders: vec![
                    simple_yes_buy(&markets, 0, m0, 400_000_000, 100),
                    simple_yes_buy(&markets, 0, m1, 350_000_000, 100),
                    simple_yes_buy(&markets, 0, m2, 300_000_000, 100),
                ],
                mm_constraint: None,
            };

            let bp = seq.produce_block(vec![sub], (block_num + 1) * 1000);

            // The position balance check inside produce_block should not fire,
            // but let's verify explicitly:
            for &mid in &[m0, m1, m2] {
                let total_yes: i64 = seq.accounts.iter().map(|(_, a)| a.position(mid, 0)).sum();
                let total_no: i64 = seq.accounts.iter().map(|(_, a)| a.position(mid, 1)).sum();
                assert_eq!(
                    total_yes, total_no,
                    "Position imbalance in market {:?} at block {}: YES={} NO={}",
                    mid, block_num, total_yes, total_no
                );
            }

            // Money conservation: total balance should only change by resolution payouts
            // (none here), so it should equal the initial deposit.
            let total_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
            assert_eq!(
                total_balance,
                1_000_000 * NANOS_PER_DOLLAR as i64,
                "Money conservation violated at block {}",
                block_num
            );

            // Verify MINT exists and has positions
            if !bp.block.fills.is_empty() {
                let mint = seq.accounts.get(crate::account::AccountId::MINT).unwrap();
                // MINT should have non-zero balance (revenue from selling)
                // and negative positions (shorts from minting)
                assert!(
                    !mint.positions.is_empty(),
                    "MINT should hold positions after group minting"
                );
            }
        }
    }

    #[test]
    fn test_mm_balance_nonnegative_across_blocks() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let mm_id = accounts.create_account(1000 * NANOS_PER_DOLLAR as i64);
        let counter_id = accounts.create_account(100_000 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle,
            SequencerConfig::default(),
        );

        // Give counterparty massive YES position to sell
        seq.accounts
            .get_mut(counter_id)
            .unwrap()
            .positions
            .insert((m0, 0), 100_000);

        for block_num in 0..10 {
            let mut constraint = MmConstraint::new(MmId::new(1), 500 * NANOS_PER_DOLLAR);
            constraint.add_order(0, matching_engine::MmSide::BuyYes);

            let mm_sub = OrderSubmission {
                account_id: mm_id,
                orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 1000)],
                mm_constraint: Some(constraint),
            };
            let counter_sub = OrderSubmission {
                account_id: counter_id,
                orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 1000)],
                mm_constraint: None,
            };

            run_batch(&mut seq, vec![mm_sub, counter_sub], &markets, &[]);

            let mm_acct = seq.accounts.get(mm_id).unwrap();
            assert!(
                mm_acct.balance >= 0,
                "MM balance negative at block {}: {}",
                block_num,
                mm_acct.balance
            );
        }
    }

    // --- Cross-block STP (SYB-110) ---

    fn make_grouped_sequencer(
        balance: i64,
    ) -> (BlockSequencer, AccountId, MarketSet, MarketId, MarketId) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let mut group = MarketGroup::new("Event");
        group.add_market(m0);
        group.add_market(m1);
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(balance);
        let oracle = Arc::new(AdminOracle::new());
        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );
        (seq, aid, markets, m0, m1)
    }

    fn single_order_sub(account_id: AccountId, order: Order) -> OrderSubmission {
        OrderSubmission {
            account_id,
            orders: vec![order],
            mm_constraint: None,
        }
    }

    #[test]
    fn cross_block_stp_rejects_set_formation_across_blocks() {
        let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 400_000_000, 10));
        let outcome = seq.try_admit_direct(first);
        assert!(matches!(outcome, AdmitOutcome::Admitted { .. }));

        seq.produce_block(vec![], 1000);
        assert_eq!(seq.height, 1);

        let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
        let outcome = seq.try_admit_direct(second);
        match outcome {
            AdmitOutcome::Rejected(SequencerError::Rejected(r)) => {
                assert!(matches!(r.reason, RejectionReason::CompleteSetFormation));
            }
            other => panic!("expected CompleteSetFormation rejection, got {:?}", other),
        }
    }

    #[test]
    fn cross_block_stp_allows_after_cancel() {
        let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 400_000_000, 10));
        let first_id = match seq.try_admit_direct(first) {
            AdmitOutcome::Admitted { order_id, .. } => order_id,
            other => panic!("expected Admitted, got {:?}", other),
        };

        seq.produce_block(vec![], 1000);

        seq.cancel_pending_order(aid, first_id).expect("cancel ok");

        let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
        let outcome = seq.try_admit_direct(second);
        assert!(
            matches!(outcome, AdmitOutcome::Admitted { .. }),
            "expected Admitted after cancel, got {:?}",
            outcome
        );
    }

    #[test]
    fn direct_ioc_order_participates_once_and_never_rests() {
        let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
        let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
        order.expires_at_block = Some(1);

        assert!(matches!(
            seq.try_admit_direct(single_order_sub(aid, order)),
            AdmitOutcome::Admitted { .. }
        ));

        let bp = seq.produce_block(vec![], 1000);
        assert_eq!(bp.flow_metrics.carried_resting_orders, 1);
        assert_eq!(seq.pending_orders_info(Some(aid)).len(), 0);
    }

    #[test]
    fn gtd_order_expires_after_requested_block() {
        let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
        let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
        order.expires_at_block = Some(2);

        assert!(matches!(
            seq.try_admit_direct(single_order_sub(aid, order)),
            AdmitOutcome::Admitted { .. }
        ));

        seq.produce_block(vec![], 1000);
        assert_eq!(seq.pending_orders_info(Some(aid)).len(), 1);

        seq.produce_block(vec![], 2000);
        assert_eq!(seq.pending_orders_info(Some(aid)).len(), 0);
    }

    #[test]
    fn direct_gtd_order_rejects_when_it_cannot_reach_next_batch() {
        let (mut seq, aid, markets, m0, _) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);
        let mut order = outcome_buy(&markets, 0, m0, 0, 400_000_000, 10);
        order.expires_at_block = Some(0);

        match seq.try_admit_direct(single_order_sub(aid, order)) {
            AdmitOutcome::Rejected(SequencerError::Rejected(rejection)) => {
                assert!(matches!(rejection.reason, RejectionReason::Expired { .. }));
            }
            other => panic!("expected expired rejection, got {:?}", other),
        }
    }

    #[test]
    fn cross_block_stp_rejects_buyno_combination_across_blocks() {
        let (markets, m0, m1, _m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );

        let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 1, 800_000_000, 10));
        assert!(matches!(
            seq.try_admit_direct(first),
            AdmitOutcome::Admitted { .. }
        ));

        seq.produce_block(vec![], 1000);

        let second = single_order_sub(aid, outcome_buy(&markets, 0, m1, 1, 800_000_000, 10));
        match seq.try_admit_direct(second) {
            AdmitOutcome::Rejected(SequencerError::Rejected(r)) => {
                assert!(matches!(r.reason, RejectionReason::CompleteSetFormation));
            }
            other => panic!("expected CompleteSetFormation rejection, got {:?}", other),
        }
    }

    #[test]
    fn cross_block_stp_sells_do_not_contribute() {
        let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

        seq.accounts
            .get_mut(aid)
            .unwrap()
            .positions
            .insert((m0, 0), 50);

        let sell_first = single_order_sub(aid, outcome_sell(&markets, 0, m0, 0, 400_000_000, 10));
        assert!(matches!(
            seq.try_admit_direct(sell_first),
            AdmitOutcome::Admitted { .. }
        ));

        seq.produce_block(vec![], 1000);

        let buy_other = single_order_sub(aid, outcome_buy(&markets, 0, m1, 0, 400_000_000, 10));
        assert!(
            matches!(
                seq.try_admit_direct(buy_other),
                AdmitOutcome::Admitted { .. }
            ),
            "sell on m0 + buy on m1 is only partial coverage — must be admitted"
        );
    }

    #[test]
    fn cross_block_stp_mm_path_sees_prior_resting() {
        // Account first places a non-MM BuyYes m0 through the admit path, then in
        // a later block submits an MM bundle that includes BuyYes m1. The MM
        // bundle's STP check (inside prepare_block) must see the prior-block
        // resting order and reject the completing leg.
        let (mut seq, aid, markets, m0, m1) = make_grouped_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let first = single_order_sub(aid, outcome_buy(&markets, 0, m0, 0, 400_000_000, 10));
        assert!(matches!(
            seq.try_admit_direct(first),
            AdmitOutcome::Admitted { .. }
        ));
        seq.produce_block(vec![], 1000);

        let mut constraint = MmConstraint::new(MmId::new(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        let mm_sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m1, 0, 400_000_000, 10)],
            mm_constraint: Some(constraint),
        };

        let bp = seq.produce_block(vec![mm_sub], 2000);
        assert_eq!(
            bp.block.rejections.len(),
            1,
            "MM completing leg should be rejected because prior-block resting covers m0"
        );
        assert!(matches!(
            bp.block.rejections[0].reason,
            RejectionReason::CompleteSetFormation
        ));
    }

    #[test]
    fn cross_block_stp_pending_bundle_contributes_to_admit_check() {
        // A multi-order non-MM bundle stays in pending_bundles (not single-order
        // so try_admit_direct defers it). A later single-order admit must see the
        // bundled coverage and reject if it would complete the set.
        let (markets, m0, m1, m2, group) = setup_group();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let oracle = Arc::new(AdminOracle::new());
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![group],
            oracle,
            SequencerConfig::default(),
        );

        let bundle = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 400_000_000, 10),
                outcome_buy(&markets, 0, m1, 0, 400_000_000, 10),
            ],
            mm_constraint: None,
        };
        match seq.try_admit_direct(bundle) {
            AdmitOutcome::Deferred(sub) => seq.push_pending_bundle(sub),
            other => panic!("expected Deferred for multi-order bundle, got {:?}", other),
        }

        let completing = single_order_sub(aid, outcome_buy(&markets, 0, m2, 0, 400_000_000, 10));
        match seq.try_admit_direct(completing) {
            AdmitOutcome::Rejected(SequencerError::Rejected(r)) => {
                assert!(matches!(r.reason, RejectionReason::CompleteSetFormation));
            }
            other => panic!(
                "expected CompleteSetFormation rejection from pending-bundle coverage, got {:?}",
                other
            ),
        }
    }
}
