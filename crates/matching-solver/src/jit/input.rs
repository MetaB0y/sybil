//! JIT input - what providers see.
//!
//! This defines the ONLY information JIT providers receive.
//! It's anonymized - no user identities, just aggregate order data.

use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, Qty, Problem, Market};

use super::types::{BatchId, UnfilledDemand};
use crate::MatchingResult;

/// Published to JIT providers after base solution is computed.
///
/// This is the ONLY information JIT providers receive.
/// Anonymized - no user identities, just aggregate order data.
#[derive(Clone, Debug)]
pub struct JitInput {
    /// Batch ID for submission.
    pub batch_id: BatchId,
    /// Anonymized orderbook per market (bid/ask depth, no order IDs).
    pub orderbook: AnonymizedOrderbook,
    /// Base solution summary (what would execute without JIT).
    pub base_solution: BaseSolutionSummary,
    /// Market metadata.
    pub markets: Vec<MarketInfo>,
}

impl JitInput {
    /// Build JitInput from a problem and base solution.
    pub fn from_problem_and_solution(
        batch_id: BatchId,
        problem: &Problem,
        base_result: &MatchingResult,
    ) -> Self {
        let orderbook = AnonymizedOrderbook::from_problem(problem);
        let base_solution = BaseSolutionSummary::from_result(problem, base_result);
        let markets = problem
            .markets
            .iter()
            .map(|m| MarketInfo::from_market(m))
            .collect();

        Self {
            batch_id,
            orderbook,
            base_solution,
            markets,
        }
    }

    /// Get unfilled demand for a market (convenience method).
    pub fn unfilled_demand(&self, market_id: MarketId) -> Option<&UnfilledDemand> {
        self.base_solution.unfilled_demand.get(&market_id)
    }

    /// Get clearing price for a market (convenience method).
    pub fn clearing_price(&self, market_id: MarketId) -> Option<Nanos> {
        self.base_solution.clearing_prices.get(&market_id).copied()
    }
}

/// Anonymized orderbook - aggregated depth without order identities.
#[derive(Clone, Debug, Default)]
pub struct AnonymizedOrderbook {
    pub markets: HashMap<MarketId, MarketDepth>,
}

impl AnonymizedOrderbook {
    pub fn new() -> Self {
        Self {
            markets: HashMap::new(),
        }
    }

    /// Build anonymized orderbook from a problem.
    pub fn from_problem(problem: &Problem) -> Self {
        let mut orderbook = Self::new();

        // Aggregate order demand per market
        for order in &problem.orders {
            for market_id in order.active_markets() {
                let depth = orderbook
                    .markets
                    .entry(market_id)
                    .or_insert_with(|| MarketDepth::new(market_id));

                // Determine if this is a buy or sell based on payoff structure
                // For simplicity, we use limit_price as buy price
                // In a full implementation, we'd analyze the payoff vector
                if order.limit_price > 0 {
                    depth.add_bid(order.limit_price, order.max_fill);
                }
            }
        }

        // Add existing liquidity from books
        for ((market_id, _outcome_idx), book) in problem.liquidity.iter() {
            let depth = orderbook
                .markets
                .entry(*market_id)
                .or_insert_with(|| MarketDepth::new(*market_id));

            for level in book.bids() {
                depth.add_bid(level.price, level.available_qty);
            }
            for level in book.asks() {
                depth.add_ask(level.price, level.available_qty);
            }
        }

        // Aggregate to price levels
        for depth in orderbook.markets.values_mut() {
            depth.aggregate();
        }

        orderbook
    }

    /// Get market depth for a specific market.
    pub fn get(&self, market_id: MarketId) -> Option<&MarketDepth> {
        self.markets.get(&market_id)
    }
}

/// Market depth - aggregated bid/ask levels.
#[derive(Clone, Debug)]
pub struct MarketDepth {
    pub market_id: MarketId,
    /// Bid levels (price, total_qty) - sorted by price descending.
    pub bids: Vec<(Nanos, Qty)>,
    /// Ask levels (price, total_qty) - sorted by price ascending.
    pub asks: Vec<(Nanos, Qty)>,
}

impl MarketDepth {
    pub fn new(market_id: MarketId) -> Self {
        Self {
            market_id,
            bids: Vec::new(),
            asks: Vec::new(),
        }
    }

    pub fn add_bid(&mut self, price: Nanos, qty: Qty) {
        self.bids.push((price, qty));
    }

    pub fn add_ask(&mut self, price: Nanos, qty: Qty) {
        self.asks.push((price, qty));
    }

    /// Aggregate and sort levels.
    pub fn aggregate(&mut self) {
        // Aggregate bids at same price
        let mut bid_map: HashMap<Nanos, Qty> = HashMap::new();
        for (price, qty) in &self.bids {
            *bid_map.entry(*price).or_insert(0) += qty;
        }
        self.bids = bid_map.into_iter().collect();
        self.bids.sort_by(|a, b| b.0.cmp(&a.0)); // Descending

        // Aggregate asks at same price
        let mut ask_map: HashMap<Nanos, Qty> = HashMap::new();
        for (price, qty) in &self.asks {
            *ask_map.entry(*price).or_insert(0) += qty;
        }
        self.asks = ask_map.into_iter().collect();
        self.asks.sort_by(|a, b| a.0.cmp(&b.0)); // Ascending
    }

    /// Best bid price.
    pub fn best_bid(&self) -> Option<Nanos> {
        self.bids.first().map(|(p, _)| *p)
    }

    /// Best ask price.
    pub fn best_ask(&self) -> Option<Nanos> {
        self.asks.first().map(|(p, _)| *p)
    }

    /// Total bid quantity.
    pub fn total_bid_qty(&self) -> Qty {
        self.bids.iter().map(|(_, q)| q).sum()
    }

    /// Total ask quantity.
    pub fn total_ask_qty(&self) -> Qty {
        self.asks.iter().map(|(_, q)| q).sum()
    }
}

/// Summary of the base solution (what would execute without JIT).
#[derive(Clone, Debug, Default)]
pub struct BaseSolutionSummary {
    /// Clearing prices per market.
    pub clearing_prices: HashMap<MarketId, Nanos>,
    /// Total welfare achieved in base solution.
    pub total_welfare: i64,
    /// Fill rate (orders filled / total orders).
    pub fill_rate: f64,
    /// Unfilled demand per market.
    pub unfilled_demand: HashMap<MarketId, UnfilledDemand>,
    /// Total volume filled.
    pub total_volume_filled: Qty,
    /// Total orders filled.
    pub orders_filled: usize,
}

impl BaseSolutionSummary {
    /// Build summary from matching result.
    pub fn from_result(problem: &Problem, result: &MatchingResult) -> Self {
        let mut summary = Self {
            total_welfare: result.total_welfare,
            fill_rate: if problem.num_orders() > 0 {
                result.orders_filled as f64 / problem.num_orders() as f64
            } else {
                0.0
            },
            total_volume_filled: result.total_quantity_filled,
            orders_filled: result.orders_filled,
            ..Default::default()
        };

        // Calculate clearing prices from fills
        // Group fills by market and compute volume-weighted price
        let mut market_fills: HashMap<MarketId, Vec<(Nanos, Qty)>> = HashMap::new();
        for fill in &result.fills {
            if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                for market_id in order.active_markets() {
                    market_fills
                        .entry(market_id)
                        .or_default()
                        .push((fill.fill_price, fill.fill_qty));
                }
            }
        }

        for (market_id, fills) in market_fills {
            if !fills.is_empty() {
                let total_value: u128 = fills
                    .iter()
                    .map(|(p, q)| *p as u128 * *q as u128)
                    .sum();
                let total_qty: Qty = fills.iter().map(|(_, q)| *q).sum();
                let avg_price = (total_value / total_qty as u128) as Nanos;
                summary.clearing_prices.insert(market_id, avg_price);
            }
        }

        // Calculate unfilled demand per market
        // This requires comparing order demand to fills
        for order in &problem.orders {
            let fill = result.fills.iter().find(|f| f.order_id == order.id);
            let filled_qty = fill.map(|f| f.fill_qty).unwrap_or(0);
            let unfilled_qty = order.max_fill.saturating_sub(filled_qty);

            if unfilled_qty > 0 {
                for market_id in order.active_markets() {
                    let unfilled = summary
                        .unfilled_demand
                        .entry(market_id)
                        .or_default();

                    // Treat as buy demand (simplified)
                    // In full implementation, we'd analyze payoff vectors
                    unfilled.buy_qty += unfilled_qty;
                    if order.limit_price > unfilled.buy_price {
                        unfilled.buy_price = order.limit_price;
                    }
                }
            }
        }

        summary
    }
}

/// Metadata about a market.
#[derive(Clone, Debug)]
pub struct MarketInfo {
    pub id: MarketId,
    pub name: String,
    pub num_outcomes: u8,
}

impl MarketInfo {
    pub fn from_market(market: &Market) -> Self {
        Self {
            id: market.id,
            name: market.name.clone(),
            num_outcomes: market.num_outcomes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_depth_aggregation() {
        let mut depth = MarketDepth::new(MarketId::new(0));
        depth.add_bid(500_000_000, 100);
        depth.add_bid(500_000_000, 50);
        depth.add_bid(490_000_000, 75);
        depth.add_ask(510_000_000, 80);
        depth.add_ask(520_000_000, 60);

        depth.aggregate();

        // Bids should be aggregated and sorted descending
        assert_eq!(depth.bids.len(), 2);
        assert_eq!(depth.bids[0], (500_000_000, 150)); // Aggregated
        assert_eq!(depth.bids[1], (490_000_000, 75));

        // Asks should be sorted ascending
        assert_eq!(depth.asks.len(), 2);
        assert_eq!(depth.asks[0], (510_000_000, 80));
        assert_eq!(depth.asks[1], (520_000_000, 60));
    }

    #[test]
    fn test_base_solution_summary() {
        let summary = BaseSolutionSummary {
            clearing_prices: HashMap::new(),
            total_welfare: 1000,
            fill_rate: 0.8,
            unfilled_demand: HashMap::new(),
            total_volume_filled: 500,
            orders_filled: 10,
        };

        assert_eq!(summary.total_welfare, 1000);
        assert_eq!(summary.fill_rate, 0.8);
    }
}
