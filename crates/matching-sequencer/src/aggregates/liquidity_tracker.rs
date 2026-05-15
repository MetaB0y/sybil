//! Off-block per-market liquidity scoring (B4).
//!
//! For each block, scores the *near-the-money* resting-book depth on every
//! market that has a clearing price: sum of `limit_price * max_fill` over
//! single-market resting orders whose `limit_price` falls within ±band of
//! the market's midprice (YES clearing price in binary markets). The
//! per-market score lands in a ring of the last `LIQUIDITY_RING_CAP` blocks
//! so the FE can read a smoothed average.
//!
//! Multi-market orders are excluded entirely — their `limit_price` is the
//! bundle total, not attributable to one market. MM orders never sit in the
//! resting book (they live in pending_bundles until the solver clears
//! them), so no MM-specific gating is needed here; the resting-book walk
//! captures only real participation.

use std::collections::{HashMap, VecDeque};

use matching_engine::{MarketId, Nanos};
use serde::{Deserialize, Serialize};

use crate::order_book::OrderBook;

/// Ring length per market — 10 blocks ≈ 20s at 2s cadence.
pub const LIQUIDITY_RING_CAP: usize = 10;

/// Persisted slice of [`LiquidityTracker`]: per-market rolling rings plus
/// the band width in effect when the most recent update landed (so the FE
/// can detect mid-flight band changes and label accordingly).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LiquidityTrackerSnapshot {
    pub last_n_per_market: HashMap<MarketId, VecDeque<u64>>,
    #[serde(default)]
    pub band_nanos_at_last_update: u64,
}

/// Off-block liquidity scoring tracker. See module doc.
#[derive(Clone, Default)]
pub struct LiquidityTracker {
    last_n_per_market: HashMap<MarketId, VecDeque<u64>>,
    band_nanos_at_last_update: u64,
}

impl LiquidityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restore from a persisted snapshot. Cold start uses
    /// `LiquidityTrackerSnapshot::default()` which is a no-op here.
    pub fn restore(snapshot: LiquidityTrackerSnapshot) -> Self {
        Self {
            last_n_per_market: snapshot.last_n_per_market,
            band_nanos_at_last_update: snapshot.band_nanos_at_last_update,
        }
    }

    pub fn snapshot(&self) -> LiquidityTrackerSnapshot {
        LiquidityTrackerSnapshot {
            last_n_per_market: self.last_n_per_market.clone(),
            band_nanos_at_last_update: self.band_nanos_at_last_update,
        }
    }

    /// Score the current order book at end-of-block. Pushes one value per
    /// known market into its ring (quiet markets get 0).
    pub fn record_block(
        &mut self,
        book: &OrderBook,
        midprices: &HashMap<MarketId, Vec<Nanos>>,
        band_nanos: u64,
    ) {
        // First pass: aggregate near-the-money depth per market from the
        // resting book in O(N) over orders.
        let mut depth_by_market: HashMap<MarketId, u64> = HashMap::new();
        for (order, _account_id) in book.resting_orders() {
            if order.num_markets != 1 {
                continue;
            }
            let market = order.markets[0];
            let Some(prices) = midprices.get(&market) else {
                continue;
            };
            let mid = prices.first().copied().unwrap_or(0);
            if mid == 0 {
                continue;
            }
            let band_lo = mid.saturating_sub(band_nanos);
            let band_hi = mid.saturating_add(band_nanos);
            if order.limit_price >= band_lo && order.limit_price <= band_hi {
                let value = order.limit_price.saturating_mul(order.max_fill);
                let entry = depth_by_market.entry(market).or_insert(0);
                *entry = entry.saturating_add(value);
            }
        }

        // Second pass: push into per-market rings for every market that has
        // a clearing price (so the average for a quiet market stays low
        // rather than stuck on the last non-zero value).
        for &market in midprices.keys() {
            let depth = depth_by_market.get(&market).copied().unwrap_or(0);
            let ring = self.last_n_per_market.entry(market).or_default();
            ring.push_back(depth);
            while ring.len() > LIQUIDITY_RING_CAP {
                ring.pop_front();
            }
        }

        self.band_nanos_at_last_update = band_nanos;
    }

    /// Average over the last `n` ring entries (capped at the ring length).
    /// Returns 0 when the market has never been recorded.
    pub fn avg_last_n(&self, market_id: MarketId, n: usize) -> u64 {
        let Some(ring) = self.last_n_per_market.get(&market_id) else {
            return 0;
        };
        if ring.is_empty() || n == 0 {
            return 0;
        }
        let take = n.min(ring.len());
        let sum: u64 = ring
            .iter()
            .rev()
            .take(take)
            .copied()
            .fold(0u64, |acc, v| acc.saturating_add(v));
        sum / (take as u64)
    }

    /// Most recent entry pushed for `market_id`, or 0 if none yet.
    pub fn current(&self, market_id: MarketId) -> u64 {
        self.last_n_per_market
            .get(&market_id)
            .and_then(|ring| ring.back().copied())
            .unwrap_or(0)
    }

    /// Band width snapshotted alongside the most recent ring update. The FE
    /// compares wire `liquidity_band_nanos` against the live config; if
    /// they diverge the label can show "(old band)" until the ring rotates.
    pub fn band_nanos_at_last_update(&self) -> u64 {
        self.band_nanos_at_last_update
    }

    /// Bulk view: `avg_last_n(m, n)` for every market the tracker knows about.
    /// Used by `list_markets` so the response is a single round-trip.
    pub fn all_avg_last_n(&self, n: usize) -> HashMap<MarketId, u64> {
        self.last_n_per_market
            .keys()
            .map(|&m| (m, self.avg_last_n(m, n)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, spread, MarketId, MarketSet, NANOS_PER_DOLLAR};

    fn two_market_setup() -> (
        MarketSet,
        AccountStore,
        crate::account::AccountId,
        MarketId,
        MarketId,
    ) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("liq_m0");
        let m1 = markets.add_binary("liq_m1");
        let mut accounts = AccountStore::new();
        let trader = accounts.create_account(10_000 * NANOS_PER_DOLLAR as i64);
        (markets, accounts, trader, m0, m1)
    }

    fn admit(
        book: &mut OrderBook,
        accounts: &AccountStore,
        order: matching_engine::Order,
        trader: crate::account::AccountId,
    ) {
        let account = accounts.get(trader).expect("trader exists");
        book.accept(order, trader, account, 1).expect("admit");
    }

    /// Multi-market resting orders (spreads/bundles) are excluded from the
    /// per-market score; single-market orders inside the band contribute.
    #[test]
    fn record_block_excludes_multi_market() {
        let (markets, accounts, trader, m0, m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);

        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, 4),
            trader,
        );
        admit(
            &mut book,
            &accounts,
            spread(&markets, 2, m0, m1, mid_yes, 4),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);
        midprices.insert(m1, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);

        tracker.record_block(&book, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), mid_yes.saturating_mul(4));
        assert_eq!(tracker.avg_last_n(m0, 10), mid_yes.saturating_mul(4));
        assert_eq!(tracker.current(m1), 0, "spread not credited to m1 either");
    }

    /// 12 consecutive record_blocks → ring caps at `LIQUIDITY_RING_CAP`,
    /// retaining the latest entries.
    #[test]
    fn ring_caps_at_10() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, 1),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);

        for _ in 0..12 {
            tracker.record_block(&book, &midprices, 50_000_000);
        }

        let ring = tracker.last_n_per_market.get(&m0).expect("ring populated");
        assert_eq!(ring.len(), LIQUIDITY_RING_CAP);
        for v in ring {
            assert_eq!(*v, mid_yes.saturating_mul(1));
        }
    }

    /// Orders outside the ±band don't count.
    #[test]
    fn order_outside_band_excluded() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes - 100_000_000, 4),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);
        tracker.record_block(&book, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), 0);
    }

    /// Snapshot ↔ restore is byte-equivalent.
    #[test]
    fn liquidity_tracker_snapshot_roundtrip() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, 3),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);
        for _ in 0..3 {
            tracker.record_block(&book, &midprices, 50_000_000);
        }

        let snapshot = tracker.snapshot();
        let restored = LiquidityTracker::restore(snapshot);
        assert_eq!(restored.avg_last_n(m0, 10), tracker.avg_last_n(m0, 10));
        assert_eq!(
            restored.band_nanos_at_last_update(),
            tracker.band_nanos_at_last_update()
        );
    }

    /// Markets with a clearing price but no near-the-money resting orders
    /// get 0s pushed into their ring — `avg_last_n` reflects the quiet state.
    #[test]
    fn quiet_markets_push_zero() {
        let (_markets, _accounts, _trader, m0, _m1) = two_market_setup();
        let book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![mid_yes, NANOS_PER_DOLLAR - mid_yes]);

        for _ in 0..5 {
            tracker.record_block(&book, &midprices, 50_000_000);
        }

        let ring = tracker.last_n_per_market.get(&m0).expect("market present");
        assert_eq!(ring.len(), 5);
        for v in ring {
            assert_eq!(*v, 0);
        }
        assert_eq!(tracker.avg_last_n(m0, 10), 0);
    }
}
