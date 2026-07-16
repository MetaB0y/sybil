//! Builds block-local fill facts and current per-account fill totals.
//!
//! Durable/queryable fill history belongs to `sybil-history`. This component
//! retains only the uncommitted export delta needed by the next fenced history
//! batch plus the all-time counters used by current product views.

use std::collections::HashMap;

use matching_engine::{Fill, Order, compute_fill_settlement};

use crate::account::{AccountId, AccountStore};
use crate::aggregates::CostBasisTracker;
use crate::market_info::AccountFillRecord;

/// Accumulates current fill totals and the next committed history delta.
#[derive(Clone)]
pub struct FillRecorder {
    /// Records appended since the last committed block snapshot.
    pending_delta: Vec<(AccountId, AccountFillRecord)>,
    /// All-time fill count per account, excluding `AccountId::MINT`. One
    /// fill record bumps the per-account counter once regardless of how
    /// many markets the underlying order touches (the fill IS the trade
    /// event — multi-market orders still produce one fill per match).
    total_count: HashMap<AccountId, u64>,
}

impl Default for FillRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl FillRecorder {
    pub fn new() -> Self {
        Self {
            pending_delta: Vec::new(),
            total_count: HashMap::new(),
        }
    }

    pub fn restore_with_counts(total_count: HashMap<AccountId, u64>) -> Self {
        Self {
            pending_delta: Vec::new(),
            total_count,
        }
    }

    /// Fills appended since the last committed off-block snapshot.
    pub fn pending_delta(&self) -> &[(AccountId, AccountFillRecord)] {
        &self.pending_delta
    }

    pub fn clear_pending(&mut self) {
        self.pending_delta.clear();
    }

    /// All-time fill counts per account (MINT excluded by construction).
    pub fn total_counts(&self) -> &HashMap<AccountId, u64> {
        &self.total_count
    }

    /// All-time fill count for one account. Returns 0 for accounts that
    /// never traded.
    pub fn total_fills(&self, account_id: AccountId) -> u64 {
        self.total_count.get(&account_id).copied().unwrap_or(0)
    }

    /// Record fills from a block into its export delta. Also drives
    /// the cost-basis tracker (C1) so realized PnL accumulates in lockstep
    /// with the fill window. The tracker reaches into `accounts` for
    /// post-fill position state (the prior position is `current - delta`).
    #[allow(clippy::too_many_arguments)]
    pub fn record_fills(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        height: u64,
        timestamp_ms: u64,
        cost_basis_tracker: &mut CostBasisTracker,
        accounts: &AccountStore,
        event_log: &mut crate::aggregates::AccountEventLog,
    ) {
        for fill in fills {
            if fill.fill_qty.0 == 0 {
                continue;
            }
            let account_id = AccountId(fill.account_id);
            let Some(order) = orders.get(&fill.order_id) else {
                continue;
            };

            // Use shared settlement function for position deltas
            let position_deltas = match compute_fill_settlement(order, fill) {
                Some(delta) => delta.position_deltas,
                None => Vec::new(),
            };

            let realized_before = cost_basis_tracker.realized_pnl(account_id);

            // Cost-basis hook (MINT short-circuits inside apply_fill). Runs
            // before the bounded-history push so a tracker panic doesn't
            // leave the recorder partially advanced.
            if let Some(account) = accounts.get(account_id) {
                cost_basis_tracker.apply_fill(
                    account_id,
                    &position_deltas,
                    fill.fill_price.0 as i64,
                    account,
                );
            }

            if account_id != AccountId::MINT {
                use crate::aggregates::{HistoryEvent, HistoryKind};
                let realized_after = cost_basis_tracker.realized_pnl(account_id);
                let kind = if fill.fill_qty == order.max_fill {
                    HistoryKind::Filled
                } else {
                    HistoryKind::PartialFill
                };
                let mut e = HistoryEvent::new(account_id, kind, height, timestamp_ms);
                e.order_id = Some(fill.order_id);
                e.qty = Some(fill.fill_qty.0);
                e.price_nanos = Some(fill.fill_price.0);
                let (mid, side, outcome, cash) =
                    crate::aggregates::fill_facets(&position_deltas, fill.fill_price.0);
                e.market_id = mid;
                e.side = side;
                e.outcome = outcome;
                e.amount_nanos = Some(cash);
                let delta = realized_after - realized_before;
                e.realized_pnl_nanos = (delta != 0).then_some(delta);
                event_log.append(e);
            }

            let record = AccountFillRecord {
                order_id: fill.order_id,
                fill_qty: fill.fill_qty.0,
                fill_price: fill.fill_price,
                block_height: height,
                timestamp_ms,
                position_deltas,
            };
            self.pending_delta.push((account_id, record));

            // All-time counter: skip MINT (system account, not a user trade).
            if account_id != AccountId::MINT {
                *self.total_count.entry(account_id).or_insert(0) += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{Fill, MarketSet, NANOS_PER_DOLLAR, Nanos, Qty, outcome_buy};

    #[test]
    fn pending_delta_captures_fill_facts_until_commit() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("cap0");
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut recorder = FillRecorder::new();
        let mut cb = CostBasisTracker::new();
        let accounts = AccountStore::new();
        let mut log = crate::aggregates::AccountEventLog::new();
        let mut fill = Fill::new(order.id, Qty(1), Nanos(NANOS_PER_DOLLAR / 2));
        fill.account_id = 42;
        recorder.record_fills(&[fill], &orders, 1, 1_000, &mut cb, &accounts, &mut log);

        assert_eq!(recorder.pending_delta().len(), 1);
        assert_eq!(recorder.pending_delta()[0].1.block_height, 1);
        recorder.clear_pending();
        assert!(recorder.pending_delta().is_empty());
    }

    #[test]
    fn total_count_bumps_per_fill_not_per_market() {
        // Even for a multi-market order, one fill record = one counter bump.
        // (Multi-market orders still produce one fill at a time — fill_qty
        // is per-order, not per-market — so the assertion here is that a
        // single record_fills call with a single Fill bumps by exactly 1.)
        let mut markets = MarketSet::new();
        let m = markets.add_binary("M");
        let mut order = outcome_buy(&markets, 1, m, 0, NANOS_PER_DOLLAR / 2, 4);
        // Pretend the order spans 2 markets — the counter should still
        // bump by 1 per fill since multi-market accounting lives in the
        // welfare/volume layer, not the trade-count layer.
        order.num_markets = 2;
        let m2 = markets.add_binary("M2");
        order.markets[1] = m2;
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut recorder = FillRecorder::new();
        let mut cb = CostBasisTracker::new();
        let accounts = AccountStore::new();
        let mut fill = Fill::new(order.id, Qty(4), Nanos(NANOS_PER_DOLLAR / 2));
        fill.account_id = 42;
        let mut log = crate::aggregates::AccountEventLog::new();
        recorder.record_fills(&[fill], &orders, 1, 1_000, &mut cb, &accounts, &mut log);

        assert_eq!(recorder.total_fills(AccountId(42)), 1);
    }

    #[test]
    fn total_count_excludes_mint() {
        let mut markets = MarketSet::new();
        let m = markets.add_binary("M");
        let order = outcome_buy(&markets, 1, m, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut recorder = FillRecorder::new();
        let mut cb = CostBasisTracker::new();
        let accounts = AccountStore::new();
        let mut fill = Fill::new(order.id, Qty(1), Nanos(NANOS_PER_DOLLAR / 2));
        fill.account_id = AccountId::MINT.0;
        let mut log = crate::aggregates::AccountEventLog::new();
        recorder.record_fills(&[fill], &orders, 1, 1_000, &mut cb, &accounts, &mut log);

        // MINT facts may still be exported, but current user trade counts must
        // not include the system account.
        assert_eq!(recorder.total_fills(AccountId::MINT), 0);
    }

    #[test]
    fn total_count_accumulates_across_blocks() {
        let mut markets = MarketSet::new();
        let m = markets.add_binary("M");
        let order = outcome_buy(&markets, 1, m, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut recorder = FillRecorder::new();
        let mut cb = CostBasisTracker::new();
        let accounts = AccountStore::new();
        let mut log = crate::aggregates::AccountEventLog::new();
        for h in 1..=5 {
            let mut fill = Fill::new(order.id, Qty(1), Nanos(NANOS_PER_DOLLAR / 2));
            fill.account_id = 42;
            recorder.record_fills(&[fill], &orders, h, h * 1_000, &mut cb, &accounts, &mut log);
        }
        assert_eq!(recorder.total_fills(AccountId(42)), 5);
    }
}
