//! Solver platform for orchestrating multiple solvers and combining results.
//!
//! The platform runs multiple independent solvers and combines their solutions
//! using MWIS to produce the best possible result.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Problem                                 │
//! └─────────────────────────────────────────────────────────────┘
//!                             │
//!             ┌───────────────┼───────────────┐
//!             ▼               ▼               ▼
//!     ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
//!     │ Solver A     │ │ Solver B     │ │ Solver C     │
//!     │ (Greedy)     │ │ (MILP 1s)    │ │ (Randomized) │
//!     └──────┬───────┘ └──────┬───────┘ └──────┬───────┘
//!            │                │                │
//!            ▼                ▼                ▼
//!     ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
//!     │ Solution A   │ │ Solution B   │ │ Solution C   │
//!     └──────┬───────┘ └──────┬───────┘ └──────┬───────┘
//!            │                │                │
//!            └────────────────┼────────────────┘
//!                             ▼
//!               ┌─────────────────────────┐
//!               │   SolutionCombiner      │
//!               │   (MWIS on conflicts)   │
//!               └────────────┬────────────┘
//!                            ▼
//!                     PlatformResult
//! ```

use std::time::Instant;

use matching_engine::Problem;

use crate::combiner::{
    CombineStats, SolutionCombiner, SolverContribution, SolverSolution,
};
use crate::composition::SolutionConfidence;
use crate::specialized::{ArbitrageDetector, BundleDecomposer, ChainFinder};
use crate::{GreedySolver, MatchingResult, MultiHeuristicSolver, Solver};

#[cfg(feature = "milp")]
use crate::milp::{MilpSolver, SolveStatus};

/// Configuration for the solver platform.
#[derive(Clone, Debug)]
pub struct PlatformConfig {
    /// Total time budget in milliseconds
    pub total_time_budget_ms: u64,
    /// Fraction of time budget to allocate to MILP (0.0-1.0)
    pub milp_time_fraction: f64,
    /// Number of randomized greedy iterations
    pub randomized_iterations: usize,
    /// Random seed for reproducibility
    pub seed: u64,
    /// Whether to run solvers in parallel (future feature)
    pub parallel: bool,
    /// Whether to include greedy solver
    pub include_greedy: bool,
    /// Whether to include randomized greedy solver
    pub include_randomized: bool,
    /// Whether to include MILP solver (if feature enabled)
    pub include_milp: bool,
    /// Whether to include arbitrage detector
    pub include_arbitrage: bool,
    /// Whether to include bundle decomposer
    pub include_bundle_decomposer: bool,
    /// Whether to include chain finder
    pub include_chain_finder: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            total_time_budget_ms: 2000,
            milp_time_fraction: 0.6,
            randomized_iterations: 100,
            seed: 42,
            parallel: false,
            include_greedy: true,
            include_randomized: true,
            include_milp: true,
            include_arbitrage: true,
            include_bundle_decomposer: true,
            include_chain_finder: true,
        }
    }
}

impl PlatformConfig {
    /// Create a fast configuration with minimal time budget.
    pub fn fast() -> Self {
        Self {
            total_time_budget_ms: 500,
            milp_time_fraction: 0.4,
            randomized_iterations: 20,
            include_arbitrage: false,
            include_bundle_decomposer: false,
            include_chain_finder: false,
            ..Default::default()
        }
    }

    /// Create a thorough configuration with more time for exploration.
    pub fn thorough() -> Self {
        Self {
            total_time_budget_ms: 5000,
            milp_time_fraction: 0.7,
            randomized_iterations: 200,
            include_arbitrage: true,
            include_bundle_decomposer: true,
            include_chain_finder: true,
            ..Default::default()
        }
    }

    /// Create a configuration optimized for specialized solvers.
    pub fn specialized() -> Self {
        Self {
            total_time_budget_ms: 3000,
            milp_time_fraction: 0.5,
            randomized_iterations: 50,
            include_greedy: true,
            include_randomized: true,
            include_milp: true,
            include_arbitrage: true,
            include_bundle_decomposer: true,
            include_chain_finder: true,
            ..Default::default()
        }
    }

    /// Get the MILP timeout in seconds.
    pub fn milp_timeout_secs(&self) -> f64 {
        (self.total_time_budget_ms as f64 / 1000.0) * self.milp_time_fraction
    }
}

/// Result from the solver platform.
#[derive(Clone, Debug)]
pub struct PlatformResult {
    /// The final combined matching result
    pub result: MatchingResult,
    /// Per-solver results before combining
    pub solver_results: Vec<SolverResultInfo>,
    /// Contributions from each solver to final result
    pub contributions: Vec<SolverContribution>,
    /// Statistics from the combining process
    pub combine_stats: CombineStats,
    /// Total time spent (seconds)
    pub total_time_secs: f64,
}

impl PlatformResult {
    /// Check if the combined result is better than all individual results.
    pub fn combined_beats_individual(&self) -> bool {
        let max_individual = self
            .solver_results
            .iter()
            .map(|r| r.welfare)
            .max()
            .unwrap_or(0);
        self.result.total_welfare > max_individual
    }

    /// Get the welfare improvement over the best individual solver.
    pub fn welfare_improvement(&self) -> i64 {
        let max_individual = self
            .solver_results
            .iter()
            .map(|r| r.welfare)
            .max()
            .unwrap_or(0);
        self.result.total_welfare - max_individual
    }

    /// Print a summary of the platform result.
    pub fn print_summary(&self) {
        println!("Platform Result Summary");
        println!("=======================");
        println!();
        println!("Final welfare: {}", self.result.total_welfare);
        println!("Orders filled: {}", self.result.orders_filled);
        println!("Total time: {:.3}s", self.total_time_secs);
        println!();

        println!("Solver Results:");
        for info in &self.solver_results {
            println!(
                "  {}: welfare={}, fills={}, time={:.3}s",
                info.name, info.welfare, info.fills, info.solve_time_secs
            );
        }
        println!();

        println!("Contributions:");
        for contrib in &self.contributions {
            println!(
                "  {}: {} fills, {} welfare",
                contrib.solver_name, contrib.fills_contributed, contrib.welfare_contributed
            );
        }
        println!();

        if self.combined_beats_individual() {
            println!(
                "Combined BEATS individual by {} welfare",
                self.welfare_improvement()
            );
        } else {
            println!("Combined equals best individual solver");
        }
    }
}

/// Information about a single solver's result.
#[derive(Clone, Debug)]
pub struct SolverResultInfo {
    /// Solver name
    pub name: String,
    /// Total welfare achieved
    pub welfare: i64,
    /// Number of fills
    pub fills: usize,
    /// Time spent solving (seconds)
    pub solve_time_secs: f64,
    /// Confidence level of the solution
    pub confidence: SolutionConfidence,
}

/// The solver platform that orchestrates multiple solvers.
pub struct SolverPlatform {
    config: PlatformConfig,
}

impl SolverPlatform {
    /// Create a new platform with default configuration.
    pub fn new() -> Self {
        Self {
            config: PlatformConfig::default(),
        }
    }

    /// Create a platform with custom configuration.
    pub fn with_config(config: PlatformConfig) -> Self {
        Self { config }
    }

    /// Run all configured solvers and combine their results.
    pub fn solve(&self, problem: &Problem) -> PlatformResult {
        let start = Instant::now();
        let mut solver_solutions = Vec::new();
        let mut solver_results = Vec::new();

        // Run greedy solver
        if self.config.include_greedy {
            let (solution, info) = self.run_greedy(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Run randomized greedy solver
        if self.config.include_randomized {
            let (solution, info) = self.run_multi_heuristic(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Run MILP solver with timeout
        #[cfg(feature = "milp")]
        if self.config.include_milp {
            let (solution, info) = self.run_milp(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Run arbitrage detector
        if self.config.include_arbitrage {
            let (solution, info) = self.run_arbitrage(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Run bundle decomposer
        if self.config.include_bundle_decomposer {
            let (solution, info) = self.run_bundle_decomposer(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Run chain finder
        if self.config.include_chain_finder {
            let (solution, info) = self.run_chain_finder(problem);
            solver_solutions.push(solution);
            solver_results.push(info);
        }

        // Combine all solutions
        let combiner = SolutionCombiner::new();
        let (result, combine_stats, contributions) =
            combiner.combine(solver_solutions, problem);

        let total_time_secs = start.elapsed().as_secs_f64();

        PlatformResult {
            result,
            solver_results,
            contributions,
            combine_stats,
            total_time_secs,
        }
    }

    /// Run the greedy solver.
    fn run_greedy(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let start = Instant::now();
        let solver = GreedySolver::new();
        let result = solver.solve(problem);
        let solve_time = start.elapsed().as_secs_f64();

        let solution = SolverSolution::from_result(
            "Greedy",
            &result,
            problem,
            SolutionConfidence::Heuristic,
        );

        let info = SolverResultInfo {
            name: "Greedy".to_string(),
            welfare: result.total_welfare,
            fills: result.orders_filled,
            solve_time_secs: solve_time,
            confidence: SolutionConfidence::Heuristic,
        };

        (solution, info)
    }

    /// Run the multi-heuristic solver.
    fn run_multi_heuristic(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let start = Instant::now();
        let solver = MultiHeuristicSolver::new();
        let result = solver.solve(problem);
        let solve_time = start.elapsed().as_secs_f64();

        let solution = SolverSolution::from_result(
            "MultiHeuristic",
            &result,
            problem,
            SolutionConfidence::Heuristic,
        );

        let info = SolverResultInfo {
            name: "MultiHeuristic".to_string(),
            welfare: result.total_welfare,
            fills: result.orders_filled,
            solve_time_secs: solve_time,
            confidence: SolutionConfidence::Heuristic,
        };

        (solution, info)
    }

    /// Run the MILP solver with timeout.
    #[cfg(feature = "milp")]
    fn run_milp(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let timeout_secs = self.config.milp_timeout_secs();
        let solver = MilpSolver::with_timeout(timeout_secs);
        let milp_result = solver.solve_with_status(problem);

        let confidence = match &milp_result.status {
            SolveStatus::Optimal => SolutionConfidence::Optimal,
            SolveStatus::TimeLimitReached { gap_percent } => SolutionConfidence::BoundedGap {
                gap_percent: *gap_percent,
            },
            SolveStatus::Infeasible => SolutionConfidence::Heuristic,
            SolveStatus::Error(_) => SolutionConfidence::Heuristic,
        };

        let solution = SolverSolution::from_result(
            "MILP",
            &milp_result.result,
            problem,
            confidence,
        );

        let info = SolverResultInfo {
            name: "MILP".to_string(),
            welfare: milp_result.result.total_welfare,
            fills: milp_result.result.orders_filled,
            solve_time_secs: milp_result.solve_time_secs,
            confidence,
        };

        (solution, info)
    }

    /// Run the arbitrage detector.
    fn run_arbitrage(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let start = Instant::now();
        let solver = ArbitrageDetector::new();
        let result = solver.solve(problem);
        let solve_time = start.elapsed().as_secs_f64();

        let solution = SolverSolution::from_result(
            "Arbitrage",
            &result,
            problem,
            SolutionConfidence::Heuristic,
        );

        let info = SolverResultInfo {
            name: "Arbitrage".to_string(),
            welfare: result.total_welfare,
            fills: result.orders_filled,
            solve_time_secs: solve_time,
            confidence: SolutionConfidence::Heuristic,
        };

        (solution, info)
    }

    /// Run the bundle decomposer.
    fn run_bundle_decomposer(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let start = Instant::now();
        let solver = BundleDecomposer::new();
        let result = solver.solve(problem);
        let solve_time = start.elapsed().as_secs_f64();

        let solution = SolverSolution::from_result(
            "BundleDecomposer",
            &result,
            problem,
            SolutionConfidence::Heuristic,
        );

        let info = SolverResultInfo {
            name: "BundleDecomposer".to_string(),
            welfare: result.total_welfare,
            fills: result.orders_filled,
            solve_time_secs: solve_time,
            confidence: SolutionConfidence::Heuristic,
        };

        (solution, info)
    }

    /// Run the chain finder.
    fn run_chain_finder(&self, problem: &Problem) -> (SolverSolution, SolverResultInfo) {
        let start = Instant::now();
        let solver = ChainFinder::new();
        let result = solver.solve(problem);
        let solve_time = start.elapsed().as_secs_f64();

        let solution = SolverSolution::from_result(
            "ChainFinder",
            &result,
            problem,
            SolutionConfidence::Heuristic,
        );

        let info = SolverResultInfo {
            name: "ChainFinder".to_string(),
            welfare: result.total_welfare,
            fills: result.orders_filled,
            solve_time_secs: solve_time,
            confidence: SolutionConfidence::Heuristic,
        };

        (solution, info)
    }
}

impl Default for SolverPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for SolverPlatform {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        self.solve(problem).result
    }

    fn name(&self) -> &str {
        "Platform"
    }
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
        for i in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                (500 + i * 10) as u64 * 1_000_000,
                50 + i * 5,
            ));
        }

        problem
    }

    #[test]
    fn test_platform_basic() {
        let problem = create_test_problem();
        let platform = SolverPlatform::new();
        let result = platform.solve(&problem);

        assert!(result.result.orders_filled > 0);
        assert!(result.solver_results.len() >= 2); // At least greedy and randomized
    }

    #[test]
    fn test_platform_fast_config() {
        let problem = create_test_problem();
        let platform = SolverPlatform::with_config(PlatformConfig::fast());
        let result = platform.solve(&problem);

        assert!(result.result.orders_filled > 0);
    }

    #[test]
    fn test_platform_contributions() {
        let problem = create_test_problem();
        let platform = SolverPlatform::new();
        let result = platform.solve(&problem);

        // Should have contributions from at least one solver
        let total_contributed: usize = result
            .contributions
            .iter()
            .map(|c| c.fills_contributed)
            .sum();
        assert_eq!(total_contributed, result.result.orders_filled);
    }
}
