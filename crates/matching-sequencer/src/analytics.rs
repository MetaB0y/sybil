//! In-process derived projections for API/UI surfaces.
//!
//! This state is updated synchronously by `BlockSequencer`, but it is not the
//! sequencing core: it does not validate orders, mutate reservations, settle
//! accounts, or decide block contents.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Nanos, Order};
use sybil_verifier::BlockWitness;

use crate::account::{AccountId, AccountStore};
use crate::aggregates::{
    AccountEventLog, CostBasisTracker, CostBasisTrackerSnapshot, EquityTracker, HistoryEvent,
    LiquidityTracker, LiquidityTrackerSnapshot, OrderStats, OrderStatsTracker,
    OrderStatsTrackerSnapshot, TraderTracker, TraderTrackerSnapshot, WelfareTracker,
    WelfareTrackerSnapshot,
};
use crate::block::{
    DerivedViewSidecar, RejectedOrderView, RemovedOrderExitReason, RemovedOrderPhase, SealedBlock,
};
use crate::error::RejectionReason;
use crate::fill_recorder::FillRecorder;
use crate::market_info::{AccountFillCursor, AccountFillRecord, PricePoint};
use crate::order_book::{OrderBook, RestingOrder};
use crate::price_tracker::{
    PriceTracker, PriceTrackerClearingHistorySnapshot, PriceTrackerVolumeSnapshot,
};
use crate::sequencer::SequencerConfig;
use crate::store::{AnalyticsRestoredState, AnalyticsSnapshot};

#[derive(Clone)]
pub struct AnalyticsState {
    price_tracker: PriceTracker,
    fill_recorder: FillRecorder,
    trader_tracker: TraderTracker,
    liquidity_tracker: LiquidityTracker,
    order_stats_tracker: OrderStatsTracker,
    welfare_tracker: WelfareTracker,
    cost_basis_tracker: CostBasisTracker,
    first_deposit_ms: HashMap<AccountId, u64>,
    equity_tracker: EquityTracker,
    account_event_log: AccountEventLog,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct OrderHistoryOptions {
    include_price: bool,
    reason: Option<&'static str>,
    required_nanos: Option<i64>,
    available_nanos: Option<i64>,
}

impl OrderHistoryOptions {
    pub(crate) fn with_price() -> Self {
        Self {
            include_price: true,
            ..Self::default()
        }
    }

    pub(crate) fn rejection(reason: &RejectionReason) -> Self {
        let (required_nanos, available_nanos) = reason.amounts();
        Self {
            include_price: true,
            reason: Some(reason.code()),
            required_nanos,
            available_nanos,
        }
    }
}

impl AnalyticsState {
    pub fn new(config: &SequencerConfig) -> Self {
        Self {
            price_tracker: PriceTracker::with_retention(config.max_price_history_points_per_market),
            fill_recorder: FillRecorder::with_retention(config.max_fill_history_per_account),
            trader_tracker: TraderTracker::new(),
            liquidity_tracker: LiquidityTracker::new(),
            order_stats_tracker: OrderStatsTracker::new(),
            welfare_tracker: WelfareTracker::new(),
            cost_basis_tracker: CostBasisTracker::new(),
            first_deposit_ms: HashMap::new(),
            equity_tracker: EquityTracker::with_retention(config.max_equity_points_per_account),
            account_event_log: AccountEventLog::with_retention(
                config.max_history_events_per_account,
            ),
        }
    }

    pub fn restore(input: AnalyticsRestoredState, config: &SequencerConfig) -> Self {
        let mut price_tracker = PriceTracker::with_state_and_retention(
            input.last_clearing_prices,
            input.market_volumes,
            config.max_price_history_points_per_market,
        );
        price_tracker.restore_volume_extensions(input.price_tracker_volume);
        price_tracker.restore_clearing_history(input.price_tracker_clearing_history);

        Self {
            price_tracker,
            fill_recorder: FillRecorder::restore_bounded_newest_first_with_counts(
                input.account_fills,
                input.fill_total_counts,
                config.max_fill_history_per_account,
            ),
            trader_tracker: TraderTracker::restore(input.trader_tracker),
            liquidity_tracker: LiquidityTracker::restore(input.liquidity_tracker),
            order_stats_tracker: OrderStatsTracker::restore(input.order_stats_tracker),
            welfare_tracker: WelfareTracker::restore(input.welfare_tracker),
            cost_basis_tracker: CostBasisTracker::restore(input.cost_basis_tracker),
            first_deposit_ms: input.first_deposit_ms,
            equity_tracker: EquityTracker::with_retention(config.max_equity_points_per_account),
            account_event_log: AccountEventLog::with_retention_and_next_seq(
                config.max_history_events_per_account,
                input.history_event_next_seq,
            ),
        }
    }

    pub fn snapshot(&self) -> AnalyticsSnapshot<'_> {
        AnalyticsSnapshot {
            last_clearing_prices: self.last_clearing_prices(),
            market_volumes: self.market_volumes(),
            account_fills: self.fill_snapshot(),
            trader_tracker: self.trader_snapshot(),
            price_tracker_volume: self.price_volume_snapshot(),
            price_tracker_clearing_history: self.price_clearing_history_snapshot(),
            liquidity_tracker: self.liquidity_snapshot(),
            order_stats_tracker: self.order_stats_snapshot(),
            welfare_tracker: self.welfare_snapshot(),
            first_deposit_ms: self.first_deposit_snapshot(),
            fill_total_counts: self.total_fill_counts(),
            cost_basis_tracker: self.cost_basis_snapshot(),
            history_event_next_seq: self.account_event_log.next_seq(),
            fill_history_delta: self.fill_recorder.pending_delta().to_vec(),
            price_points_delta: self.price_tracker.pending_price_points().to_vec(),
            equity_points_delta: self.equity_tracker.pending().to_vec(),
            history_events_delta: self
                .account_event_log
                .pending()
                .iter()
                .map(crate::aggregates::StoredHistoryEvent::from_event)
                .collect(),
        }
    }

    pub fn last_clearing_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        self.price_tracker.last_clearing_prices()
    }

    pub fn last_mark_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        self.price_tracker.last_mark_prices()
    }

    pub fn market_volumes(&self) -> &HashMap<MarketId, u64> {
        self.price_tracker.market_volumes()
    }

    pub fn market_volume(&self, market_id: MarketId) -> u64 {
        self.price_tracker.market_volume(market_id)
    }

    pub fn market_volume_24h(&self, market_id: MarketId, now_ms: u64) -> u64 {
        self.price_tracker.market_volume_24h(market_id, now_ms)
    }

    pub fn all_market_volumes_24h(&self, now_ms: u64) -> HashMap<MarketId, u64> {
        self.price_tracker.all_market_volumes_24h(now_ms)
    }

    pub fn platform_volumes(&self, now_ms: u64) -> (u64, u64) {
        (
            self.price_tracker.platform_volume_total(),
            self.price_tracker.platform_volume_24h(now_ms),
        )
    }

    pub fn price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Vec<PricePoint> {
        self.price_tracker.price_history(market_id, from_ms, to_ms)
    }

    pub fn price_n_hours_ago(
        &self,
        market_id: MarketId,
        n: u64,
        now_ms: u64,
    ) -> Option<(u64, u64)> {
        self.price_tracker.price_n_hours_ago(market_id, n, now_ms)
    }

    pub fn all_market_prices_n_hours_ago(
        &self,
        n: u64,
        now_ms: u64,
    ) -> HashMap<MarketId, (u64, u64)> {
        self.price_tracker.all_market_prices_n_hours_ago(n, now_ms)
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

    pub fn account_fills_after(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        after: Option<AccountFillCursor>,
        limit: usize,
    ) -> Vec<AccountFillRecord> {
        self.fill_recorder
            .account_fills_after(account_id, market_id_filter, after, limit)
    }

    pub fn fill_snapshot(&self) -> Vec<(AccountId, AccountFillRecord)> {
        self.fill_recorder.snapshot()
    }

    pub fn total_fill_counts(&self) -> HashMap<AccountId, u64> {
        self.fill_recorder.total_counts().clone()
    }

    pub fn total_fills(&self, account_id: AccountId) -> u64 {
        self.fill_recorder.total_fills(account_id)
    }

    pub fn trader_count(&self, market_id: MarketId) -> u32 {
        self.trader_tracker.per_market_count(market_id)
    }

    pub fn all_trader_counts(&self) -> HashMap<MarketId, u32> {
        self.trader_tracker.all_market_counts()
    }

    pub fn platform_trader_count(&self) -> u32 {
        self.trader_tracker.platform_count()
    }

    pub fn platform_trader_24h_count(&self, now_ms: u64) -> u32 {
        self.trader_tracker.platform_24h_count(now_ms)
    }

    pub fn event_trader_count(&self, market_ids: &[MarketId]) -> u32 {
        self.trader_tracker.event_count(market_ids)
    }

    pub fn liquidity_avg10(&self, market_id: MarketId) -> u64 {
        self.liquidity_tracker.sum_last_n(market_id, 10)
    }

    pub fn all_liquidity_avg10(&self) -> HashMap<MarketId, u64> {
        self.liquidity_tracker.all_sum_last_n(10)
    }

    pub fn all_market_order_stats(&self) -> HashMap<MarketId, OrderStats> {
        self.order_stats_tracker.all_per_market()
    }

    pub fn platform_order_stats(&self, now_ms: u64) -> (OrderStats, OrderStats) {
        (
            self.order_stats_tracker.platform(),
            self.order_stats_tracker.platform_24h(now_ms),
        )
    }

    /// Platform welfare `(all_time, last_24h)` — cumulative running sum plus
    /// the rolling 24h window. Mirrors `platform_volumes` / `platform_order_stats`.
    pub fn platform_welfare(&self, now_ms: u64) -> (i64, i64) {
        (
            self.welfare_tracker.platform_total(),
            self.welfare_tracker.platform_24h(now_ms),
        )
    }

    pub fn cost_basis_tracker(&self) -> &CostBasisTracker {
        &self.cost_basis_tracker
    }

    pub fn first_deposit_ms(&self, account_id: AccountId) -> Option<u64> {
        self.first_deposit_ms.get(&account_id).copied()
    }

    pub fn note_first_deposit(&mut self, account_id: AccountId) {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.note_first_deposit_at(account_id, timestamp_ms);
    }

    pub fn note_first_deposit_at(&mut self, account_id: AccountId, timestamp_ms: u64) {
        self.first_deposit_ms
            .entry(account_id)
            .or_insert(timestamp_ms);
    }

    pub fn merge_prices(
        &mut self,
        price_discovery: &Option<matching_solver::PriceDiscoveryResult>,
        markets_with_fills: &std::collections::HashSet<MarketId>,
        active_markets: &std::collections::HashSet<MarketId>,
        position_markets: &std::collections::HashSet<MarketId>,
    ) -> HashMap<MarketId, Vec<Nanos>> {
        self.price_tracker.merge_prices(
            price_discovery,
            markets_with_fills,
            active_markets,
            position_markets,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_finalized_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        midpoints: &HashMap<MarketId, Nanos>,
        height: u64,
        timestamp_ms: u64,
        accounts: &AccountStore,
    ) -> (HashMap<MarketId, u64>, HashMap<MarketId, Vec<Nanos>>) {
        let (volume_by_market, mark_prices) = self.price_tracker.record_block(
            fills,
            orders,
            clearing_prices,
            midpoints,
            height,
            timestamp_ms,
        );
        self.fill_recorder.record_fills(
            fills,
            orders,
            height,
            timestamp_ms,
            &mut self.cost_basis_tracker,
            accounts,
            &mut self.account_event_log,
        );
        (volume_by_market, mark_prices)
    }

    /// Accumulate one finalized block's authoritative `total_welfare` scalar
    /// into the cumulative + 24h platform-welfare tracker. Called once per
    /// committed block, alongside `record_finalized_block`.
    pub fn record_welfare(&mut self, total_welfare: i64, timestamp_ms: u64) {
        self.welfare_tracker.record(total_welfare, timestamp_ms);
    }

    pub fn record_trader_placement(
        &mut self,
        account_id: AccountId,
        markets: Vec<MarketId>,
        timestamp_ms: u64,
        is_mm: bool,
    ) {
        self.trader_tracker
            .record_placed(account_id, markets, timestamp_ms, is_mm);
    }

    pub fn record_order_placed(
        &mut self,
        markets: impl IntoIterator<Item = MarketId>,
        timestamp_ms: u64,
    ) {
        self.order_stats_tracker
            .record_placed(markets, timestamp_ms);
    }

    /// Record one distinct order admission (platform-level). Call once per
    /// order at intake; see `OrderStatsTracker::record_admitted`.
    pub fn record_order_admitted(&mut self, timestamp_ms: u64) {
        self.order_stats_tracker.record_admitted(timestamp_ms);
    }

    pub fn record_order_exit(&mut self, resting_order: &RestingOrder, timestamp_ms: u64) {
        self.order_stats_tracker
            .record_exit(resting_order, timestamp_ms);
    }

    /// Record an MM flash order's one-block outcome: `matched` if it received
    /// a fill this block, else unmatched. MM orders never rest in the book, so
    /// they don't reach `record_order_exit`; this is their matched/unmatched
    /// hook. See `OrderStatsTracker::record_outcome`.
    pub fn record_order_outcome(
        &mut self,
        markets: impl IntoIterator<Item = MarketId>,
        matched: bool,
        timestamp_ms: u64,
    ) {
        self.order_stats_tracker
            .record_outcome(markets, matched, timestamp_ms);
    }

    pub(crate) fn record_order_history(
        &mut self,
        account_id: AccountId,
        kind: crate::aggregates::HistoryKind,
        block_height: u64,
        timestamp_ms: u64,
        order: &Order,
        options: OrderHistoryOptions,
    ) {
        let mut event = HistoryEvent::new(account_id, kind, block_height, timestamp_ms);
        event.order_id = Some(order.id);
        event.market_id = order.active_markets().next();
        event.qty = Some(order.max_fill.0);
        if options.include_price {
            event.price_nanos = Some(order.limit_price.0);
        }
        let (side, outcome) = crate::aggregates::side_outcome_from_order(order);
        event.side = side;
        event.outcome = outcome;
        event.reason = options.reason;
        event.required_nanos = options.required_nanos;
        event.available_nanos = options.available_nanos;
        self.record_history(event);
    }

    pub fn observe_block(
        &mut self,
        block: &SealedBlock,
        sidecar: &DerivedViewSidecar,
        witness: &BlockWitness,
    ) {
        let height = block.canonical.header.height;
        let timestamp_ms = block.canonical.header.timestamp_ms;

        self.observe_system_event_history(&block.canonical.system_events, height, timestamp_ms);

        for witness_order in &witness.orders {
            self.record_order_placed(witness_order.order.active_markets(), timestamp_ms);
        }

        let mut witness_orders: HashMap<u64, (&Order, AccountId, bool)> = HashMap::new();
        for witness_order in &witness.orders {
            witness_orders.insert(
                witness_order.order.id,
                (
                    &witness_order.order,
                    AccountId(witness_order.account_id),
                    witness_order.is_mm,
                ),
            );
        }

        for admit in &sidecar.admits {
            if !admit.is_new {
                continue;
            }
            self.record_order_admitted(timestamp_ms);
            let Some((order, account_id, is_mm)) = witness_orders.get(&admit.order_id) else {
                continue;
            };
            let markets: Vec<MarketId> = order.active_markets().collect();
            self.record_trader_placement(*account_id, markets, timestamp_ms, *is_mm);
            if !*is_mm {
                self.record_order_history(
                    *account_id,
                    crate::aggregates::HistoryKind::Placed,
                    height,
                    timestamp_ms,
                    order,
                    OrderHistoryOptions::with_price(),
                );
            }
        }

        for rejected in &sidecar.rejection_history {
            self.observe_rejection_history(rejected, height, timestamp_ms);
        }

        for removed in &sidecar.removed_orders {
            self.order_stats_tracker.record_outcome(
                removed.active_markets.iter().copied(),
                removed.has_been_matched,
                timestamp_ms,
            );

            match removed.exit_reason {
                RemovedOrderExitReason::Expired => self.record_order_history(
                    AccountId(removed.account_id),
                    crate::aggregates::HistoryKind::Expired,
                    height,
                    timestamp_ms,
                    &removed.order,
                    OrderHistoryOptions::default(),
                ),
                RemovedOrderExitReason::RevalidateInsufficientBalance
                | RemovedOrderExitReason::RevalidateInsufficientPosition
                | RemovedOrderExitReason::RevalidateRejected => {
                    if removed.phase == RemovedOrderPhase::BlockStartRevalidate {
                        if let Some(reason) = &removed.rejection_reason {
                            self.record_order_history(
                                AccountId(removed.account_id),
                                crate::aggregates::HistoryKind::Rejected,
                                height,
                                timestamp_ms,
                                &removed.order,
                                OrderHistoryOptions::rejection(reason),
                            );
                        }
                    }
                }
                RemovedOrderExitReason::RevalidateMarketInactive
                | RemovedOrderExitReason::RevalidateAccountGone
                | RemovedOrderExitReason::RevalidateAccountInsolvent
                | RemovedOrderExitReason::Filled
                | RemovedOrderExitReason::Settled => {}
            }
        }

        let mm_filled_qty: HashMap<u64, u64> = block
            .canonical
            .fills
            .iter()
            .filter(|fill| fill.fill_qty.0 > 0)
            .fold(HashMap::new(), |mut acc, fill| {
                *acc.entry(fill.order_id).or_insert(0) += fill.fill_qty.0;
                acc
            });
        for witness_order in &witness.orders {
            if !witness_order.is_mm {
                continue;
            }
            let matched = mm_filled_qty
                .get(&witness_order.order.id)
                .copied()
                .unwrap_or(0)
                > 0;
            self.record_order_outcome(witness_order.order.active_markets(), matched, timestamp_ms);
        }
    }

    fn observe_rejection_history(
        &mut self,
        rejected: &RejectedOrderView,
        height: u64,
        timestamp_ms: u64,
    ) {
        self.record_order_history(
            AccountId(rejected.account_id),
            crate::aggregates::HistoryKind::Rejected,
            height,
            timestamp_ms,
            &rejected.order,
            OrderHistoryOptions::rejection(&rejected.reason),
        );
    }

    fn observe_system_event_history(
        &mut self,
        system_events: &[crate::system_event::SystemEvent],
        height: u64,
        timestamp_ms: u64,
    ) {
        use crate::aggregates::HistoryKind;
        use crate::system_event::SystemEvent;

        for event in system_events {
            match event {
                SystemEvent::CreateAccount { .. } | SystemEvent::Deposit { .. } => {}
                SystemEvent::L1Deposit {
                    account_id, amount, ..
                } => {
                    let mut event =
                        HistoryEvent::new(*account_id, HistoryKind::Deposit, height, timestamp_ms);
                    event.amount_nanos = Some(*amount);
                    self.record_history(event);
                }
                SystemEvent::WithdrawalCreated {
                    account_id, amount, ..
                } => {
                    let mut event = HistoryEvent::new(
                        *account_id,
                        HistoryKind::Withdrawal,
                        height,
                        timestamp_ms,
                    );
                    event.amount_nanos = Some(-*amount);
                    self.record_history(event);
                }
                SystemEvent::WithdrawalRefunded {
                    account_id, amount, ..
                } => {
                    let mut event = HistoryEvent::new(
                        *account_id,
                        HistoryKind::Withdrawal,
                        height,
                        timestamp_ms,
                    );
                    event.amount_nanos = Some(*amount);
                    self.record_history(event);
                }
                SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                } => {
                    let payout_outcome = if payout_nanos.0 >= matching_engine::NANOS_PER_DOLLAR {
                        Some("YES")
                    } else if payout_nanos.0 == 0 {
                        Some("NO")
                    } else {
                        None
                    };
                    for account_id in affected_accounts {
                        let mut event = HistoryEvent::new(
                            *account_id,
                            HistoryKind::Resolved,
                            height,
                            timestamp_ms,
                        );
                        event.market_id = Some(*market_id);
                        event.payout_outcome = payout_outcome;
                        self.record_history(event);
                    }
                }
                SystemEvent::WithdrawalFinalized { .. }
                | SystemEvent::L1BlockObserved { .. }
                | SystemEvent::OrderCancelled { .. }
                | SystemEvent::MarketGroupExtended { .. }
                | SystemEvent::KeyRegistered { .. }
                | SystemEvent::KeyRevoked { .. } => {}
            }
        }
    }

    pub fn record_liquidity(
        &mut self,
        order_book: &OrderBook,
        mm_orders: &[&Order],
        mark_prices: &HashMap<MarketId, Vec<Nanos>>,
        band_nanos: u64,
    ) {
        self.liquidity_tracker
            .record_block(order_book, mm_orders, mark_prices, band_nanos);
    }

    pub fn record_equity(
        &mut self,
        touched: &std::collections::HashSet<AccountId>,
        accounts: &AccountStore,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        height: u64,
        timestamp_ms: u64,
    ) {
        self.equity_tracker
            .record(touched, accounts, prices, height, timestamp_ms);
    }

    pub fn equity_series(&self, account_id: AccountId) -> Vec<crate::aggregates::EquityPoint> {
        self.equity_tracker.series(account_id)
    }

    pub fn record_history(&mut self, event: HistoryEvent) {
        self.account_event_log.append(event);
    }

    pub fn account_history(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<&str>,
    ) -> Vec<HistoryEvent> {
        self.account_event_log
            .query(account_id, limit, before, category)
    }

    pub fn pending_account_history(
        &self,
        account_id: AccountId,
        before: Option<(u64, u64)>,
        category: Option<&str>,
    ) -> Vec<HistoryEvent> {
        self.account_event_log
            .query_pending(account_id, before, category)
    }

    pub fn apply_resolution(
        &mut self,
        market_id: MarketId,
        payout_nanos: i64,
        pre_settle_positions: Vec<(AccountId, u8, i64)>,
    ) {
        self.cost_basis_tracker
            .apply_resolution(market_id, payout_nanos, pre_settle_positions);
    }

    pub fn trader_snapshot(&self) -> TraderTrackerSnapshot {
        self.trader_tracker.snapshot()
    }

    pub fn price_volume_snapshot(&self) -> PriceTrackerVolumeSnapshot {
        self.price_tracker.volume_extensions_snapshot()
    }

    pub fn price_clearing_history_snapshot(&self) -> PriceTrackerClearingHistorySnapshot {
        self.price_tracker.clearing_history_snapshot()
    }

    pub fn liquidity_snapshot(&self) -> LiquidityTrackerSnapshot {
        self.liquidity_tracker.snapshot()
    }

    pub fn order_stats_snapshot(&self) -> OrderStatsTrackerSnapshot {
        self.order_stats_tracker.snapshot()
    }

    pub fn welfare_snapshot(&self) -> WelfareTrackerSnapshot {
        self.welfare_tracker.snapshot()
    }

    pub fn cost_basis_snapshot(&self) -> CostBasisTrackerSnapshot {
        self.cost_basis_tracker.snapshot()
    }

    pub fn first_deposit_snapshot(&self) -> HashMap<AccountId, u64> {
        self.first_deposit_ms.clone()
    }

    #[cfg(test)]
    pub(crate) fn price_tracker_mut(&mut self) -> &mut PriceTracker {
        &mut self.price_tracker
    }

    pub fn clear_offblock_pending(&mut self) {
        self.fill_recorder.clear_pending();
        self.price_tracker.clear_pending();
        self.equity_tracker.clear_pending();
        self.account_event_log.clear_pending();
    }

    pub fn seed_equity_known(&mut self, ids: impl IntoIterator<Item = AccountId>) {
        self.equity_tracker.seed_known(ids);
    }

    pub fn memory_stats(&self) -> crate::sequencer::AnalyticsMemoryStats {
        crate::sequencer::AnalyticsMemoryStats {
            equity_known_accounts: self.equity_tracker.known_account_count(),
            equity_cached_accounts: self.equity_tracker.retained_account_count(),
            equity_cached_points: self.equity_tracker.retained_point_count(),
            equity_pending_points: self.equity_tracker.pending().len(),
            equity_points_per_account_capacity: self.equity_tracker.retention_per_account(),
            history_cached_accounts: self.account_event_log.retained_account_count(),
            history_cached_events: self.account_event_log.retained_event_count(),
            history_pending_events: self.account_event_log.pending().len(),
            history_events_per_account_capacity: self.account_event_log.retention_per_account(),
            history_event_next_seq: self.account_event_log.next_seq(),
        }
    }
}
