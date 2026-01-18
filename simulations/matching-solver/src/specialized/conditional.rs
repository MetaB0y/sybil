//! Conditional order evaluation.
//!
//! Handles price-triggered orders by:
//! 1. Solving without conditionals to get price estimates
//! 2. Evaluating which conditions trigger
//! 3. Re-solving with activated orders

use std::collections::HashMap;

use matching_engine::{ConditionDir, LiquidityPool, MarketId, Nanos, Problem};

use crate::{GreedySolver, MatchingResult, Solver};

/// Evaluates and activates conditional orders.
pub struct ConditionalEvaluator {
    /// Number of evaluation iterations
    max_iterations: usize,
    /// Convergence threshold for price stability
    convergence_threshold: Nanos,
}

impl ConditionalEvaluator {
    /// Create a new conditional evaluator.
    pub fn new() -> Self {
        Self {
            max_iterations: 3,
            convergence_threshold: 10_000_000, // 0.01 dollars
        }
    }

    /// Set the maximum number of evaluation iterations.
    pub fn with_max_iterations(mut self, iterations: usize) -> Self {
        self.max_iterations = iterations;
        self
    }

    /// Set the convergence threshold.
    pub fn with_convergence_threshold(mut self, threshold: Nanos) -> Self {
        self.convergence_threshold = threshold;
        self
    }

    /// Evaluate conditional orders and return indices of those that should activate.
    ///
    /// Uses iterative approach:
    /// 1. Solve without conditionals
    /// 2. Estimate prices from solution
    /// 3. Check which conditionals trigger
    /// 4. Repeat until stable or max iterations
    pub fn evaluate(
        &self,
        problem: &Problem,
        initial_prices: Option<&HashMap<MarketId, Nanos>>,
    ) -> Vec<usize> {
        let mut activated = Vec::new();
        let mut prices = initial_prices.cloned().unwrap_or_default();

        // Initial price estimates from liquidity if not provided
        if prices.is_empty() {
            prices = self.estimate_prices_from_liquidity(&problem.liquidity, problem);
        }

        for _iteration in 0..self.max_iterations {
            let newly_activated = self.check_conditions(problem, &prices);

            if newly_activated.is_empty() {
                break;
            }

            // Check for new activations
            let mut any_new = false;
            for idx in &newly_activated {
                if !activated.contains(idx) {
                    activated.push(*idx);
                    any_new = true;
                }
            }

            if !any_new {
                break;
            }

            // Re-solve with activated orders to get new price estimates
            let sub_problem = self.create_problem_with_activated(problem, &activated);
            let solver = GreedySolver::new();
            let result = solver.solve(&sub_problem);

            // Update price estimates
            let new_prices = self.estimate_prices_from_result(&result, &sub_problem);
            if self.prices_converged(&prices, &new_prices) {
                break;
            }
            prices = new_prices;
        }

        activated
    }

    /// Check which conditional orders trigger given current prices.
    fn check_conditions(&self, problem: &Problem, prices: &HashMap<MarketId, Nanos>) -> Vec<usize> {
        let mut triggered = Vec::new();

        for (idx, order) in problem.orders.iter().enumerate() {
            if let Some(ref condition) = order.condition {
                if let Some(&price) = prices.get(&condition.market) {
                    let triggers = match condition.direction {
                        ConditionDir::Above => price > condition.threshold,
                        ConditionDir::Below => price < condition.threshold,
                    };

                    if triggers {
                        triggered.push(idx);
                    }
                }
            }
        }

        triggered
    }

    /// Estimate prices from liquidity pool mid-prices.
    fn estimate_prices_from_liquidity(
        &self,
        liquidity: &LiquidityPool,
        problem: &Problem,
    ) -> HashMap<MarketId, Nanos> {
        let mut prices = HashMap::new();

        for market in problem.markets.iter() {
            let market_id = market.id;

            // Use best ask for outcome 0 as price estimate
            // (In a binary market, P(YES) is a reasonable price estimate)
            if let Some(book) = liquidity.book(market_id, 0) {
                if let Some(best_ask) = book.best_ask() {
                    prices.insert(market_id, best_ask);
                }
            }
        }

        prices
    }

    /// Estimate prices from a solve result.
    fn estimate_prices_from_result(
        &self,
        result: &MatchingResult,
        problem: &Problem,
    ) -> HashMap<MarketId, Nanos> {
        let mut price_sums: HashMap<MarketId, (u128, u64)> = HashMap::new();

        // Aggregate fill prices by market
        for fill in &result.fills {
            if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                for market in order.active_markets() {
                    let entry = price_sums.entry(market).or_insert((0, 0));
                    entry.0 += fill.fill_price as u128 * fill.fill_qty as u128;
                    entry.1 += fill.fill_qty;
                }
            }
        }

        // Compute weighted averages
        price_sums
            .into_iter()
            .filter_map(|(market, (sum, qty))| {
                if qty > 0 {
                    Some((market, (sum / qty as u128) as Nanos))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if prices have converged.
    fn prices_converged(
        &self,
        old_prices: &HashMap<MarketId, Nanos>,
        new_prices: &HashMap<MarketId, Nanos>,
    ) -> bool {
        for (market, &old_price) in old_prices {
            if let Some(&new_price) = new_prices.get(market) {
                let diff = old_price.abs_diff(new_price);
                if diff > self.convergence_threshold {
                    return false;
                }
            }
        }
        true
    }

    /// Create a problem with activated conditional orders.
    fn create_problem_with_activated(&self, problem: &Problem, activated: &[usize]) -> Problem {
        let mut sub_problem = Problem::new(format!("{}_conditionals", problem.name));
        sub_problem.markets = problem.markets.clone();
        sub_problem.liquidity = problem.liquidity.snapshot();
        sub_problem.constraints = problem.constraints.clone();

        for (idx, order) in problem.orders.iter().enumerate() {
            if order.is_conditional() {
                if activated.contains(&idx) {
                    // Add conditional with condition removed (it's now active)
                    let mut active_order = order.clone();
                    active_order.condition = None;
                    sub_problem.orders.push(active_order);
                }
                // Skip non-activated conditionals
            } else {
                // Include all non-conditional orders
                sub_problem.orders.push(order.clone());
            }
        }

        sub_problem
    }

    /// Solve a problem with conditional order handling.
    pub fn solve_with_conditionals(&self, problem: &Problem) -> ConditionalResult {
        // First pass: identify conditional orders
        let conditional_indices: Vec<usize> = problem
            .orders
            .iter()
            .enumerate()
            .filter(|(_, o)| o.is_conditional())
            .map(|(i, _)| i)
            .collect();

        if conditional_indices.is_empty() {
            // No conditionals, just solve normally
            let solver = GreedySolver::new();
            return ConditionalResult {
                result: solver.solve(problem),
                activated_orders: Vec::new(),
                iteration_count: 0,
            };
        }

        // Evaluate conditionals
        let activated = self.evaluate(problem, None);

        // Create final problem with activated conditionals
        let final_problem = self.create_problem_with_activated(problem, &activated);
        let solver = GreedySolver::new();
        let result = solver.solve(&final_problem);

        ConditionalResult {
            result,
            activated_orders: activated,
            iteration_count: self.max_iterations.min(3),
        }
    }
}

impl Default for ConditionalEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of conditional order evaluation and solving.
#[derive(Clone, Debug)]
pub struct ConditionalResult {
    /// The matching result
    pub result: MatchingResult,
    /// Indices of conditional orders that were activated
    pub activated_orders: Vec<usize>,
    /// Number of evaluation iterations performed
    pub iteration_count: usize,
}

impl Solver for ConditionalEvaluator {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        self.solve_with_conditionals(problem).result
    }

    fn name(&self) -> &str {
        "Conditional"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::conditional_buy;

    #[test]
    fn test_condition_above() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");

        // Add liquidity at 0.60
        problem.liquidity.add_ask(m1, 0, 600_000_000, 1000);

        // Conditional: activate if price > 0.50
        let order = conditional_buy(
            &problem.markets,
            1,
            m1,
            700_000_000,
            100,
            m1,
            500_000_000,
            ConditionDir::Above,
        );
        problem.orders.push(order);

        let evaluator = ConditionalEvaluator::new();
        let mut prices = HashMap::new();
        prices.insert(m1, 600_000_000); // Current price is 0.60

        let activated = evaluator.check_conditions(&problem, &prices);

        // Should activate because 0.60 > 0.50
        assert_eq!(activated.len(), 1);
        assert_eq!(activated[0], 0);
    }

    #[test]
    fn test_condition_below() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");

        problem.liquidity.add_ask(m1, 0, 400_000_000, 1000);

        // Conditional: activate if price < 0.50
        let order = conditional_buy(
            &problem.markets,
            1,
            m1,
            450_000_000,
            100,
            m1,
            500_000_000,
            ConditionDir::Below,
        );
        problem.orders.push(order);

        let evaluator = ConditionalEvaluator::new();
        let mut prices = HashMap::new();
        prices.insert(m1, 400_000_000); // Current price is 0.40

        let activated = evaluator.check_conditions(&problem, &prices);

        // Should activate because 0.40 < 0.50
        assert_eq!(activated.len(), 1);
    }

    #[test]
    fn test_condition_not_triggered() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");

        // Conditional: activate if price > 0.70
        let order = conditional_buy(
            &problem.markets,
            1,
            m1,
            800_000_000,
            100,
            m1,
            700_000_000,
            ConditionDir::Above,
        );
        problem.orders.push(order);

        let evaluator = ConditionalEvaluator::new();
        let mut prices = HashMap::new();
        prices.insert(m1, 600_000_000); // Current price is 0.60

        let activated = evaluator.check_conditions(&problem, &prices);

        // Should NOT activate because 0.60 < 0.70
        assert!(activated.is_empty());
    }

    #[test]
    fn test_full_conditional_solve() {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");

        // Add liquidity
        problem.liquidity.add_ask(m1, 0, 500_000_000, 1000);

        // Regular order
        problem.orders.push(
            matching_engine::simple_yes_buy(&problem.markets, 1, m1, 600_000_000, 100)
        );

        // Conditional order that should trigger
        let cond_order = conditional_buy(
            &problem.markets,
            2,
            m1,
            600_000_000,
            50,
            m1,
            400_000_000,
            ConditionDir::Above,
        );
        problem.orders.push(cond_order);

        let evaluator = ConditionalEvaluator::new();
        let cond_result = evaluator.solve_with_conditionals(&problem);

        // Both orders should have opportunity to fill
        assert!(cond_result.result.orders_filled > 0);
    }
}
