//! Off-block per-market liquidity scoring (B4).
//!
//! For each block, scores the *near-the-money* resting-book depth on every
//! market that has a clearing price: sum of `limit_price * max_fill / SHARE_SCALE` over
//! single-market resting orders whose `limit_price` falls within ±band of
//! the market's midprice (YES clearing price in binary markets). The
//! per-market score lands in a ring of the last `LIQUIDITY_RING_CAP` blocks
//! so the FE can read a smoothed average.
//!
//! Multi-market orders are excluded entirely — their `limit_price` is the
//! bundle total, not attributable to one market. Flash market-maker (MM)
//! orders never enter the resting book, so they are passed into
//! `record_block` separately and scored in a dedicated MM pass using the
//! same band rule (by quoted `max_fill`).

use std::collections::{HashMap, VecDeque};

use matching_engine::{notional_nanos, MarketId, Nanos, Order};
use serde::{Deserialize, Serialize};

use crate::order_book::OrderBook;

/// Ring length per market — 10 blocks ≈ 100s at the 10s cadence.
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
        mm_orders: &[&Order],
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
            let mid = prices.first().copied().unwrap_or(Nanos::ZERO);
            if mid == Nanos::ZERO {
                continue;
            }
            let band_lo = mid.saturating_sub(Nanos(band_nanos));
            let band_hi = mid.saturating_add(Nanos(band_nanos));
            if order.limit_price >= band_lo && order.limit_price <= band_hi {
                let value = notional_nanos(order.limit_price, order.max_fill);
                let entry = depth_by_market.entry(market).or_insert(0);
                *entry = entry.saturating_add(value.0);
            }
        }

        // MM pass: flash MM orders never enter the book, but they provide
        // real near-the-money depth for this batch. Score them with the same
        // single-market band rule (by quoted `max_fill`).
        for order in mm_orders {
            if order.num_markets != 1 {
                continue;
            }
            let market = order.markets[0];
            let Some(prices) = midprices.get(&market) else {
                continue;
            };
            let mid = prices.first().copied().unwrap_or(Nanos::ZERO);
            if mid == Nanos::ZERO {
                continue;
            }
            let band_lo = mid.saturating_sub(Nanos(band_nanos));
            let band_hi = mid.saturating_add(Nanos(band_nanos));
            if order.limit_price >= band_lo && order.limit_price <= band_hi {
                let value = notional_nanos(order.limit_price, order.max_fill);
                let entry = depth_by_market.entry(market).or_insert(0);
                *entry = entry.saturating_add(value.0);
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

    /// Sum over the last `n` ring entries (capped at the ring length). This is
    /// the windowed near-the-money depth across recent blocks — the headline
    /// liquidity metric. Returns 0 when the market has never been recorded.
    pub fn sum_last_n(&self, market_id: MarketId, n: usize) -> u64 {
        let Some(ring) = self.last_n_per_market.get(&market_id) else {
            return 0;
        };
        if ring.is_empty() || n == 0 {
            return 0;
        }
        let take = n.min(ring.len());
        ring.iter()
            .rev()
            .take(take)
            .copied()
            .fold(0u64, |acc, v| acc.saturating_add(v))
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

    /// Bulk view: `sum_last_n(m, n)` for every market the tracker knows about.
    /// Used by `list_markets` so the response is a single round-trip.
    pub fn all_sum_last_n(&self, n: usize) -> HashMap<MarketId, u64> {
        self.last_n_per_market
            .keys()
            .map(|&m| (m, self.sum_last_n(m, n)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{
        outcome_buy, shares_to_qty, spread, MarketId, MarketSet, NANOS_PER_DOLLAR,
    };

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
        book.accept(order, trader, account, 1, 0).expect("admit");
    }

    fn q(shares: u64) -> u64 {
        shares_to_qty(shares).0
    }

    /// Multi-market orders are excluded from the per-market score;
    /// single-market orders inside the band contribute.
    #[test]
    fn record_block_excludes_multi_market() {
        let (markets, accounts, trader, m0, m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);

        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(4)),
            trader,
        );
        let multi_market = spread(&markets, 2, m0, m1, mid_yes, q(4));
        let mm_orders = [&multi_market];

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);
        midprices.insert(m1, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);

        tracker.record_block(&book, &mm_orders, &midprices, 50_000_000);

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
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(1)),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);

        for _ in 0..12 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
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
            outcome_buy(&markets, 1, m0, 0, mid_yes - 100_000_000, q(4)),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);
        tracker.record_block(&book, &[], &midprices, 50_000_000);

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
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(3)),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);
        for _ in 0..3 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
        }

        let snapshot = tracker.snapshot();
        let restored = LiquidityTracker::restore(snapshot);
        assert_eq!(restored.avg_last_n(m0, 10), tracker.avg_last_n(m0, 10));
        assert_eq!(
            restored.band_nanos_at_last_update(),
            tracker.band_nanos_at_last_update()
        );
    }

    /// MM orders never sit in the book but must still count toward liquidity.
    /// A resting order (qty 4) + an MM order (qty 6), both in-band, score as
    /// mid*(4+6).
    #[test]
    fn record_block_includes_mm_orders() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(4)),
            trader,
        );

        // Flash MM order — built but NOT accepted into the book.
        let mm = outcome_buy(&markets, 99, m0, 0, mid_yes, q(6));
        let mm_slice: Vec<&matching_engine::Order> = vec![&mm];

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);

        tracker.record_block(&book, &mm_slice, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), mid_yes.saturating_mul(10));
    }

    /// An out-of-band MM order is excluded, same as resting orders.
    #[test]
    fn record_block_excludes_out_of_band_mm() {
        let (markets, _accounts, _trader, m0, _m1) = two_market_setup();
        let book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        let mm = outcome_buy(&markets, 99, m0, 0, mid_yes - 100_000_000, q(6));
        let mm_slice: Vec<&matching_engine::Order> = vec![&mm];

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);
        tracker.record_block(&book, &mm_slice, &midprices, 50_000_000);

        assert_eq!(tracker.current(m0), 0);
    }

    /// `sum_last_n` totals the ring instead of averaging it.
    #[test]
    fn sum_last_n_totals_the_ring() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(2)),
            trader,
        );

        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);

        for _ in 0..3 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
        }
        let per_block = mid_yes.saturating_mul(2);
        assert_eq!(tracker.sum_last_n(m0, 10), per_block * 3);
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
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);

        for _ in 0..5 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
        }

        let ring = tracker.last_n_per_market.get(&m0).expect("market present");
        assert_eq!(ring.len(), 5);
        for v in ring {
            assert_eq!(*v, 0);
        }
        assert_eq!(tracker.avg_last_n(m0, 10), 0);
        assert_eq!(tracker.sum_last_n(m0, 10), 0);
    }

    /// `all_sum_last_n` returns the per-market summed ring for every known market.
    #[test]
    fn all_sum_last_n_covers_known_markets() {
        let (markets, accounts, trader, m0, _m1) = two_market_setup();
        let mut book = OrderBook::new(1_000);
        let mid_yes = NANOS_PER_DOLLAR / 2;
        admit(
            &mut book,
            &accounts,
            outcome_buy(&markets, 1, m0, 0, mid_yes, q(2)),
            trader,
        );
        let mut tracker = LiquidityTracker::new();
        let mut midprices = HashMap::new();
        midprices.insert(m0, vec![Nanos(mid_yes), Nanos(NANOS_PER_DOLLAR - mid_yes)]);
        for _ in 0..2 {
            tracker.record_block(&book, &[], &midprices, 50_000_000);
        }
        let all = tracker.all_sum_last_n(10);
        assert_eq!(all.get(&m0).copied(), Some(mid_yes.saturating_mul(2) * 2));
    }
}
