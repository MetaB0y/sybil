//! Tracks clearing prices, price history, and per-market volume.

use std::collections::{HashMap, HashSet, VecDeque};

use matching_engine::{
    Fill, MarketId, NANOS_PER_DOLLAR, Nanos, Order, mark_yes_no, notional_nanos,
};
use serde::{Deserialize, Serialize};

use crate::market_info::PricePoint;

/// Bounded in-memory price history retained per market.
///
/// This cache supports recent in-process diagnostics and rolling values.
/// Historical price ranges are served by `sybil-history`, so the sequencer
/// never needs to retain every committed point in RAM or in query tables.
pub const DEFAULT_MAX_RECENT_PRICE_POINTS_PER_MARKET: usize = 2_000;

/// Milliseconds in one hour — bucket granularity for the 24h volume window.
const HOUR_MS: u64 = 3_600_000;

/// Cap on retained hourly volume buckets (24 closed hours + 1 open hour).
const HOURLY_VOLUME_CAP: usize = 25;

/// Cap on retained hourly clearing-price snapshots PER MARKET (mirrors
/// `HOURLY_VOLUME_CAP`; same 24h + 1 open-hour reasoning).
const HOURLY_CLEARING_HISTORY_CAP: usize = 25;

/// Persisted slice of [`PriceTracker`] covering the volume extensions
/// introduced in B2: a running platform total plus rolling hourly buckets for
/// both the per-market split and the platform headline. Stored as one combined
/// blob in redb (see `store.rs`) so the missing-table → default path remains
/// trivial.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RollingVolumeSnapshot {
    pub platform_volume: u64,
    pub hourly_per_market: VecDeque<(u64, HashMap<MarketId, u64>)>,
    pub hourly_platform: VecDeque<(u64, u64)>,
}

/// Persisted slice of [`PriceTracker`] covering the clearing-price history
/// extension introduced in B3: per-market rolling buckets of the first
/// clearing price seen in each hour. Stored as its own redb blob — separate
/// from `RollingVolumeSnapshot` so reverting B3 drops one table cleanly.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RollingPriceAnchorsSnapshot {
    /// Per-market `(hour_start_ms, clearing prices at first observation)`
    /// buckets, cap `HOURLY_CLEARING_HISTORY_CAP` per market.
    pub hourly_clearing_prices: HashMap<MarketId, VecDeque<(u64, Vec<Nanos>)>>,
}

/// Tracks clearing prices, price history, and per-market trading volume.
#[derive(Clone)]
pub struct PriceTracker {
    /// Persisted clearing prices across blocks (fallback when no trades happen).
    last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    /// Sibling of `last_clearing_prices`: the most recent **mark** per market
    /// (clearing when traded, else book midpoint, else carry-over). Serving
    /// layer only — never persisted or sent to consensus. Seeded from
    /// `last_clearing_prices` on restore so the portfolio has a mark before the
    /// first post-restart block.
    last_mark_prices: HashMap<MarketId, Vec<Nanos>>,
    /// Price history per market.
    price_history: HashMap<MarketId, Vec<PricePoint>>,
    /// Price points appended since the last committed snapshot. Store-backed
    /// history persists these rows, then the actor clears them after commit.
    pending_price_points: Vec<(MarketId, PricePoint)>,
    /// Cumulative per-market volume in nanos.
    market_volumes: HashMap<MarketId, u64>,
    /// Maximum retained price points per market in the in-memory serving cache.
    max_history_points_per_market: usize,
    /// Running platform-wide volume total. Computed from raw fills so
    /// multi-market orders don't double-count (per-market entries credit each
    /// active market; the platform scalar counts each fill once).
    platform_volume: u64,
    /// Rolling per-market volume bucketed by hour-start (epoch ms). Cap is
    /// `HOURLY_VOLUME_CAP`; oldest bucket drops when a 26th rolls in.
    hourly_per_market: VecDeque<(u64, HashMap<MarketId, u64>)>,
    /// Rolling platform-wide volume bucketed by hour-start (mirrors
    /// `hourly_per_market` keys so 24h slices stay aligned).
    hourly_platform: VecDeque<(u64, u64)>,
    /// Per-market hourly clearing-price snapshots. First clearing-price
    /// observation in each hour wins; subsequent observations the same hour
    /// don't displace it. Cap `HOURLY_CLEARING_HISTORY_CAP` per market.
    hourly_clearing_prices: HashMap<MarketId, VecDeque<(u64, Vec<Nanos>)>>,
}

impl Default for PriceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceTracker {
    pub fn new() -> Self {
        Self::with_retention(DEFAULT_MAX_RECENT_PRICE_POINTS_PER_MARKET)
    }

    pub fn with_retention(max_history_points_per_market: usize) -> Self {
        Self {
            last_clearing_prices: HashMap::new(),
            last_mark_prices: HashMap::new(),
            price_history: HashMap::new(),
            pending_price_points: Vec::new(),
            market_volumes: HashMap::new(),
            max_history_points_per_market,
            platform_volume: 0,
            hourly_per_market: VecDeque::new(),
            hourly_platform: VecDeque::new(),
            hourly_clearing_prices: HashMap::new(),
        }
    }

    /// Restore from persisted clearing prices and market volumes.
    /// Price history remains a derived view rebuilt over time.
    pub fn with_state(
        last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
        market_volumes: HashMap<MarketId, u64>,
    ) -> Self {
        Self::with_state_and_retention(
            last_clearing_prices,
            market_volumes,
            DEFAULT_MAX_RECENT_PRICE_POINTS_PER_MARKET,
        )
    }

    pub fn with_state_and_retention(
        last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
        market_volumes: HashMap<MarketId, u64>,
        max_history_points_per_market: usize,
    ) -> Self {
        let last_clearing_prices_seed = last_clearing_prices.clone();
        Self {
            last_clearing_prices,
            last_mark_prices: last_clearing_prices_seed,
            price_history: HashMap::new(),
            pending_price_points: Vec::new(),
            market_volumes,
            max_history_points_per_market,
            platform_volume: 0,
            hourly_per_market: VecDeque::new(),
            hourly_platform: VecDeque::new(),
            hourly_clearing_prices: HashMap::new(),
        }
    }

    /// Replace the volume-extension state with a persisted snapshot. Called
    /// once during restore after `with_state`; on cold start the snapshot is
    /// `Default::default()` and this is a no-op.
    pub fn restore_rolling_volume(&mut self, snapshot: RollingVolumeSnapshot) {
        self.platform_volume = snapshot.platform_volume;
        self.hourly_per_market = snapshot.hourly_per_market;
        self.hourly_platform = snapshot.hourly_platform;
    }

    /// Owned snapshot of the volume-extension state for persistence.
    pub fn rolling_volume_snapshot(&self) -> RollingVolumeSnapshot {
        RollingVolumeSnapshot {
            platform_volume: self.platform_volume,
            hourly_per_market: self.hourly_per_market.clone(),
            hourly_platform: self.hourly_platform.clone(),
        }
    }

    /// Replace the clearing-price-history state with a persisted snapshot.
    /// Cold-start hits the `Default::default()` path and this becomes a no-op.
    pub fn restore_rolling_price_anchors(&mut self, snapshot: RollingPriceAnchorsSnapshot) {
        self.hourly_clearing_prices = snapshot.hourly_clearing_prices;
    }

    /// Owned snapshot of the clearing-price-history state for persistence.
    pub fn clearing_history_snapshot(&self) -> RollingPriceAnchorsSnapshot {
        RollingPriceAnchorsSnapshot {
            hourly_clearing_prices: self.hourly_clearing_prices.clone(),
        }
    }

    /// Current clearing prices. Single source of truth — replaces actor's `last_prices` cache.
    pub fn last_clearing_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        &self.last_clearing_prices
    }

    /// Current mark prices (clearing-or-indicative). Always at least as
    /// populated as `last_clearing_prices` after the first block.
    pub fn last_mark_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        &self.last_mark_prices
    }

    /// Merge solver output with persisted prices.
    ///
    /// Fresh prices from the solver replace stored prices only for markets that
    /// had actual fills. Markets without activity keep their last known price.
    /// Returns the merged clearing prices for active markets plus any market
    /// still present in account state.
    pub fn merge_prices(
        &mut self,
        price_discovery: &Option<matching_solver::PriceDiscoveryResult>,
        markets_with_fills: &HashSet<MarketId>,
        active_markets: &HashSet<MarketId>,
        position_markets: &HashSet<MarketId>,
    ) -> HashMap<MarketId, Vec<Nanos>> {
        // Update stored prices in-place with fresh solver output
        if let Some(pd) = price_discovery {
            for (market_id, prices) in &pd.prices {
                if markets_with_fills.contains(market_id) {
                    self.last_clearing_prices.insert(*market_id, prices.clone());
                }
            }
        }

        // Return active-market view (one allocation, no full clone)
        self.last_clearing_prices
            .iter()
            .filter(|(m, _)| active_markets.contains(m) || position_markets.contains(m))
            .map(|(m, p)| (*m, p.clone()))
            .collect()
    }

    /// Record the per-block price series, volumes, and mark prices. Returns
    /// `(per_market_volume, mark_prices)`. The mark series powers live charts
    /// and 24h deltas; `mark_prices` is also reused by the liquidity and
    /// equity trackers so they value markets that have a book but no cross.
    pub fn record_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        midpoints: &HashMap<MarketId, Nanos>,
        height: u64,
        timestamp_ms: u64,
    ) -> (HashMap<MarketId, u64>, HashMap<MarketId, Vec<Nanos>>) {
        // Per-market and platform volume from raw fills (multi-market orders
        // credit each active market; the platform total counts each fill once).
        let mut per_market_volume: HashMap<MarketId, u64> = HashMap::new();
        let mut platform_block_volume: u64 = 0;
        for fill in fills {
            if fill.fill_qty.0 == 0 {
                continue;
            }
            let vol = notional_nanos(fill.fill_price, fill.fill_qty).0;
            platform_block_volume = platform_block_volume.saturating_add(vol);
            if let Some(order) = orders.get(&fill.order_id) {
                for mid in order.active_markets() {
                    *per_market_volume.entry(mid).or_insert(0) += vol;
                }
            }
        }

        // Universe of markets to mark this block: anything with a (carry-over)
        // clearing price, anything with a fresh midpoint, plus filled markets.
        let mut universe: HashSet<MarketId> = clearing_prices.keys().copied().collect();
        universe.extend(midpoints.keys().copied());
        universe.extend(per_market_volume.keys().copied());

        let mut mark_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        for &mid in &universe {
            let vol = per_market_volume.get(&mid).copied().unwrap_or(0);
            let had_fill = vol > 0;
            let mark = mark_yes_no(
                had_fill,
                clearing_prices.get(&mid).map(|v| v.as_slice()),
                midpoints.get(&mid).copied(),
                self.last_mark_prices.get(&mid).map(|v| v.as_slice()),
            );
            let yes_price = mark.first().copied().unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
            let no_price = mark
                .get(1)
                .copied()
                .unwrap_or_else(|| Nanos(NANOS_PER_DOLLAR).saturating_sub(yes_price));

            // Coalesce flat no-trade ticks: skip the append when the price is
            // unchanged AND nothing traded. Trades always produce a point.
            let unchanged = vol == 0
                && self
                    .price_history
                    .get(&mid)
                    .and_then(|h| h.last())
                    .map(|p| p.yes_price == yes_price && p.no_price == no_price)
                    .unwrap_or(false);
            if !unchanged {
                let point = PricePoint {
                    height,
                    timestamp_ms,
                    yes_price,
                    no_price,
                    volume_nanos: vol,
                };
                {
                    let history = self.price_history.entry(mid).or_default();
                    history.push(point.clone());
                    let overflow = history
                        .len()
                        .saturating_sub(self.max_history_points_per_market);
                    if overflow > 0 {
                        history.drain(0..overflow);
                    }
                }
                self.pending_price_points.push((mid, point));
            }

            if vol > 0 {
                *self.market_volumes.entry(mid).or_insert(0) += vol;
            }
            self.last_mark_prices.insert(mid, mark.clone());
            mark_prices.insert(mid, mark);
        }

        // Volume extensions: running platform total + current hourly buckets.
        self.platform_volume = self.platform_volume.saturating_add(platform_block_volume);
        let hour_start_ms = timestamp_ms - (timestamp_ms % HOUR_MS);
        self.ensure_current_volume_bucket(hour_start_ms);
        if let Some((_, market_bucket)) = self.hourly_per_market.back_mut() {
            for (&mid, &vol) in &per_market_volume {
                let entry = market_bucket.entry(mid).or_insert(0);
                *entry = entry.saturating_add(vol);
            }
        }
        if let Some((_, platform_bucket)) = self.hourly_platform.back_mut() {
            *platform_bucket = platform_bucket.saturating_add(platform_block_volume);
        }

        // Hourly clearing/mark history (24h delta anchor): first observation per
        // hour wins. Use the mark so deltas reflect indicative movement too.
        for (&mid, mark) in &mark_prices {
            let bucket = self.hourly_clearing_prices.entry(mid).or_default();
            let need_new = bucket
                .back()
                .map(|(t, _)| *t != hour_start_ms)
                .unwrap_or(true);
            if need_new {
                bucket.push_back((hour_start_ms, mark.clone()));
                while bucket.len() > HOURLY_CLEARING_HISTORY_CAP {
                    bucket.pop_front();
                }
            }
        }

        (per_market_volume, mark_prices)
    }

    fn ensure_current_volume_bucket(&mut self, hour_start_ms: u64) {
        let need_new = self
            .hourly_per_market
            .back()
            .map(|(t, _)| *t != hour_start_ms)
            .unwrap_or(true);
        if !need_new {
            return;
        }
        self.hourly_per_market
            .push_back((hour_start_ms, HashMap::new()));
        self.hourly_platform.push_back((hour_start_ms, 0));
        while self.hourly_per_market.len() > HOURLY_VOLUME_CAP {
            self.hourly_per_market.pop_front();
        }
        while self.hourly_platform.len() > HOURLY_VOLUME_CAP {
            self.hourly_platform.pop_front();
        }
    }

    /// Get price history for a market, optionally filtered by time range.
    pub fn price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Vec<PricePoint> {
        let Some(history) = self.price_history.get(&market_id) else {
            return Vec::new();
        };
        history
            .iter()
            .filter(|p| from_ms.is_none_or(|f| p.timestamp_ms >= f))
            .filter(|p| to_ms.is_none_or(|t| p.timestamp_ms <= t))
            .cloned()
            .collect()
    }

    pub fn pending_price_points(&self) -> &[(MarketId, PricePoint)] {
        &self.pending_price_points
    }

    pub fn clear_pending(&mut self) {
        self.pending_price_points.clear();
    }

    /// Get cumulative volume for a market.
    pub fn market_volume(&self, market_id: MarketId) -> u64 {
        self.market_volumes.get(&market_id).copied().unwrap_or(0)
    }

    /// Persisted per-market cumulative volume view.
    pub fn market_volumes(&self) -> &HashMap<MarketId, u64> {
        &self.market_volumes
    }

    /// Rolling 24h volume for one market (±1h bucket resolution).
    pub fn market_volume_24h(&self, market_id: MarketId, now_ms: u64) -> u64 {
        let cutoff = 24 * HOUR_MS;
        self.hourly_per_market
            .iter()
            .filter(|(hour_start_ms, _)| now_ms.saturating_sub(*hour_start_ms) < cutoff)
            .filter_map(|(_, by_market)| by_market.get(&market_id).copied())
            .fold(0u64, |acc, v| acc.saturating_add(v))
    }

    /// All-time platform-wide volume total (running sum, constant time).
    pub fn platform_volume_total(&self) -> u64 {
        self.platform_volume
    }

    /// Rolling 24h platform-wide volume (±1h bucket resolution).
    pub fn platform_volume_24h(&self, now_ms: u64) -> u64 {
        let cutoff = 24 * HOUR_MS;
        self.hourly_platform
            .iter()
            .filter(|(hour_start_ms, _)| now_ms.saturating_sub(*hour_start_ms) < cutoff)
            .map(|(_, v)| *v)
            .fold(0u64, |acc, v| acc.saturating_add(v))
    }

    /// Clearing prices as-of `n` hours before `now_ms`, looked up against
    /// the per-market hourly buckets recorded in `record_block`. Returns the
    /// `(yes, no)` pair from the bucket whose `hour_start_ms` is the largest
    /// one ≤ `target_ms = now_ms - n * HOUR_MS`; `None` if the market has no
    /// bucket older than `target_ms` (too-young market, or wiped on restart).
    /// Also `None` when `n * HOUR_MS > now_ms` (target predates the epoch —
    /// no answer is possible, so the FE renders "—").
    pub fn price_n_hours_ago(
        &self,
        market_id: MarketId,
        n: u64,
        now_ms: u64,
    ) -> Option<(u64, u64)> {
        let target_ms = now_ms.checked_sub(n.saturating_mul(HOUR_MS))?;
        let buckets = self.hourly_clearing_prices.get(&market_id)?;
        let prices = buckets
            .iter()
            .rev()
            .find(|(hour_start_ms, _)| *hour_start_ms <= target_ms)
            .map(|(_, prices)| prices)?;
        let yes = prices.first().copied()?.0;
        let no = prices.get(1).copied().unwrap_or(Nanos::ZERO).0;
        Some((yes, no))
    }

    /// All-market 24h-ago clearing prices as a single map. Companion to
    /// `all_market_volumes_24h` — populated in one pass by `list_markets`.
    /// Returns an empty map when the target predates the epoch.
    pub fn all_market_prices_n_hours_ago(
        &self,
        n: u64,
        now_ms: u64,
    ) -> HashMap<MarketId, (u64, u64)> {
        let Some(target_ms) = now_ms.checked_sub(n.saturating_mul(HOUR_MS)) else {
            return HashMap::new();
        };
        let mut out = HashMap::new();
        for (&mid, buckets) in &self.hourly_clearing_prices {
            if let Some((_, prices)) = buckets
                .iter()
                .rev()
                .find(|(hour_start_ms, _)| *hour_start_ms <= target_ms)
            {
                let yes = prices.first().copied().unwrap_or(Nanos::ZERO).0;
                let no = prices.get(1).copied().unwrap_or(Nanos::ZERO).0;
                out.insert(mid, (yes, no));
            }
        }
        out
    }

    /// All-market 24h volumes as a single map (used by `list_markets` to
    /// populate every `MarketResponse.volume_24h_nanos` in one pass).
    pub fn all_market_volumes_24h(&self, now_ms: u64) -> HashMap<MarketId, u64> {
        let cutoff = 24 * HOUR_MS;
        let mut out: HashMap<MarketId, u64> = HashMap::new();
        for (hour_start_ms, by_market) in &self.hourly_per_market {
            if now_ms.saturating_sub(*hour_start_ms) >= cutoff {
                continue;
            }
            for (&mid, &vol) in by_market {
                let entry = out.entry(mid).or_insert(0);
                *entry = entry.saturating_add(vol);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        Fill, MarketSet, NANOS_PER_DOLLAR, Nanos, Qty, notional_nanos, outcome_buy, shares_to_qty,
    };

    fn q(shares: u64) -> u64 {
        shares_to_qty(shares).0
    }

    #[test]
    fn price_history_is_bounded_per_market() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("bounded");
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(
            market,
            vec![Nanos(NANOS_PER_DOLLAR / 2), Nanos(NANOS_PER_DOLLAR / 2)],
        );

        let max_points = 8;
        let mut tracker = PriceTracker::with_retention(max_points);
        for height in 1..=(max_points as u64 + 5) {
            tracker.record_block(
                &[Fill::new(order.id, Qty(1), Nanos(NANOS_PER_DOLLAR / 2))],
                &orders,
                &clearing_prices,
                &HashMap::new(),
                height,
                height * 1_000,
            );
        }

        let history = tracker.price_history(market, None, None);
        assert_eq!(history.len(), max_points);
        assert_eq!(history.first().unwrap().height, 6);
        assert_eq!(history.last().unwrap().height, max_points as u64 + 5);
    }

    fn single_market_setup() -> (MarketSet, MarketId, Order, HashMap<MarketId, Vec<Nanos>>) {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("vol");
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, q(4));
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(
            market,
            vec![Nanos(NANOS_PER_DOLLAR / 2), Nanos(NANOS_PER_DOLLAR / 2)],
        );
        (markets, market, order, clearing_prices)
    }

    /// Two blocks one hour apart land in two different buckets; the running
    /// platform total accumulates both.
    #[test]
    fn volume_extensions_bucket_roll() {
        let (_markets, market, order, clearing_prices) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        let price = NANOS_PER_DOLLAR / 2;
        let qty = q(4);
        let per_block = notional_nanos(Nanos(price), Qty(qty)).0;

        tracker.record_block(
            &[Fill::new(order.id, Qty(qty), Nanos(price))],
            &orders,
            &clearing_prices,
            &HashMap::new(),
            1,
            500_000, // hour 0
        );
        tracker.record_block(
            &[Fill::new(order.id, Qty(qty), Nanos(price))],
            &orders,
            &clearing_prices,
            &HashMap::new(),
            2,
            HOUR_MS + 100, // hour 1
        );

        assert_eq!(tracker.hourly_per_market.len(), 2);
        assert_eq!(tracker.hourly_platform.len(), 2);
        assert_eq!(tracker.platform_volume_total(), per_block.saturating_mul(2));
        // The most recent bucket only carries one block of volume.
        let (last_hour, last_market_bucket) = tracker.hourly_per_market.back().unwrap();
        assert_eq!(*last_hour, HOUR_MS);
        assert_eq!(
            last_market_bucket.get(&market).copied().unwrap_or(0),
            per_block
        );
    }

    /// Buckets older than 24h fall out of the 24h window arithmetic.
    #[test]
    fn volume_24h_window_arithmetic() {
        let (_markets, market, order, clearing_prices) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        let price = NANOS_PER_DOLLAR / 2;
        let qty = q(4);
        let per_block = notional_nanos(Nanos(price), Qty(qty)).0;

        // Three blocks, one per hour.
        for h in 0..3u64 {
            tracker.record_block(
                &[Fill::new(order.id, Qty(qty), Nanos(price))],
                &orders,
                &clearing_prices,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 1_000,
            );
        }

        // Inside 24h of hour 0: all three buckets in-window.
        let now_at_h2 = 2 * HOUR_MS + 5_000;
        assert_eq!(tracker.market_volume_24h(market, now_at_h2), per_block * 3);
        assert_eq!(tracker.platform_volume_24h(now_at_h2), per_block * 3);

        // 24h+ε after bucket-0 start — bucket-0 slides past the cutoff while
        // buckets 1 and 2 remain in-window (filter is on bucket start, ±1h res).
        let now_just_past_24h = 24 * HOUR_MS + 100;
        assert_eq!(
            tracker.market_volume_24h(market, now_just_past_24h),
            per_block * 2
        );
        assert_eq!(
            tracker.platform_volume_24h(now_just_past_24h),
            per_block * 2
        );

        // Running totals are always all-time.
        assert_eq!(tracker.platform_volume_total(), per_block * 3);
    }

    /// When 26+ unique hours roll in, the oldest bucket is dropped.
    #[test]
    fn volume_cap_25_drop_oldest() {
        let (_markets, _market, order, clearing_prices) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        let price = NANOS_PER_DOLLAR / 2;
        let qty = q(4);

        // 30 blocks in 30 distinct hours.
        for h in 0..30u64 {
            tracker.record_block(
                &[Fill::new(order.id, Qty(qty), Nanos(price))],
                &orders,
                &clearing_prices,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 1_000,
            );
        }

        assert_eq!(tracker.hourly_per_market.len(), HOURLY_VOLUME_CAP);
        assert_eq!(tracker.hourly_platform.len(), HOURLY_VOLUME_CAP);
        // First retained bucket should be hour 5 (30 - 25 = 5 dropped).
        assert_eq!(tracker.hourly_per_market.front().unwrap().0, 5 * HOUR_MS);
        assert_eq!(tracker.hourly_platform.front().unwrap().0, 5 * HOUR_MS);
        // Platform running total covers ALL 30 blocks (cap doesn't affect it).
        assert_eq!(
            tracker.platform_volume_total(),
            notional_nanos(Nanos(price), Qty(qty)).0.saturating_mul(30)
        );
    }

    /// Two record_block calls in the SAME hour: the first observation wins;
    /// the second leaves the bucket untouched.
    #[test]
    fn hourly_clearing_prices_first_wins() {
        let (_markets, market, order, _) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();

        let prices_a = vec![Nanos(400_000_000), Nanos(600_000_000)];
        let mut cp_a = HashMap::new();
        cp_a.insert(market, prices_a.clone());

        let prices_b = vec![Nanos(700_000_000), Nanos(300_000_000)];
        let mut cp_b = HashMap::new();
        cp_b.insert(market, prices_b.clone());

        // Two blocks 5 minutes apart — same hour-start_ms.
        tracker.record_block(&[], &orders, &cp_a, &HashMap::new(), 1, 100_000);
        tracker.record_block(&[], &orders, &cp_b, &HashMap::new(), 2, 400_000);

        let bucket = tracker
            .hourly_clearing_prices
            .get(&market)
            .expect("market bucket");
        assert_eq!(bucket.len(), 1);
        assert_eq!(bucket.back().unwrap().0, 0);
        assert_eq!(bucket.back().unwrap().1, prices_a, "first-of-hour wins");
    }

    /// 25 hours of distinct clearing prices: `price_n_hours_ago(24)` resolves
    /// to the bucket bracketing `now - 24h`; markets too new return None.
    #[test]
    fn price_24h_ago_lookup() {
        let (_markets, market, order, _) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        for h in 0..25u64 {
            let yes = 400_000_000 + h * 1_000_000;
            let no = 1_000_000_000 - yes;
            let mut cp = HashMap::new();
            cp.insert(market, vec![Nanos(yes), Nanos(no)]);
            // Use a fill so the clearing price flows into the mark (had_fill=true).
            tracker.record_block(
                &[Fill::new(order.id, Qty(1), Nanos(yes))],
                &orders,
                &cp,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 500,
            );
        }

        // Now sits in hour 25; 24h ago = hour 1 boundary; bucket containing
        // hour 1 has price observed at h=1.
        let now_ms = 25 * HOUR_MS + 500;
        let (yes, no) = tracker
            .price_n_hours_ago(market, 24, now_ms)
            .expect("market has 25h of history");
        assert_eq!(yes, 400_000_000 + 1_000_000);
        assert_eq!(no, 1_000_000_000 - (400_000_000 + 1_000_000));

        // 26h ago — target predates the epoch given now_ms ≈ 25h → None.
        assert!(tracker.price_n_hours_ago(market, 26, now_ms).is_none());

        // Unknown market → None.
        let unknown = MarketId::new(9_999);
        assert!(tracker.price_n_hours_ago(unknown, 24, now_ms).is_none());

        // Too-young market: only 5 buckets, asked for 24h ago → None.
        let mut markets2 = MarketSet::new();
        let young = markets2.add_binary("young");
        let young_order = outcome_buy(&markets2, 2, young, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut young_orders = HashMap::new();
        young_orders.insert(young_order.id, &young_order);

        let mut young_tracker = PriceTracker::new();
        for h in 100..105u64 {
            // hours 100..104 — far above 24h
            let mut cp = HashMap::new();
            cp.insert(young, vec![Nanos(500_000_000), Nanos(500_000_000)]);
            young_tracker.record_block(
                &[],
                &young_orders,
                &cp,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 500,
            );
        }
        // Now sits in hour 105; 24h ago = hour 81 boundary. Oldest bucket is
        // hour 100 > 81h → None.
        let young_now = 105 * HOUR_MS + 500;
        assert!(
            young_tracker
                .price_n_hours_ago(young, 24, young_now)
                .is_none()
        );
    }

    /// Cap at `HOURLY_CLEARING_HISTORY_CAP` per market — oldest bucket drops.
    #[test]
    fn clearing_history_cap_drops_oldest_per_market() {
        let (_markets, market, order, _) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        for h in 0..(HOURLY_CLEARING_HISTORY_CAP as u64 + 5) {
            let mut cp = HashMap::new();
            cp.insert(market, vec![Nanos(400_000_000 + h), Nanos(600_000_000 - h)]);
            tracker.record_block(&[], &orders, &cp, &HashMap::new(), h + 1, h * HOUR_MS + 500);
        }

        let bucket = tracker
            .hourly_clearing_prices
            .get(&market)
            .expect("market bucket");
        assert_eq!(bucket.len(), HOURLY_CLEARING_HISTORY_CAP);
        // Oldest retained should be hour 5 (30 - 25 dropped from the front).
        assert_eq!(bucket.front().unwrap().0, 5 * HOUR_MS);
    }

    /// Snapshot ↔ restore is byte-equivalent for the clearing-history slice.
    #[test]
    fn clearing_history_snapshot_roundtrip() {
        let (_markets, market, order, _) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        for h in 0..3u64 {
            let yes = 400_000_000 + h;
            let no = 600_000_000 - h;
            let mut cp = HashMap::new();
            cp.insert(market, vec![Nanos(yes), Nanos(no)]);
            // Use a fill so the clearing price flows into the mark (had_fill=true).
            tracker.record_block(
                &[Fill::new(order.id, Qty(1), Nanos(yes))],
                &orders,
                &cp,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 100,
            );
        }

        let snapshot = tracker.clearing_history_snapshot();
        let mut restored = PriceTracker::new();
        restored.restore_rolling_price_anchors(snapshot);

        let now_ms = 3 * HOUR_MS + 100;
        let (yes, no) = restored
            .price_n_hours_ago(market, 2, now_ms)
            .expect("bucket within window");
        assert_eq!(yes, 400_000_000 + 1);
        assert_eq!(no, 600_000_000 - 1);
    }

    /// Snapshot ↔ restore is byte-equivalent and restored tracker answers
    /// the same 24h queries.
    #[test]
    fn volume_extensions_snapshot_roundtrip() {
        let (_markets, market, order, clearing_prices) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        let price = NANOS_PER_DOLLAR / 2;
        let qty = q(4);
        let per_block = notional_nanos(Nanos(price), Qty(qty)).0;

        for h in 0..3u64 {
            tracker.record_block(
                &[Fill::new(order.id, Qty(qty), Nanos(price))],
                &orders,
                &clearing_prices,
                &HashMap::new(),
                h + 1,
                h * HOUR_MS + 1_000,
            );
        }

        let snapshot = tracker.rolling_volume_snapshot();
        let mut restored = PriceTracker::new();
        restored.restore_rolling_volume(snapshot);

        let now = 2 * HOUR_MS + 5_000;
        assert_eq!(restored.platform_volume_total(), per_block * 3);
        assert_eq!(restored.platform_volume_24h(now), per_block * 3);
        assert_eq!(restored.market_volume_24h(market, now), per_block * 3);
        assert_eq!(
            restored.all_market_volumes_24h(now).get(&market).copied(),
            Some(per_block * 3)
        );
    }

    #[test]
    fn record_block_emits_midpoint_point_for_no_cross_market() {
        use matching_engine::{MarketId, NANOS_PER_DOLLAR};
        let mut pt = PriceTracker::new();

        let m0 = MarketId::new(0);
        let clearing: HashMap<MarketId, Vec<Nanos>> = HashMap::new(); // never traded
        let mut midpoints: HashMap<MarketId, Nanos> = HashMap::new();
        midpoints.insert(m0, Nanos(450_000_000));
        let orders: HashMap<u64, &Order> = HashMap::new();

        let (vol, mark) = pt.record_block(&[], &orders, &clearing, &midpoints, 1, 1_000);

        assert!(vol.is_empty(), "no fills => no volume");
        assert_eq!(
            mark.get(&m0).cloned(),
            Some(vec![
                Nanos(450_000_000),
                Nanos(NANOS_PER_DOLLAR - 450_000_000)
            ])
        );

        let hist = pt.price_history(m0, None, None);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].yes_price, Nanos(450_000_000));
        assert_eq!(hist[0].volume_nanos, 0);

        // A second identical no-cross block coalesces (no new flat point).
        pt.record_block(&[], &orders, &clearing, &midpoints, 2, 2_000);
        assert_eq!(
            pt.price_history(m0, None, None).len(),
            1,
            "flat tick coalesced"
        );

        // Midpoint moves => new point.
        midpoints.insert(m0, Nanos(470_000_000));
        pt.record_block(&[], &orders, &clearing, &midpoints, 3, 3_000);
        assert_eq!(pt.price_history(m0, None, None).len(), 2);
    }
}
