use std::collections::HashMap;

use matching_engine::{MarketGroup, MarketSet, Order};

use crate::account::{AccountId, AccountStore};

/// A pending order that persists across batches until filled or expired.
pub struct PendingOrder {
    pub order: Order,
    pub account_id: AccountId,
    /// Batch/block number when this order was created.
    pub created_at_batch: u64,
}

/// Core sequencer state: accounts, pending orders, block metadata.
pub struct SequencerState {
    pub accounts: AccountStore,
    pub markets: MarketSet,
    pub market_groups: Vec<MarketGroup>,
    pub order_account_map: HashMap<u64, AccountId>,
    pub next_order_id: u64,
    pub pending_orders: Vec<PendingOrder>,
    pub height: u64,
    pub order_ttl: u64,
    /// Track when each order was originally created: order_id -> batch/block number.
    pub order_created_at: HashMap<u64, u64>,
}

impl SequencerState {
    pub fn new(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
    ) -> Self {
        Self {
            accounts,
            markets,
            market_groups,
            order_account_map: HashMap::new(),
            next_order_id: 1,
            pending_orders: Vec::new(),
            height: 0,
            order_ttl: 3,
            order_created_at: HashMap::new(),
        }
    }
}
