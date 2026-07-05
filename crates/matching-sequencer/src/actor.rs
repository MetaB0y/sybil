// Exempt from the f64 ban (SYB-196): this module is off the consensus/state-root
// path. Its floats are the token-bucket admission rate limiter and Prometheus
// metric gauges/histograms — both explicitly exempt (admission heuristic +
// observability). No value here is committed into a block's state root.
#![allow(clippy::disallowed_types)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent};
use tokio::sync::broadcast;
use tokio::time::{interval_at, Instant};

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos, Order, Problem};
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, MarketStatus, Oracle, ResolutionRecord, SignedAttestation,
};

use crate::account::{Account, AccountId};
use crate::block::{BlockProduction, SealedBlock};
use crate::bridge::{BridgeState, BridgeWithdrawalRequest, L1Deposit, WithdrawalLeaf};
use crate::crypto::{
    verify_signed_bridge_withdrawal, verify_signed_cancel, verify_signed_order, PublicKey,
    SignedBridgeWithdrawal, SignedCancel, SignedOrder,
};
use crate::error::{Rejection, RejectionReason, SequencerError};
use crate::market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, MarketSearchQuery, PriceCandle,
    PriceCandlePage, PriceHistoryPage, PricePoint,
};
use crate::portfolio::PortfolioSummary;
use crate::sequencer::{
    BlockSequencer, OrderSubmission, PendingOrderInfo, PreparedBlock, SequencerConfig,
};
use crate::store::{ControlPlaneCommand, HistoryRetentionPolicy};
use crate::{
    AccountSnapshotSlot, QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts,
    QMDB_STATE_MAX_KEY_BYTES,
};

const SEQUENCER_ACTOR_METRIC_NAME: &str = "sequencer";
pub const DEFAULT_PRICE_HISTORY_QUERY_POINTS: usize = 500;
pub const MAX_PRICE_HISTORY_QUERY_POINTS: usize = 5_000;

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn limit_price_point_page(
    mut points: Vec<PricePoint>,
    before_height: Option<u64>,
    limit: usize,
) -> PriceHistoryPage {
    if let Some(before_height) = before_height {
        points.retain(|point| point.height < before_height);
    }
    if limit == 0 {
        return PriceHistoryPage {
            points: Vec::new(),
            next_before_height: None,
            retention_min_height: None,
        };
    }

    if points.len() > limit {
        let page = points.split_off(points.len() - limit);
        PriceHistoryPage {
            next_before_height: page.first().map(|point| point.height),
            points: page,
            retention_min_height: None,
        }
    } else {
        PriceHistoryPage {
            points,
            next_before_height: None,
            retention_min_height: None,
        }
    }
}

fn price_candle_page_from_points(
    points: Vec<PricePoint>,
    resolution_secs: u32,
    from_ms: Option<u64>,
    to_ms: Option<u64>,
    before_ms: Option<u64>,
    limit: usize,
) -> PriceCandlePage {
    if resolution_secs == 0 || limit == 0 {
        return PriceCandlePage {
            resolution_secs,
            candles: Vec::new(),
            next_before_ms: None,
            retention_min_bucket_ms: None,
        };
    }

    let mut by_bucket = BTreeMap::<u64, PriceCandle>::new();
    for point in points {
        if from_ms.is_some_and(|from| point.timestamp_ms < from)
            || to_ms.is_some_and(|to| point.timestamp_ms > to)
        {
            continue;
        }
        let candle = PriceCandle::from_point(resolution_secs, &point);
        if before_ms.is_some_and(|before| candle.bucket_start_ms >= before) {
            continue;
        }
        by_bucket
            .entry(candle.bucket_start_ms)
            .and_modify(|existing| existing.merge_point(&point))
            .or_insert(candle);
    }

    let mut candles = VecDeque::new();
    for candle in by_bucket.into_values() {
        if candles.len() == limit.saturating_add(1) {
            candles.pop_front();
        }
        candles.push_back(candle);
    }
    let mut candles: Vec<_> = candles.into_iter().collect();
    let next_before_ms = if candles.len() > limit {
        candles.remove(0);
        candles.first().map(|candle| candle.bucket_start_ms)
    } else {
        None
    };
    PriceCandlePage {
        resolution_secs,
        candles,
        next_before_ms,
        retention_min_bucket_ms: None,
    }
}

/// Messages sent from handles to the sequencer actor.
pub enum SequencerMsg {
    Tick,
    #[cfg(test)]
    TestCrashOnNextBlock(SequencerTestCrashpoint),
    #[cfg(test)]
    TestHoldNextTick(SequencerTestTickHold, RpcReplyPort<()>),
    SubmitOrder(OrderSubmission, RpcReplyPort<Result<(), SequencerError>>),
    SubmitSignedOrder(SignedOrder, RpcReplyPort<Result<(), SequencerError>>),
    CancelSignedOrder(SignedCancel, RpcReplyPort<Result<(), SequencerError>>),
    GetStateProof(
        Vec<u8>,
        RpcReplyPort<Result<SequencerStateProof, SequencerError>>,
    ),
    ProduceBlock(RpcReplyPort<Result<SealedBlock, SequencerError>>),
    CreateAccount(i64, RpcReplyPort<Result<Account, SequencerError>>),
    FundAccount(
        AccountId,
        i64,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    SubmitL1Deposit(L1Deposit, RpcReplyPort<Result<Account, SequencerError>>),
    CreateBridgeWithdrawal(
        BridgeWithdrawalRequest,
        RpcReplyPort<Result<WithdrawalLeaf, SequencerError>>,
    ),
    CreateSignedBridgeWithdrawal(
        SignedBridgeWithdrawal,
        RpcReplyPort<Result<WithdrawalLeaf, SequencerError>>,
    ),
    RegisterPubkey(
        AccountId,
        PublicKey,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    CreateMarket(String, RpcReplyPort<Result<MarketId, SequencerError>>),
    CreateMarketGroup(
        String,
        Vec<MarketId>,
        RpcReplyPort<Result<(u64, MarketGroup), SequencerError>>,
    ),
    ExtendMarketGroup(
        u64,
        MarketId,
        RpcReplyPort<Result<(MarketGroup, bool), SequencerError>>,
    ),
    ResolveMarket(
        MarketId,
        Nanos,
        RpcReplyPort<Result<ResolutionRecord, SequencerError>>,
    ),
    ResolveMarketAttested(
        MarketId,
        SignedAttestation,
        RpcReplyPort<Result<ResolutionRecord, SequencerError>>,
    ),
    RegisterFeed(
        FeedPubkey,
        String,
        RpcReplyPort<Result<FeedId, SequencerError>>,
    ),
    InstallTemplate(
        sybil_oracle::ResolutionTemplate,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    GetBlock(u64, RpcReplyPort<Result<SealedBlock, SequencerError>>),
    CreateMarketWithMetadata(
        String,
        MarketMetadata,
        RpcReplyPort<Result<MarketId, SequencerError>>,
    ),
    GetPriceHistory(
        MarketId,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        usize,
        RpcReplyPort<Result<PriceHistoryPage, SequencerError>>,
    ),
    GetPriceCandles(
        MarketId,
        u32,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        usize,
        RpcReplyPort<Result<PriceCandlePage, SequencerError>>,
    ),
    GetAccountFills(
        AccountId,
        Option<MarketId>,
        usize,
        usize,
        RpcReplyPort<Vec<AccountFillRecord>>,
    ),
    GetAccountFillsAfter(
        AccountId,
        Option<MarketId>,
        Option<AccountFillCursor>,
        usize,
        RpcReplyPort<Vec<AccountFillRecord>>,
    ),
    GetEquitySeries(
        AccountId,
        u64,
        RpcReplyPort<Vec<crate::aggregates::EquityPoint>>,
    ),
    GetAccountEvents(
        AccountId,
        usize,
        Option<(u64, u64)>,
        Option<String>,
        RpcReplyPort<Vec<crate::aggregates::HistoryEvent>>,
    ),
    PauseBlockProduction(RpcReplyPort<()>),
    ResumeBlockProduction(RpcReplyPort<()>),
    Query(SequencerReadQuery),
    /// Periodic indicative-solve tick (C2). Fires from a dedicated timer
    /// task at ~750ms cadence, decoupled from block production. The arm
    /// kicks off a `spawn_blocking` shadow-solve over the resting book and
    /// self-sends an `IndicativeUpdate` once the solver returns.
    IndicativeTick,
    /// Result of one shadow-solve: a per-market cache of indicative prices
    /// + per-market notional volume + computed_at_ms.
    IndicativeUpdate(HashMap<MarketId, IndicativeSnapshot>),
    /// Shadow-solve failed before producing a cache update.
    IndicativeSolveFailed {
        solver: String,
        error: String,
    },
}

pub struct SequencerReadQuery {
    run: Box<dyn FnOnce(&mut SequencerActorState) + Send + 'static>,
}

impl SequencerReadQuery {
    fn new<T>(
        reply: RpcReplyPort<T>,
        query: impl FnOnce(&SequencerActorState) -> T + Send + 'static,
    ) -> Self
    where
        T: Send + 'static,
    {
        Self {
            run: Box::new(move |state| {
                let _ = reply.send(query(state));
            }),
        }
    }

    fn execute(self, state: &mut SequencerActorState) {
        (self.run)(state);
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequencerTestCrashpoint {
    BeforePrepare,
    AfterPrepareBeforePersist,
    AfterPersistBeforeCommit,
    AfterCommit,
}

#[cfg(test)]
pub struct SequencerTestTickHold {
    pub started: tokio::sync::oneshot::Sender<()>,
    pub release: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(not(test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SequencerTestCrashpoint {}

/// Shadow-solve snapshot for one market (C2). Off-block — produced by the
/// indicative scheduler, cached on `SequencerActorState`, served by
/// `GET /v1/markets/{id}/open-batch`. Pure-core (`BlockSequencer`) never
/// sees this.
#[derive(Clone, Debug, Default)]
pub struct IndicativeSnapshot {
    /// YES clearing price the solver discovered, or the last-clearing
    /// fallback when the book has orders but no matchable cross. `None`
    /// when the book is empty for this market or the solver is infeasible.
    pub yes_price_nanos: Option<u64>,
    /// NO clearing price; same semantics as `yes_price_nanos`.
    pub no_price_nanos: Option<u64>,
    /// Sum of `fill_price * fill_qty` over fills the shadow-solve produced
    /// that touched this market. `0` for fallback (no cross) or empty book.
    pub volume_nanos: u64,
    /// Wall-clock timestamp (ms since UNIX epoch) when this snapshot was
    /// computed. FE renders staleness from it.
    pub computed_at_ms: u64,
}

#[derive(Debug, Default)]
struct IndicativeSolveGate {
    in_flight: bool,
}

impl IndicativeSolveGate {
    fn try_start(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    fn finish(&mut self) {
        self.in_flight = false;
    }
}

enum BlockTickOutcome {
    Produced(Box<SealedBlock>),
    Paused,
    Halted(SequencerError),
    PersistFailed(SequencerError),
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Extract per-market `IndicativeSnapshot` values from a shadow-solve
/// result. Markets that produced fills get the solver's discovered prices
/// and the fill notional; markets that are in the book but had no
/// matchable cross fall back to the last clearing price (volume = 0).
/// Markets absent from the book are not included in the map (lookup-miss
/// returns `IndicativeSnapshot::default()` on the read path).
fn build_indicative_snapshots(
    problem: &Problem,
    result: &matching_solver::PipelineResult,
    last_clearing: &HashMap<MarketId, Vec<Nanos>>,
    computed_at_ms: u64,
) -> HashMap<MarketId, IndicativeSnapshot> {
    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

    // Per-market notional volume from the shadow-solve's fills.
    let mut volume_by_market: HashMap<MarketId, u64> = HashMap::new();
    for fill in &result.result.fills {
        if fill.fill_qty == matching_engine::Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let notional = matching_engine::notional_nanos(fill.fill_price, fill.fill_qty);
        for m in order.active_markets() {
            let entry = volume_by_market.entry(m).or_insert(0);
            *entry = entry.saturating_add(notional.0);
        }
    }

    // Every market touched by a resting order in the speculative book.
    let mut markets_in_book: std::collections::HashSet<MarketId> = std::collections::HashSet::new();
    for order in &problem.orders {
        for m in order.active_markets() {
            markets_in_book.insert(m);
        }
    }

    let pd_prices = result.price_discovery.as_ref().map(|pd| &pd.prices);

    let mut snapshots: HashMap<MarketId, IndicativeSnapshot> = HashMap::new();
    for m in markets_in_book {
        let (yes, no) = if let Some(prices) = pd_prices.and_then(|p| p.get(&m)) {
            (prices.first().copied(), prices.get(1).copied())
        } else if let Some(prices) = last_clearing.get(&m) {
            (prices.first().copied(), prices.get(1).copied())
        } else {
            (None, None)
        };
        snapshots.insert(
            m,
            IndicativeSnapshot {
                yes_price_nanos: yes.map(|n| n.0),
                no_price_nanos: no.map(|n| n.0),
                volume_nanos: volume_by_market.get(&m).copied().unwrap_or(0),
                computed_at_ms,
            },
        );
    }
    snapshots
}

/// A market search result enriched with metadata, prices, and volume.
#[derive(Clone, Debug)]
pub struct MarketSearchResult {
    pub market_id: MarketId,
    pub name: String,
    pub metadata: Option<MarketMetadata>,
    pub yes_price_nanos: Option<Nanos>,
    pub no_price_nanos: Option<Nanos>,
    pub volume_nanos: u64,
    pub status: MarketStatus,
}

#[derive(Clone, Debug)]
pub struct SequencerStateProof {
    pub block_height: u64,
    pub state_root: [u8; 32],
    pub slot: AccountSnapshotSlot,
    pub leaf_key: Vec<u8>,
    pub verified: bool,
    pub kind: SequencerStateProofKind,
}

#[derive(Clone, Debug)]
pub enum SequencerStateProofKind {
    Inclusion {
        leaf_value: Vec<u8>,
        proof: QmdbStateKeyValueProofParts,
    },
    Exclusion {
        proof: QmdbStateExclusionProofParts,
    },
}

struct SequencerActor;

struct SequencerActorArgs {
    sequencer: BlockSequencer,
    store: Option<Arc<crate::store::Store>>,
    block_broadcast: broadcast::Sender<SealedBlock>,
    mailbox_monitor: MailboxMonitor,
}

struct SequencerActorState {
    sequencer: BlockSequencer,
    latest_block: Option<SealedBlock>,
    block_history: VecDeque<SealedBlock>,
    block_broadcast: broadcast::Sender<SealedBlock>,
    pause_count: u32,
    halted_error: Option<SequencerError>,
    store: Option<Arc<crate::store::Store>>,
    global_submission_bucket: TokenBucket,
    account_submission_buckets: HashMap<AccountId, TokenBucket>,
    mailbox_monitor: MailboxMonitor,
    /// Per-market indicative snapshots from the C2 shadow-solver. Cache
    /// lives on the actor (not `BlockSequencer`) so pure-core stays pure.
    /// Empty until the first `IndicativeUpdate` arrives; lookup-miss
    /// returns `IndicativeSnapshot::default()` (None/None/0/0).
    indicative_cache: HashMap<MarketId, IndicativeSnapshot>,
    indicative_solve_gate: IndicativeSolveGate,
    #[cfg(test)]
    next_tick_hold: Option<SequencerTestTickHold>,
}

#[derive(Clone, Debug)]
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_second: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(refill_per_second: u32, capacity: u32, now: Instant) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_per_second: refill_per_second as f64,
            last_refill: now,
        }
    }

    fn allow(&mut self, now: Instant) -> Result<(), u64> {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        self.tokens =
            (self.tokens + elapsed.as_secs_f64() * self.refill_per_second).min(self.capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.retry_after_secs())
        }
    }

    fn retry_after_secs(&self) -> u64 {
        if self.refill_per_second <= 0.0 {
            return 1;
        }
        ((1.0 - self.tokens).max(0.0) / self.refill_per_second)
            .ceil()
            .max(1.0) as u64
    }
}

#[derive(Clone)]
struct MailboxMonitor {
    actor: &'static str,
    depth: Arc<AtomicUsize>,
    level: Arc<AtomicU8>,
    warn_depth: usize,
    error_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MailboxPressureLevel {
    Normal = 0,
    Warn = 1,
    Error = 2,
}

impl MailboxPressureLevel {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Warn,
            2 => Self::Error,
            _ => Self::Normal,
        }
    }
}

impl MailboxMonitor {
    fn new(actor: &'static str, warn_depth: usize, error_depth: usize) -> Self {
        Self {
            actor,
            depth: Arc::new(AtomicUsize::new(0)),
            level: Arc::new(AtomicU8::new(MailboxPressureLevel::Normal as u8)),
            warn_depth,
            error_depth,
        }
    }

    fn queued(&self) {
        let depth = self.depth.fetch_add(1, Ordering::Relaxed) + 1;
        self.record(depth);
    }

    fn started(&self) {
        let mut observed = self.depth.load(Ordering::Relaxed);
        loop {
            if observed == 0 {
                self.record(0);
                return;
            }

            match self.depth.compare_exchange_weak(
                observed,
                observed - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.record(observed - 1);
                    return;
                }
                Err(next) => observed = next,
            }
        }
    }

    fn send_failed(&self) {
        self.started();
    }

    fn reset(&self) {
        self.depth.store(0, Ordering::Relaxed);
        self.record(0);
    }

    #[cfg(test)]
    fn depth(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    fn pressure_level(&self, depth: usize) -> MailboxPressureLevel {
        if self.error_depth > 0 && depth >= self.error_depth {
            MailboxPressureLevel::Error
        } else if self.warn_depth > 0 && depth >= self.warn_depth {
            MailboxPressureLevel::Warn
        } else {
            MailboxPressureLevel::Normal
        }
    }

    fn record(&self, depth: usize) {
        metrics::gauge!("sybil_actor_queue_depth", "actor" => self.actor).set(depth as f64);

        let level = self.pressure_level(depth);
        let previous =
            MailboxPressureLevel::from_u8(self.level.swap(level as u8, Ordering::Relaxed));

        if level == previous {
            return;
        }

        match level {
            MailboxPressureLevel::Error => {
                tracing::error!(
                    actor = self.actor,
                    depth,
                    error_depth = self.error_depth,
                    "actor mailbox queue depth is critical"
                );
            }
            MailboxPressureLevel::Warn => {
                tracing::warn!(
                    actor = self.actor,
                    depth,
                    warn_depth = self.warn_depth,
                    "actor mailbox queue depth is high"
                );
            }
            MailboxPressureLevel::Normal => {
                tracing::info!(
                    actor = self.actor,
                    depth,
                    "actor mailbox queue depth recovered"
                );
            }
        }
    }
}

impl SequencerActorState {
    #[tracing::instrument(
        skip_all,
        fields(height = tracing::field::Empty, pending_bundles = tracing::field::Empty)
    )]
    async fn on_tick(&mut self) -> Result<BlockTickOutcome, ActorProcessingErr> {
        self.on_tick_inner(None).await
    }

    #[tracing::instrument(
        skip_all,
        fields(height = tracing::field::Empty, pending_bundles = tracing::field::Empty)
    )]
    async fn on_tick_inner(
        &mut self,
        test_crashpoint: Option<SequencerTestCrashpoint>,
    ) -> Result<BlockTickOutcome, ActorProcessingErr> {
        #[cfg(not(test))]
        let _ = test_crashpoint;

        if let Some(error) = &self.halted_error {
            tracing::error!(
                error = %error,
                "block production halted after invariant failure"
            );
            return Ok(BlockTickOutcome::Halted(error.clone()));
        }
        if self.pause_count > 0 {
            return Ok(BlockTickOutcome::Paused);
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let pending_bundles = self.sequencer.pending_bundles_len();
        tracing::Span::current().record("pending_bundles", pending_bundles);

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::BeforePrepare) {
            return Err(ActorProcessingErr::from(
                "injected crash before block prepare".to_string(),
            ));
        }

        let prepared = match self.sequencer.prepare_block(Vec::new(), timestamp_ms) {
            Ok(prepared) => prepared,
            Err(error) => {
                let outcome_error = error.clone();
                self.halt_after_invariant_failure(error);
                return Ok(BlockTickOutcome::Halted(outcome_error));
            }
        };
        tracing::Span::current().record("height", prepared.production().block.header.height);

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterPrepareBeforePersist) {
            return Err(ActorProcessingErr::from(
                "injected crash after block prepare before persist".to_string(),
            ));
        }

        #[cfg(test)]
        self.await_next_tick_hold_for_test().await;

        if let Err(error) = self.persist_block(&prepared).await {
            metrics::counter!("sybil_persistence_failures").increment(1);
            // The live sequencer still holds the pending bundles — the drain
            // happened on the clone. The next tick will retry, and the
            // PENDING_BUNDLES redb table wasn't cleared because save_block's
            // transaction rolled back atomically.
            tracing::error!(error = %error, "prepared block discarded before commit; pending bundles retained for retry");
            return Ok(BlockTickOutcome::PersistFailed(error));
        }

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterPersistBeforeCommit) {
            return Err(ActorProcessingErr::from(
                "injected crash after block persist before commit".to_string(),
            ));
        }

        let bp = match self.sequencer.commit_prepared_block(prepared) {
            Ok(bp) => bp,
            Err(error) => {
                let outcome_error = error.clone();
                self.halt_after_invariant_failure(error);
                return Ok(BlockTickOutcome::Halted(outcome_error));
            }
        };

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterCommit) {
            return Err(ActorProcessingErr::from(
                "injected crash after block commit".to_string(),
            ));
        }

        self.record_metrics(&bp, pending_bundles);
        let sealed = bp.sealed_block();
        self.push_to_history(sealed.clone());
        let _ = self.block_broadcast.send(sealed.clone());
        self.latest_block = Some(sealed.clone());
        Ok(BlockTickOutcome::Produced(Box::new(sealed)))
    }

    fn halt_after_invariant_failure(&mut self, error: SequencerError) {
        metrics::counter!("sybil_block_verification_failures").increment(1);
        // Verification failures mean the prepared state transition itself is
        // untrustworthy. Leave the live pre-block state intact and fail-stop
        // future ticks until an operator fixes the solver/state bug and
        // restarts from the last committed block. Persistence failures keep
        // the existing retry path above because they do not invalidate the
        // in-memory state transition.
        tracing::error!(
            error = %error,
            "prepared block discarded before commit; block production halted"
        );
        self.halted_error = Some(error);
    }

    #[cfg(test)]
    async fn await_next_tick_hold_for_test(&mut self) {
        if let Some(hold) = self.next_tick_hold.take() {
            let _ = hold.started.send(());
            let _ = hold.release.await;
        }
    }

    /// Indicative scheduler tick (C2). Builds a speculative `Problem` from
    /// the current resting book (Tier 1: no pending bundles, no MM flash),
    /// kicks off a `spawn_blocking` solve, and self-sends an
    /// `IndicativeUpdate` once the solver returns. An actor-local in-flight
    /// gate prevents the ~750ms timer from stacking LP jobs when one solve is
    /// slower than the cadence.
    fn on_indicative_tick(&mut self, myself: ActorRef<SequencerMsg>) {
        if self.pause_count > 0 {
            return;
        }
        if !self.indicative_solve_gate.try_start() {
            return;
        }
        // Tier 1: single-market only. The LP solver asserts num_markets==1,
        // and multi-market orders sit in `pending_bundles`, not the book.
        let resting_orders: Vec<Order> = self
            .sequencer
            .order_book()
            .resting_orders()
            .filter(|(o, _)| o.num_markets == 1)
            .map(|(o, _)| o.clone())
            .collect();

        if resting_orders.is_empty() {
            // Nothing to solve. Leave the cache untouched so the last good
            // snapshot remains visible (FE displays staleness via
            // `computed_at_ms`).
            self.indicative_solve_gate.finish();
            return;
        }

        let mut problem = Problem::new("indicative");
        problem.markets = self.sequencer.markets().clone();
        problem.orders = resting_orders;
        problem.market_groups = self.sequencer.market_groups().to_vec();
        // mm_constraints intentionally empty (Tier 1 — no MM flash liquidity).

        let last_clearing = self.sequencer.analytics().last_clearing_prices().clone();
        let computed_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let target = myself.clone();
        let mailbox = self.mailbox_monitor.clone();

        let solver = self.sequencer.solver();
        let solver_name = solver.name().to_string();

        tokio::task::spawn_blocking(move || {
            let message = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                solver.solve(&problem)
            })) {
                Ok(result) => {
                    let snapshots = build_indicative_snapshots(
                        &problem,
                        &result,
                        &last_clearing,
                        computed_at_ms,
                    );
                    SequencerMsg::IndicativeUpdate(snapshots)
                }
                Err(payload) => SequencerMsg::IndicativeSolveFailed {
                    solver: solver_name,
                    error: panic_payload_to_string(payload.as_ref()),
                },
            };
            mailbox.queued();
            if target.send_message(message).is_err() {
                mailbox.send_failed();
            }
        });
    }

    async fn persist_block(&self, prepared: &PreparedBlock) -> Result<(), SequencerError> {
        if let Some(ref store) = self.store {
            let sealed = prepared.production().sealed_block();
            let height = sealed.canonical.header.height;
            // Commit clears pending off-block read-model rows after this returns.
            // Persist every prepared block so empty-fill batches can still durably
            // carry direct-admit history and equity deltas.
            store
                .save_block_with_witness_and_history(
                    prepared.next_sequencer().snapshot(),
                    &prepared.production().witness,
                    &sealed,
                )
                .await
                .map_err(|error| SequencerError::Persistence(error.to_string()))?;

            let policy = HistoryRetentionPolicy {
                block_history_retention_blocks: self
                    .sequencer
                    .config
                    .block_history_retention_blocks,
                raw_price_retention_blocks: self.sequencer.config.raw_price_retention_blocks,
                prune_interval_blocks: self.sequencer.config.history_prune_interval_blocks,
                prune_max_rows: self.sequencer.config.history_prune_max_rows,
            };
            if policy.should_prune_at(height) {
                match store.prune_history(height, policy).await {
                    Ok(report) => {
                        metrics::counter!(
                            "sybil_history_pruned_rows_total",
                            "stream" => "blocks_full"
                        )
                        .increment(report.blocks_full_pruned as u64);
                        metrics::counter!(
                            "sybil_history_pruned_rows_total",
                            "stream" => "price_points"
                        )
                        .increment(report.price_points_pruned as u64);
                        if let Some(min_height) = report.meta.blocks_full_min_height {
                            metrics::gauge!(
                                "sybil_history_retention_min_height",
                                "stream" => "blocks_full"
                            )
                            .set(min_height as f64);
                        }
                        if let Some(min_height) = report.meta.price_points_min_height {
                            metrics::gauge!(
                                "sybil_history_retention_min_height",
                                "stream" => "price_points"
                            )
                            .set(min_height as f64);
                        }
                    }
                    Err(error) => {
                        metrics::counter!("sybil_history_prune_failures_total").increment(1);
                        tracing::warn!(height, %error, "history pruning failed after block save");
                    }
                }
            }
        }
        Ok(())
    }

    fn record_metrics(&self, bp: &BlockProduction, pending_bundles_before: usize) {
        metrics::counter!("sybil_blocks_produced").increment(1);
        metrics::gauge!("sybil_block_height").set(bp.block.header.height as f64);
        metrics::histogram!("sybil_orders_per_block").record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_batch_orders_per_block")
            .record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_fresh_submissions_per_block")
            .record(bp.flow_metrics.fresh_submissions as f64);
        metrics::histogram!("sybil_fresh_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_received as f64);
        metrics::histogram!("sybil_carried_resting_orders_per_block")
            .record(bp.flow_metrics.carried_resting_orders as f64);
        metrics::histogram!("sybil_fresh_accepted_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_accepted as f64);
        metrics::histogram!("sybil_rejections_per_block")
            .record(bp.flow_metrics.rejected_orders as f64);
        metrics::histogram!("sybil_fills_per_block").record(bp.block.header.fill_count as f64);
        metrics::gauge!("sybil_welfare_nanos").set(bp.analytics.total_welfare as f64);
        metrics::gauge!("sybil_volume_nanos").set(bp.analytics.total_volume as f64);
        metrics::gauge!("sybil_pending_bundles").set(pending_bundles_before as f64);
        metrics::gauge!("sybil_pending_orders").set(bp.flow_metrics.pending_orders_after as f64);
        metrics::histogram!("sybil_solve_time_seconds").record(bp.pipeline.total_time_secs);
        metrics::gauge!("sybil_recent_block_history_len").set(self.block_history.len() as f64);

        let analytics = self.sequencer.analytics().memory_stats();
        metrics::gauge!("sybil_analytics_equity_known_accounts")
            .set(analytics.equity_known_accounts as f64);
        metrics::gauge!("sybil_analytics_equity_cached_accounts")
            .set(analytics.equity_cached_accounts as f64);
        metrics::gauge!("sybil_analytics_equity_cached_points")
            .set(analytics.equity_cached_points as f64);
        metrics::gauge!("sybil_analytics_equity_pending_points")
            .set(analytics.equity_pending_points as f64);
        metrics::gauge!("sybil_analytics_equity_points_per_account_capacity")
            .set(analytics.equity_points_per_account_capacity as f64);
        metrics::gauge!("sybil_analytics_history_cached_accounts")
            .set(analytics.history_cached_accounts as f64);
        metrics::gauge!("sybil_analytics_history_cached_events")
            .set(analytics.history_cached_events as f64);
        metrics::gauge!("sybil_analytics_history_pending_events")
            .set(analytics.history_pending_events as f64);
        metrics::gauge!("sybil_analytics_history_events_per_account_capacity")
            .set(analytics.history_events_per_account_capacity as f64);
        metrics::gauge!("sybil_analytics_history_event_next_seq")
            .set(analytics.history_event_next_seq as f64);

        self.record_per_market_metrics(bp);
    }

    // Cardinality note: bounded by active markets this block (those with clearing
    // prices). Fine for MVP scale (tens of markets). Revisit top-N bucketing if
    // we ever exceed ~1000 concurrently active markets.
    fn record_per_market_metrics(&self, bp: &BlockProduction) {
        let order_to_market: HashMap<u64, MarketId> = bp
            .witness
            .orders
            .iter()
            .filter_map(|wo| wo.order.active_markets().next().map(|m| (wo.order.id, m)))
            .collect();

        let mut fills_per_market: HashMap<MarketId, u64> = HashMap::new();
        for fill in &bp.block.fills {
            if fill.fill_qty == matching_engine::Qty::ZERO {
                continue;
            }
            if let Some(&market_id) = order_to_market.get(&fill.order_id) {
                *fills_per_market.entry(market_id).or_default() += 1;
            }
        }
        for (market_id, count) in fills_per_market {
            metrics::counter!(
                "sybil_market_fills_total",
                "market_id" => market_id.0.to_string()
            )
            .increment(count);
        }

        let market_volumes = self.sequencer.analytics().market_volumes();
        for (market_id, prices) in &bp.block.clearing_prices {
            for (outcome, &price) in prices.iter().enumerate() {
                metrics::gauge!(
                    "sybil_market_clearing_price_nanos",
                    "market_id" => market_id.0.to_string(),
                    "outcome" => outcome.to_string()
                )
                .set(price.0 as f64);
            }
            if let Some(&volume) = market_volumes.get(market_id) {
                metrics::gauge!(
                    "sybil_market_volume_nanos",
                    "market_id" => market_id.0.to_string()
                )
                .set(volume as f64);
            }
        }
    }

    fn record_submission_metrics(
        &self,
        source: &'static str,
        order_count: usize,
        result: &Result<(), SequencerError>,
    ) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_submissions_total", "source" => source, "result" => outcome)
            .increment(1);
        metrics::counter!("sybil_orders_received_total", "source" => source, "result" => outcome)
            .increment(order_count as u64);
    }

    fn record_cancel_metrics(&self, source: &'static str, result: &Result<(), SequencerError>) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_cancels_total", "source" => source, "result" => outcome)
            .increment(1);
    }

    fn push_to_history(&mut self, block: SealedBlock) {
        if self.block_history.len() >= self.sequencer.config.block_history_capacity {
            self.block_history.pop_front();
        }
        self.block_history.push_back(block);
    }

    async fn handle_signed_order(&mut self, signed: SignedOrder) -> Result<(), SequencerError> {
        verify_signed_order(&signed)?;

        let account_id = self
            .sequencer
            .lookup_pubkey(&signed.signer)
            .ok_or(SequencerError::UnknownSigner)?;
        self.accept_replay_nonce(account_id, signed.nonce).await?;

        let submission = OrderSubmission {
            account_id,
            orders: vec![signed.order],
            mm_constraint: None,
        };

        self.admit_or_defer(submission).await
    }

    async fn handle_signed_cancel(&mut self, signed: SignedCancel) -> Result<(), SequencerError> {
        verify_signed_cancel(&signed)?;

        let account_id = self
            .sequencer
            .lookup_pubkey(&signed.signer)
            .ok_or(SequencerError::UnknownSigner)?;

        if account_id != signed.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        self.accept_replay_nonce(account_id, signed.nonce).await?;

        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.cancel_pending_order_at(signed.account_id, signed.order_id, timestamp_ms)?;
        self.persist_control_plane(&ControlPlaneCommand::CancelPendingOrder {
            account_id: signed.account_id,
            order_id: signed.order_id,
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .cancel_pending_order_at(signed.account_id, signed.order_id, timestamp_ms)
    }

    fn handle_search_markets(&self, query: MarketSearchQuery) -> Vec<MarketSearchResult> {
        let markets = self.sequencer.markets();
        let mut results: Vec<MarketSearchResult> = Vec::new();

        for market in markets.iter() {
            let mid = market.id;
            let metadata = self.sequencer.market_metadata(mid);
            let status = self.sequencer.market_status(mid);

            if let Some(ref status_filter) = query.status {
                if status.as_str() != status_filter.as_str() {
                    continue;
                }
            }

            if let Some(ref text) = query.text {
                let text_lower = text.to_lowercase();
                let name_matches = market.name.to_lowercase().contains(&text_lower);
                let desc_matches = metadata
                    .as_ref()
                    .map(|m| m.description.to_lowercase().contains(&text_lower))
                    .unwrap_or(false);
                if !name_matches && !desc_matches {
                    continue;
                }
            }

            if let Some(ref filter_tags) = query.tags {
                let has_match = metadata
                    .as_ref()
                    .map(|m| filter_tags.iter().any(|t| m.tags.contains(t)))
                    .unwrap_or(false);
                if !has_match {
                    continue;
                }
            }

            if let Some(ref cat) = query.category {
                let matches = metadata
                    .as_ref()
                    .map(|m| &m.category == cat)
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
            }

            let market_prices = self.sequencer.analytics().last_clearing_prices().get(&mid);
            let yes_price = market_prices.and_then(|p| p.first().copied());
            let no_price = market_prices.and_then(|p| p.get(1).copied());
            let volume = self.sequencer.analytics().market_volume(mid);

            if let Some(min_p) = query.min_yes_price {
                if yes_price.unwrap_or(Nanos::ZERO) < min_p {
                    continue;
                }
            }
            if let Some(max_p) = query.max_yes_price {
                if yes_price.unwrap_or(Nanos::ZERO) > max_p {
                    continue;
                }
            }

            if let Some(min_vol) = query.min_volume {
                if volume < min_vol {
                    continue;
                }
            }

            results.push(MarketSearchResult {
                market_id: mid,
                name: market.name.clone(),
                metadata: metadata.cloned(),
                yes_price_nanos: yes_price,
                no_price_nanos: no_price,
                volume_nanos: volume,
                status,
            });
        }

        if let Some(ref sort_field) = query.sort_by {
            match sort_field {
                crate::market_info::MarketSortField::Volume => {
                    results.sort_by(|a, b| b.volume_nanos.cmp(&a.volume_nanos));
                }
                crate::market_info::MarketSortField::CreatedAt => {
                    results.sort_by(|a, b| {
                        let a_ts = a.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        let b_ts = b.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        b_ts.cmp(&a_ts)
                    });
                }
                crate::market_info::MarketSortField::Name => {
                    results.sort_by(|a, b| a.name.cmp(&b.name));
                }
                crate::market_info::MarketSortField::Price => {
                    results.sort_by(|a, b| {
                        b.yes_price_nanos
                            .unwrap_or(Nanos::ZERO)
                            .cmp(&a.yes_price_nanos.unwrap_or(Nanos::ZERO))
                    });
                }
            }
        }

        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100);
        results.into_iter().skip(offset).take(limit).collect()
    }

    /// Admit a submission: fast path if it fits straight into the resting
    /// book (single-market, non-MM, single order), otherwise buffer it on
    /// the sequencer's pending queue. Either way the submission is durably
    /// logged before this returns `Ok`, so a crash before the next block
    /// commit doesn't drop anything acknowledged with a 200 OK. Returns
    /// `Err` for synchronous rejections so the caller can surface them to
    /// the client.
    async fn admit_or_defer(&mut self, submission: OrderSubmission) -> Result<(), SequencerError> {
        self.check_account_submission_limits(&submission)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match self.sequencer.try_admit_direct(submission, now_ms) {
            crate::sequencer::AdmitOutcome::Admitted {
                order_id,
                resting_order,
            } => {
                if let Some(store) = &self.store {
                    if let Err(err) = store.append_admit_log(&resting_order).await {
                        // Durability lost — rollback the in-memory admit so
                        // the 200 OK contract holds. If cancel somehow fails
                        // (shouldn't: we just pushed the order), log loudly
                        // and leave the order in-book as a degraded state.
                        if let Err(cancel_err) = self
                            .sequencer
                            .cancel_pending_order(resting_order.account_id, order_id)
                        {
                            tracing::error!(
                                error = %cancel_err,
                                order_id,
                                "admit-log persist failed and rollback could not cancel the order"
                            );
                        }
                        return Err(SequencerError::Persistence(err.to_string()));
                    }
                }
                // Trader tracker hook for the try_admit_direct path. Fires
                // only after durability succeeds so the rolling counts only
                // reflect orders that durably landed (cancellation on store
                // failure walks back the in-memory admit but not the
                // tracker — acceptable: we still served the place).
                // try_admit_direct only accepts non-MM single-market
                // submissions, so `is_mm = false`.
                let markets: Vec<MarketId> = resting_order.order.active_markets().collect();
                self.sequencer.record_trader_placement_analytics(
                    resting_order.account_id,
                    markets.clone(),
                    now_ms,
                    false,
                );
                {
                    use crate::aggregates::{HistoryEvent, HistoryKind};
                    let o = &resting_order.order;
                    let mut e = HistoryEvent::new(
                        resting_order.account_id,
                        HistoryKind::Placed,
                        self.sequencer.height(),
                        now_ms,
                    );
                    e.order_id = Some(order_id);
                    e.market_id = o.active_markets().next();
                    e.qty = Some(o.max_fill.0);
                    e.price_nanos = Some(o.limit_price.0);
                    let (side, outcome) = crate::aggregates::side_outcome_from_order(o);
                    e.side = side;
                    e.outcome = outcome;
                    self.sequencer.record_history(e);
                }
                Ok(())
            }
            crate::sequencer::AdmitOutcome::Deferred(sub) => {
                self.check_deferred_submission_limits(&sub)?;
                if let Some(store) = &self.store {
                    store
                        .append_pending_bundle(&sub)
                        .await
                        .map_err(|err| SequencerError::Persistence(err.to_string()))?;
                }
                self.sequencer.push_pending_bundle(sub);
                Ok(())
            }
            crate::sequencer::AdmitOutcome::Rejected(err) => Err(err),
        }
    }

    fn check_global_submission_rate(&mut self) -> Result<(), SequencerError> {
        let now = Instant::now();
        self.global_submission_bucket
            .allow(now)
            .map_err(|retry_after_secs| {
                metrics::counter!(
                    "sybil_admission_limit_rejections_total",
                    "limit" => "global_rate"
                )
                .increment(1);
                SequencerError::RateLimited { retry_after_secs }
            })
    }

    fn check_account_submission_limits(
        &mut self,
        submission: &OrderSubmission,
    ) -> Result<(), SequencerError> {
        let config = &self.sequencer.config;
        if self.sequencer.accounts.get(submission.account_id).is_none() {
            return Err(SequencerError::Rejected(crate::error::Rejection {
                order_id: 0,
                account_id: submission.account_id,
                reason: crate::error::RejectionReason::AccountNotFound,
            }));
        }

        let order_count = submission.orders.len();
        if order_count > config.max_orders_per_submission {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "orders_per_submission"
            )
            .increment(1);
            return Err(SequencerError::TooManyOrdersInSubmission {
                count: order_count,
                limit: config.max_orders_per_submission,
            });
        }

        let now = Instant::now();
        let bucket = self
            .account_submission_buckets
            .entry(submission.account_id)
            .or_insert_with(|| {
                TokenBucket::new(
                    config.max_submissions_per_account_per_second,
                    config.submission_burst_per_account,
                    now,
                )
            });
        bucket.allow(now).map_err(|retry_after_secs| {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "account_rate"
            )
            .increment(1);
            SequencerError::RateLimited { retry_after_secs }
        })?;

        if submission.mm_constraint.is_none() {
            let open_orders = self
                .sequencer
                .open_orders_for_account(submission.account_id);
            let staged_orders = self
                .sequencer
                .pending_non_mm_orders_for_account(submission.account_id);
            if open_orders + staged_orders + order_count > config.max_open_orders_per_account {
                metrics::counter!(
                    "sybil_admission_limit_rejections_total",
                    "limit" => "open_orders_per_account"
                )
                .increment(1);
                return Err(SequencerError::TooManyOpenOrders {
                    account_id: submission.account_id,
                    limit: config.max_open_orders_per_account,
                });
            }
        }

        Ok(())
    }

    fn check_deferred_submission_limits(
        &self,
        submission: &OrderSubmission,
    ) -> Result<(), SequencerError> {
        let config = &self.sequencer.config;
        if self.sequencer.pending_bundles_len() >= config.max_pending_bundles {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "pending_bundles_total"
            )
            .increment(1);
            return Err(SequencerError::MempoolFull);
        }

        if self
            .sequencer
            .pending_bundles_for_account(submission.account_id)
            >= config.max_pending_bundles_per_account
        {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "pending_bundles_per_account"
            )
            .increment(1);
            return Err(SequencerError::TooManyPendingBundles {
                account_id: submission.account_id,
                limit: config.max_pending_bundles_per_account,
            });
        }

        Ok(())
    }

    async fn persist_control_plane(
        &self,
        command: &crate::store::ControlPlaneCommand,
    ) -> Result<(), SequencerError> {
        if let Some(store) = &self.store {
            store
                .append_control_plane_command(command)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        Ok(())
    }

    async fn accept_replay_nonce(
        &mut self,
        account_id: AccountId,
        nonce: u64,
    ) -> Result<(), SequencerError> {
        self.sequencer.validate_replay_nonce(account_id, nonce)?;
        self.persist_control_plane(&ControlPlaneCommand::AdvanceReplayNonce { account_id, nonce })
            .await?;
        self.sequencer.advance_replay_nonce(account_id, nonce)
    }

    async fn handle_create_account(
        &mut self,
        initial_balance: i64,
    ) -> Result<Account, SequencerError> {
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::CreateAccountAt {
            initial_balance,
            timestamp_ms,
        })
        .await?;
        let account_id = self
            .sequencer
            .create_account_at(initial_balance, timestamp_ms);
        Ok(self
            .sequencer
            .accounts
            .get(account_id)
            .cloned()
            .expect("created account should exist"))
    }

    async fn handle_fund_account(
        &mut self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        if self.sequencer.accounts.get(account_id).is_none() {
            return self.sequencer.fund_account(account_id, amount);
        }
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::FundAccount {
            account_id,
            amount,
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .fund_account_at(account_id, amount, timestamp_ms)
    }

    async fn handle_register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        if self.sequencer.accounts.get(account_id).is_none() {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        }
        if self.sequencer.lookup_pubkey(&pubkey).is_some() {
            return Err(SequencerError::AccountAlreadyRegistered);
        }
        self.persist_control_plane(&ControlPlaneCommand::RegisterPubkey {
            account_id,
            compressed_pubkey: pubkey.compressed_bytes(),
        })
        .await?;
        self.sequencer.register_pubkey(account_id, pubkey)
    }

    async fn handle_create_market(&mut self, name: String) -> Result<MarketId, SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarket { name: name.clone() })
            .await?;
        Ok(self.sequencer.create_market(name))
    }

    async fn handle_create_market_with_metadata(
        &mut self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarketWithMetadata {
            name: name.clone(),
            metadata: metadata.clone(),
        })
        .await?;
        Ok(self.sequencer.create_market_with_metadata(name, metadata))
    }

    async fn handle_create_market_group(
        &mut self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<(u64, MarketGroup), SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarketGroup {
            name: name.clone(),
            market_ids: market_ids.clone(),
        })
        .await?;
        Ok(self.sequencer.create_market_group(name, market_ids))
    }

    async fn handle_extend_market_group(
        &mut self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(MarketGroup, bool), SequencerError> {
        let mut validation = self.sequencer.clone();
        validation.extend_market_group(group_id, market_id)?;
        self.persist_control_plane(&ControlPlaneCommand::ExtendMarketGroup {
            group_id,
            market_id,
        })
        .await?;
        self.sequencer.extend_market_group(group_id, market_id)
    }

    async fn handle_resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.resolve_market(market_id, payout_nanos, timestamp_ms)?;
        self.persist_control_plane(&ControlPlaneCommand::ResolveMarket {
            market_id,
            payout_nanos,
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .resolve_market(market_id, payout_nanos, timestamp_ms)
    }

    async fn handle_resolve_market_attested(
        &mut self,
        market_id: MarketId,
        signed: SignedAttestation,
    ) -> Result<ResolutionRecord, SequencerError> {
        crate::crypto::verify_signed_attestation(&signed)?;
        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.resolve_market_attested(market_id, &signed, timestamp_ms)?;
        self.persist_control_plane(&ControlPlaneCommand::ResolveMarketAttested {
            market_id,
            signed: signed.clone(),
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .resolve_market_attested(market_id, &signed, timestamp_ms)
    }

    async fn handle_register_feed(
        &mut self,
        pubkey: FeedPubkey,
        name: String,
    ) -> Result<FeedId, SequencerError> {
        if let Some(feed) = self.sequencer.feed_by_pubkey(&pubkey) {
            return Ok(feed.id);
        }
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::RegisterFeed {
            pubkey: pubkey.clone(),
            name: name.clone(),
            timestamp_ms,
        })
        .await?;
        Ok(self.sequencer.register_feed(pubkey, name, timestamp_ms))
    }

    async fn handle_install_template(
        &mut self,
        template: sybil_oracle::ResolutionTemplate,
    ) -> Result<(), SequencerError> {
        if self
            .sequencer
            .market_lifecycle()
            .templates()
            .get(&template.id)
            .is_some_and(|existing| existing == &template)
        {
            return Ok(());
        }
        self.persist_control_plane(&ControlPlaneCommand::InstallTemplate {
            template: template.clone(),
        })
        .await?;
        self.sequencer.install_template(template);
        Ok(())
    }

    async fn handle_l1_deposit(&mut self, deposit: L1Deposit) -> Result<Account, SequencerError> {
        self.sequencer.validate_l1_deposit(&deposit)?;
        if let Some(store) = &self.store {
            store
                .append_pending_l1_deposit(&deposit)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer.ingest_l1_deposit(deposit)
    }

    async fn handle_bridge_withdrawal(
        &mut self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.sequencer.validate_bridge_withdrawal(&request)?;
        if let Some(store) = &self.store {
            store
                .append_pending_bridge_withdrawal(&request)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer.request_bridge_withdrawal(request)
    }

    async fn handle_signed_bridge_withdrawal(
        &mut self,
        signed: SignedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        verify_signed_bridge_withdrawal(&signed)?;
        let account_id = self
            .sequencer
            .lookup_pubkey(&signed.signer)
            .ok_or(SequencerError::UnknownSigner)?;
        if account_id != signed.request.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        self.accept_replay_nonce(account_id, signed.nonce).await?;
        self.handle_bridge_withdrawal(signed.request).await
    }

    async fn handle_state_proof(
        &self,
        leaf_key: Vec<u8>,
    ) -> Result<SequencerStateProof, SequencerError> {
        if leaf_key.len() > QMDB_STATE_MAX_KEY_BYTES {
            return Err(SequencerError::ProofUnavailable(format!(
                "state leaf key exceeds {QMDB_STATE_MAX_KEY_BYTES} bytes"
            )));
        }

        let Some(store) = &self.store else {
            return Err(SequencerError::ProofUnavailable(
                "state proofs require a persistent store".to_string(),
            ));
        };

        let root = store
            .current_state_qmdb_root()
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .ok_or(SequencerError::BlockNotFound)?;

        if let Some(proof) = store
            .current_state_qmdb_leaf_proof(&leaf_key)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
        {
            if proof.root != root.root || proof.slot != root.slot {
                return Err(SequencerError::Persistence(
                    "state proof root does not match committed qMDB root".to_string(),
                ));
            }
            return Ok(SequencerStateProof {
                block_height: self.sequencer.height(),
                state_root: proof.root,
                slot: proof.slot,
                leaf_key: proof.leaf_key.clone(),
                verified: proof.verify(),
                kind: SequencerStateProofKind::Inclusion {
                    leaf_value: proof.leaf_value.clone(),
                    proof: proof.proof_parts(),
                },
            });
        }

        let proof = store
            .current_state_qmdb_leaf_exclusion_proof(&leaf_key)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                SequencerError::Persistence(
                    "state qmdb returned neither inclusion nor exclusion proof".to_string(),
                )
            })?;
        if proof.root != root.root || proof.slot != root.slot {
            return Err(SequencerError::Persistence(
                "state proof root does not match committed qMDB root".to_string(),
            ));
        }

        Ok(SequencerStateProof {
            block_height: self.sequencer.height(),
            state_root: root.root,
            slot: proof.slot,
            leaf_key: proof.leaf_key.clone(),
            verified: proof.verify(),
            kind: SequencerStateProofKind::Exclusion {
                proof: proof.proof_parts(),
            },
        })
    }
}

#[ractor::async_trait]
impl Actor for SequencerActor {
    type Msg = SequencerMsg;
    type State = SequencerActorState;
    type Arguments = SequencerActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let now = Instant::now();
        let global_submission_bucket = TokenBucket::new(
            args.sequencer.config.max_global_submissions_per_second,
            args.sequencer.config.global_submission_burst,
            now,
        );
        Ok(SequencerActorState {
            sequencer: args.sequencer,
            latest_block: None,
            block_history: VecDeque::new(),
            block_broadcast: args.block_broadcast,
            pause_count: 0,
            halted_error: None,
            store: args.store,
            global_submission_bucket,
            account_submission_buckets: HashMap::new(),
            mailbox_monitor: args.mailbox_monitor,
            indicative_cache: HashMap::new(),
            indicative_solve_gate: IndicativeSolveGate::default(),
            #[cfg(test)]
            next_tick_hold: None,
        })
    }

    async fn post_start(
        &self,
        myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let actor = myself.clone();
        let block_interval = state.sequencer.config.block_interval;
        let mailbox_monitor = state.mailbox_monitor.clone();
        tokio::spawn(async move {
            let mut ticker = interval_at(Instant::now() + block_interval, block_interval);
            loop {
                ticker.tick().await;
                mailbox_monitor.queued();
                if actor.send_message(SequencerMsg::Tick).is_err() {
                    mailbox_monitor.send_failed();
                    break;
                }
            }
        });

        // Indicative scheduler (C2). Separate timer task, NOT an idle
        // branch in on_tick — block production and indicative refresh are
        // decoupled. Cadence chosen well under one block period so the
        // open-batch snapshot refreshes mid-batch.
        let actor_indicative = myself.clone();
        let mailbox_indicative = state.mailbox_monitor.clone();
        let indicative_interval = std::time::Duration::from_millis(750);
        tokio::spawn(async move {
            let mut ticker = interval_at(Instant::now() + indicative_interval, indicative_interval);
            loop {
                ticker.tick().await;
                mailbox_indicative.queued();
                if actor_indicative
                    .send_message(SequencerMsg::IndicativeTick)
                    .is_err()
                {
                    mailbox_indicative.send_failed();
                    break;
                }
            }
        });
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.mailbox_monitor.started();
        match message {
            SequencerMsg::Tick => {
                let _ = state.on_tick().await?;
            }
            #[cfg(test)]
            SequencerMsg::TestCrashOnNextBlock(crashpoint) => {
                let _ = state.on_tick_inner(Some(crashpoint)).await?;
            }
            #[cfg(test)]
            SequencerMsg::TestHoldNextTick(hold, reply) => {
                state.next_tick_hold = Some(hold);
                let _ = reply.send(());
            }
            SequencerMsg::IndicativeTick => {
                state.on_indicative_tick(myself.clone());
            }
            SequencerMsg::IndicativeUpdate(snapshots) => {
                state.indicative_cache = snapshots;
                state.indicative_solve_gate.finish();
            }
            SequencerMsg::IndicativeSolveFailed { solver, error } => {
                metrics::counter!(
                    "sybil_indicative_solve_failures_total",
                    "solver" => solver.clone(),
                    "reason" => "panic"
                )
                .increment(1);
                tracing::error!(
                    solver = %solver,
                    error = %error,
                    "indicative shadow solve failed; releasing gate"
                );
                state.indicative_solve_gate.finish();
            }
            SequencerMsg::Query(query) => {
                query.execute(state);
            }
            SequencerMsg::SubmitOrder(submission, reply) => {
                let order_count = submission.orders.len();
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.admit_or_defer(submission).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("unsigned", order_count, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitSignedOrder(signed, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_signed_order(signed).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("signed", 1, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::CancelSignedOrder(signed, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_signed_cancel(signed).await,
                    Err(err) => Err(err),
                };
                state.record_cancel_metrics("signed", &result);
                let _ = reply.send(result);
            }
            SequencerMsg::GetStateProof(leaf_key, reply) => {
                let _ = reply.send(state.handle_state_proof(leaf_key).await);
            }
            SequencerMsg::ProduceBlock(reply) => {
                let result = match state.on_tick().await? {
                    BlockTickOutcome::Produced(block) => Ok(*block),
                    BlockTickOutcome::Paused => Err(SequencerError::BlockProductionPaused),
                    BlockTickOutcome::Halted(error) | BlockTickOutcome::PersistFailed(error) => {
                        Err(error)
                    }
                };
                let _ = reply.send(result);
            }
            SequencerMsg::CreateAccount(initial_balance, reply) => {
                let _ = reply.send(state.handle_create_account(initial_balance).await);
            }
            SequencerMsg::FundAccount(account_id, amount, reply) => {
                let _ = reply.send(state.handle_fund_account(account_id, amount).await);
            }
            SequencerMsg::SubmitL1Deposit(deposit, reply) => {
                let _ = reply.send(state.handle_l1_deposit(deposit).await);
            }
            SequencerMsg::CreateBridgeWithdrawal(request, reply) => {
                let _ = reply.send(state.handle_bridge_withdrawal(request).await);
            }
            SequencerMsg::CreateSignedBridgeWithdrawal(signed, reply) => {
                let _ = reply.send(state.handle_signed_bridge_withdrawal(signed).await);
            }
            SequencerMsg::RegisterPubkey(account_id, pubkey, reply) => {
                let _ = reply.send(state.handle_register_pubkey(account_id, pubkey).await);
            }
            SequencerMsg::CreateMarket(name, reply) => {
                let _ = reply.send(state.handle_create_market(name).await);
            }
            SequencerMsg::CreateMarketGroup(name, market_ids, reply) => {
                let _ = reply.send(state.handle_create_market_group(name, market_ids).await);
            }
            SequencerMsg::ExtendMarketGroup(group_id, market_id, reply) => {
                let _ = reply.send(state.handle_extend_market_group(group_id, market_id).await);
            }
            SequencerMsg::ResolveMarket(market_id, payout_nanos, reply) => {
                let _ = reply.send(state.handle_resolve_market(market_id, payout_nanos).await);
            }
            SequencerMsg::ResolveMarketAttested(market_id, signed, reply) => {
                let _ = reply.send(
                    state
                        .handle_resolve_market_attested(market_id, signed)
                        .await,
                );
            }
            SequencerMsg::RegisterFeed(pubkey, name, reply) => {
                let _ = reply.send(state.handle_register_feed(pubkey, name).await);
            }
            SequencerMsg::InstallTemplate(template, reply) => {
                let _ = reply.send(state.handle_install_template(template).await);
            }
            SequencerMsg::GetBlock(height, reply) => {
                let block = state
                    .block_history
                    .iter()
                    .find(|b| b.canonical.header.height == height)
                    .cloned();
                let result = match block {
                    Some(block) => Ok(block),
                    None => match &state.store {
                        Some(store) => match store.load_block(height).await {
                            Ok(Some(block)) => Ok(block),
                            Ok(None) => match store.history_retention_meta() {
                                Ok(meta) => {
                                    if let Some(retention_min_height) = meta.blocks_full_min_height
                                    {
                                        if height < retention_min_height {
                                            Err(SequencerError::BlockPruned {
                                                requested_height: height,
                                                retention_min_height,
                                            })
                                        } else {
                                            Err(SequencerError::BlockNotFound)
                                        }
                                    } else {
                                        Err(SequencerError::BlockNotFound)
                                    }
                                }
                                Err(error) => Err(SequencerError::Persistence(error.to_string())),
                            },
                            Err(error) => Err(SequencerError::Persistence(error.to_string())),
                        },
                        None => Err(SequencerError::BlockNotFound),
                    },
                };
                let _ = reply.send(result);
            }
            SequencerMsg::CreateMarketWithMetadata(name, metadata, reply) => {
                let _ = reply.send(
                    state
                        .handle_create_market_with_metadata(name, metadata)
                        .await,
                );
            }
            SequencerMsg::GetPriceHistory(
                market_id,
                from_ms,
                to_ms,
                before_height,
                limit,
                reply,
            ) => {
                let limit = limit.min(MAX_PRICE_HISTORY_QUERY_POINTS);
                let result = match &state.store {
                    Some(store) => store
                        .load_price_history(market_id, from_ms, to_ms, before_height, limit)
                        .await
                        .map_err(|error| SequencerError::Persistence(error.to_string())),
                    None => Ok(limit_price_point_page(
                        state
                            .sequencer
                            .analytics()
                            .price_history(market_id, from_ms, to_ms),
                        before_height,
                        limit,
                    )),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetPriceCandles(
                market_id,
                resolution_secs,
                from_ms,
                to_ms,
                before_ms,
                limit,
                reply,
            ) => {
                let limit = limit.min(MAX_PRICE_HISTORY_QUERY_POINTS);
                let result = match &state.store {
                    Some(store) => store
                        .load_price_candles(
                            market_id,
                            resolution_secs,
                            from_ms,
                            to_ms,
                            before_ms,
                            limit,
                        )
                        .await
                        .map_err(|error| SequencerError::Persistence(error.to_string())),
                    None => Ok(price_candle_page_from_points(
                        state
                            .sequencer
                            .analytics()
                            .price_history(market_id, from_ms, to_ms),
                        resolution_secs,
                        from_ms,
                        to_ms,
                        before_ms,
                        limit,
                    )),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetAccountFills(account_id, market_id, limit, offset, reply) => {
                // Serve from the durable store (full persisted history); the
                // in-memory recorder is a bounded window that's empty under prod
                // retention caps. Fall back to memory on read error or when no
                // store is configured. Mirrors GetEquitySeries / GetAccountEvents.
                let result = match &state.store {
                    Some(store) => store
                        .account_fills(account_id, market_id, limit, offset)
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, account_id = account_id.0, "account_fills read failed; falling back to memory");
                            state
                                .sequencer
                                .analytics()
                                .account_fills(account_id, market_id, limit, offset)
                        }),
                    None => state
                        .sequencer
                        .analytics()
                        .account_fills(account_id, market_id, limit, offset),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetAccountFillsAfter(account_id, market_id, after, limit, reply) => {
                let result = match &state.store {
                    Some(store) => store
                        .account_fills_after(account_id, market_id, after, limit)
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, account_id = account_id.0, "account_fills_after read failed; falling back to memory");
                            state
                                .sequencer
                                .analytics()
                                .account_fills_after(account_id, market_id, after, limit)
                        }),
                    None => state
                        .sequencer
                        .analytics()
                        .account_fills_after(account_id, market_id, after, limit),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetEquitySeries(account_id, since_ms, reply) => {
                // NOTE: in prod the in-memory caps are 0, so this fallback returns
                // an empty series. A persistent store read error therefore surfaces
                // as an empty (200 OK) response plus the warn! below — not an error.
                // The `since_ms` range is pushed into the store scan; the in-memory
                // fallback re-applies it so both paths return the same window.
                let result = match &state.store {
                    Some(store) => store.equity_series(account_id, since_ms).unwrap_or_else(|e| {
                        tracing::warn!(error = %e, account_id = account_id.0, "equity_series read failed; falling back to memory");
                        state
                            .sequencer
                            .analytics()
                            .equity_series(account_id)
                            .into_iter()
                            .filter(|point| point.timestamp_ms >= since_ms)
                            .collect()
                    }),
                    None => state
                        .sequencer
                        .analytics()
                        .equity_series(account_id)
                        .into_iter()
                        .filter(|point| point.timestamp_ms >= since_ms)
                        .collect(),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply) => {
                let result = match &state.store {
                    Some(store) => {
                        match store.account_events(account_id, limit, before, category.clone()) {
                            Ok(mut events) => {
                                events.extend(state.sequencer.analytics().pending_account_history(
                                    account_id,
                                    before,
                                    category.as_deref(),
                                ));
                                events.sort_by(|a, b| {
                                    (b.block_height, b.seq).cmp(&(a.block_height, a.seq))
                                });
                                events.dedup_by_key(|e| (e.account_id.0, e.block_height, e.seq));
                                events.truncate(limit);
                                events
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, account_id = account_id.0, "account_events read failed; falling back to memory");
                                state.sequencer.analytics().account_history(
                                    account_id,
                                    limit,
                                    before,
                                    category.as_deref(),
                                )
                            }
                        }
                    }
                    None => state.sequencer.analytics().account_history(
                        account_id,
                        limit,
                        before,
                        category.as_deref(),
                    ),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::PauseBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_add(1);
                let _ = reply.send(());
            }
            SequencerMsg::ResumeBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_sub(1);
                let _ = reply.send(());
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SequencerHandleInner {
    actor: Arc<RwLock<Option<ActorRef<SequencerMsg>>>>,
    block_broadcast: broadcast::Sender<SealedBlock>,
    mailbox_monitor: MailboxMonitor,
    shutdown_requested: Arc<AtomicBool>,
}

impl SequencerHandleInner {
    fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        self.mailbox_monitor.reset();
        *self
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = actor;
    }
}

struct SequencerSupervisor;

struct SequencerSupervisorArgs {
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    oracle: Arc<dyn Oracle>,
    handle: SequencerHandleInner,
}

struct SequencerSupervisorState {
    current_actor: Option<ActorRef<SequencerMsg>>,
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    oracle: Arc<dyn Oracle>,
    handle: SequencerHandleInner,
}

enum SequencerSupervisorMsg {
    AdoptChild(ActorRef<SequencerMsg>),
}

impl SequencerSupervisorState {
    fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        self.handle.publish_actor(actor);
    }

    async fn spawn_child(
        &mut self,
        myself: ActorRef<SequencerSupervisorMsg>,
        sequencer: BlockSequencer,
    ) -> Result<(), ActorProcessingErr> {
        let args = SequencerActorArgs {
            sequencer,
            store: self.store.clone(),
            block_broadcast: self.handle.block_broadcast.clone(),
            mailbox_monitor: self.handle.mailbox_monitor.clone(),
        };
        let (child, _) =
            <SequencerActor as Actor>::spawn_linked(None, SequencerActor, args, myself.get_cell())
                .await
                .map_err(|error| ActorProcessingErr::from(error.to_string()))?;
        self.current_actor = Some(child.clone());
        self.publish_actor(Some(child));
        Ok(())
    }

    async fn restart_from_store(&mut self, myself: ActorRef<SequencerSupervisorMsg>) {
        self.current_actor = None;
        self.publish_actor(None);

        if self.handle.shutdown_requested.load(Ordering::Acquire) {
            return;
        }

        let Some(store) = self.store.clone() else {
            tracing::error!(
                "sequencer actor exited without a persistent store; restart unavailable"
            );
            return;
        };

        let restored = match store.load_state().await {
            Ok(state) => state,
            Err(error) => {
                tracing::error!(error = %error, "failed to load sequencer snapshot for restart");
                return;
            }
        };

        let Some(state) = restored else {
            tracing::error!("no persisted sequencer snapshot available for restart");
            return;
        };

        if self.handle.shutdown_requested.load(Ordering::Acquire) {
            return;
        }

        let sequencer = BlockSequencer::restore(state, self.oracle.clone(), self.config.clone());

        match self.spawn_child(myself, sequencer).await {
            Ok(()) => tracing::warn!("sequencer actor restarted from persistent snapshot"),
            Err(error) => {
                tracing::error!(error = %error, "failed to restart sequencer actor from snapshot");
            }
        }
    }
}

#[ractor::async_trait]
impl Actor for SequencerSupervisor {
    type Msg = SequencerSupervisorMsg;
    type State = SequencerSupervisorState;
    type Arguments = SequencerSupervisorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SequencerSupervisorState {
            current_actor: None,
            config: args.config,
            store: args.store,
            oracle: args.oracle,
            handle: args.handle,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SequencerSupervisorMsg::AdoptChild(actor) => {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    return Ok(());
                }
                state.current_actor = Some(actor.clone());
                state.publish_actor(Some(actor));
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let Some(current_actor) = state.current_actor.as_ref() else {
            return Ok(());
        };

        match message {
            SupervisionEvent::ActorStarted(actor) if actor.get_id() == current_actor.get_id() => {
                tracing::info!("sequencer actor started under supervisor");
            }
            SupervisionEvent::ActorFailed(actor, error)
                if actor.get_id() == current_actor.get_id() =>
            {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    tracing::warn!(
                        error = %error,
                        "sequencer actor failed during shutdown; not restarting"
                    );
                    state.current_actor = None;
                    state.publish_actor(None);
                    return Ok(());
                }
                tracing::error!(error = %error, "sequencer actor failed; attempting restart");
                state.restart_from_store(myself).await;
            }
            SupervisionEvent::ActorTerminated(actor, _, reason)
                if actor.get_id() == current_actor.get_id() =>
            {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    state.current_actor = None;
                    state.publish_actor(None);
                    tracing::info!("sequencer actor terminated during shutdown");
                    return Ok(());
                }
                if let Some(reason) = reason.as_deref() {
                    tracing::warn!(reason, "sequencer actor terminated; attempting restart");
                } else {
                    tracing::warn!("sequencer actor terminated; attempting restart");
                }
                state.restart_from_store(myself).await;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Cloneable handle to the sequencer actor.
#[derive(Clone)]
pub struct SequencerHandle {
    inner: SequencerHandleInner,
    supervisor: ActorRef<SequencerSupervisorMsg>,
}

impl SequencerHandle {
    async fn actor_ref(&self) -> Result<ActorRef<SequencerMsg>, SequencerError> {
        self.inner
            .actor
            .read()
            .expect("sequencer actor ref lock poisoned")
            .clone()
            .ok_or(SequencerError::ActorGone)
    }

    async fn rpc<T>(
        &self,
        build_message: impl FnOnce(RpcReplyPort<T>) -> SequencerMsg,
    ) -> Result<T, SequencerError>
    where
        T: Send + 'static,
    {
        let actor = self.actor_ref().await?;
        self.inner.mailbox_monitor.queued();
        match actor.call(build_message, None).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Err(_) => {
                self.inner.mailbox_monitor.send_failed();
                Err(SequencerError::ActorGone)
            }
            _ => Err(SequencerError::ActorGone),
        }
    }

    async fn read_query<T>(
        &self,
        query: impl FnOnce(&SequencerActorState) -> T + Send + 'static,
    ) -> Result<T, SequencerError>
    where
        T: Send + 'static,
    {
        self.rpc(|reply| SequencerMsg::Query(SequencerReadQuery::new(reply, query)))
            .await
    }

    /// Spawn with default config (1-second block interval).
    /// Prefer [`spawn_with_store`] for production use.
    pub fn spawn(sequencer: BlockSequencer) -> Self {
        Self::spawn_with_store(sequencer, None)
    }

    /// Spawn the sequencer actor with an attached persistent store.
    ///
    /// The store is used for:
    ///   1. Persisting each committed block's state (write-through on every block).
    ///   2. Reloading state when the supervisor restarts the actor after a crash.
    ///
    /// It is **not** used to hydrate the `sequencer` argument. The caller is
    /// responsible for calling [`Store::load_state`] and passing the result to
    /// [`BlockSequencer::restore`] before invoking this function — see
    /// `crates/sybil-api/src/main.rs` for the canonical hydration + spawn pattern.
    pub fn spawn_with_store(sequencer: BlockSequencer, store: Option<crate::store::Store>) -> Self {
        Self::spawn_with_store_arc(sequencer, store.map(Arc::new))
    }

    fn spawn_with_store_arc(
        sequencer: BlockSequencer,
        store: Option<Arc<crate::store::Store>>,
    ) -> Self {
        let oracle = sequencer.oracle();
        let config = sequencer.config.clone();
        let (block_broadcast, _) = broadcast::channel(64);
        let mailbox_monitor = MailboxMonitor::new(
            SEQUENCER_ACTOR_METRIC_NAME,
            config.actor_queue_warn_depth,
            config.actor_queue_error_depth,
        );
        let inner = SequencerHandleInner {
            actor: Arc::new(RwLock::new(None)),
            block_broadcast: block_broadcast.clone(),
            mailbox_monitor,
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        };
        let supervisor_args = SequencerSupervisorArgs {
            config,
            store: store.clone(),
            oracle,
            handle: inner.clone(),
        };
        let (supervisor, _) =
            ractor::ActorRuntime::spawn_instant(None, SequencerSupervisor, supervisor_args)
                .expect("failed to spawn sequencer supervisor");
        let actor_args = SequencerActorArgs {
            sequencer,
            store,
            block_broadcast,
            mailbox_monitor: inner.mailbox_monitor.clone(),
        };
        let (child, _) = ractor::ActorRuntime::spawn_linked_instant(
            None,
            SequencerActor,
            actor_args,
            supervisor.get_cell(),
        )
        .expect("failed to spawn sequencer actor");
        *inner
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = Some(child.clone());
        supervisor
            .send_message(SequencerSupervisorMsg::AdoptChild(child))
            .expect("failed to hand child actor to supervisor");
        Self { inner, supervisor }
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_store_arc_for_test(
        sequencer: BlockSequencer,
        store: Arc<crate::store::Store>,
    ) -> Self {
        Self::spawn_with_store_arc(sequencer, Some(store))
    }

    #[cfg(test)]
    async fn wait_for_actor_restart_for_test(
        &self,
        old_id: ractor::ActorId,
    ) -> Result<(), SequencerError> {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let actor = self
                .inner
                .actor
                .read()
                .expect("sequencer actor ref lock poisoned")
                .clone();
            if let Some(actor) = actor {
                if actor.get_id() != old_id {
                    return Ok(());
                }
            }

            if Instant::now() >= deadline {
                return Err(SequencerError::ActorGone);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn crash_actor_for_test(&self) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        let old_id = actor.get_id();
        actor
            .kill_and_wait(Some(std::time::Duration::from_secs(5)))
            .await
            .map_err(|error| {
                SequencerError::Persistence(format!("actor test kill failed: {error}"))
            })?;
        self.wait_for_actor_restart_for_test(old_id).await
    }

    #[cfg(test)]
    pub(crate) async fn produce_block_and_crash_for_test(
        &self,
        crashpoint: SequencerTestCrashpoint,
    ) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        let old_id = actor.get_id();
        self.inner.mailbox_monitor.queued();
        if actor
            .send_message(SequencerMsg::TestCrashOnNextBlock(crashpoint))
            .is_err()
        {
            self.inner.mailbox_monitor.send_failed();
            return Err(SequencerError::ActorGone);
        }
        self.wait_for_actor_restart_for_test(old_id).await
    }

    #[cfg(test)]
    async fn hold_next_tick_for_test(
        &self,
        hold: SequencerTestTickHold,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::TestHoldNextTick(hold, reply))
            .await
    }

    #[cfg(test)]
    async fn send_tick_for_test(&self) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        self.inner.mailbox_monitor.queued();
        if actor.send_message(SequencerMsg::Tick).is_err() {
            self.inner.mailbox_monitor.send_failed();
            return Err(SequencerError::ActorGone);
        }
        Ok(())
    }

    /// Stop the sequencer actor and supervisor, waiting up to `timeout`.
    ///
    /// This uses ractor's graceful `stop_and_wait`, so the actor finishes the
    /// message it is currently handling before shutdown completes. It does not
    /// drain queued future messages.
    pub async fn stop_and_wait(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        self.inner.shutdown_requested.store(true, Ordering::Release);

        let actor = self
            .inner
            .actor
            .read()
            .expect("sequencer actor ref lock poisoned")
            .clone();

        if let Some(actor) = actor {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }

            match actor
                .stop_and_wait(Some("shutdown".to_string()), Some(remaining))
                .await
            {
                Ok(()) => self.inner.publish_actor(None),
                Err(ractor::RactorErr::Timeout) => return false,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "sequencer actor stop returned an error; continuing shutdown"
                    );
                    self.inner.publish_actor(None);
                }
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }

        match self
            .supervisor
            .stop_and_wait(Some("shutdown".to_string()), Some(remaining))
            .await
        {
            Ok(()) => true,
            Err(ractor::RactorErr::Timeout) => false,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "sequencer supervisor stop returned an error after actor shutdown"
                );
                true
            }
        }
    }

    pub async fn submit_order(&self, submission: OrderSubmission) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitOrder(submission, reply))
            .await?
    }

    pub async fn submit_signed_order(&self, signed: SignedOrder) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitSignedOrder(signed, reply))
            .await?
    }

    pub async fn cancel_signed_order(&self, signed: SignedCancel) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelSignedOrder(signed, reply))
            .await?
    }

    pub async fn get_latest_block(&self) -> Result<Option<SealedBlock>, SequencerError> {
        self.read_query(|state| state.latest_block.clone()).await
    }

    pub async fn get_committed_height(&self) -> Result<Option<u64>, SequencerError> {
        self.read_query(|state| {
            let height = state.sequencer.height();
            (height > 0).then_some(height)
        })
        .await
    }

    pub async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Account>, SequencerError> {
        self.read_query(move |state| state.sequencer.accounts.get(account_id).cloned())
            .await
    }

    pub async fn get_state_root(&self) -> Result<[u8; 32], SequencerError> {
        self.read_query(|state| {
            crate::block::compute_complete_state_root(
                &state.sequencer.accounts,
                state.sequencer.bridge_state(),
                state.sequencer.order_book(),
                state.sequencer.markets(),
                state.sequencer.market_groups(),
                state.sequencer.market_lifecycle(),
            )
        })
        .await
    }

    pub async fn get_state_proof(
        &self,
        leaf_key: Vec<u8>,
    ) -> Result<SequencerStateProof, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetStateProof(leaf_key, reply))
            .await?
    }

    pub async fn produce_block(&self) -> Result<SealedBlock, SequencerError> {
        self.rpc(SequencerMsg::ProduceBlock).await?
    }

    pub async fn create_account(&self, initial_balance: i64) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateAccount(initial_balance, reply))
            .await?
    }

    pub async fn fund_account(
        &self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::FundAccount(account_id, amount, reply))
            .await?
    }

    pub async fn submit_l1_deposit(&self, deposit: L1Deposit) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitL1Deposit(deposit, reply))
            .await?
    }

    pub async fn create_bridge_withdrawal(
        &self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateBridgeWithdrawal(request, reply))
            .await?
    }

    pub async fn create_signed_bridge_withdrawal(
        &self,
        signed: SignedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateSignedBridgeWithdrawal(signed, reply))
            .await?
    }

    pub async fn get_bridge_state(&self) -> Result<BridgeState, SequencerError> {
        self.read_query(|state| state.sequencer.bridge_state().clone())
            .await
    }

    pub async fn get_bridge_account_key(
        &self,
        account_id: AccountId,
    ) -> Result<Option<[u8; 32]>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_key(account_id))
            .await
    }

    pub async fn get_bridge_account_id_by_key(
        &self,
        key: [u8; 32],
    ) -> Result<Option<AccountId>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_id_by_key(key))
            .await
    }

    pub async fn get_bridge_withdrawal(
        &self,
        withdrawal_id: u64,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_withdrawal(withdrawal_id).cloned())
            .await
    }

    pub async fn get_default_bridge_withdrawal_expiry(&self) -> Result<u64, SequencerError> {
        self.read_query(|state| state.sequencer.default_bridge_withdrawal_expiry_height())
            .await
    }

    pub async fn register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkey(account_id, pubkey, reply))
            .await?
    }

    #[tracing::instrument(skip_all)]
    pub async fn list_markets(&self) -> Result<MarketSet, SequencerError> {
        self.read_query(|state| state.sequencer.markets().clone())
            .await
    }

    pub async fn create_market(&self, name: String) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarket(name, reply))
            .await?
    }

    pub async fn create_market_group(
        &self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<(u64, MarketGroup), SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketGroup(name, market_ids, reply))
            .await?
    }

    pub async fn extend_market_group(
        &self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(MarketGroup, bool), SequencerError> {
        self.rpc(|reply| SequencerMsg::ExtendMarketGroup(group_id, market_id, reply))
            .await?
    }

    pub async fn list_market_groups(&self) -> Result<Vec<MarketGroup>, SequencerError> {
        self.read_query(|state| state.sequencer.market_groups().to_vec())
            .await
    }

    pub async fn resolve_market(
        &self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        self.rpc(|reply| SequencerMsg::ResolveMarket(market_id, payout_nanos, reply))
            .await?
    }

    pub async fn resolve_market_attested(
        &self,
        market_id: MarketId,
        signed: SignedAttestation,
    ) -> Result<ResolutionRecord, SequencerError> {
        self.rpc(|reply| SequencerMsg::ResolveMarketAttested(market_id, signed, reply))
            .await?
    }

    pub async fn register_feed(
        &self,
        pubkey: FeedPubkey,
        name: String,
    ) -> Result<FeedId, SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterFeed(pubkey, name, reply))
            .await?
    }

    pub async fn get_feed(&self, id: FeedId) -> Result<Option<DataFeed>, SequencerError> {
        self.read_query(move |state| state.sequencer.feed_by_id(id).cloned())
            .await
    }

    pub async fn get_feed_by_pubkey(
        &self,
        pubkey: FeedPubkey,
    ) -> Result<Option<DataFeed>, SequencerError> {
        self.read_query(move |state| state.sequencer.feed_by_pubkey(&pubkey).cloned())
            .await
    }

    pub async fn list_feeds(&self) -> Result<Vec<DataFeed>, SequencerError> {
        self.read_query(|state| state.sequencer.lifecycle.feeds().iter().cloned().collect())
            .await
    }

    pub async fn install_template(
        &self,
        template: sybil_oracle::ResolutionTemplate,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::InstallTemplate(template, reply))
            .await?
    }

    pub async fn template_exists(&self, id: String) -> Result<bool, SequencerError> {
        self.read_query(move |state| state.sequencer.template_exists(&id))
            .await
    }

    pub async fn get_market_status(
        &self,
        market_id: MarketId,
    ) -> Result<MarketStatus, SequencerError> {
        self.read_query(move |state| state.sequencer.market_status(market_id))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_statuses(
        &self,
    ) -> Result<HashMap<MarketId, MarketStatus>, SequencerError> {
        self.read_query(|state| state.sequencer.market_statuses().clone())
            .await
    }

    pub async fn get_block(&self, height: u64) -> Result<SealedBlock, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetBlock(height, reply))
            .await?
    }

    pub async fn get_recent_blocks(&self, n: usize) -> Result<Vec<SealedBlock>, SequencerError> {
        self.read_query(move |state| {
            let cap = state.sequencer.config.block_history_capacity;
            let take = n.min(cap);
            state
                .block_history
                .iter()
                .rev()
                .take(take)
                .cloned()
                .collect()
        })
        .await
    }

    pub async fn subscribe_blocks(
        &self,
    ) -> Result<broadcast::Receiver<SealedBlock>, SequencerError> {
        Ok(self.inner.block_broadcast.subscribe())
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_market_prices(&self) -> Result<HashMap<MarketId, Vec<Nanos>>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().last_clearing_prices().clone())
            .await
    }

    pub async fn get_portfolio(
        &self,
        account_id: AccountId,
    ) -> Result<PortfolioSummary, SequencerError> {
        self.read_query(move |state| state.sequencer.portfolio_summary(account_id))
            .await?
    }

    pub async fn create_market_with_metadata(
        &self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketWithMetadata(name, metadata, reply))
            .await?
    }

    pub async fn get_market_metadata(
        &self,
        market_id: MarketId,
    ) -> Result<Option<MarketMetadata>, SequencerError> {
        self.read_query(move |state| state.sequencer.market_metadata(market_id).cloned())
            .await
    }

    pub async fn get_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<PriceHistoryPage, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetPriceHistory(market_id, from_ms, to_ms, before_height, limit, reply)
        })
        .await?
    }

    pub async fn get_price_candles(
        &self,
        market_id: MarketId,
        resolution_secs: u32,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<PriceCandlePage, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetPriceCandles(
                market_id,
                resolution_secs,
                from_ms,
                to_ms,
                before_ms,
                limit,
                reply,
            )
        })
        .await?
    }

    pub async fn get_account_fills(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountFills(account_id, market_id, limit, offset, reply))
            .await
    }

    pub async fn get_account_fills_after(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        after: Option<AccountFillCursor>,
        limit: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetAccountFillsAfter(account_id, market_id, after, limit, reply)
        })
        .await
    }

    /// Equity series for an account, restricted to points with
    /// `timestamp_ms >= since_ms` (pass `0` for the full series). The range is
    /// applied in the durable store scan rather than by the caller.
    pub async fn get_equity_series(
        &self,
        account_id: AccountId,
        since_ms: u64,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetEquitySeries(account_id, since_ms, reply))
            .await
    }

    pub async fn get_account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply))
            .await
    }

    pub async fn get_pending_orders(
        &self,
        account_id: Option<AccountId>,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.read_query(move |state| state.sequencer.pending_orders_info(account_id))
            .await
    }

    pub async fn get_market_order_book(
        &self,
        market_id: MarketId,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.read_query(move |state| state.sequencer.market_orderbook(market_id))
            .await
    }

    pub async fn search_markets(
        &self,
        query: MarketSearchQuery,
    ) -> Result<Vec<MarketSearchResult>, SequencerError> {
        self.read_query(move |state| state.handle_search_markets(query))
            .await
    }

    pub async fn get_market_volume(&self, market_id: MarketId) -> Result<u64, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().market_volume(market_id))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_volumes(&self) -> Result<HashMap<MarketId, u64>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().market_volumes().clone())
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_metadata(
        &self,
    ) -> Result<HashMap<MarketId, MarketMetadata>, SequencerError> {
        self.read_query(|state| state.sequencer.market_metadata_all().clone())
            .await
    }

    pub async fn pause_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::PauseBlockProduction).await
    }

    pub async fn resume_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::ResumeBlockProduction).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_trader_counts(&self) -> Result<HashMap<MarketId, u32>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().all_trader_counts())
            .await
    }

    /// Platform-wide unique-trader counts `(all_time, last_24h)`. Caller
    /// passes `now_ms` so test paths can synthesise the cutoff.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_trader_counts(
        &self,
        now_ms: u64,
    ) -> Result<(u32, u32), SequencerError> {
        self.read_query(move |state| {
            let all_time = state.sequencer.analytics().platform_trader_count();
            let last_24h = state
                .sequencer
                .analytics()
                .platform_trader_24h_count(now_ms);
            (all_time, last_24h)
        })
        .await
    }

    #[tracing::instrument(skip_all, fields(markets = market_ids.len()))]
    pub async fn get_event_trader_count(
        &self,
        market_ids: Vec<MarketId>,
    ) -> Result<u32, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().event_trader_count(&market_ids))
            .await
    }

    #[tracing::instrument(skip_all, fields(market_id = market_id.0))]
    pub async fn get_open_batch_placers(&self, market_id: MarketId) -> Result<u32, SequencerError> {
        self.read_query(move |state| state.sequencer.open_batch_unique_placers(market_id))
            .await
    }

    /// Platform-wide volume `(all_time, last_24h)` with caller-supplied
    /// `now_ms` so 24h-bucket cutoff is deterministic for tests.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_volumes(&self, now_ms: u64) -> Result<(u64, u64), SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().platform_volumes(now_ms))
            .await
    }

    /// All-market 24h volume map — single round-trip companion to
    /// `get_all_market_volumes` (the cumulative variant).
    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_volumes_24h(
        &self,
        now_ms: u64,
    ) -> Result<HashMap<MarketId, u64>, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().all_market_volumes_24h(now_ms))
            .await
    }

    /// All-market clearing prices `n` hours ago in one shot — populates
    /// `MarketResponse.{yes,no}_price_24h_ago_nanos` in a single round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_prices_n_hours_ago(
        &self,
        n: u64,
        now_ms: u64,
    ) -> Result<HashMap<MarketId, (u64, u64)>, SequencerError> {
        self.read_query(move |state| {
            state
                .sequencer
                .analytics()
                .all_market_prices_n_hours_ago(n, now_ms)
        })
        .await
    }

    /// All-market liquidity averages + the band width currently in effect.
    /// Bulks `MarketResponse.liquidity_avg10_nanos` + `.liquidity_band_nanos`
    /// into one round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_liquidity_snapshot(
        &self,
    ) -> Result<(HashMap<MarketId, u64>, u64), SequencerError> {
        self.read_query(|state| {
            let liq = state.sequencer.analytics().all_liquidity_avg10();
            let band = state.sequencer.liquidity_band_nanos();
            (liq, band)
        })
        .await
    }

    /// All-market all-time order stats — populates
    /// `MarketResponse.orders_*_total` in one round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_order_stats_by_market(
        &self,
    ) -> Result<HashMap<MarketId, crate::aggregates::OrderStats>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().all_market_order_stats())
            .await
    }

    /// Platform order stats `(all_time, last_24h)` for the activity hero.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_order_stats(
        &self,
        now_ms: u64,
    ) -> Result<(crate::aggregates::OrderStats, crate::aggregates::OrderStats), SequencerError>
    {
        self.read_query(move |state| state.sequencer.analytics().platform_order_stats(now_ms))
            .await
    }

    /// Platform welfare `(all_time, last_24h)` in nanos for the activity hero.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_welfare(&self, now_ms: u64) -> Result<(i64, i64), SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().platform_welfare(now_ms))
            .await
    }

    /// Cached indicative snapshot for one market (C2). Returns a default
    /// `(None, None, 0, 0)` snapshot if the market hasn't been touched by
    /// the shadow-solver yet — e.g. on cold start or for markets with no
    /// resting orders.
    #[tracing::instrument(skip_all)]
    pub async fn get_indicative(
        &self,
        market_id: MarketId,
    ) -> Result<IndicativeSnapshot, SequencerError> {
        self.read_query(move |state| {
            state
                .indicative_cache
                .get(&market_id)
                .cloned()
                .unwrap_or_default()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::crypto::sign_cancel;
    use crate::market_info::ResolutionConfig;
    use crate::sequencer::SequencerConfig;
    use crate::system_event::SystemEvent;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use sybil_oracle::{AdminOracle, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_store_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sybil-actor-{prefix}-{}-{unique}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn make_test_sequencer() -> (BlockSequencer, AccountId) {
        make_test_sequencer_with_config(SequencerConfig::default())
    }

    fn make_test_sequencer_with_config(config: SequencerConfig) -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        let oracle = Arc::new(AdminOracle::new());
        (
            BlockSequencer::with_default_solver(accounts, markets, vec![], oracle, config),
            aid,
        )
    }

    #[test]
    fn mailbox_monitor_tracks_depth_without_underflow() {
        let monitor = MailboxMonitor::new("test_actor", 2, 4);
        assert_eq!(monitor.depth(), 0);

        monitor.queued();
        monitor.queued();
        assert_eq!(monitor.depth(), 2);

        monitor.started();
        assert_eq!(monitor.depth(), 1);

        monitor.started();
        monitor.started();
        assert_eq!(monitor.depth(), 0);

        monitor.queued();
        monitor.reset();
        assert_eq!(monitor.depth(), 0);
    }

    #[tokio::test]
    async fn test_spawn_and_produce_block() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
    }

    #[tokio::test]
    async fn produce_block_rpc_returns_error_when_paused() {
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn(seq);

        let first = handle.produce_block().await.unwrap();
        assert_eq!(first.canonical.header.height, 1);

        handle.pause_block_production().await.unwrap();
        let error = match handle.produce_block().await {
            Ok(block) => panic!(
                "expected paused error, got block height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(matches!(error, SequencerError::BlockProductionPaused));

        let latest = handle
            .get_latest_block()
            .await
            .unwrap()
            .expect("first block remains latest");
        assert_eq!(latest.canonical.header.height, 1);
    }

    #[tokio::test]
    async fn produce_block_rpc_returns_error_when_persistence_fails() {
        let path = temp_store_path("produce-persist-failure");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        store.inject_next_save_block_fault(crate::store::StoreFaultPoint::BeforeQmdbPersist);
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc_for_test(seq, store);

        let error = match handle.produce_block().await {
            Ok(block) => panic!(
                "expected persistence error, got block height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(
            matches!(error, SequencerError::Persistence(ref message) if message.contains("BeforeQmdbPersist")),
            "expected persistence error from injected fault, got {error:?}"
        );
        assert!(
            handle.get_latest_block().await.unwrap().is_none(),
            "failed persistence attempt must not publish a stale or prepared block"
        );

        let block = handle.produce_block().await.unwrap();
        assert_eq!(
            block.canonical.header.height, 1,
            "retry after failed persistence should commit the same next height"
        );
    }

    #[tokio::test]
    async fn test_submit_and_produce() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let handle = SequencerHandle::spawn(seq);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        handle.submit_order(sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
        assert!(block.canonical.header.order_count >= 1);
    }

    #[tokio::test]
    async fn per_account_submission_rate_limit_rejects_runaway_client() {
        let config = SequencerConfig {
            max_submissions_per_account_per_second: 1,
            submission_burst_per_account: 1,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 1_000,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = |qty| OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, qty)],
            mm_constraint: None,
        };

        handle.submit_order(sub(1)).await.unwrap();
        let err = handle.submit_order(sub(1)).await.unwrap_err();
        assert!(matches!(err, SequencerError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn global_submission_rate_limit_bounds_many_account_floods() {
        let config = SequencerConfig {
            max_global_submissions_per_second: 1,
            global_submission_burst: 1,
            max_submissions_per_account_per_second: 1_000,
            submission_burst_per_account: 1_000,
            ..SequencerConfig::default()
        };
        let mut accounts = AccountStore::new();
        let a = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let b = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            config,
        );
        let handle = SequencerHandle::spawn(seq);

        let sub = |account_id| OrderSubmission {
            account_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        handle.submit_order(sub(a)).await.unwrap();
        let err = handle.submit_order(sub(b)).await.unwrap_err();
        assert!(matches!(err, SequencerError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn open_order_cap_rejects_excess_resting_orders() {
        let config = SequencerConfig {
            max_open_orders_per_account: 1,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = |qty| OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, qty)],
            mm_constraint: None,
        };

        handle.submit_order(sub(1)).await.unwrap();
        let err = handle.submit_order(sub(1)).await.unwrap_err();
        assert!(matches!(
            err,
            SequencerError::TooManyOpenOrders { account_id, limit: 1 } if account_id == aid
        ));
    }

    #[tokio::test]
    async fn pending_bundle_cap_does_not_block_direct_orders() {
        let config = SequencerConfig {
            max_pending_bundles: 0,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let deferred = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&ms, 0, m0, 0, 500_000_000, 1),
                outcome_buy(&ms, 0, m0, 1, 500_000_000, 1),
            ],
            mm_constraint: None,
        };
        let err = handle.submit_order(deferred).await.unwrap_err();
        assert!(matches!(err, SequencerError::MempoolFull));

        let direct = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };
        handle.submit_order(direct).await.unwrap();
    }

    #[tokio::test]
    async fn max_orders_per_submission_bounds_request_amplification() {
        let config = SequencerConfig {
            max_orders_per_submission: 1,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&ms, 0, m0, 0, 500_000_000, 1),
                outcome_buy(&ms, 0, m0, 1, 500_000_000, 1),
            ],
            mm_constraint: None,
        };
        let err = handle.submit_order(sub).await.unwrap_err();
        assert!(matches!(
            err,
            SequencerError::TooManyOrdersInSubmission { count: 2, limit: 1 }
        ));
    }

    #[tokio::test]
    async fn test_get_state_root() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let root = handle.get_state_root().await.unwrap();
        assert_ne!(root, [0u8; 32]); // non-empty accounts -> non-zero root
    }

    #[tokio::test]
    async fn test_get_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle.get_account(aid).await.unwrap();
        assert!(account.is_some());
        assert_eq!(account.unwrap().balance, 100 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_get_latest_block_none_initially() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_block_after_produce() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().canonical.header.height, 1);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        drop(handle);

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn stop_and_wait_drains_in_flight_tick() {
        let path = temp_store_path("stop-drain-in-flight");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc_for_test(seq, store.clone());

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        handle
            .hold_next_tick_for_test(SequencerTestTickHold {
                started: started_tx,
                release: release_rx,
            })
            .await
            .unwrap();

        handle.send_tick_for_test().await.unwrap();
        started_rx.await.unwrap();

        let stopper = tokio::spawn({
            let handle = handle.clone();
            async move { handle.stop_and_wait(Duration::from_secs(5)).await }
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            !stopper.is_finished(),
            "stop_and_wait returned before the in-flight Tick was released"
        );

        release_tx.send(()).unwrap();
        assert!(stopper.await.unwrap());

        let restored = store
            .load_state()
            .await
            .unwrap()
            .expect("in-flight Tick should persist a committed block");
        assert_eq!(restored.height, 1);
        assert!(matches!(
            handle.get_latest_block().await,
            Err(SequencerError::ActorGone)
        ));
    }

    #[tokio::test]
    async fn test_block_chain_via_actor() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block1 = handle.produce_block().await.unwrap();
        assert_eq!(block1.canonical.header.height, 1);
        assert_eq!(block1.canonical.header.parent_hash, [0u8; 32]); // genesis

        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block2.canonical.header.height, 2);
        let expected = crate::block::hash_header(&block1.canonical.header);
        assert_eq!(block2.canonical.header.parent_hash, expected);
    }

    #[tokio::test]
    async fn test_state_root_changes_after_fill() {
        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((m0, 0), 100);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        let root_before = handle.get_state_root().await.unwrap();

        let buy_sub = OrderSubmission {
            account_id: buyer,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 5)],
            mm_constraint: None,
        };
        handle.submit_order(buy_sub).await.unwrap();

        let sell_sub = OrderSubmission {
            account_id: seller,
            orders: vec![matching_engine::outcome_sell(
                &markets,
                0,
                m0,
                0,
                400_000_000,
                5,
            )],
            mm_constraint: None,
        };
        handle.submit_order(sell_sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();

        if block.analytics.orders_filled > 0 {
            let root_after = handle.get_state_root().await.unwrap();
            assert_ne!(root_before, root_after);
        }
    }

    #[tokio::test]
    async fn test_create_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 50 * NANOS_PER_DOLLAR as i64);

        let fetched = handle.get_account(account.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().balance, 50 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_fund_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 125 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_system_events_emitted_for_create_and_fund() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        handle
            .fund_account(account.id, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.system_events.len(), 2);

        match &block.canonical.system_events[0] {
            SystemEvent::CreateAccount {
                account_id,
                initial_balance,
            } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*initial_balance, 50 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected CreateAccount event, got {:?}", other),
        }

        match &block.canonical.system_events[1] {
            SystemEvent::Deposit { account_id, amount } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*amount, 25 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected Deposit event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn acknowledged_control_plane_writes_survive_restart_before_next_block() {
        let path = temp_store_path("control-plane-wal");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        baseline.produce_block(Vec::new(), 1);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let created = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey.clone()).await.unwrap();

        let plain_market = handle
            .create_market("plain wal market".to_string())
            .await
            .unwrap();
        let metadata = MarketMetadata {
            description: "metadata survives WAL replay".to_string(),
            category: "regression".to_string(),
            tags: vec!["wal".to_string()],
            resolution_criteria: "resolve by admin".to_string(),
            expiry_timestamp_ms: 9_999_999,
            created_at_ms: 123_456,
            resolution_config: Some(ResolutionConfig {
                template: "wal_template".to_string(),
            }),
        };
        let metadata_market = handle
            .create_market_with_metadata("metadata wal market".to_string(), metadata.clone())
            .await
            .unwrap();
        let group_market = handle
            .create_market("group wal market".to_string())
            .await
            .unwrap();
        let (group_id, group) = handle
            .create_market_group("wal group".to_string(), vec![group_market])
            .await
            .unwrap();
        assert_eq!(group_id, 0);
        assert_eq!(group.markets, vec![group_market]);
        let (extended_group, inserted) = handle
            .extend_market_group(group_id, metadata_market)
            .await
            .unwrap();
        assert!(inserted);
        assert_eq!(extended_group.markets, vec![group_market, metadata_market]);

        let feed_pubkey = FeedPubkey(vec![2u8; 33]);
        let feed_id = handle
            .register_feed(feed_pubkey.clone(), "wal_feed".to_string())
            .await
            .unwrap();
        let template = ResolutionTemplate {
            id: TemplateId("wal_template".to_string()),
            policy: ResolutionPolicy::Immediate { feed_id },
        };
        handle.install_template(template.clone()).await.unwrap();

        let resolved_market = plain_market;
        handle
            .resolve_market(resolved_market, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();

        let markets = handle.list_markets().await.unwrap();
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(
                &markets,
                0,
                MarketId::new(0),
                0,
                500_000_000,
                1,
            )],
            mm_constraint: None,
        };
        handle.submit_order(sub).await.unwrap();
        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);
        let signed_cancel = sign_cancel(aid, pending[0].order_id, 1, &signing_key);
        handle.cancel_signed_order(signed_cancel).await.unwrap();

        let assert_replayed = |restored_seq: &BlockSequencer| {
            assert!(
                restored_seq.accounts.get(created.id).is_some(),
                "created account was acknowledged but vanished after restore"
            );
            assert_eq!(
                restored_seq.accounts.get(aid).unwrap().balance,
                125 * NANOS_PER_DOLLAR as i64,
                "funding was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq.lookup_pubkey(&pubkey),
                Some(aid),
                "pubkey registration was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.markets().get(plain_market).is_some(),
                "created market was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq.market_metadata(metadata_market),
                Some(&metadata),
                "market metadata was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq
                    .market_groups()
                    .iter()
                    .any(|group| group.name == "wal group"
                        && group.markets == vec![group_market, metadata_market]),
                "market group extension was acknowledged but not replayed after restore"
            );
            assert!(
                matches!(
                    restored_seq.market_status(resolved_market),
                    MarketStatus::Resolved { .. }
                ),
                "market resolution was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq
                    .feed_by_pubkey(&feed_pubkey)
                    .map(|feed| feed.id),
                Some(feed_id),
                "feed registration was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.template_exists("wal_template"),
                "template installation was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.pending_orders_info(Some(aid)).is_empty(),
                "signed cancel was acknowledged but the order reappeared after restore"
            );
        };

        const EXPECTED_CONTROL_PLANE_COMMANDS: usize = 13;
        for restart_round in 0..3 {
            let restored = store.load_state().await.unwrap().unwrap();
            assert_eq!(
                restored.control_plane_log.len(),
                EXPECTED_CONTROL_PLANE_COMMANDS,
                "restart round {restart_round} should see every acknowledged control-plane command before commit"
            );
            assert!(
                restored.control_plane_log.iter().any(|command| matches!(
                    command,
                    ControlPlaneCommand::ExtendMarketGroup {
                        group_id,
                        market_id
                    } if *group_id == 0 && *market_id == metadata_market
                )),
                "restart round {restart_round} should replay the market group extension"
            );
            assert!(
                restored.control_plane_log.iter().any(|command| matches!(
                    command,
                    ControlPlaneCommand::AdvanceReplayNonce {
                        account_id,
                        nonce: 1
                    } if *account_id == aid
                )),
                "restart round {restart_round} should replay the signed cancel nonce"
            );
            assert_eq!(
                restored.admit_log.len(),
                1,
                "restart round {restart_round} should replay the direct admit before the cancel command"
            );
            let restored_seq =
                BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
            assert_replayed(&restored_seq);
            let created_history =
                restored_seq
                    .analytics()
                    .pending_account_history(created.id, None, Some("funding"));
            assert!(
                created_history
                    .iter()
                    .any(|event| matches!(event.kind, crate::aggregates::HistoryKind::Created)),
                "restart round {restart_round} should expose pending account creation history"
            );
            let funding_history =
                restored_seq
                    .analytics()
                    .pending_account_history(aid, None, Some("funding"));
            assert!(
                funding_history
                    .iter()
                    .any(|event| matches!(event.kind, crate::aggregates::HistoryKind::Deposit)),
                "restart round {restart_round} should expose pending deposit history"
            );

            let mut probe = restored_seq.clone();
            let probe_block = probe
                .produce_block(Vec::new(), 10_000 + restart_round)
                .block;
            assert!(
                probe_block
                    .system_events
                    .iter()
                    .any(|event| matches!(event, SystemEvent::CreateAccount { account_id, .. } if *account_id == created.id)),
                "restart round {restart_round} should stage the uncommitted account creation event"
            );
            assert!(
                probe_block
                    .system_events
                    .iter()
                    .any(|event| matches!(event, SystemEvent::Deposit { account_id, .. } if *account_id == aid)),
                "restart round {restart_round} should stage the uncommitted funding event"
            );
            assert!(
                probe_block.system_events.iter().any(|event| matches!(
                    event,
                    SystemEvent::MarketResolved { market_id, .. } if *market_id == resolved_market
                )),
                "restart round {restart_round} should stage the uncommitted resolution event"
            );
            assert!(
                probe_block.system_events.iter().any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
                "restart round {restart_round} should stage the uncommitted cancellation event"
            );
        }

        let committed = handle.produce_block().await.unwrap();
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::MarketResolved { market_id, .. } if *market_id == resolved_market
                )),
            "committed block should include the WAL-replayed market resolution"
        );
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
            "committed block should include the WAL-replayed cancellation"
        );

        for restart_round in 0..3 {
            let restored_after_commit = store.load_state().await.unwrap().unwrap();
            assert!(
                restored_after_commit.control_plane_log.is_empty(),
                "control-plane WAL should clear once a block commits the writes"
            );
            assert!(
                restored_after_commit.admit_log.is_empty(),
                "admit WAL should clear once a block commits the direct admit and cancel"
            );
            let restored_seq = BlockSequencer::restore(
                restored_after_commit,
                Arc::new(AdminOracle::new()),
                config.clone(),
            );
            assert_replayed(&restored_seq);

            let mut probe = restored_seq.clone();
            let probe_block = probe
                .produce_block(Vec::new(), 20_000 + restart_round)
                .block;
            assert!(
                probe_block.system_events.is_empty(),
                "restart round {restart_round} after commit must not duplicate control-plane system events"
            );
        }
    }

    #[tokio::test]
    async fn bridge_withdrawal_replays_after_control_plane_cancel_wal() {
        let path = temp_store_path("bridge-after-cancel-wal");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        baseline.register_pubkey(aid, pubkey).unwrap();
        baseline.produce_block(Vec::new(), 1);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let markets = handle.list_markets().await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(
                    &markets,
                    1,
                    MarketId::new(0),
                    0,
                    500_000_000,
                    100,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);

        let signed_cancel = sign_cancel(aid, pending[0].order_id, 1, &signing_key);
        handle.cancel_signed_order(signed_cancel).await.unwrap();

        let withdrawal = handle
            .create_bridge_withdrawal(BridgeWithdrawalRequest {
                account_id: aid,
                chain_id: 1,
                vault_address: [0x10; 20],
                recipient: [0x40; 20],
                token_address: [0x20; 20],
                amount_token_units: 80_000_000,
                expiry_height: 10,
            })
            .await
            .unwrap();
        assert_eq!(withdrawal.amount_nanos, 80 * NANOS_PER_DOLLAR);

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.admit_log.len(), 1);
        assert_eq!(restored.control_plane_log.len(), 2);
        assert!(restored.control_plane_log.iter().any(|command| matches!(
            command,
            ControlPlaneCommand::AdvanceReplayNonce {
                account_id,
                nonce: 1
            } if *account_id == aid
        )));
        assert_eq!(restored.pending_bridge_withdrawals.len(), 1);
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());

        assert!(
            restored_seq.pending_orders_info(Some(aid)).is_empty(),
            "cancel must replay before the bridge withdrawal validates"
        );
        assert_eq!(
            restored_seq.bridge_withdrawal(withdrawal.withdrawal_id),
            Some(&withdrawal)
        );
        assert_eq!(
            restored_seq.accounts.get(aid).unwrap().balance,
            20 * NANOS_PER_DOLLAR as i64
        );

        let committed = handle.produce_block().await.unwrap();
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
            "committed block should include the WAL-replayed cancellation"
        );
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::WithdrawalCreated { account_id, withdrawal: leaf, .. }
                        if *account_id == aid && leaf.withdrawal_id == withdrawal.withdrawal_id
                )),
            "committed block should include the WAL-replayed bridge withdrawal"
        );
    }

    #[tokio::test]
    async fn test_fund_nonexistent_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle.fund_account(AccountId(999), 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_pubkey_and_signed_order() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());

        handle.register_pubkey(aid, pubkey).await.unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let signed = crate::crypto::sign_order(&order, 1, &signing_key);
        handle.submit_signed_order(signed).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert!(block.canonical.header.order_count >= 1);
    }

    #[tokio::test]
    async fn test_signed_order_replay_rejected_and_nonce_gap_allowed() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        handle
            .submit_signed_order(crate::crypto::sign_order(&order, 1, &signing_key))
            .await
            .unwrap();

        let replay_error = handle
            .submit_signed_order(crate::crypto::sign_order(&order, 1, &signing_key))
            .await
            .unwrap_err();
        assert!(matches!(
            replay_error,
            SequencerError::ReplayNonceStale {
                account_id,
                nonce: 1,
                last_nonce: 1
            } if account_id == aid
        ));

        handle
            .submit_signed_order(crate::crypto::sign_order(&order, 10, &signing_key))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_cancel_signed_order_by_owner() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);

        let cancel = crate::crypto::sign_cancel(aid, pending[0].order_id, 2, &signing_key);
        handle.cancel_signed_order(cancel).await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_signed_cancel_replay_rejected() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);
        let order_id = pending[0].order_id;

        handle
            .cancel_signed_order(crate::crypto::sign_cancel(aid, order_id, 1, &signing_key))
            .await
            .unwrap();
        let replay_error = handle
            .cancel_signed_order(crate::crypto::sign_cancel(aid, order_id, 1, &signing_key))
            .await
            .unwrap_err();
        assert!(matches!(
            replay_error,
            SequencerError::ReplayNonceStale {
                account_id,
                nonce: 1,
                last_nonce: 1
            } if account_id == aid
        ));
    }

    #[tokio::test]
    async fn test_cancel_signed_order_rejects_wrong_account_claim() {
        let (seq, aid) = make_test_sequencer();
        let other = AccountId(aid.0 + 1);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        let cancel = crate::crypto::sign_cancel(other, pending[0].order_id, 2, &signing_key);
        let error = handle.cancel_signed_order(cancel).await.unwrap_err();
        assert!(matches!(error, SequencerError::SignerAccountMismatch));
    }

    #[tokio::test]
    async fn test_register_pubkey_duplicate() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());

        handle.register_pubkey(aid, pubkey.clone()).await.unwrap();
        let result = handle.register_pubkey(aid, pubkey).await;
        assert!(matches!(
            result,
            Err(SequencerError::AccountAlreadyRegistered)
        ));
    }

    #[tokio::test]
    async fn test_list_and_create_markets() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 1);

        let new_id = handle
            .create_market("New Market".to_string())
            .await
            .unwrap();
        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 2);
        assert!(markets.get(new_id).is_some());
    }

    #[tokio::test]
    async fn test_create_and_list_market_groups() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m1 = handle.create_market("A wins".to_string()).await.unwrap();
        let m2 = handle.create_market("B wins".to_string()).await.unwrap();

        let (group_id, group) = handle
            .create_market_group("Election".to_string(), vec![m1, m2])
            .await
            .unwrap();
        assert_eq!(group_id, 0);
        assert_eq!(group.name, "Election");
        assert_eq!(group.markets.len(), 2);

        let groups = handle.list_market_groups().await.unwrap();
        assert_eq!(groups.len(), 1);
    }

    #[tokio::test]
    async fn test_resolve_market() {
        let (seq, _aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m0 = MarketId::new(0);
        let record = handle
            .resolve_market(m0, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();
        assert_eq!(record.payout_nanos, Nanos(NANOS_PER_DOLLAR));
        assert_eq!(record.market_id, m0);
    }

    #[tokio::test]
    async fn test_system_event_emitted_for_market_resolution() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts.get_mut(aid).unwrap().positions.insert((m0, 0), 10);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        handle
            .resolve_market(m0, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();
        let block = handle.produce_block().await.unwrap();

        assert_eq!(block.canonical.system_events.len(), 1);
        match &block.canonical.system_events[0] {
            SystemEvent::MarketResolved {
                market_id,
                payout_nanos,
                affected_accounts,
            } => {
                assert_eq!(*market_id, m0);
                assert_eq!(*payout_nanos, Nanos(NANOS_PER_DOLLAR));
                assert_eq!(affected_accounts, &vec![aid]);
            }
            other => panic!("expected MarketResolved event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_market() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle
            .resolve_market(MarketId::new(999), Nanos(NANOS_PER_DOLLAR))
            .await;
        assert!(matches!(result, Err(SequencerError::MarketNotFound)));
    }

    #[tokio::test]
    async fn test_get_block_by_height() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();

        let block = handle.get_block(1).await.unwrap();
        assert_eq!(block.canonical.header.height, 1);

        let block = handle.get_block(2).await.unwrap();
        assert_eq!(block.canonical.header.height, 2);

        let result = handle.get_block(99).await;
        assert!(matches!(result, Err(SequencerError::BlockNotFound)));
    }

    #[tokio::test]
    async fn store_backed_get_block_survives_ring_eviction_and_restart() {
        let path = temp_store_path("block-history");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_history_capacity: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let block1 = handle.produce_block().await.unwrap();
        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block1.canonical.header.height, 1);
        assert_eq!(block2.canonical.header.height, 2);

        let recent = handle.get_recent_blocks(10).await.unwrap();
        assert_eq!(recent.len(), 1, "hot ring should evict block 1");
        assert_eq!(recent[0].canonical.header.height, 2);

        let evicted = handle.get_block(1).await.unwrap();
        assert_eq!(evicted.canonical.header.height, 1);
        assert_eq!(
            evicted.canonical.header.state_root,
            block1.canonical.header.state_root
        );

        drop(handle);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let reader = SequencerHandle::spawn_with_store_arc(restored_seq, Some(store.clone()));

        let stored_block1 = reader.get_block(1).await.unwrap();
        assert_eq!(stored_block1.canonical.header.height, 1);
        assert_eq!(
            stored_block1.canonical.header.state_root,
            block1.canonical.header.state_root
        );
        let stored_block2 = reader.get_block(2).await.unwrap();
        assert_eq!(stored_block2.canonical.header.height, 2);
        assert_eq!(
            stored_block2.canonical.header.state_root,
            block2.canonical.header.state_root
        );
    }

    #[tokio::test]
    async fn store_backed_history_pruning_runs_after_block_commit() {
        let path = temp_store_path("block-history-retention");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_history_capacity: 1,
            block_history_retention_blocks: 1,
            history_prune_interval_blocks: 1,
            history_prune_max_rows: 10,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();
        let block3 = handle.produce_block().await.unwrap();
        assert_eq!(block3.canonical.header.height, 3);

        let meta = store.history_retention_meta().unwrap();
        assert_eq!(meta.blocks_full_min_height, Some(3));
        assert_eq!(meta.last_history_prune_height, Some(3));
        assert!(store.load_block(1).await.unwrap().is_none());
        assert!(store.load_block(2).await.unwrap().is_none());
        assert_eq!(
            store
                .load_block(3)
                .await
                .unwrap()
                .unwrap()
                .canonical
                .header
                .height,
            3
        );

        let pruned = match handle.get_block(1).await {
            Ok(block) => panic!(
                "expected block 1 to be pruned, got height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(matches!(
            pruned,
            SequencerError::BlockPruned {
                requested_height: 1,
                retention_min_height: 3,
            }
        ));
        assert_eq!(
            handle.get_block(3).await.unwrap().canonical.header.height,
            3
        );
    }

    #[tokio::test]
    async fn store_backed_price_history_survives_cache_eviction_and_restart() {
        let path = temp_store_path("price-history");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            max_price_history_points_per_market: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };

        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Price history");
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((market_id, 0), 10);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            config.clone(),
        );
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        handle
            .submit_order(OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 1, market_id, 0, 600_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle
            .submit_order(OrderSubmission {
                account_id: seller,
                orders: vec![matching_engine::outcome_sell(
                    &markets,
                    2,
                    market_id,
                    0,
                    400_000_000,
                    1,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let block1 = handle.produce_block().await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 3, market_id, 0, 700_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle
            .submit_order(OrderSubmission {
                account_id: seller,
                orders: vec![matching_engine::outcome_sell(
                    &markets,
                    4,
                    market_id,
                    0,
                    300_000_000,
                    1,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let block2 = handle.produce_block().await.unwrap();

        let page = handle
            .get_price_history(market_id, None, None, None, 2)
            .await
            .unwrap();
        assert_eq!(page.next_before_height, None);
        let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
        assert_eq!(
            heights,
            vec![
                block1.canonical.header.height,
                block2.canonical.header.height
            ],
            "store-backed reads should include points older than the hot price cache"
        );
        assert!(page.points.iter().all(|point| point.volume_nanos > 0));

        let newest_page = handle
            .get_price_history(market_id, None, None, None, 1)
            .await
            .unwrap();
        assert_eq!(newest_page.next_before_height, Some(2));
        assert_eq!(newest_page.points.len(), 1);
        assert_eq!(newest_page.points[0].height, 2);
        let older_page = handle
            .get_price_history(market_id, None, None, newest_page.next_before_height, 1)
            .await
            .unwrap();
        assert_eq!(older_page.next_before_height, None);
        assert_eq!(older_page.points.len(), 1);
        assert_eq!(older_page.points[0].height, 1);
        let candle_page = handle
            .get_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert!(!candle_page.candles.is_empty());
        assert_eq!(
            candle_page
                .candles
                .iter()
                .map(|candle| candle.point_count)
                .sum::<u64>(),
            2,
            "candles should aggregate the two committed raw price points"
        );

        drop(handle);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let reader = SequencerHandle::spawn_with_store_arc(restored_seq, Some(store.clone()));
        let restored_page = reader
            .get_price_history(market_id, None, None, None, 2)
            .await
            .unwrap();
        let restored_heights: Vec<_> = restored_page
            .points
            .iter()
            .map(|point| point.height)
            .collect();
        assert_eq!(restored_heights, heights);
        let restored_candles = reader
            .get_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(restored_candles.candles, candle_page.candles);
    }

    #[tokio::test]
    async fn store_backed_direct_admit_read_model_rows_survive_empty_block() {
        let path = temp_store_path("direct-admit-read-model");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            max_equity_points_per_account: 0,
            max_history_events_per_account: 0,
            ..SequencerConfig::default()
        };

        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        baseline.produce_block(Vec::new(), 1_000);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let markets = handle.list_markets().await.unwrap();
        let market_id = MarketId::new(0);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&markets, 0, market_id, 0, 600_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 2);
        assert_eq!(
            block.canonical.header.fill_count, 0,
            "the read-model rows must persist even when the admitted order does not cross"
        );
        assert!(
            block.canonical.system_events.is_empty(),
            "the regression targets empty non-system blocks"
        );

        let events = store
            .account_events(aid, 10, None, Some("trades".into()))
            .unwrap();
        let placed_count = events
            .iter()
            .filter(|event| matches!(event.kind, crate::aggregates::HistoryKind::Placed))
            .count();
        assert_eq!(
            placed_count, 1,
            "direct-admit Placed history must be durable exactly once even with no in-memory fallback"
        );

        let equity = store.equity_series(aid, 0).unwrap();
        assert!(
            equity
                .iter()
                .any(|point| point.height == block.canonical.header.height),
            "equity point from the empty-fill block must be durable"
        );

        handle.produce_block().await.unwrap();
        let events_after_next_block = store
            .account_events(aid, 10, None, Some("trades".into()))
            .unwrap();
        let placed_count_after_next_block = events_after_next_block
            .iter()
            .filter(|event| matches!(event.kind, crate::aggregates::HistoryKind::Placed))
            .count();
        assert_eq!(
            placed_count_after_next_block, 1,
            "later persisted blocks must not re-flush already-cleared Placed history"
        );
    }

    #[tokio::test]
    async fn test_subscribe_blocks() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let mut rx = handle.subscribe_blocks().await.unwrap();

        handle.produce_block().await.unwrap();

        let block = rx.recv().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
    }

    // C2 — indicative scheduler ---------------------------------------------

    fn mk_problem(orders: Vec<Order>, markets: MarketSet) -> Problem {
        let mut p = Problem::new("test");
        p.markets = markets;
        p.orders = orders;
        p
    }

    #[test]
    fn build_indicative_snapshots_empty_book_yields_empty_map() {
        let problem = mk_problem(Vec::new(), MarketSet::new());
        let result = matching_solver::PipelineResult::empty();
        let last_clearing = HashMap::new();
        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 1_000);
        assert!(snaps.is_empty());
    }

    #[test]
    fn build_indicative_snapshots_fallback_to_last_clearing() {
        // Book has an order on market m, but no fills (no cross). Snapshot
        // should fall back to the last clearing price for m.
        let mut markets = MarketSet::new();
        let m = markets.add_binary("m");
        let order = outcome_buy(&markets, 0, m, 0, NANOS_PER_DOLLAR / 2, 5);
        let problem = mk_problem(vec![order], markets);
        let result = matching_solver::PipelineResult::empty();
        let mut last_clearing = HashMap::new();
        last_clearing.insert(m, vec![Nanos(400_000_000), Nanos(600_000_000)]);

        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 12_345);

        let snap = snaps.get(&m).expect("market should be in the cache");
        assert_eq!(snap.yes_price_nanos, Some(400_000_000));
        assert_eq!(snap.no_price_nanos, Some(600_000_000));
        assert_eq!(snap.volume_nanos, 0);
        assert_eq!(snap.computed_at_ms, 12_345);
    }

    #[test]
    fn build_indicative_snapshots_fills_use_price_discovery_and_volume() {
        // Book has an order; result has a fill on it. Snapshot should
        // surface the price_discovery price and a non-zero volume.
        let mut markets = MarketSet::new();
        let m = markets.add_binary("m");
        let order = outcome_buy(
            &markets,
            1,
            m,
            0,
            NANOS_PER_DOLLAR / 2,
            matching_engine::shares_to_qty(5).0,
        );
        let order_id = order.id;
        let problem = mk_problem(vec![order], markets);

        let mut result = matching_solver::PipelineResult::empty();
        let mut fill = matching_engine::Fill::new(
            order_id,
            matching_engine::shares_to_qty(3),
            Nanos(400_000_000),
        );
        fill.account_id = 7;
        result.result.fills.push(fill);
        let mut pd = matching_solver::PriceDiscoveryResult::empty();
        pd.prices
            .insert(m, vec![Nanos(450_000_000), Nanos(550_000_000)]);
        result.price_discovery = Some(pd);

        // last_clearing has older values; PD should override.
        let mut last_clearing = HashMap::new();
        last_clearing.insert(m, vec![Nanos(100_000_000), Nanos(900_000_000)]);

        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 0);
        let snap = snaps.get(&m).expect("market should be in cache");
        assert_eq!(snap.yes_price_nanos, Some(450_000_000));
        assert_eq!(snap.no_price_nanos, Some(550_000_000));
        // 3 qty * 400_000_000 = 1.2e9
        assert_eq!(snap.volume_nanos, 1_200_000_000);
    }

    #[test]
    fn indicative_solve_gate_prevents_stacked_solves() {
        let mut gate = IndicativeSolveGate::default();

        assert!(gate.try_start(), "first tick should start a solve");
        assert!(
            !gate.try_start(),
            "second tick must not start while solve is in flight"
        );

        gate.finish();
        assert!(gate.try_start(), "next tick may start after update arrives");
    }

    struct PanicOnceSolver {
        calls: Arc<AtomicUsize>,
    }

    impl matching_solver::Solver for PanicOnceSolver {
        fn solve(&self, _problem: &Problem) -> matching_solver::PipelineResult {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("injected indicative solver panic");
            }
            matching_solver::PipelineResult::empty()
        }

        fn name(&self) -> &str {
            "panic-once"
        }
    }

    #[tokio::test]
    async fn get_indicative_returns_default_when_uncached() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);
        let mid = MarketId::new(99);
        let snap = handle.get_indicative(mid).await.unwrap();
        assert!(snap.yes_price_nanos.is_none());
        assert!(snap.no_price_nanos.is_none());
        assert_eq!(snap.volume_nanos, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indicative_solver_panic_releases_gate_for_next_tick() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let seq = BlockSequencer::new(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            Arc::new(PanicOnceSolver {
                calls: Arc::clone(&calls),
            }),
            config,
        );
        let handle = SequencerHandle::spawn(seq);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut released_and_reran = false;
        while Instant::now() < deadline {
            if calls.load(Ordering::SeqCst) >= 2 {
                let snap = handle.get_indicative(m0).await.unwrap();
                if snap.computed_at_ms > 0 {
                    released_and_reran = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(
            released_and_reran,
            "indicative solve gate did not release after panic; calls={}",
            calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indicative_timer_populates_cache_after_one_tick() {
        // With at least one resting order on a market, the 750ms
        // indicative ticker should populate the cache for that market
        // within one ticker cycle. We verify by checking `computed_at_ms`
        // moves off the default `0`.
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        // First indicative tick fires at +750ms; the solve + cache-write
        // round-trip needs a little extra. 1500ms gives generous slack
        // on a CI box.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let snap = handle.get_indicative(m0).await.unwrap();
        assert!(
            snap.computed_at_ms > 0,
            "indicative tick should have written a snapshot by now"
        );
    }
}
