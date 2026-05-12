//! Records fill history per account for querying.

use std::collections::HashMap;

use matching_engine::{compute_fill_settlement, Fill, MarketId, Order};

use crate::account::AccountId;
use crate::market_info::AccountFillRecord;

/// Bounded in-memory fill history retained per account.
///
/// Persistent deployments can keep full fill history in the store; the actor's
/// hot state only needs a recent serving window for API queries.
const MAX_FILL_HISTORY_PER_ACCOUNT: usize = 5_000;

/// Records fill history per account.
#[derive(Clone, Default)]
pub struct FillRecorder {
    account_fills: HashMap<AccountId, Vec<AccountFillRecord>>,
}

impl FillRecorder {
    pub fn new() -> Self {
        Self {
            account_fills: HashMap::new(),
        }
    }

    pub fn restore(records: Vec<(AccountId, AccountFillRecord)>) -> Self {
        let mut account_fills: HashMap<AccountId, Vec<AccountFillRecord>> = HashMap::new();
        for (account_id, record) in records {
            account_fills.entry(account_id).or_default().push(record);
        }
        for fills in account_fills.values_mut() {
            fills.sort_by_key(|record| (record.block_height, record.order_id));
            trim_account_fills(fills);
        }
        Self { account_fills }
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
            trim_account_fills(records);
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

fn trim_account_fills(fills: &mut Vec<AccountFillRecord>) {
    let overflow = fills.len().saturating_sub(MAX_FILL_HISTORY_PER_ACCOUNT);
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

        let mut recorder = FillRecorder::new();
        for height in 1..=(MAX_FILL_HISTORY_PER_ACCOUNT as u64 + 5) {
            let mut fill = Fill::new(order.id, 1, NANOS_PER_DOLLAR / 2);
            fill.account_id = 42;
            recorder.record_fills(&[fill], &orders, height, height * 1_000);
        }

        let fills =
            recorder.account_fills(AccountId(42), None, MAX_FILL_HISTORY_PER_ACCOUNT + 10, 0);
        assert_eq!(fills.len(), MAX_FILL_HISTORY_PER_ACCOUNT);
        assert_eq!(fills.first().unwrap().block_height, 6);
        assert_eq!(
            fills.last().unwrap().block_height,
            MAX_FILL_HISTORY_PER_ACCOUNT as u64 + 5
        );
    }

    #[test]
    fn fill_history_is_bounded_per_account_on_restore() {
        let records = (1..=(MAX_FILL_HISTORY_PER_ACCOUNT as u64 + 5))
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

        let recorder = FillRecorder::restore(records);
        let fills = recorder.account_fills(AccountId(7), None, usize::MAX, 0);
        assert_eq!(fills.len(), MAX_FILL_HISTORY_PER_ACCOUNT);
        assert_eq!(fills.first().unwrap().block_height, 6);
    }
}
