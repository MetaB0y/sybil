//! Multi-heuristic solver for the matching problem.
//!
//! Instead of random shuffling (which rarely beats sorted greedy),
//! this solver tries different sorting heuristics and returns the best result.

use matching_engine::{Order, Problem};

use crate::{GreedySolver, MatchingResult, Solver};

/// Sorting strategy for order processing.
#[derive(Clone, Copy, Debug)]
pub enum SortStrategy {
    /// Sort by welfare potential: limit_price × max_fill (descending)
    Welfare,
    /// Sort by price only (descending) - aggressive orders first
    Price,
    /// Sort by quantity (descending) - large orders first
    Quantity,
    /// Sort by welfare potential (ascending) - small orders first
    InverseWelfare,
    /// Sort by price/quantity ratio - best "value" orders first
    PricePerUnit,
}

impl SortStrategy {
    /// Sort order indices according to this strategy.
    fn sort_indices(&self, orders: &[Order]) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..orders.len()).collect();

        match self {
            SortStrategy::Welfare => {
                indices.sort_by(|&a, &b| {
                    let wa = orders[a].limit_price as u128 * orders[a].max_fill as u128;
                    let wb = orders[b].limit_price as u128 * orders[b].max_fill as u128;
                    wb.cmp(&wa)
                });
            }
            SortStrategy::Price => {
                indices.sort_by(|&a, &b| {
                    orders[b].limit_price.cmp(&orders[a].limit_price)
                });
            }
            SortStrategy::Quantity => {
                indices.sort_by(|&a, &b| {
                    orders[b].max_fill.cmp(&orders[a].max_fill)
                });
            }
            SortStrategy::InverseWelfare => {
                indices.sort_by(|&a, &b| {
                    let wa = orders[a].limit_price as u128 * orders[a].max_fill as u128;
                    let wb = orders[b].limit_price as u128 * orders[b].max_fill as u128;
                    wa.cmp(&wb)  // Ascending
                });
            }
            SortStrategy::PricePerUnit => {
                indices.sort_by(|&a, &b| {
                    // Higher price per unit of risk (max_fill) first
                    let ratio_a = orders[a].limit_price as f64 / orders[a].max_fill.max(1) as f64;
                    let ratio_b = orders[b].limit_price as f64 / orders[b].max_fill.max(1) as f64;
                    ratio_b.partial_cmp(&ratio_a).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }

        indices
    }

    fn name(&self) -> &'static str {
        match self {
            SortStrategy::Welfare => "welfare",
            SortStrategy::Price => "price",
            SortStrategy::Quantity => "quantity",
            SortStrategy::InverseWelfare => "inverse",
            SortStrategy::PricePerUnit => "ratio",
        }
    }
}

/// Multi-heuristic solver that tries different sorting strategies.
pub struct MultiHeuristicSolver {
    strategies: Vec<SortStrategy>,
}

impl MultiHeuristicSolver {
    pub fn new() -> Self {
        Self {
            strategies: vec![
                SortStrategy::Welfare,      // Standard greedy
                SortStrategy::Price,        // Aggressive orders first
                SortStrategy::Quantity,     // Large orders first
                SortStrategy::InverseWelfare, // Small orders first (helps AON)
                SortStrategy::PricePerUnit, // Best value first
            ],
        }
    }
}

impl Default for MultiHeuristicSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for MultiHeuristicSolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        let mut best_result: Option<MatchingResult> = None;
        let mut best_welfare = i64::MIN;

        for strategy in &self.strategies {
            let order_indices = strategy.sort_indices(&problem.orders);
            let result = solve_with_order(&problem, &order_indices);

            if result.total_welfare > best_welfare {
                best_welfare = result.total_welfare;
                best_result = Some(result);
            }
        }

        best_result.unwrap_or_else(|| MatchingResult::new(problem.liquidity.snapshot()))
    }

    fn name(&self) -> &str {
        "MultiHeuristic"
    }
}

/// Solve using a specific order of processing.
fn solve_with_order(problem: &Problem, order_indices: &[usize]) -> MatchingResult {
    let mut liquidity = problem.liquidity.snapshot();
    let mut result = MatchingResult::new(liquidity.clone());

    for &idx in order_indices {
        let order = &problem.orders[idx];

        if order.is_conditional() {
            continue;
        }

        match GreedySolver::try_fill_order_static(order, &mut liquidity) {
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

// Keep the old name as an alias for compatibility
pub type RandomizedGreedySolver = MultiHeuristicSolver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_heuristic_creation() {
        let solver = MultiHeuristicSolver::new();
        assert_eq!(solver.strategies.len(), 5);
    }

    #[test]
    fn test_strategy_names() {
        assert_eq!(SortStrategy::Welfare.name(), "welfare");
        assert_eq!(SortStrategy::Price.name(), "price");
    }
}
