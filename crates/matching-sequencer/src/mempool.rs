use std::collections::BTreeMap;

use matching_engine::MarketId;

use crate::error::SequencerError;
use crate::sequencer::OrderSubmission;

/// Configuration for mempool capacity limits.
#[derive(Clone)]
pub struct MempoolConfig {
    /// Max orders drained per market per block.
    pub per_market_limit: usize,
    /// Max bundles drained per block.
    pub bundle_limit: usize,
    /// Max total orders waiting across all pools.
    pub max_total: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            per_market_limit: 100,
            bundle_limit: 50,
            max_total: 10_000,
        }
    }
}

/// Per-market order pools with capacity limits.
///
/// Single-market orders are keyed by their market. Multi-market orders
/// (bundles, spreads) go into a separate bundle pool. At drain time,
/// up to `per_market_limit` orders from each active market and up to
/// `bundle_limit` bundles are collected. This bounds solver input size
/// and keeps block production on schedule regardless of order volume.
pub struct Mempool {
    /// Single-market orders, keyed by their market.
    market_pools: BTreeMap<MarketId, Vec<OrderSubmission>>,
    /// Multi-market orders (bundles, spreads).
    bundle_pool: Vec<OrderSubmission>,
    /// Max orders drained per market per block.
    per_market_limit: usize,
    /// Max bundles drained per block.
    bundle_limit: usize,
    /// Max total orders waiting across all pools.
    max_total: usize,
}

impl Mempool {
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            market_pools: BTreeMap::new(),
            bundle_pool: Vec::new(),
            per_market_limit: config.per_market_limit,
            bundle_limit: config.bundle_limit,
            max_total: config.max_total,
        }
    }

    /// Submit an order submission to the mempool.
    ///
    /// Submissions with a single order targeting one market go into
    /// that market's pool. Everything else goes into the bundle pool.
    pub fn submit(&mut self, submission: OrderSubmission) -> Result<(), SequencerError> {
        if self.len() >= self.max_total {
            return Err(SequencerError::MempoolFull);
        }

        // Classify: single-market order vs bundle
        if submission.orders.len() == 1 && submission.mm_constraint.is_none() {
            let order = &submission.orders[0];
            if order.num_markets == 1 {
                let market = order.markets[0];
                self.market_pools
                    .entry(market)
                    .or_default()
                    .push(submission);
                return Ok(());
            }
        }

        self.bundle_pool.push(submission);
        Ok(())
    }

    /// Drain up to `per_market_limit` from each market pool + up to
    /// `bundle_limit` from the bundle pool.
    pub fn drain(&mut self) -> Vec<OrderSubmission> {
        let mut result = Vec::new();

        // Drain from each market pool
        for pool in self.market_pools.values_mut() {
            let take = pool.len().min(self.per_market_limit);
            // Drain from the front (FIFO — oldest first)
            result.extend(pool.drain(..take));
        }

        // Remove empty pools
        self.market_pools.retain(|_, pool| !pool.is_empty());

        // Drain from bundle pool
        let bundle_take = self.bundle_pool.len().min(self.bundle_limit);
        result.extend(self.bundle_pool.drain(..bundle_take));

        result
    }

    /// Total number of submissions across all pools.
    pub fn len(&self) -> usize {
        let market_count: usize = self.market_pools.values().map(|p| p.len()).sum();
        market_count + self.bundle_pool.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountId;
    use matching_engine::{outcome_buy, MarketId, MarketSet, MmConstraint, MmId, NANOS_PER_DOLLAR};

    fn make_single_order_sub(
        markets: &MarketSet,
        market: MarketId,
        account: AccountId,
    ) -> OrderSubmission {
        OrderSubmission {
            account_id: account,
            orders: vec![outcome_buy(markets, 0, market, 0, 500_000_000, 1)],
            mm_constraint: None,
        }
    }

    fn make_mm_sub(markets: &MarketSet, market: MarketId, account: AccountId) -> OrderSubmission {
        let order = outcome_buy(markets, 0, market, 0, 500_000_000, 1);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);
        OrderSubmission {
            account_id: account,
            orders: vec![order],
            mm_constraint: Some(constraint),
        }
    }

    #[test]
    fn test_submit_and_drain_single_market() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let aid = AccountId(0);

        let mut pool = Mempool::new(MempoolConfig::default());
        pool.submit(make_single_order_sub(&markets, m0, aid))
            .unwrap();
        pool.submit(make_single_order_sub(&markets, m0, aid))
            .unwrap();

        assert_eq!(pool.len(), 2);
        let drained = pool.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_per_market_limit() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let aid = AccountId(0);

        let mut pool = Mempool::new(MempoolConfig {
            per_market_limit: 2,
            bundle_limit: 50,
            max_total: 10_000,
        });

        for _ in 0..5 {
            pool.submit(make_single_order_sub(&markets, m0, aid))
                .unwrap();
        }

        assert_eq!(pool.len(), 5);
        let drained = pool.drain();
        assert_eq!(drained.len(), 2); // Limited to 2
        assert_eq!(pool.len(), 3); // 3 remain
    }

    #[test]
    fn test_multiple_markets_drain_independently() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("A");
        let m1 = markets.add_binary("B");
        let aid = AccountId(0);

        let mut pool = Mempool::new(MempoolConfig {
            per_market_limit: 2,
            bundle_limit: 50,
            max_total: 10_000,
        });

        for _ in 0..3 {
            pool.submit(make_single_order_sub(&markets, m0, aid))
                .unwrap();
        }
        for _ in 0..3 {
            pool.submit(make_single_order_sub(&markets, m1, aid))
                .unwrap();
        }

        assert_eq!(pool.len(), 6);
        let drained = pool.drain();
        assert_eq!(drained.len(), 4); // 2 from m0 + 2 from m1
        assert_eq!(pool.len(), 2); // 1 remaining from each
    }

    #[test]
    fn test_bundle_limit() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let aid = AccountId(0);

        let mut pool = Mempool::new(MempoolConfig {
            per_market_limit: 100,
            bundle_limit: 2,
            max_total: 10_000,
        });

        // MM orders go to bundle pool
        for _ in 0..5 {
            pool.submit(make_mm_sub(&markets, m0, aid)).unwrap();
        }

        assert_eq!(pool.len(), 5);
        let drained = pool.drain();
        assert_eq!(drained.len(), 2); // Limited to 2 bundles
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn test_max_total_rejects() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let aid = AccountId(0);

        let mut pool = Mempool::new(MempoolConfig {
            per_market_limit: 100,
            bundle_limit: 50,
            max_total: 3,
        });

        pool.submit(make_single_order_sub(&markets, m0, aid))
            .unwrap();
        pool.submit(make_single_order_sub(&markets, m0, aid))
            .unwrap();
        pool.submit(make_single_order_sub(&markets, m0, aid))
            .unwrap();

        let result = pool.submit(make_single_order_sub(&markets, m0, aid));
        assert!(matches!(result, Err(SequencerError::MempoolFull)));
    }

    #[test]
    fn test_empty_drain() {
        let mut pool = Mempool::new(MempoolConfig::default());
        assert!(pool.is_empty());
        let drained = pool.drain();
        assert!(drained.is_empty());
    }

    #[test]
    fn test_fifo_order() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let mut pool = Mempool::new(MempoolConfig {
            per_market_limit: 2,
            bundle_limit: 50,
            max_total: 10_000,
        });

        // Submit from different accounts to track ordering
        for i in 0..4u64 {
            pool.submit(OrderSubmission {
                account_id: AccountId(i),
                orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .unwrap();
        }

        let drained = pool.drain();
        assert_eq!(drained.len(), 2);
        // Should be first two (FIFO)
        assert_eq!(drained[0].account_id, AccountId(0));
        assert_eq!(drained[1].account_id, AccountId(1));
    }
}
