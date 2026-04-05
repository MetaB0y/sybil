//! Records fill history per account for querying.

use std::collections::HashMap;

use matching_engine::{compute_fill_settlement, Fill, MarketId, Order};

use crate::account::AccountId;
use crate::market_info::AccountFillRecord;

/// Records fill history per account.
pub struct FillRecorder {
    account_fills: HashMap<AccountId, Vec<AccountFillRecord>>,
}

impl FillRecorder {
    pub fn new() -> Self {
        Self {
            account_fills: HashMap::new(),
        }
    }

    /// Record fills from a block into per-account fill history.
    pub fn record_fills(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        order_account_map: &HashMap<u64, AccountId>,
        height: u64,
        timestamp_ms: u64,
    ) {
        for fill in fills {
            if fill.fill_qty == 0 {
                continue;
            }
            let Some(&account_id) = order_account_map.get(&fill.order_id) else {
                continue;
            };
            let Some(order) = orders.get(&fill.order_id) else {
                continue;
            };

            // Use shared settlement function for position deltas
            let position_deltas = match compute_fill_settlement(order, fill) {
                Some(delta) => delta.position_deltas,
                None => Vec::new(),
            };

            self.account_fills
                .entry(account_id)
                .or_default()
                .push(AccountFillRecord {
                    order_id: fill.order_id,
                    fill_qty: fill.fill_qty,
                    fill_price: fill.fill_price,
                    block_height: height,
                    timestamp_ms,
                    position_deltas,
                });
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
