//! Records fill history per account for querying.

use std::collections::HashMap;

use matching_engine::{compute_fill_settlement, Fill, MarketId, Order};

use crate::account::AccountId;
use crate::market_info::AccountFillRecord;

/// Bounded in-memory fill history retained per account.
///
/// Persistent deployments can keep full fill history in the store; the actor's
/// hot state only needs a recent serving window for API queries.
pub const DEFAULT_MAX_FILL_HISTORY_PER_ACCOUNT: usize = 5_000;

/// Records fill history per account.
#[derive(Clone)]
pub struct FillRecorder {
    account_fills: HashMap<AccountId, Vec<AccountFillRecord>>,
    max_history_per_account: usize,
    /// All-time fill count per account, excluding `AccountId::MINT`. One
    /// fill record bumps the per-account counter once regardless of how
    /// many markets the underlying order touches (the fill IS the trade
    /// event — multi-market orders still produce one fill per match).
    /// Survives the `MAX_FILL_HISTORY_PER_ACCOUNT` trim, which only drops
    /// the bounded `account_fills` records.
    total_count: HashMap<AccountId, u64>,
}

impl Default for FillRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl FillRecorder {
    pub fn new() -> Self {
        Self::with_retention(DEFAULT_MAX_FILL_HISTORY_PER_ACCOUNT)
    }

    pub fn with_retention(max_history_per_account: usize) -> Self {
        Self {
            account_fills: HashMap::new(),
            max_history_per_account,
            total_count: HashMap::new(),
        }
    }

    pub fn restore(records: Vec<(AccountId, AccountFillRecord)>) -> Self {
        Self::restore_with_retention(records, DEFAULT_MAX_FILL_HISTORY_PER_ACCOUNT)
    }

    pub fn restore_with_retention(
        records: Vec<(AccountId, AccountFillRecord)>,
        max_history_per_account: usize,
    ) -> Self {
        let mut account_fills: HashMap<AccountId, Vec<AccountFillRecord>> = HashMap::new();
        for (account_id, record) in records {
            account_fills.entry(account_id).or_default().push(record);
        }
        for fills in account_fills.values_mut() {
            fills.sort_by_key(|record| (record.block_height, record.order_id));
            trim_account_fills(fills, max_history_per_account);
        }
        Self {
            account_fills,
            max_history_per_account,
            // Cold-start the total counter from the visible window. After
            // trim this under-reports, which is acceptable until snapshot
            // round-tripping for total_count lands alongside C1.
            total_count: HashMap::new(),
        }
    }

    /// Restore both the bounded fill window AND the all-time fill counter
    /// in one call. Used by the persistence path so total_count survives
    /// restart even when the visible window has been trimmed.
    pub fn restore_with_counts(
        records: Vec<(AccountId, AccountFillRecord)>,
        total_count: HashMap<AccountId, u64>,
        max_history_per_account: usize,
    ) -> Self {
        let mut recorder = Self::restore_with_retention(records, max_history_per_account);
        recorder.total_count = total_count;
        recorder
    }

    pub fn snapshot(&self) -> Vec<(AccountId, AccountFillRecord)> {
        let mut records: Vec<_> = self
            .account_fills
            .iter()
            .flat_map(|(&account_id, fills)| {
                fills
                    .iter()
                    .cloned()
                    .map(move |record| (account_id, record))
            })
            .collect();
        records.sort_by_key(|(account_id, record)| {
            (account_id.0, record.block_height, record.order_id)
        });
        records
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

    /// Record fills from a block into per-account fill history.
    pub fn record_fills(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        height: u64,
        timestamp_ms: u64,
    ) {
        for fill in fills {
            if fill.fill_qty == 0 {
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

            let records = self.account_fills.entry(account_id).or_default();
            records.push(AccountFillRecord {
                order_id: fill.order_id,
                fill_qty: fill.fill_qty,
                fill_price: fill.fill_price,
                block_height: height,
                timestamp_ms,
                position_deltas,
            });
            trim_account_fills(records, self.max_history_per_account);

            // All-time counter: skip MINT (system account, not a user trade).
            if account_id != AccountId::MINT {
                *self.total_count.entry(account_id).or_insert(0) += 1;
            }
        }
    }

    /// Get fill records for an account, optionally filtered by market.
    pub fn account_fills(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Vec<AccountFillRecord> {
        let Some(fills) = self.account_fills.get(&account_id) else {
            return Vec::new();
        };
        fills
            .iter()
            .filter(|f| {
                market_id_filter
                    .is_none_or(|mid| f.position_deltas.iter().any(|(m, _, _)| *m == mid))
            })
            .skip(offset)
            .take(limit)
            .cloned()
            .collect()
    }
}

fn trim_account_fills(fills: &mut Vec<AccountFillRecord>, max_history_per_account: usize) {
    let overflow = fills.len().saturating_sub(max_history_per_account);
    if overflow > 0 {
        fills.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{outcome_buy, Fill, MarketSet, NANOS_PER_DOLLAR};

    #[test]
    fn fill_history_is_bounded_per_account_on_record() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("bounded");
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let max_fills = 8;
        let mut recorder = FillRecorder::with_retention(max_fills);
        for height in 1..=(max_fills as u64 + 5) {
            let mut fill = Fill::new(order.id, 1, NANOS_PER_DOLLAR / 2);
            fill.account_id = 42;
            recorder.record_fills(&[fill], &orders, height, height * 1_000);
        }

        let fills = recorder.account_fills(AccountId(42), None, max_fills + 10, 0);
        assert_eq!(fills.len(), max_fills);
        assert_eq!(fills.first().unwrap().block_height, 6);
        assert_eq!(fills.last().unwrap().block_height, max_fills as u64 + 5);
    }

    #[test]
    fn fill_history_is_bounded_per_account_on_restore() {
        let max_fills = 8;
        let records = (1..=(max_fills as u64 + 5))
            .map(|height| {
                (
                    AccountId(7),
                    AccountFillRecord {
                        order_id: height,
                        fill_qty: 1,
                        fill_price: NANOS_PER_DOLLAR / 2,
                        block_height: height,
                        timestamp_ms: height * 1_000,
                        position_deltas: Vec::new(),
                    },
                )
            })
            .collect();

        let recorder = FillRecorder::restore_with_retention(records, max_fills);
        let fills = recorder.account_fills(AccountId(7), None, usize::MAX, 0);
        assert_eq!(fills.len(), max_fills);
        assert_eq!(fills.first().unwrap().block_height, 6);
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
        let mut fill = Fill::new(order.id, 4, NANOS_PER_DOLLAR / 2);
        fill.account_id = 42;
        recorder.record_fills(&[fill], &orders, 1, 1_000);

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
        let mut fill = Fill::new(order.id, 1, NANOS_PER_DOLLAR / 2);
        fill.account_id = AccountId::MINT.0;
        recorder.record_fills(&[fill], &orders, 1, 1_000);

        // MINT fills still land in account_fills (we may want to query
        // them) but total_count must not include them — MINT is a system
        // account, not a trader.
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
        for h in 1..=5 {
            let mut fill = Fill::new(order.id, 1, NANOS_PER_DOLLAR / 2);
            fill.account_id = 42;
            recorder.record_fills(&[fill], &orders, h, h * 1_000);
        }
        assert_eq!(recorder.total_fills(AccountId(42)), 5);
    }
}
