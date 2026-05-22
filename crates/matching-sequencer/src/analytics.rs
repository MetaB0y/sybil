//! In-process derived projections for API/UI surfaces.
//!
//! This state is updated synchronously by `BlockSequencer`, but it is not the
//! sequencing core: it does not validate orders, mutate reservations, settle
//! accounts, or decide block contents.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Nanos, Order};

use crate::account::{AccountId, AccountStore};
use crate::aggregates::{
    AccountEventLog, CostBasisTracker, CostBasisTrackerSnapshot, EquityTracker, HistoryEvent,
    LiquidityTracker, LiquidityTrackerSnapshot, OrderStats, OrderStatsTracker,
    OrderStatsTrackerSnapshot, TraderTracker, TraderTrackerSnapshot,
};
use crate::fill_recorder::FillRecorder;
use crate::market_info::{AccountFillRecord, PricePoint};
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
    cost_basis_tracker: CostBasisTracker,
    first_deposit_ms: HashMap<AccountId, u64>,
    equity_tracker: EquityTracker,
    account_event_log: AccountEventLog,
}

impl AnalyticsState {
    pub fn new(config: &SequencerConfig) -> Self {
        Self {
            price_tracker: PriceTracker::with_retention(config.max_price_history_points_per_market),
            fill_recorder: FillRecorder::with_retention(config.max_fill_history_per_account),
            trader_tracker: TraderTracker::new(),
            liquidity_tracker: LiquidityTracker::new(),
            order_stats_tracker: OrderStatsTracker::new(),
            cost_basis_tracker: CostBasisTracker::new(),
            first_deposit_ms: HashMap::new(),
            equity_tracker: EquityTracker::new(),
            account_event_log: AccountEventLog::new(),
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
            fill_recorder: FillRecorder::restore_with_counts(
                input.account_fills,
                input.fill_total_counts,
                config.max_fill_history_per_account,
            ),
            trader_tracker: TraderTracker::restore(input.trader_tracker),
            liquidity_tracker: LiquidityTracker::restore(input.liquidity_tracker),
            order_stats_tracker: OrderStatsTracker::restore(input.order_stats_tracker),
            cost_basis_tracker: CostBasisTracker::restore(input.cost_basis_tracker),
            first_deposit_ms: input.first_deposit_ms,
            equity_tracker: EquityTracker::new(),
            account_event_log: AccountEventLog::new(),
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
            first_deposit_ms: self.first_deposit_snapshot(),
            fill_total_counts: self.total_fill_counts(),
            cost_basis_tracker: self.cost_basis_snapshot(),
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
        self.liquidity_tracker.avg_last_n(market_id, 10)
    }

    pub fn all_liquidity_avg10(&self) -> HashMap<MarketId, u64> {
        self.liquidity_tracker.all_avg_last_n(10)
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

    pub fn cost_basis_tracker(&self) -> &CostBasisTracker {
        &self.cost_basis_tracker
    }

    pub fn first_deposit_ms(&self, account_id: AccountId) -> Option<u64> {
        self.first_deposit_ms.get(&account_id).copied()
    }

    pub fn note_first_deposit(&mut self, account_id: AccountId) {
        self.first_deposit_ms.entry(account_id).or_insert_with(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        });
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

    pub fn record_order_exit(&mut self, resting_order: &RestingOrder, timestamp_ms: u64) {
        self.order_stats_tracker
            .record_exit(resting_order, timestamp_ms);
    }

    pub fn record_liquidity(
        &mut self,
        order_book: &OrderBook,
        mm_orders: &[&Order],
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        band_nanos: u64,
    ) {
        self.liquidity_tracker
            .record_block(order_book, mm_orders, clearing_prices, band_nanos);
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
}
