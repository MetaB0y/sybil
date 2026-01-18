//! Solution merging for combining partial solutions.
//!
//! Merges partial solutions from multiple solvers/clusters while
//! handling conflicts and validating liquidity constraints.

use std::collections::HashSet;

use matching_engine::{Fill, LiquidityPool, Order, Problem, Qty};

use crate::MatchingResult;

use super::partial::{MergeStats, PartialSolution, SolutionConfidence};

/// Strategy for resolving conflicts when merging solutions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Prefer the solution with higher confidence
    #[default]
    ByConfidence,
    /// Prefer the solution with higher welfare
    ByWelfare,
    /// Prefer the first solution added
    FirstWins,
    /// Take both if liquidity permits
    Greedy,
}


/// Merges partial solutions into a complete matching result.
pub struct SolutionMerger {
    /// Strategy for resolving conflicts
    conflict_strategy: ConflictStrategy,
    /// Track filled orders to avoid double-fills
    filled_orders: HashSet<usize>,
    /// Remaining liquidity after applying fills
    remaining_liquidity: LiquidityPool,
    /// Statistics about the merge process
    stats: MergeStats,
}

impl SolutionMerger {
    /// Create a new merger with the given initial liquidity.
    pub fn new(initial_liquidity: LiquidityPool) -> Self {
        Self {
            conflict_strategy: ConflictStrategy::default(),
            filled_orders: HashSet::new(),
            remaining_liquidity: initial_liquidity,
            stats: MergeStats::new(),
        }
    }

    /// Set the conflict resolution strategy.
    pub fn with_conflict_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.conflict_strategy = strategy;
        self
    }

    /// Merge multiple partial solutions into a single matching result.
    pub fn merge(
        &mut self,
        partials: Vec<PartialSolution>,
        problem: &Problem,
    ) -> (MatchingResult, MergeStats) {
        // Sort partials by confidence (highest first)
        let mut sorted_partials = partials;
        sorted_partials.sort_by(|a, b| {
            let conf_a = match a.confidence {
                SolutionConfidence::Optimal => 0,
                SolutionConfidence::BoundedGap { gap_percent } => (gap_percent * 100.0) as i32 + 1,
                SolutionConfidence::Heuristic => 10000,
            };
            let conf_b = match b.confidence {
                SolutionConfidence::Optimal => 0,
                SolutionConfidence::BoundedGap { gap_percent } => (gap_percent * 100.0) as i32 + 1,
                SolutionConfidence::Heuristic => 10000,
            };
            conf_a.cmp(&conf_b)
        });

        self.stats.num_partials = sorted_partials.len();
        self.stats.pre_merge_welfare = sorted_partials.iter().map(|p| p.welfare).sum();

        let mut result = MatchingResult::new(self.remaining_liquidity.clone());

        // Apply fills from each partial solution
        for partial in sorted_partials {
            self.apply_partial(&partial, problem, &mut result);
        }

        // Finalize remaining liquidity
        result.remaining_liquidity = self.remaining_liquidity.clone();

        (result, self.stats.clone())
    }

    /// Apply fills from a partial solution to the result.
    fn apply_partial(
        &mut self,
        partial: &PartialSolution,
        problem: &Problem,
        result: &mut MatchingResult,
    ) {
        for (original_order_idx, fill) in &partial.fills {
            // Skip if this order was already filled
            if self.filled_orders.contains(original_order_idx) {
                self.stats.num_conflicts += 1;
                if result.fills.iter().any(|f| f.order_id == fill.order_id) {
                    // Calculate welfare of the skipped fill
                    if let Some(order) = problem.orders.get(*original_order_idx) {
                        let skipped_welfare = fill.welfare(order);
                        self.stats.welfare_lost_to_conflicts += skipped_welfare;
                    }
                }
                continue;
            }

            // Check if we have enough liquidity
            if let Some(order) = problem.orders.get(*original_order_idx) {
                if self.can_fill(order, fill.fill_qty) {
                    // Apply the fill
                    self.apply_fill(order, fill.clone(), result);
                    self.filled_orders.insert(*original_order_idx);
                } else {
                    // Liquidity conflict
                    self.stats.num_conflicts += 1;
                    self.stats.orders_with_conflicts += 1;
                    let welfare_lost = fill.welfare(order);
                    self.stats.welfare_lost_to_conflicts += welfare_lost;
                }
            }
        }
    }

    /// Check if an order can be filled with the remaining liquidity.
    fn can_fill(&self, order: &Order, qty: Qty) -> bool {
        // For single-market orders, check the relevant outcome
        if order.num_markets == 1 {
            let market = order.markets[0];
            // Determine which outcome we're buying based on payoffs
            let outcome = self.determine_outcome_from_payoffs(order);
            if let Some(book) = self.remaining_liquidity.book(market, outcome) {
                return book.total_ask_qty() >= qty;
            }
            return false;
        }

        // For multi-market orders, this is more complex
        // For now, assume we can fill if any relevant book has liquidity
        // (proper implementation would track exact liquidity per outcome)
        true
    }

    /// Determine which outcome an order is buying based on its payoff structure.
    fn determine_outcome_from_payoffs(&self, order: &Order) -> u8 {
        // Find the outcome with positive payoff
        for (i, &payoff) in order.payoffs.iter().take(order.num_states as usize).enumerate() {
            if payoff > 0 {
                return i as u8;
            }
        }
        0 // Default to first outcome
    }

    /// Apply a fill and update remaining liquidity.
    fn apply_fill(&mut self, order: &Order, fill: Fill, result: &mut MatchingResult) {
        // Update remaining liquidity
        if order.num_markets == 1 {
            let market = order.markets[0];
            let outcome = self.determine_outcome_from_payoffs(order);
            if let Some(book) = self.remaining_liquidity.get_mut(market, outcome) {
                // consume_asks requires (max_qty, max_price)
                let _ = book.consume_asks(fill.fill_qty, fill.fill_price);
            }
        }
        // For multi-market orders, liquidity consumption is more complex
        // and handled by the individual solver

        result.add_fill(fill, order);
    }

    /// Check if an order has already been filled.
    pub fn is_filled(&self, order_idx: usize) -> bool {
        self.filled_orders.contains(&order_idx)
    }

    /// Get the current merge statistics.
    pub fn stats(&self) -> &MergeStats {
        &self.stats
    }
}

/// Builder for creating and configuring a solution merger.
pub struct MergerBuilder {
    conflict_strategy: ConflictStrategy,
}

impl MergerBuilder {
    /// Create a new merger builder.
    pub fn new() -> Self {
        Self {
            conflict_strategy: ConflictStrategy::default(),
        }
    }

    /// Set the conflict resolution strategy.
    pub fn conflict_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.conflict_strategy = strategy;
        self
    }

    /// Build the merger with the given initial liquidity.
    pub fn build(self, initial_liquidity: LiquidityPool) -> SolutionMerger {
        SolutionMerger::new(initial_liquidity).with_conflict_strategy(self.conflict_strategy)
    }
}

impl Default for MergerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, simple_no_buy, MarketId};

    fn create_test_problem_and_liquidity() -> (Problem, LiquidityPool) {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("market_1");

        // Add liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

        let liquidity = problem.liquidity.snapshot();
        (problem, liquidity)
    }

    #[test]
    fn test_merge_non_conflicting() {
        let (mut problem, liquidity) = create_test_problem_and_liquidity();
        let mut merger = SolutionMerger::new(liquidity);

        let market = MarketId::new(0);

        // Add two orders
        problem.orders.push(
            simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100)
        );
        problem.orders.push(
            simple_no_buy(&problem.markets, 2, market, 600_000_000, 100)
        );

        // Create two partial solutions for different orders
        let mut partial1 = PartialSolution::new(0, "solver1");
        partial1.add_fill(0, Fill::new(1, 100, 500_000_000), 10_000_000_000);
        partial1.set_confidence(SolutionConfidence::Optimal);

        let mut partial2 = PartialSolution::new(1, "solver2");
        partial2.add_fill(1, Fill::new(2, 100, 500_000_000), 10_000_000_000);
        partial2.set_confidence(SolutionConfidence::Optimal);

        let (result, stats) = merger.merge(vec![partial1, partial2], &problem);

        assert_eq!(result.orders_filled, 2);
        assert_eq!(stats.num_conflicts, 0);
    }

    #[test]
    fn test_merge_with_conflict() {
        let (mut problem, liquidity) = create_test_problem_and_liquidity();
        let mut merger = SolutionMerger::new(liquidity);

        let market = MarketId::new(0);

        // Add one order
        problem.orders.push(
            simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100)
        );

        // Create two partial solutions for the same order (conflict)
        let mut partial1 = PartialSolution::new(0, "solver1");
        partial1.add_fill(0, Fill::new(1, 100, 500_000_000), 10_000_000_000);
        partial1.set_confidence(SolutionConfidence::Optimal);

        let mut partial2 = PartialSolution::new(1, "solver2");
        partial2.add_fill(0, Fill::new(1, 100, 450_000_000), 15_000_000_000);
        partial2.set_confidence(SolutionConfidence::Heuristic);

        let (result, stats) = merger.merge(vec![partial1, partial2], &problem);

        // Should only fill once (optimal solution first due to sorting)
        assert_eq!(result.orders_filled, 1);
        assert_eq!(stats.num_conflicts, 1);
    }
}
