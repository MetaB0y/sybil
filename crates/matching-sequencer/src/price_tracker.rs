//! Tracks clearing prices, price history, and per-market volume.

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketId, Nanos, Order};

use crate::market_info::PricePoint;

/// Bounded in-memory price history retained per market.
///
/// This is a serving cache for live charts, not canonical state. The durable
/// price-history table is still a future store concern; keeping this bounded
/// prevents long-running live deployments from retaining every fill forever.
pub const DEFAULT_MAX_PRICE_HISTORY_POINTS_PER_MARKET: usize = 2_000;

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

    /// Record price history and volume for this block.
    pub fn record_block(
        &mut self,
        fills: &[Fill],
        orders: &HashMap<u64, &Order>,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        height: u64,
        timestamp_ms: u64,
    ) {
        // Compute per-market volume from fills
        let mut per_market_volume: HashMap<MarketId, u64> = HashMap::new();
        for fill in fills {
            if fill.fill_qty == 0 {
                continue;
            }
            if let Some(order) = orders.get(&fill.order_id) {
                let vol = fill.fill_price.saturating_mul(fill.fill_qty);
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
}
