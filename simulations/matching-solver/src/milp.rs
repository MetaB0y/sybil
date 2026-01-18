//! MILP solver for optimal matching using good_lp.
//!
//! Formulates the matching problem as a Mixed-Integer Linear Program to find
//! the optimal solution that maximizes welfare subject to liquidity constraints.

use matching_engine::{Fill, MarketId, Order, Problem};

use crate::{MatchingResult, Solver};

use good_lp::{
    constraint, default_solver, variable, variables, Expression, Solution, SolverModel, Variable,
};
use std::collections::HashMap;

/// MILP solver that finds the optimal matching solution.
pub struct MilpSolver;

impl MilpSolver {
    pub fn new() -> Self {
        Self
    }

    /// Extract the outcome being bought for each market in an order.
    /// Returns a vec of (market_id, outcome_idx) pairs.
    fn extract_order_targets(order: &Order) -> Vec<(MarketId, u8)> {
        let mut targets = Vec::new();

        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            // Use the same logic as greedy solver to determine which outcome
            let outcome = Self::determine_outcome(order, market_idx);
            targets.push((market, outcome));
        }

        targets
    }

    /// Determine which outcome is being bought for a specific market in the order.
    fn determine_outcome(order: &Order, market_idx: usize) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        // Simple case: single market order
        if num_markets == 1 {
            // Find the best payoff outcome
            let mut best_outcome = 0u8;
            let mut best_payoff = i8::MIN;

            for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
                if payoff > best_payoff {
                    best_payoff = payoff;
                    best_outcome = i as u8;
                }
            }
            return best_outcome;
        }

        // Multi-market case: analyze payoff vector
        let market_sizes: Vec<u8> = vec![2; num_markets]; // Assume binary markets

        let mut outcome_votes: [i32; 4] = [0; 4];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = Self::extract_outcome_from_state(state_idx, market_idx, &market_sizes);
                if (outcome as usize) < outcome_votes.len() {
                    outcome_votes[outcome as usize] += payoff as i32;
                }
            }
        }

        outcome_votes
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(idx, _)| idx as u8)
            .unwrap_or(0)
    }

    /// Extract the outcome for a specific market from a state index.
    fn extract_outcome_from_state(state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let mut remaining = state_idx;
        for (i, &size) in market_sizes.iter().enumerate() {
            let outcome = (remaining % size as usize) as u8;
            if i == market_idx {
                return outcome;
            }
            remaining /= size as usize;
        }
        0
    }

    /// Compute available liquidity for each (market, outcome) pair from the asks.
    fn compute_available_liquidity(problem: &Problem) -> HashMap<(MarketId, u8), u64> {
        let mut available = HashMap::new();

        for (&(market, outcome), book) in problem.liquidity.books.iter() {
            let total_ask_qty = book.total_ask_qty();
            if total_ask_qty > 0 {
                available.insert((market, outcome), total_ask_qty);
            }
        }

        available
    }
}

impl Default for MilpSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for MilpSolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut liquidity = problem.liquidity.snapshot();
        let mut result = MatchingResult::new(liquidity.clone());

        // Filter out conditional orders
        let active_orders: Vec<_> = problem
            .orders
            .iter()
            .filter(|o| !o.is_conditional())
            .collect();

        if active_orders.is_empty() {
            return result;
        }

        // Compute available liquidity per (market, outcome)
        let available_liq = Self::compute_available_liquidity(problem);

        // Build the MILP model
        let mut vars = variables!();

        // Decision variables: z_i (binary: is order filled?) and q_i (quantity filled)
        let z_vars: Vec<Variable> = active_orders
            .iter()
            .map(|_| vars.add(variable().binary()))
            .collect();

        let q_vars: Vec<Variable> = active_orders
            .iter()
            .map(|o| vars.add(variable().min(0).max(o.max_fill as f64)))
            .collect();

        // Objective: maximize welfare = sum of (limit_price * quantity)
        // We use limit_price as a proxy for welfare contribution
        let objective: Expression = active_orders
            .iter()
            .zip(q_vars.iter())
            .map(|(order, &q)| (order.limit_price as f64) * q)
            .sum();

        let mut model = vars.maximise(objective).using(default_solver);

        // Add constraints for each order
        for (i, order) in active_orders.iter().enumerate() {
            let z = z_vars[i];
            let q = q_vars[i];

            if order.is_all_or_none() {
                // AON: q_i = z_i * max_fill_i
                model = model.with(constraint!(q == order.max_fill as f64 * z));
            } else {
                // Partial: q_i >= z_i * min_fill_i and q_i <= z_i * max_fill_i
                if order.min_fill > 0 {
                    model = model.with(constraint!(q >= order.min_fill as f64 * z));
                }
                model = model.with(constraint!(q <= order.max_fill as f64 * z));
            }
        }

        // Add liquidity constraints per (market, outcome)
        // For each (market, outcome), sum of quantities from orders touching it <= available
        let mut liq_usage: HashMap<(MarketId, u8), Expression> = HashMap::new();

        for (i, order) in active_orders.iter().enumerate() {
            let q = q_vars[i];
            let targets = Self::extract_order_targets(order);

            for (market, outcome) in targets {
                liq_usage
                    .entry((market, outcome))
                    .or_insert_with(Expression::default)
                    .add_assign(q);
            }
        }

        for ((market, outcome), usage) in liq_usage {
            let available = available_liq.get(&(market, outcome)).copied().unwrap_or(0);
            model = model.with(constraint!(usage <= available as f64));
        }

        // Solve the MILP
        match model.solve() {
            Ok(solution) => {
                // Extract fills from solution
                for (i, order) in active_orders.iter().enumerate() {
                    let z_val = solution.value(z_vars[i]);
                    let q_val = solution.value(q_vars[i]);

                    if z_val > 0.5 && q_val > 0.5 {
                        let fill_qty = q_val.round() as u64;

                        if fill_qty >= order.min_fill {
                            // Find the best price from available liquidity
                            let targets = Self::extract_order_targets(order);

                            // Compute average fill price by consuming from the books
                            let mut total_cost: u128 = 0;
                            let mut markets_filled = 0;

                            for (market, outcome) in &targets {
                                if let Some(book) = liquidity.books.get_mut(&(*market, *outcome)) {
                                    let (filled, avg_price) =
                                        book.consume_asks(fill_qty, order.limit_price);
                                    if filled >= fill_qty.min(order.min_fill) {
                                        total_cost += avg_price as u128 * filled as u128;
                                        markets_filled += 1;
                                    }
                                }
                            }

                            if markets_filled == targets.len() && fill_qty > 0 {
                                let avg_fill_price = if !targets.is_empty() {
                                    (total_cost / (fill_qty as u128 * targets.len() as u128))
                                        as u64
                                } else {
                                    0
                                };

                                let fill = Fill::new(order.id, fill_qty, avg_fill_price);
                                result.add_fill(fill, order);
                            } else {
                                if order.is_all_or_none() {
                                    result.orders_unfilled_aon += 1;
                                } else {
                                    result.orders_unfilled_liquidity += 1;
                                }
                            }
                        }
                    } else {
                        if order.is_all_or_none() {
                            result.orders_unfilled_aon += 1;
                        } else {
                            result.orders_unfilled_liquidity += 1;
                        }
                    }
                }
            }
            Err(_) => {
                // Solver failed - mark all orders as unfilled
                for order in &active_orders {
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
        "MILP"
    }
}

// Helper trait for Expression
trait AddAssign {
    fn add_assign(&mut self, var: Variable);
}

impl AddAssign for Expression {
    fn add_assign(&mut self, var: Variable) {
        *self = self.clone() + var;
    }
}
