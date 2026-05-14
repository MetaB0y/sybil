//! Tracks clearing prices, price history, and per-market volume.

use std::collections::{HashMap, HashSet, VecDeque};

use matching_engine::{Fill, MarketId, Nanos, Order};
use serde::{Deserialize, Serialize};

use crate::market_info::PricePoint;

/// Bounded in-memory price history retained per market.
///
/// This is a serving cache for live charts, not canonical state. The durable
/// price-history table is still a future store concern; keeping this bounded
/// prevents long-running live deployments from retaining every fill forever.
pub const DEFAULT_MAX_PRICE_HISTORY_POINTS_PER_MARKET: usize = 2_000;

/// Milliseconds in one hour — bucket granularity for the 24h volume window.
const HOUR_MS: u64 = 3_600_000;

/// Cap on retained hourly volume buckets (24 closed hours + 1 open hour).
const HOURLY_VOLUME_CAP: usize = 25;

/// Persisted slice of [`PriceTracker`] covering the volume extensions
/// introduced in B2: a running platform total plus rolling hourly buckets for
/// both the per-market split and the platform headline. Stored as one combined
/// blob in redb (see `store.rs`) so the missing-table → default path remains
/// trivial.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PriceTrackerVolumeSnapshot {
    pub platform_volume: u64,
    pub hourly_per_market: VecDeque<(u64, HashMap<MarketId, u64>)>,
    pub hourly_platform: VecDeque<(u64, u64)>,
}

/// Tracks clearing prices, price history, and per-market trading volume.
#[derive(Clone)]
pub struct PriceTracker {
    /// Persisted clearing prices across blocks (fallback when no trades happen).
    last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    /// Price history per market.
    price_history: HashMap<MarketId, Vec<PricePoint>>,
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
}

impl Default for PriceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceTracker {
    pub fn new() -> Self {
        Self::with_retention(DEFAULT_MAX_PRICE_HISTORY_POINTS_PER_MARKET)
    }

    pub fn with_retention(max_history_points_per_market: usize) -> Self {
        Self {
            last_clearing_prices: HashMap::new(),
            price_history: HashMap::new(),
            market_volumes: HashMap::new(),
            max_history_points_per_market,
            platform_volume: 0,
            hourly_per_market: VecDeque::new(),
            hourly_platform: VecDeque::new(),
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
            DEFAULT_MAX_PRICE_HISTORY_POINTS_PER_MARKET,
        )
    }

    pub fn with_state_and_retention(
        last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
        market_volumes: HashMap<MarketId, u64>,
        max_history_points_per_market: usize,
    ) -> Self {
        Self {
            last_clearing_prices,
            price_history: HashMap::new(),
            market_volumes,
            max_history_points_per_market,
            platform_volume: 0,
            hourly_per_market: VecDeque::new(),
            hourly_platform: VecDeque::new(),
        }
    }

    /// Replace the volume-extension state with a persisted snapshot. Called
    /// once during restore after `with_state`; on cold start the snapshot is
    /// `Default::default()` and this is a no-op.
    pub fn restore_volume_extensions(&mut self, snapshot: PriceTrackerVolumeSnapshot) {
        self.platform_volume = snapshot.platform_volume;
        self.hourly_per_market = snapshot.hourly_per_market;
        self.hourly_platform = snapshot.hourly_platform;
    }

    /// Owned snapshot of the volume-extension state for persistence.
    pub fn volume_extensions_snapshot(&self) -> PriceTrackerVolumeSnapshot {
        PriceTrackerVolumeSnapshot {
            platform_volume: self.platform_volume,
            hourly_per_market: self.hourly_per_market.clone(),
            hourly_platform: self.hourly_platform.clone(),
        }
    }

    /// Current clearing prices. Single source of truth — replaces actor's `last_prices` cache.
    pub fn last_clearing_prices(&self) -> &HashMap<MarketId, Vec<Nanos>> {
        &self.last_clearing_prices
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
        if let Some(ref pd) = price_discovery {
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

    /// Record price history and volume for this block. Returns the per-market
    /// volume split (already computed for the price-history append) so callers
    /// can plumb it onto the Block without recomputing.
    pub fn record_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        height: u64,
        timestamp_ms: u64,
    ) -> HashMap<MarketId, u64> {
        // Compute per-market volume and the platform-block total from raw
        // fills. Each fill credits ALL of its order's active markets (a
        // multi-market bundle multiplies into N per-market entries), but the
        // platform total counts each fill once — summing per-market values
        // would over-count for multi-market orders.
        let mut per_market_volume: HashMap<MarketId, u64> = HashMap::new();
        let mut platform_block_volume: u64 = 0;
        for fill in fills {
            if fill.fill_qty == 0 {
                continue;
            }
            let vol = fill.fill_price.saturating_mul(fill.fill_qty);
            platform_block_volume = platform_block_volume.saturating_add(vol);
            if let Some(order) = orders.get(&fill.order_id) {
                for mid in order.active_markets() {
                    *per_market_volume.entry(mid).or_insert(0) += vol;
                }
            }
        }

        // Append PricePoint for each market that had fills
        for (&mid, &vol) in &per_market_volume {
            if let Some(prices) = clearing_prices.get(&mid) {
                let yes_price = prices.first().copied().unwrap_or(0);
                let no_price = prices.get(1).copied().unwrap_or(0);
                self.price_history.entry(mid).or_default().push(PricePoint {
                    height,
                    timestamp_ms,
                    yes_price,
                    no_price,
                    volume_nanos: vol,
                });
                if let Some(history) = self.price_history.get_mut(&mid) {
                    let overflow = history
                        .len()
                        .saturating_sub(self.max_history_points_per_market);
                    if overflow > 0 {
                        history.drain(0..overflow);
                    }
                }
            }
            *self.market_volumes.entry(mid).or_insert(0) += vol;
        }

        // Volume extensions: bump running platform total + route into the
        // current hourly bucket (push a fresh one on hour roll, drop oldest
        // once we exceed `HOURLY_VOLUME_CAP`).
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

        per_market_volume
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
    use matching_engine::{outcome_buy, Fill, MarketSet, NANOS_PER_DOLLAR};

    #[test]
    fn price_history_is_bounded_per_market() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("bounded");
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, 1);
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(market, vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2]);

        let max_points = 8;
        let mut tracker = PriceTracker::with_retention(max_points);
        for height in 1..=(max_points as u64 + 5) {
            tracker.record_block(
                &[Fill::new(order.id, 1, NANOS_PER_DOLLAR / 2)],
                &orders,
                &clearing_prices,
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
        let order = outcome_buy(&markets, 1, market, 0, NANOS_PER_DOLLAR / 2, 4);
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(market, vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2]);
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
        let qty = 4u64;
        let per_block = price.saturating_mul(qty);

        tracker.record_block(
            &[Fill::new(order.id, qty, price)],
            &orders,
            &clearing_prices,
            1,
            500_000, // hour 0
        );
        tracker.record_block(
            &[Fill::new(order.id, qty, price)],
            &orders,
            &clearing_prices,
            2,
            HOUR_MS + 100, // hour 1
        );

        assert_eq!(tracker.hourly_per_market.len(), 2);
        assert_eq!(tracker.hourly_platform.len(), 2);
        assert_eq!(tracker.platform_volume_total(), per_block.saturating_mul(2));
        // The most recent bucket only carries one block of volume.
        let (last_hour, last_market_bucket) = tracker.hourly_per_market.back().unwrap();
        assert_eq!(*last_hour, HOUR_MS);
        assert_eq!(last_market_bucket.get(&market).copied().unwrap_or(0), per_block);
    }

    /// Buckets older than 24h fall out of the 24h window arithmetic.
    #[test]
    fn volume_24h_window_arithmetic() {
        let (_markets, market, order, clearing_prices) = single_market_setup();
        let mut orders = HashMap::new();
        orders.insert(order.id, &order);

        let mut tracker = PriceTracker::new();
        let price = NANOS_PER_DOLLAR / 2;
        let qty = 4u64;
        let per_block = price.saturating_mul(qty);

        // Three blocks, one per hour.
        for h in 0..3u64 {
            tracker.record_block(
                &[Fill::new(order.id, qty, price)],
                &orders,
                &clearing_prices,
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
        assert_eq!(tracker.platform_volume_24h(now_just_past_24h), per_block * 2);

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
        let qty = 4u64;

        // 30 blocks in 30 distinct hours.
        for h in 0..30u64 {
            tracker.record_block(
                &[Fill::new(order.id, qty, price)],
                &orders,
                &clearing_prices,
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
            price.saturating_mul(qty).saturating_mul(30)
        );
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
        let qty = 4u64;
        let per_block = price.saturating_mul(qty);

        for h in 0..3u64 {
            tracker.record_block(
                &[Fill::new(order.id, qty, price)],
                &orders,
                &clearing_prices,
                h + 1,
                h * HOUR_MS + 1_000,
            );
        }

        let snapshot = tracker.volume_extensions_snapshot();
        let mut restored = PriceTracker::new();
        restored.restore_volume_extensions(snapshot);

        let now = 2 * HOUR_MS + 5_000;
        assert_eq!(restored.platform_volume_total(), per_block * 3);
        assert_eq!(restored.platform_volume_24h(now), per_block * 3);
        assert_eq!(restored.market_volume_24h(market, now), per_block * 3);
        assert_eq!(
            restored.all_market_volumes_24h(now).get(&market).copied(),
            Some(per_block * 3)
        );
    }
}
