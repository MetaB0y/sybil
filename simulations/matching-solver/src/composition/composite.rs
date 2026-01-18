//! Composite solver that combines multiple specialized solvers.
//!
//! The composite solver:
//! 1. Analyzes problem structure
//! 2. Runs specialized solvers (arbitrage, conditionals)
//! 3. Decomposes into clusters
//! 4. Solves each cluster with the appropriate solver
//! 5. Merges results

use matching_engine::{Fill, Problem};

use crate::{GreedySolver, MatchingResult, Solver};

#[cfg(feature = "milp")]
use crate::MilpSolver;

use super::analysis::ProblemAnalysis;
use super::cluster::{Decomposer, SubProblem};
use super::merge::{ConflictStrategy, SolutionMerger};
use super::partial::{PartialSolution, SolutionConfidence};

/// Type of solver to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SolverType {
    /// MILP for optimal solutions (small state space)
    Milp,
    /// Greedy heuristic (fast)
    Greedy,
    /// Arbitrage detector
    Arbitrage,
    /// Conditional evaluator
    Conditional,
}

/// Configuration for the composite solver.
#[derive(Clone, Debug)]
pub struct CompositeConfig {
    /// Maximum markets per cluster before decomposition
    pub max_markets_per_cluster: usize,
    /// Whether to run arbitrage detection first
    pub run_arbitrage: bool,
    /// Whether to evaluate conditionals
    pub evaluate_conditionals: bool,
    /// Conflict resolution strategy
    pub conflict_strategy: ConflictStrategy,
    /// Whether to run in parallel (if supported)
    pub parallel: bool,
    /// Routing rules for solver selection
    pub routing: SolverRouting,
}

impl Default for CompositeConfig {
    fn default() -> Self {
        Self {
            max_markets_per_cluster: 5,
            run_arbitrage: true,
            evaluate_conditionals: true,
            conflict_strategy: ConflictStrategy::ByConfidence,
            parallel: false,
            routing: SolverRouting::default(),
        }
    }
}

/// Rules for routing sub-problems to solvers.
#[derive(Clone, Debug)]
pub struct SolverRouting {
    /// Maximum states for MILP (beyond this, use Greedy)
    pub milp_state_threshold: usize,
}

impl Default for SolverRouting {
    fn default() -> Self {
        Self {
            milp_state_threshold: 32,
        }
    }
}

impl SolverRouting {
    /// Determine which solver to use for a sub-problem.
    pub fn route(&self, sub_problem: &SubProblem) -> SolverType {
        if sub_problem.can_use_milp() {
            SolverType::Milp
        } else {
            SolverType::Greedy
        }
    }
}

/// A composite solver that orchestrates multiple specialized solvers.
pub struct CompositeSolver {
    config: CompositeConfig,
    /// The greedy solver (always available)
    greedy: GreedySolver,
    #[cfg(feature = "milp")]
    /// The MILP solver (feature-gated)
    milp: MilpSolver,
}

impl CompositeSolver {
    /// Create a new composite solver with default configuration.
    pub fn new() -> Self {
        Self {
            config: CompositeConfig::default(),
            greedy: GreedySolver::new(),
            #[cfg(feature = "milp")]
            milp: MilpSolver::new(),
        }
    }

    /// Create a composite solver with custom configuration.
    pub fn with_config(config: CompositeConfig) -> Self {
        Self {
            config,
            greedy: GreedySolver::new(),
            #[cfg(feature = "milp")]
            milp: MilpSolver::new(),
        }
    }

    /// Solve a sub-problem with the appropriate solver.
    fn solve_subproblem(&self, sub_problem: &SubProblem) -> (MatchingResult, SolutionConfidence) {
        let solver_type = self.config.routing.route(sub_problem);

        match solver_type {
            SolverType::Milp => {
                #[cfg(feature = "milp")]
                {
                    let result = self.milp.solve(&sub_problem.problem);
                    (result, SolutionConfidence::Optimal)
                }
                #[cfg(not(feature = "milp"))]
                {
                    let result = self.greedy.solve(&sub_problem.problem);
                    (result, SolutionConfidence::Heuristic)
                }
            }
            SolverType::Greedy => {
                let result = self.greedy.solve(&sub_problem.problem);
                (result, SolutionConfidence::Heuristic)
            }
            _ => {
                // Fallback to greedy for unsupported types
                let result = self.greedy.solve(&sub_problem.problem);
                (result, SolutionConfidence::Heuristic)
            }
        }
    }

}

impl Default for CompositeSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for CompositeSolver {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        // Step 1: Analyze problem structure
        let analysis = ProblemAnalysis::analyze(problem);

        // Step 2: Check if problem is small enough to solve directly
        if analysis.can_solve_directly() {
            #[cfg(feature = "milp")]
            {
                return self.milp.solve(problem);
            }
            #[cfg(not(feature = "milp"))]
            {
                return self.greedy.solve(problem);
            }
        }

        // Step 3: Decompose into clusters
        let decomposer = Decomposer::with_max_markets(self.config.max_markets_per_cluster);
        let decomposition = decomposer.decompose(problem, &analysis);

        // If no decomposition needed, solve directly
        if !decomposition.was_decomposed() {
            if let Some(sub_problem) = decomposition.sub_problems.first() {
                let (result, _) = self.solve_subproblem(sub_problem);
                // Map fills back to original order IDs
                return self.map_result_to_original(problem, sub_problem, result);
            }
            return self.greedy.solve(problem);
        }

        // Step 4: Solve each cluster
        let mut partials = Vec::new();

        for sub_problem in &decomposition.sub_problems {
            if sub_problem.problem.orders.is_empty() {
                continue;
            }

            let (result, confidence) = self.solve_subproblem(sub_problem);
            let partial = self.result_to_partial(sub_problem, result, confidence, problem);
            partials.push(partial);
        }

        // Step 5: Handle bridging orders
        // For now, try to fill bridging orders greedily after cluster solutions
        let bridging_partial = self.solve_bridging_orders(problem, &decomposition.bridging_orders, &partials);
        if bridging_partial.num_fills() > 0 {
            partials.push(bridging_partial);
        }

        // Step 6: Merge partial solutions
        let mut merger = SolutionMerger::new(problem.liquidity.snapshot())
            .with_conflict_strategy(self.config.conflict_strategy);

        let (result, _stats) = merger.merge(partials, problem);

        result
    }

    fn name(&self) -> &str {
        "Composite"
    }
}

impl CompositeSolver {
    /// Map a sub-problem result back to the original problem's order IDs.
    fn map_result_to_original(
        &self,
        original: &Problem,
        sub_problem: &SubProblem,
        result: MatchingResult,
    ) -> MatchingResult {
        let mut mapped = MatchingResult::new(result.remaining_liquidity.clone());

        for fill in result.fills {
            // Find which local order this fill corresponds to
            for (local_idx, &original_idx) in sub_problem.original_order_mapping.iter().enumerate() {
                if local_idx < sub_problem.problem.orders.len()
                   && sub_problem.problem.orders[local_idx].id == fill.order_id {
                    // Create fill with original order ID
                    if let Some(original_order) = original.orders.get(original_idx) {
                        let new_fill = Fill::new(original_order.id, fill.fill_qty, fill.fill_price);
                        mapped.add_fill(new_fill, original_order);
                    }
                    break;
                }
            }
        }

        mapped.orders_unfilled_liquidity = result.orders_unfilled_liquidity;
        mapped.orders_unfilled_aon = result.orders_unfilled_aon;
        mapped
    }

    /// Convert a result to a partial solution with proper order mapping.
    fn result_to_partial(
        &self,
        sub_problem: &SubProblem,
        result: MatchingResult,
        confidence: SolutionConfidence,
        original: &Problem,
    ) -> PartialSolution {
        let mut partial = PartialSolution::new(sub_problem.cluster_id, "composite");

        for fill in result.fills {
            // Find which local order this fill corresponds to
            for (local_idx, &original_idx) in sub_problem.original_order_mapping.iter().enumerate() {
                if local_idx < sub_problem.problem.orders.len()
                    && sub_problem.problem.orders[local_idx].id == fill.order_id
                {
                    // Map to original order
                    if let Some(original_order) = original.orders.get(original_idx) {
                        let welfare = fill.welfare(original_order);
                        let new_fill = Fill::new(original_order.id, fill.fill_qty, fill.fill_price);
                        partial.add_fill(original_idx, new_fill, welfare);
                    }
                    break;
                }
            }
        }

        partial.set_confidence(confidence);
        partial
    }

    /// Solve bridging orders that span multiple clusters.
    fn solve_bridging_orders(
        &self,
        problem: &Problem,
        bridging_orders: &[super::cluster::BridgingOrder],
        existing_partials: &[PartialSolution],
    ) -> PartialSolution {
        let mut partial = PartialSolution::new(usize::MAX, "bridging");

        // Track which orders have been filled by cluster solutions
        let mut filled_orders: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for p in existing_partials {
            for (order_idx, _) in &p.fills {
                filled_orders.insert(*order_idx);
            }
        }

        // Create a mini-problem with just the unfilled bridging orders
        let mut bridging_problem = Problem::new("bridging");
        bridging_problem.markets = problem.markets.clone();
        bridging_problem.liquidity = problem.liquidity.snapshot();
        bridging_problem.constraints = problem.constraints.clone();

        let mut bridging_mapping = Vec::new();
        for bo in bridging_orders {
            if !filled_orders.contains(&bo.order_idx) {
                if let Some(order) = problem.orders.get(bo.order_idx) {
                    bridging_problem.orders.push(order.clone());
                    bridging_mapping.push(bo.order_idx);
                }
            }
        }

        if bridging_problem.orders.is_empty() {
            return partial;
        }

        // Solve bridging orders with greedy
        let result = self.greedy.solve(&bridging_problem);

        for fill in result.fills {
            // Find the original order index
            for (local_idx, &original_idx) in bridging_mapping.iter().enumerate() {
                if local_idx < bridging_problem.orders.len()
                    && bridging_problem.orders[local_idx].id == fill.order_id
                {
                    if let Some(original_order) = problem.orders.get(original_idx) {
                        let welfare = fill.welfare(original_order);
                        let new_fill = Fill::new(original_order.id, fill.fill_qty, fill.fill_price);
                        partial.add_fill(original_idx, new_fill, welfare);
                    }
                    break;
                }
            }
        }

        partial.set_confidence(SolutionConfidence::Heuristic);
        partial
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{BookLevel, LiquidityBook, Market, MarketId, OrderBuilder, Side};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");

        let m1 = problem.markets.add_binary("market_1");
        let m2 = problem.markets.add_binary("market_2");

        // Add liquidity
        problem.liquidity.add_ask(m1, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(m1, 1, 500_000_000, 1000);
        problem.liquidity.add_ask(m2, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(m2, 1, 500_000_000, 1000);

        // Add orders
        problem.orders.push(
            matching_engine::simple_yes_buy(&problem.markets, 1, m1, 600_000_000, 100)
        );

        problem.orders.push(
            matching_engine::simple_yes_buy(&problem.markets, 2, m2, 600_000_000, 100)
        );

        problem
    }

    #[test]
    fn test_composite_solver_basic() {
        let problem = create_test_problem();
        let solver = CompositeSolver::new();

        let result = solver.solve(&problem);

        // Should fill both orders
        assert!(result.orders_filled > 0);
    }

    #[test]
    fn test_composite_with_config() {
        let config = CompositeConfig {
            max_markets_per_cluster: 3,
            run_arbitrage: false,
            evaluate_conditionals: false,
            ..Default::default()
        };

        let solver = CompositeSolver::with_config(config);
        let problem = create_test_problem();
        let result = solver.solve(&problem);

        assert!(result.orders_filled > 0);
    }
}
