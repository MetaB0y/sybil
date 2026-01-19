//! Per-market clearing with multi-outcome normalization.
//!
//! This module provides local solvers for individual markets that enforce
//! price normalization (sum of outcome prices = 1.0 for multi-outcome markets).
//!
//! # Architecture
//!
//! ```text
//! For each market:
//!   1. Collect orders touching only this market
//!   2. Solve for clearing prices + fills
//!   3. Enforce: sum(prices) = 1.0 for multi-outcome
//!   4. Return MarketSolution
//! ```

use std::collections::HashMap;

use matching_engine::{Fill, LiquidityBook, MarketId, MarketSet, Nanos, Order, Qty, NANOS_PER_DOLLAR};

/// Solution for a single market.
#[derive(Clone, Debug)]
pub struct MarketSolution {
    /// Market ID this solution is for
    pub market_id: MarketId,
    /// Clearing prices per outcome (normalized to sum to 1.0)
    pub prices: Vec<Nanos>,
    /// Fills for orders in this market
    pub fills: Vec<Fill>,
    /// Total welfare achieved
    pub welfare: i64,
    /// Orders that couldn't be filled
    pub unfilled: Vec<u64>,
}

impl MarketSolution {
    /// Create an empty solution for a market.
    pub fn empty(market_id: MarketId, num_outcomes: usize) -> Self {
        // Default prices: uniform distribution
        let price_per_outcome = NANOS_PER_DOLLAR / num_outcomes as u64;
        let prices = vec![price_per_outcome as Nanos; num_outcomes];

        Self {
            market_id,
            prices,
            fills: Vec::new(),
            welfare: 0,
            unfilled: Vec::new(),
        }
    }

    /// Check if prices are properly normalized (sum to 1.0).
    pub fn is_normalized(&self) -> bool {
        let sum: Nanos = self.prices.iter().sum();
        // Allow small rounding error (within 1 nano)
        let diff = if sum > NANOS_PER_DOLLAR as Nanos {
            sum - NANOS_PER_DOLLAR as Nanos
        } else {
            NANOS_PER_DOLLAR as Nanos - sum
        };
        diff <= 1
    }

    /// Normalize prices to sum to 1.0.
    pub fn normalize_prices(&mut self) {
        let sum: Nanos = self.prices.iter().sum();
        if sum == 0 || sum == NANOS_PER_DOLLAR as Nanos {
            return;
        }

        // Scale all prices proportionally
        for price in &mut self.prices {
            *price = (*price as u128 * NANOS_PER_DOLLAR as u128 / sum as u128) as Nanos;
        }

        // Adjust last price to ensure exact sum
        let new_sum: Nanos = self.prices.iter().sum();
        if let Some(last) = self.prices.last_mut() {
            if new_sum < NANOS_PER_DOLLAR as Nanos {
                *last += NANOS_PER_DOLLAR as Nanos - new_sum;
            } else if new_sum > NANOS_PER_DOLLAR as Nanos {
                *last = last.saturating_sub(new_sum - NANOS_PER_DOLLAR as Nanos);
            }
        }
    }
}

/// Configuration for the local solver.
#[derive(Clone, Debug)]
pub struct LocalSolverConfig {
    /// Whether to enforce price normalization
    pub normalize_prices: bool,
    /// Maximum iterations for price discovery
    pub max_iterations: usize,
    /// Convergence threshold (in nanos)
    pub convergence_threshold: Nanos,
}

impl Default for LocalSolverConfig {
    fn default() -> Self {
        Self {
            normalize_prices: true,
            max_iterations: 100,
            convergence_threshold: 1_000, // 1 micro-dollar
        }
    }
}

/// Per-market clearing solver.
///
/// Solves a single market by matching buy and sell orders at a clearing price.
/// For multi-outcome markets, enforces that outcome prices sum to 1.0.
pub struct LocalSolver {
    config: LocalSolverConfig,
}

impl LocalSolver {
    /// Create a new local solver with default config.
    pub fn new() -> Self {
        Self {
            config: LocalSolverConfig::default(),
        }
    }

    /// Create a local solver with custom config.
    pub fn with_config(config: LocalSolverConfig) -> Self {
        Self { config }
    }

    /// Solve a single market.
    ///
    /// This finds clearing prices and fills for orders in the given market.
    /// For multi-outcome markets, prices are normalized to sum to 1.0.
    pub fn solve_market(
        &self,
        market_id: MarketId,
        markets: &MarketSet,
        orders: &[Order],
        liquidity: &LiquidityBook,
    ) -> MarketSolution {
        let num_outcomes = markets.num_outcomes(market_id) as usize;

        // Filter to single-market orders for this market
        let market_orders: Vec<&Order> = orders
            .iter()
            .filter(|o| o.num_markets == 1 && o.markets[0] == market_id)
            .collect();

        if market_orders.is_empty() {
            return MarketSolution::empty(market_id, num_outcomes);
        }

        // Solve each outcome independently, then normalize
        let mut solution = MarketSolution::empty(market_id, num_outcomes);

        for outcome in 0..num_outcomes as u8 {
            let (price, outcome_fills, outcome_welfare, unfilled) =
                self.solve_outcome(market_id, outcome, &market_orders, liquidity);

            solution.prices[outcome as usize] = price;
            solution.fills.extend(outcome_fills);
            solution.welfare += outcome_welfare;
            solution.unfilled.extend(unfilled);
        }

        // Normalize prices if configured
        if self.config.normalize_prices && num_outcomes > 1 {
            solution.normalize_prices();
        }

        solution
    }

    /// Solve for a single outcome within a market.
    ///
    /// Returns (clearing_price, fills, welfare, unfilled_order_ids).
    fn solve_outcome(
        &self,
        market_id: MarketId,
        outcome: u8,
        orders: &[&Order],
        liquidity: &LiquidityBook,
    ) -> (Nanos, Vec<Fill>, i64, Vec<u64>) {
        // Separate buyers and sellers for this outcome
        let mut buyers: Vec<(&Order, Qty)> = Vec::new();
        let mut sellers: Vec<(&Order, Qty)> = Vec::new();

        for order in orders {
            // Determine if this order is buying or selling this outcome
            // by looking at the payoff for the single-outcome state
            let payoff = order.payoffs[outcome as usize];

            if payoff > 0 {
                // Buying this outcome (positive payoff)
                buyers.push((order, order.max_fill));
            } else if payoff < 0 {
                // Selling this outcome (negative payoff)
                sellers.push((order, order.max_fill));
            }
            // payoff == 0 means order doesn't care about this outcome
        }

        // Sort buyers by limit price descending (most aggressive first)
        buyers.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));

        // Sort sellers by limit price ascending (most aggressive first)
        sellers.sort_by(|a, b| a.0.limit_price.cmp(&b.0.limit_price));

        // Find clearing price by matching supply and demand
        let (clearing_price, matched_qty) =
            self.find_clearing_price(&buyers, &sellers, market_id, outcome, liquidity);

        // Generate fills at clearing price
        let mut fills = Vec::new();
        let mut welfare: i64 = 0;
        let mut unfilled = Vec::new();
        let mut remaining = matched_qty;

        // Fill buyers
        for (order, max_qty) in &buyers {
            if remaining == 0 {
                unfilled.push(order.id);
                continue;
            }

            let fill_qty = (*max_qty).min(remaining);
            if fill_qty >= order.min_fill {
                let fill = Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price,
                };

                // Welfare = limit_price - clearing_price for buyers
                welfare += (order.limit_price as i64 - clearing_price as i64) * fill_qty as i64;

                fills.push(fill);
                remaining = remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        (clearing_price, fills, welfare, unfilled)
    }

    /// Find the clearing price for an outcome.
    ///
    /// Uses a simple supply-demand crossing algorithm.
    fn find_clearing_price(
        &self,
        buyers: &[(&Order, Qty)],
        _sellers: &[(&Order, Qty)],
        market_id: MarketId,
        outcome: u8,
        liquidity: &LiquidityBook,
    ) -> (Nanos, Qty) {
        // Get available liquidity asks for this outcome
        let asks = liquidity.asks();

        if asks.is_empty() || buyers.is_empty() {
            return (NANOS_PER_DOLLAR / 2, 0); // Default to 50 cents if no liquidity
        }

        // Build cumulative demand curve (price -> total qty demanded at or above price)
        let mut demand_at_price: Vec<(Nanos, Qty)> = Vec::new();
        let mut cumulative_demand: Qty = 0;

        for (order, qty) in buyers {
            cumulative_demand += qty;
            demand_at_price.push((order.limit_price, cumulative_demand));
        }

        // Build cumulative supply curve from liquidity
        let mut supply_at_price: Vec<(Nanos, Qty)> = Vec::new();
        let mut cumulative_supply: Qty = 0;

        for level in asks {
            cumulative_supply += level.available_qty;
            supply_at_price.push((level.price, cumulative_supply));
        }

        // Find crossing point
        let mut clearing_price = asks[0].price;
        let mut clearing_qty: Qty = 0;

        for (price, supply) in &supply_at_price {
            // Find demand at this price
            let demand = demand_at_price
                .iter()
                .filter(|(p, _)| *p >= *price)
                .map(|(_, q)| *q)
                .max()
                .unwrap_or(0);

            let matched = demand.min(*supply);
            if matched > clearing_qty {
                clearing_qty = matched;
                clearing_price = *price;
            }
        }

        // Log for debugging (in tests)
        let _ = (market_id, outcome); // Suppress unused warnings

        (clearing_price, clearing_qty)
    }
}

impl Default for LocalSolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Solve all markets and return per-market solutions.
///
/// This is the main entry point for market clearing.
/// For parallel execution, consider using rayon externally.
pub fn solve_all_markets_parallel(
    markets: &MarketSet,
    orders: &[Order],
    liquidity: &matching_engine::LiquidityPool,
) -> HashMap<MarketId, MarketSolution> {
    let solver = LocalSolver::new();

    markets
        .iter()
        .map(|market| {
            let book = liquidity
                .books
                .get(&(market.id, 0))
                .cloned()
                .unwrap_or_else(|| LiquidityBook::new(market.id, 0));
            let solution = solver.solve_market(market.id, markets, orders, &book);
            (market.id, solution)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, Problem};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("test_market");

        // Add liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

        // Add some buy orders
        for i in 0..5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                (550 + i * 10) as u64 * 1_000_000,
                100,
            ));
        }

        problem
    }

    #[test]
    fn test_local_solver_basic() {
        let problem = create_test_problem();
        let market_id = problem.markets.iter().next().unwrap().id;

        let solver = LocalSolver::new();
        let book = problem
            .liquidity
            .books
            .get(&(market_id, 0))
            .cloned()
            .unwrap_or_else(|| LiquidityBook::new(market_id, 0));

        let solution = solver.solve_market(
            market_id,
            &problem.markets,
            &problem.orders,
            &book,
        );

        assert_eq!(solution.market_id, market_id);
        assert_eq!(solution.prices.len(), 2); // Binary market
        assert!(solution.is_normalized());
    }

    #[test]
    fn test_price_normalization() {
        let mut solution = MarketSolution::empty(MarketId::new(0), 3);
        solution.prices = vec![400_000_000, 400_000_000, 400_000_000]; // 1.2 total

        assert!(!solution.is_normalized());
        solution.normalize_prices();
        assert!(solution.is_normalized());

        let sum: Nanos = solution.prices.iter().sum();
        assert_eq!(sum, NANOS_PER_DOLLAR);
    }

    #[test]
    fn test_empty_market() {
        let mut problem = Problem::new("empty");
        let market = problem.markets.add("three_way", vec!["A".to_string(), "B".to_string(), "C".to_string()]);

        let solver = LocalSolver::new();
        let book = LiquidityBook::new(market, 0);

        let solution = solver.solve_market(market, &problem.markets, &[], &book);

        assert_eq!(solution.prices.len(), 3);
        assert!(solution.is_normalized());
        assert!(solution.fills.is_empty());
    }
}
