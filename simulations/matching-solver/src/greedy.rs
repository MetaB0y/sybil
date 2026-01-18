//! Greedy solver for the matching problem.
//!
//! Processes orders in decreasing order of welfare potential (limit_price * max_fill).
//! This is a reasonable heuristic but will fail to find optimal solutions on hard instances.

use matching_engine::{LiquidityPool, Order, Fill, MarketId, Nanos, Qty, Problem};

use crate::{MatchingResult, Solver};

/// Greedy solver that processes orders by welfare potential.
pub struct GreedySolver {
    /// Whether to randomize order of equal-welfare orders
    pub randomize_ties: bool,
}

impl GreedySolver {
    pub fn new() -> Self {
        Self {
            randomize_ties: false,
        }
    }

    pub fn with_randomize(mut self, randomize: bool) -> Self {
        self.randomize_ties = randomize;
        self
    }

    /// Sort orders by welfare potential (descending).
    fn sort_by_welfare(orders: &[Order]) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..orders.len()).collect();
        indices.sort_by(|&a, &b| {
            let welfare_a = orders[a].limit_price as u128 * orders[a].max_fill as u128;
            let welfare_b = orders[b].limit_price as u128 * orders[b].max_fill as u128;
            welfare_b.cmp(&welfare_a)
        });
        indices
    }

    /// Try to fill a single order against available liquidity.
    fn try_fill_order(
        order: &Order,
        liquidity: &mut LiquidityPool,
    ) -> Option<Fill> {
        if order.num_markets == 0 {
            return None;
        }

        if order.num_markets == 1 {
            return Self::try_fill_simple_order(order, liquidity);
        }

        // Multi-market orders (bundles, spreads)
        Self::try_fill_bundle_order(order, liquidity)
    }

    /// Fill a simple single-market order.
    fn try_fill_simple_order(
        order: &Order,
        liquidity: &mut LiquidityPool,
    ) -> Option<Fill> {
        let market = order.markets[0];

        // Determine which outcome we're buying based on payoffs
        let buying_outcome = Self::determine_buying_outcome(order);

        if let Some(book) = liquidity.books.get_mut(&(market, buying_outcome)) {
            // Try to consume from asks (we're buying)
            let (filled_qty, avg_price) = book.consume_asks(order.max_fill, order.limit_price);

            if filled_qty >= order.min_fill && filled_qty > 0 {
                return Some(Fill::new(order.id, filled_qty, avg_price));
            } else if order.is_all_or_none() && filled_qty < order.min_fill {
                return None;
            }
        }

        None
    }

    /// Determine which outcome the order is buying based on payoffs.
    fn determine_buying_outcome(order: &Order) -> u8 {
        let mut best_outcome = 0u8;
        let mut best_payoff = i8::MIN;

        for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
            if payoff > best_payoff {
                best_payoff = payoff;
                best_outcome = i as u8;
            }
        }

        best_outcome
    }

    /// Fill a bundle order (all-or-none across multiple markets).
    fn try_fill_bundle_order(
        order: &Order,
        liquidity: &mut LiquidityPool,
    ) -> Option<Fill> {
        // First pass: check availability
        let mut required_fills: Vec<(MarketId, u8, Qty)> = Vec::new();
        let mut total_cost: u128 = 0;

        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = Self::determine_bundle_outcome(order, market_idx);

            if let Some(book) = liquidity.book(market, outcome) {
                let (avail, avg_price) = book.available_to_buy(order.limit_price);
                if avail < order.min_fill {
                    return None;
                }
                required_fills.push((market, outcome, order.max_fill.min(avail)));
                total_cost += avg_price as u128 * order.max_fill.min(avail) as u128;
            } else {
                return None;
            }
        }

        let _avg_cost = if !required_fills.is_empty() {
            (total_cost / required_fills.len() as u128) as Nanos
        } else {
            return None;
        };

        let fill_qty = required_fills.iter().map(|(_, _, q)| *q).min().unwrap_or(0);

        if fill_qty < order.min_fill {
            return None;
        }

        // Second pass: consume liquidity
        let mut actual_cost: u128 = 0;
        for (market, outcome, _) in required_fills {
            if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
                let (filled, price) = book.consume_asks(fill_qty, order.limit_price);
                actual_cost += price as u128 * filled as u128;
            }
        }

        let avg_fill_price = if fill_qty > 0 {
            (actual_cost / fill_qty as u128) as Nanos
        } else {
            0
        };

        Some(Fill::new(order.id, fill_qty, avg_fill_price))
    }

    /// Determine which outcome to buy for a specific market in a bundle.
    fn determine_bundle_outcome(_order: &Order, _market_idx: usize) -> u8 {
        0
    }
}

impl Default for GreedySolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for GreedySolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut liquidity = problem.liquidity.snapshot();
        let mut result = MatchingResult::new(liquidity.clone());

        let order_indices = Self::sort_by_welfare(&problem.orders);

        for &idx in &order_indices {
            let order = &problem.orders[idx];

            if order.is_conditional() {
                continue;
            }

            match Self::try_fill_order(order, &mut liquidity) {
                Some(fill) => {
                    result.add_fill(fill, order);
                }
                None => {
                    if order.is_all_or_none() {
                        result.orders_unfilled_aon += 1;
                    } else {
                        result.orders_unfilled_liquidity += 1;
                    }
                }
            }
        }

        result.remaining_liquidity = liquidity;
        result
    }

    fn name(&self) -> &str {
        "Greedy"
    }
}
