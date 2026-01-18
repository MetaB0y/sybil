//! Builder pattern for configuring composite solvers.
//!
//! Provides a fluent API for constructing composite solvers with
//! custom configurations, routing rules, and specialized solvers.

use super::composite::{CompositeConfig, CompositeSolver, SolverRouting, SolverType};
use super::merge::ConflictStrategy;

/// Builder for creating configured composite solvers.
///
/// # Example
///
/// ```ignore
/// let solver = SolverBuilder::new()
///     .max_markets_per_cluster(5)
///     .parallel()
///     .with_arbitrage()
///     .with_conditionals()
///     .milp_state_threshold(32)
///     .conflict_strategy(ConflictStrategy::ByWelfare)
///     .build();
/// ```
pub struct SolverBuilder {
    config: CompositeConfig,
}

impl SolverBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: CompositeConfig::default(),
        }
    }

    /// Set the maximum markets per cluster before decomposition.
    ///
    /// Smaller values lead to more decomposition (more parallel work, potentially
    /// more gap from optimal). Larger values preserve more structure but may
    /// cause MILP to be slow or infeasible.
    pub fn max_markets_per_cluster(mut self, max: usize) -> Self {
        self.config.max_markets_per_cluster = max;
        self
    }

    /// Enable parallel solving of clusters (when supported).
    pub fn parallel(mut self) -> Self {
        self.config.parallel = true;
        self
    }

    /// Disable parallel solving.
    pub fn sequential(mut self) -> Self {
        self.config.parallel = false;
        self
    }

    /// Enable arbitrage detection and exploitation.
    pub fn with_arbitrage(mut self) -> Self {
        self.config.run_arbitrage = true;
        self
    }

    /// Disable arbitrage detection.
    pub fn without_arbitrage(mut self) -> Self {
        self.config.run_arbitrage = false;
        self
    }

    /// Enable conditional order evaluation.
    pub fn with_conditionals(mut self) -> Self {
        self.config.evaluate_conditionals = true;
        self
    }

    /// Disable conditional order evaluation.
    pub fn without_conditionals(mut self) -> Self {
        self.config.evaluate_conditionals = false;
        self
    }

    /// Set the conflict resolution strategy.
    pub fn conflict_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.config.conflict_strategy = strategy;
        self
    }

    /// Set the state threshold for MILP routing.
    ///
    /// Sub-problems with state space at or below this threshold will be
    /// routed to MILP. Those above will use greedy.
    pub fn milp_state_threshold(mut self, threshold: usize) -> Self {
        self.config.routing.milp_state_threshold = threshold;
        self
    }

    /// Configure routing with a custom SolverRouting.
    pub fn routing(mut self, routing: SolverRouting) -> Self {
        self.config.routing = routing;
        self
    }

    /// Build the configured composite solver.
    pub fn build(self) -> CompositeSolver {
        CompositeSolver::with_config(self.config)
    }
}

impl Default for SolverBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Presets for common solver configurations.
pub struct SolverPresets;

impl SolverPresets {
    /// Fast configuration prioritizing speed over optimality.
    ///
    /// - Small cluster size (3 markets)
    /// - No arbitrage detection
    /// - Greedy conflict resolution
    pub fn fast() -> CompositeSolver {
        SolverBuilder::new()
            .max_markets_per_cluster(3)
            .without_arbitrage()
            .without_conditionals()
            .milp_state_threshold(8)
            .conflict_strategy(ConflictStrategy::FirstWins)
            .build()
    }

    /// Balanced configuration for general use.
    ///
    /// - Medium cluster size (5 markets)
    /// - Arbitrage detection enabled
    /// - Confidence-based conflict resolution
    pub fn balanced() -> CompositeSolver {
        SolverBuilder::new()
            .max_markets_per_cluster(5)
            .with_arbitrage()
            .with_conditionals()
            .milp_state_threshold(32)
            .conflict_strategy(ConflictStrategy::ByConfidence)
            .build()
    }

    /// Quality configuration prioritizing solution quality.
    ///
    /// - Larger cluster size (7 markets)
    /// - All features enabled
    /// - Welfare-based conflict resolution
    pub fn quality() -> CompositeSolver {
        SolverBuilder::new()
            .max_markets_per_cluster(7)
            .with_arbitrage()
            .with_conditionals()
            .milp_state_threshold(64)
            .conflict_strategy(ConflictStrategy::ByWelfare)
            .build()
    }

    /// Greedy-only configuration that skips MILP entirely.
    ///
    /// Useful when MILP is too slow or not available.
    pub fn greedy_only() -> CompositeSolver {
        SolverBuilder::new()
            .max_markets_per_cluster(10)
            .without_arbitrage()
            .without_conditionals()
            .milp_state_threshold(0) // Never use MILP
            .conflict_strategy(ConflictStrategy::Greedy)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Solver;
    use matching_engine::{BookLevel, LiquidityBook, Market, MarketId, OrderBuilder, Problem, Side};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let m1 = problem.markets.add_binary("market_1");

        problem.liquidity.add_ask(m1, 0, 500_000_000, 1000);

        problem.orders.push(
            matching_engine::simple_yes_buy(&problem.markets, 1, m1, 600_000_000, 100)
        );

        problem
    }

    #[test]
    fn test_builder_basic() {
        let solver = SolverBuilder::new()
            .max_markets_per_cluster(4)
            .build();

        let problem = create_test_problem();
        let result = solver.solve(&problem);

        assert!(result.orders_filled > 0);
    }

    #[test]
    fn test_builder_with_options() {
        let solver = SolverBuilder::new()
            .max_markets_per_cluster(5)
            .with_arbitrage()
            .with_conditionals()
            .parallel()
            .milp_state_threshold(16)
            .conflict_strategy(ConflictStrategy::ByWelfare)
            .build();

        let problem = create_test_problem();
        let result = solver.solve(&problem);

        assert!(result.orders_filled > 0);
    }

    #[test]
    fn test_presets() {
        let problem = create_test_problem();

        let fast = SolverPresets::fast();
        let fast_result = fast.solve(&problem);
        assert!(fast_result.orders_filled > 0);

        let balanced = SolverPresets::balanced();
        let balanced_result = balanced.solve(&problem);
        assert!(balanced_result.orders_filled > 0);

        let quality = SolverPresets::quality();
        let quality_result = quality.solve(&problem);
        assert!(quality_result.orders_filled > 0);
    }
}
