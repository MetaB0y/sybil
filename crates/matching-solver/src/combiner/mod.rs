//! Solution combiner for merging independent solver outputs.
//!
//! This module implements a platform-style solution combiner where:
//! 1. Multiple independent solvers propose solutions (sets of fills)
//! 2. A conflict graph is built identifying fills that can't coexist
//! 3. MWIS (Maximum Weight Independent Set) selects the best non-conflicting fills
//!
//! # Key Differences from SolutionMerger
//!
//! Unlike `SolutionMerger` which merges partial solutions from problem decomposition,
//! `SolutionCombiner` works with complete solutions from independent solvers:
//!
//! - SolutionMerger: Merges cluster-based partial solutions
//! - SolutionCombiner: Combines competing full solutions via MWIS
//!
//! # Architecture
//!
//! ```text
//! Solver A ──► Solution A (fills) ─┐
//! Solver B ──► Solution B (fills) ─┼──► SolutionCombiner ──► Best Fill Set
//! Solver C ──► Solution C (fills) ─┘
//!                                          │
//!                                   ┌──────┴──────┐
//!                                   │             │
//!                             ConflictGraph    MWIS Solver
//! ```

pub mod conflict;
pub mod mwis;

use std::collections::HashMap;

use matching_engine::{Fill, LiquidityPool, Order, Problem};

use crate::MatchingResult;

/// Confidence level of a solution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolutionConfidence {
    /// Solution is known to be optimal
    Optimal,
    /// Solution is within a bounded gap of optimal
    BoundedGap { gap_percent: f64 },
    /// Solution is a heuristic (no optimality guarantee)
    Heuristic,
}

pub use conflict::{ConflictGraph, FillFootprint};
pub use mwis::{MwisAlgorithm, MwisSolver};

/// A solution proposed by a solver.
#[derive(Clone, Debug)]
pub struct SolverSolution {
    /// Name of the solver that produced this solution
    pub solver_name: String,
    /// Fills proposed by this solver (with original order indices)
    pub fills: Vec<(usize, Fill)>,
    /// Total welfare achieved
    pub welfare: i64,
    /// Confidence level of this solution
    pub confidence: SolutionConfidence,
}

impl SolverSolution {
    /// Create a new solver solution.
    #[cfg(test)]
    pub fn new(solver_name: impl Into<String>) -> Self {
        Self {
            solver_name: solver_name.into(),
            fills: Vec::new(),
            welfare: 0,
            confidence: SolutionConfidence::Heuristic,
        }
    }

    /// Add a fill to this solution.
    #[cfg(test)]
    pub fn add_fill(&mut self, order_idx: usize, fill: Fill, welfare_delta: i64) {
        self.fills.push((order_idx, fill));
        self.welfare += welfare_delta;
    }
}

/// Statistics from combining solutions.
#[derive(Clone, Debug, Default)]
pub struct CombineStats {
    /// Number of solutions combined
    pub num_solutions: usize,
    /// Total fills across all solutions
    pub total_fills_input: usize,
    /// Fills selected in final result
    pub fills_selected: usize,
    /// Conflicts detected in graph
    pub conflicts_detected: usize,
    /// Welfare before combining
    pub input_max_welfare: i64,
    /// Final welfare after combining
    pub output_welfare: i64,
    /// Time spent building conflict graph (seconds)
    pub conflict_graph_time_secs: f64,
    /// Time spent solving MWIS (seconds)
    pub mwis_time_secs: f64,
}

impl CombineStats {
    /// Welfare improvement over best input solution.
    pub fn welfare_improvement(&self) -> i64 {
        self.output_welfare - self.input_max_welfare
    }

    /// Percentage improvement over best input.
    pub fn improvement_percent(&self) -> f64 {
        if self.input_max_welfare > 0 {
            100.0 * (self.output_welfare - self.input_max_welfare) as f64
                / self.input_max_welfare as f64
        } else {
            0.0
        }
    }
}

/// Contribution tracking for a solver.
#[derive(Clone, Debug)]
pub struct SolverContribution {
    /// Name of the solver
    pub solver_name: String,
    /// Number of fills contributed to final solution
    pub fills_contributed: usize,
    /// Welfare contributed by this solver's fills
    pub welfare_contributed: i64,
}

/// Configuration for the solution combiner.
#[derive(Clone, Debug)]
pub struct CombinerConfig {
    /// Algorithm to use for MWIS
    pub mwis_algorithm: MwisAlgorithm,
    /// Whether to prefer fills from higher-confidence solutions in tie-breaks
    pub prefer_high_confidence: bool,
}

impl Default for CombinerConfig {
    fn default() -> Self {
        Self {
            mwis_algorithm: MwisAlgorithm::Auto,
            prefer_high_confidence: true,
        }
    }
}

/// Combines solutions from multiple solvers using MWIS.
pub struct SolutionCombiner {
    config: CombinerConfig,
}

impl SolutionCombiner {
    /// Create a new solution combiner with default config.
    pub fn new() -> Self {
        Self {
            config: CombinerConfig::default(),
        }
    }

    /// Combine multiple solver solutions into a single optimal result.
    ///
    /// Returns the combined matching result and statistics.
    pub fn combine(
        &self,
        solutions: Vec<SolverSolution>,
        problem: &Problem,
    ) -> (MatchingResult, CombineStats, Vec<SolverContribution>) {
        let mut stats = CombineStats {
            num_solutions: solutions.len(),
            input_max_welfare: solutions.iter().map(|s| s.welfare).max().unwrap_or(0),
            ..Default::default()
        };

        if solutions.is_empty() {
            let result = MatchingResult::new(problem.liquidity.snapshot());
            return (result, stats, Vec::new());
        }

        // Collect all fills with their metadata
        let mut all_fills: Vec<CandidateFill> = Vec::new();
        for solution in solutions.iter() {
            for (order_idx, fill) in &solution.fills {
                if let Some(order) = problem.orders.get(*order_idx) {
                    all_fills.push(CandidateFill {
                        order_idx: *order_idx,
                        fill: fill.clone(),
                        welfare: fill.welfare(order),
                        confidence: solution.confidence,
                        solver_name: solution.solver_name.clone(),
                    });
                }
            }
        }

        stats.total_fills_input = all_fills.len();

        if all_fills.is_empty() {
            let result = MatchingResult::new(problem.liquidity.snapshot());
            return (result, stats, Vec::new());
        }

        // Build conflict graph
        let graph_start = std::time::Instant::now();
        let conflict_graph = self.build_conflict_graph(&all_fills, problem);
        stats.conflict_graph_time_secs = graph_start.elapsed().as_secs_f64();
        stats.conflicts_detected = conflict_graph.num_edges();

        // Solve MWIS to select best non-conflicting fills
        let mwis_start = std::time::Instant::now();
        let selected_indices =
            self.solve_mwis(&all_fills, &conflict_graph, stats.conflicts_detected);
        stats.mwis_time_secs = mwis_start.elapsed().as_secs_f64();

        // Build result from selected fills
        let mut liquidity = problem.liquidity.snapshot();
        let mut result = MatchingResult::new(liquidity.clone());
        let mut contributions: HashMap<String, SolverContribution> = HashMap::new();

        for idx in selected_indices {
            let candidate = &all_fills[idx];
            if let Some(order) = problem.orders.get(candidate.order_idx) {
                // Apply the fill
                result.add_fill(candidate.fill.clone(), order);

                // Update liquidity for all markets in the order
                self.consume_liquidity_for_fill(&mut liquidity, order, &candidate.fill, problem);

                // Track contribution
                let entry = contributions
                    .entry(candidate.solver_name.clone())
                    .or_insert(SolverContribution {
                        solver_name: candidate.solver_name.clone(),
                        fills_contributed: 0,
                        welfare_contributed: 0,
                    });
                entry.fills_contributed += 1;
                entry.welfare_contributed += candidate.welfare;
            }
        }

        result.remaining_liquidity = liquidity;
        stats.fills_selected = result.fills.len();
        stats.output_welfare = result.total_welfare;

        let contributions: Vec<_> = contributions.into_values().collect();

        (result, stats, contributions)
    }

    /// Build the conflict graph for candidate fills.
    fn build_conflict_graph(
        &self,
        fills: &[CandidateFill],
        problem: &Problem,
    ) -> ConflictGraph {
        let mut graph = ConflictGraph::new(fills.len());

        // Two fills conflict if:
        // 1. They fill the same order differently
        // 2. They consume more liquidity than available when combined

        // Build footprints for each fill
        let footprints: Vec<FillFootprint> = fills
            .iter()
            .map(|f| {
                let order = &problem.orders[f.order_idx];
                FillFootprint::from_fill(order, &f.fill, &problem.markets)
            })
            .collect();

        // Check for conflicts between all pairs
        for i in 0..fills.len() {
            for j in (i + 1)..fills.len() {
                if self.fills_conflict(&fills[i], &fills[j], &footprints[i], &footprints[j], problem)
                {
                    graph.add_edge(i, j);
                }
            }
        }

        graph
    }

    /// Check if two fills conflict.
    fn fills_conflict(
        &self,
        fill_a: &CandidateFill,
        fill_b: &CandidateFill,
        footprint_a: &FillFootprint,
        footprint_b: &FillFootprint,
        problem: &Problem,
    ) -> bool {
        // Same order conflict: can only fill an order once
        if fill_a.order_idx == fill_b.order_idx {
            return true;
        }

        // Liquidity conflict: check if combined consumption exceeds available
        for ((market, outcome), qty_a) in &footprint_a.liquidity_consumed {
            if let Some(&qty_b) = footprint_b.liquidity_consumed.get(&(*market, *outcome)) {
                // Check if combined consumption exceeds available
                if let Some(book) = problem.liquidity.book(*market, *outcome) {
                    let available = book.total_ask_qty();
                    if qty_a + qty_b > available {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Solve MWIS on the conflict graph.
    fn solve_mwis(
        &self,
        fills: &[CandidateFill],
        graph: &ConflictGraph,
        _num_conflicts: usize,
    ) -> Vec<usize> {
        let weights: Vec<i64> = fills.iter().map(|f| f.welfare).collect();

        // Adjust weights for confidence if configured
        let adjusted_weights: Vec<i64> = if self.config.prefer_high_confidence {
            fills
                .iter()
                .map(|f| {
                    let base = f.welfare;
                    let confidence_bonus = match f.confidence {
                        SolutionConfidence::Optimal => 1,
                        SolutionConfidence::BoundedGap { .. } => 0,
                        SolutionConfidence::Heuristic => 0,
                    };
                    base + confidence_bonus // Small bonus for optimal solutions
                })
                .collect()
        } else {
            weights
        };

        let solver = MwisSolver::new(self.config.mwis_algorithm);
        solver.solve(graph, &adjusted_weights)
    }

    /// Consume liquidity for a fill.
    ///
    /// For single-market orders, consumes from the appropriate outcome book.
    /// For multi-market orders, consumes from each market's relevant outcome.
    fn consume_liquidity_for_fill(
        &self,
        liquidity: &mut LiquidityPool,
        order: &Order,
        fill: &Fill,
        problem: &Problem,
    ) {
        // Get market sizes for proper outcome extraction
        let market_sizes: Vec<u8> = (0..order.num_markets as usize)
            .map(|i| {
                let market_id = order.markets[i];
                if market_id.is_none() {
                    2
                } else {
                    problem.markets.num_outcomes(market_id).max(2)
                }
            })
            .collect();

        // For each market, determine which outcome is being bought and consume liquidity
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }

            let outcome = self.determine_outcome_for_market(order, market_idx, &market_sizes);
            if let Some(book) = liquidity.get_mut(market, outcome) {
                book.consume_asks(fill.fill_qty, fill.fill_price);
            }
        }
    }

    /// Determine which outcome an order is buying for a specific market.
    fn determine_outcome_for_market(&self, order: &Order, market_idx: usize, market_sizes: &[u8]) -> u8 {
        let num_markets = order.num_markets as usize;
        if market_idx >= num_markets {
            return 0;
        }

        // Simple case: single market order
        if num_markets == 1 {
            // Find the outcome with highest payoff
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
        let max_outcomes = market_sizes.iter().max().copied().unwrap_or(2) as usize;
        let mut outcome_votes: Vec<i32> = vec![0; max_outcomes.max(4)];

        for state_idx in 0..order.num_states as usize {
            let payoff = order.payoffs[state_idx];
            if payoff > 0 {
                let outcome = self.extract_outcome_from_state(state_idx, market_idx, market_sizes);
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
    fn extract_outcome_from_state(&self, state_idx: usize, market_idx: usize, market_sizes: &[u8]) -> u8 {
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
}

impl Default for SolutionCombiner {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal representation of a candidate fill.
#[derive(Clone, Debug)]
struct CandidateFill {
    /// Order index in the problem
    order_idx: usize,
    /// The fill itself
    fill: Fill,
    /// Welfare from this fill
    welfare: i64,
    /// Confidence of the source solution
    confidence: SolutionConfidence,
    /// Name of the solver
    solver_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::simple_yes_buy;

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("market");

        // Add liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

        // Add orders
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market,
            550_000_000,
            200,
        ));

        problem
    }

    #[test]
    fn test_combine_single_solution() {
        let problem = create_test_problem();
        let combiner = SolutionCombiner::new();

        let mut solution = SolverSolution::new("test_solver");
        solution.add_fill(0, Fill::new(1, 100, 500_000_000), 10_000_000_000);

        let (result, stats, _contributions) = combiner.combine(vec![solution], &problem);

        assert_eq!(result.orders_filled, 1);
        assert_eq!(stats.num_solutions, 1);
        assert_eq!(stats.fills_selected, 1);
    }

    #[test]
    fn test_combine_non_conflicting() {
        let problem = create_test_problem();
        let combiner = SolutionCombiner::new();

        // Two solutions filling different orders
        let mut sol_a = SolverSolution::new("solver_a");
        sol_a.add_fill(0, Fill::new(1, 100, 500_000_000), 10_000_000_000);

        let mut sol_b = SolverSolution::new("solver_b");
        sol_b.add_fill(1, Fill::new(2, 200, 500_000_000), 10_000_000_000);

        let (result, stats, _) = combiner.combine(vec![sol_a, sol_b], &problem);

        // Should include both since they don't conflict
        assert_eq!(result.orders_filled, 2);
        assert_eq!(stats.conflicts_detected, 0);
    }

    #[test]
    fn test_combine_same_order_conflict() {
        let problem = create_test_problem();
        let combiner = SolutionCombiner::new();

        // Two solutions filling the same order
        let mut sol_a = SolverSolution::new("solver_a");
        sol_a.add_fill(0, Fill::new(1, 50, 500_000_000), 5_000_000_000);

        let mut sol_b = SolverSolution::new("solver_b");
        sol_b.add_fill(0, Fill::new(1, 100, 500_000_000), 10_000_000_000);

        let (result, stats, _) = combiner.combine(vec![sol_a, sol_b], &problem);

        // Should only include one (the better one)
        assert_eq!(result.orders_filled, 1);
        assert_eq!(stats.conflicts_detected, 1);
    }
}
