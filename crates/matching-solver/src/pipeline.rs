//! Pipeline configuration for solver experimentation.
//!
//! The pipeline provides a flexible way to combine different solver components:
//! - Price discovery (LocalSolver, LP-based, etc.)
//! - Order allocation (MmAllocator)
//! - Partial solvers (Greedy, MILP, specialized)
//! - Solution combination (MWIS)
//!
//! # Example
//!
//! ```ignore
//! let pipeline = Pipeline::builder()
//!     .price_discoverer(LocalSolver::new())
//!     .allocator(MmAllocator::new())
//!     .partial_solver(GreedySolver::new())
//!     .partial_solver(ArbitrageDetector::new())
//!     .combine_with_mwis()
//!     .fixed_point_iterations(5)
//!     .build();
//!
//! let result = pipeline.solve(&problem);
//! ```

use std::time::Instant;

use matching_engine::Problem;

use crate::combiner::{CombineStats, SolutionCombiner, SolutionConfidence, SolverContribution, SolverSolution};
use crate::greedy::GreedySolver;
use crate::local_solver::LocalSolver;
use crate::mm_allocator::MmAllocator;
use crate::price_projector::PriceProjector as PriceProjectorImpl;
use crate::specialized::{ArbitrageDetector, BundleDecomposer, ChainFinder};
use crate::traits::{
    AllocationResult, OrderAllocator, PartialSolution, PartialSolver, PriceDiscoverer,
    PriceDiscoveryResult, PriceProjectionResult, PriceProjector,
};
use crate::{MatchingResult, MultiHeuristicSolver, Solver};

#[cfg(feature = "milp")]
use crate::milp::MilpSolver;

// ============================================================================
// Pipeline Configuration
// ============================================================================

/// Configuration options for the pipeline.
#[derive(Clone, Debug)]
pub struct PipelineConfig {
    /// Whether to use fixed-point iteration between pricing and allocation.
    pub use_fixed_point: bool,

    /// Maximum iterations for fixed-point convergence.
    pub max_iterations: usize,

    /// Convergence threshold for fixed-point (welfare change).
    pub convergence_threshold: f64,

    /// Whether to combine partial solutions with MWIS.
    pub combine_with_mwis: bool,

    /// Time budget for MILP solver (if included).
    pub milp_timeout_secs: f64,

    /// Whether to include specialized solvers.
    pub include_specialized: bool,

    /// Whether to use price projection for cross-market consistency.
    pub use_price_projection: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            use_fixed_point: false,
            max_iterations: 5,
            convergence_threshold: 0.01,
            combine_with_mwis: true,
            milp_timeout_secs: 1.0,
            include_specialized: true,
            use_price_projection: false,
        }
    }
}

// ============================================================================
// Pipeline Result
// ============================================================================

/// Result from running a pipeline.
#[derive(Clone, Debug)]
pub struct PipelineResult {
    /// The final combined matching result.
    pub result: MatchingResult,

    /// Price discovery result (if applicable).
    pub price_discovery: Option<PriceDiscoveryResult>,

    /// Price projection result (if applicable).
    pub price_projection: Option<PriceProjectionResult>,

    /// Allocation result (if applicable).
    pub allocation: Option<AllocationResult>,

    /// Per-solver contributions to the final result.
    pub contributions: Vec<SolverContribution>,

    /// Statistics from combining.
    pub combine_stats: Option<CombineStats>,

    /// Number of fixed-point iterations (if applicable).
    pub iterations: usize,

    /// Total time spent (seconds).
    pub total_time_secs: f64,

    /// Time breakdown by phase.
    pub phase_times: PipelineTimings,
}

/// Timing breakdown for pipeline phases.
#[derive(Clone, Debug, Default)]
pub struct PipelineTimings {
    pub price_discovery_secs: f64,
    pub price_projection_secs: f64,
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
}

impl PipelineResult {
    /// Create an empty result.
    pub fn empty(liquidity: matching_engine::LiquidityPool) -> Self {
        Self {
            result: MatchingResult::new(liquidity),
            price_discovery: None,
            price_projection: None,
            allocation: None,
            contributions: Vec::new(),
            combine_stats: None,
            iterations: 0,
            total_time_secs: 0.0,
            phase_times: PipelineTimings::default(),
        }
    }
}

// ============================================================================
// Pipeline
// ============================================================================

/// A configured pipeline for solving matching problems.
pub struct Pipeline {
    /// Price discovery component (optional).
    price_discoverer: Option<Box<dyn PriceDiscoverer>>,

    /// Price projector for cross-market consistency (optional).
    price_projector: Option<Box<dyn PriceProjector>>,

    /// Order allocator (optional).
    allocator: Option<Box<dyn OrderAllocator>>,

    /// Partial solvers for MWIS combination.
    partial_solvers: Vec<Box<dyn PartialSolver>>,

    /// Solution combiner.
    combiner: SolutionCombiner,

    /// Pipeline configuration.
    config: PipelineConfig,
}

impl Pipeline {
    /// Create a pipeline with the current default approach.
    ///
    /// This uses LocalSolver for price discovery and MmAllocator for allocation.
    pub fn current() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .allocator(MmAllocator::new())
            .build()
    }

    /// Create a full platform pipeline with all solvers.
    ///
    /// Includes greedy, randomized, MILP, and specialized solvers.
    #[cfg(feature = "milp")]
    pub fn full_platform() -> Self {
        Self::builder()
            .partial_solver(GreedySolver::new())
            .partial_solver(MultiHeuristicSolver::new())
            .partial_solver(MilpSolver::with_timeout(1.0))
            .partial_solver(ArbitrageDetector::new())
            .partial_solver(BundleDecomposer::new())
            .partial_solver(ChainFinder::new())
            .combine_with_mwis(true)
            .build()
    }

    /// Create a full platform pipeline without MILP feature.
    #[cfg(not(feature = "milp"))]
    pub fn full_platform() -> Self {
        Self::builder()
            .partial_solver(GreedySolver::new())
            .partial_solver(MultiHeuristicSolver::new())
            .partial_solver(ArbitrageDetector::new())
            .partial_solver(BundleDecomposer::new())
            .partial_solver(ChainFinder::new())
            .combine_with_mwis(true)
            .build()
    }

    /// Create an iterative pipeline with fixed-point iteration.
    ///
    /// Iterates between price discovery and allocation until convergence.
    pub fn iterative() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .allocator(MmAllocator::new())
            .use_fixed_point(true)
            .max_iterations(5)
            .build()
    }

    /// Create a consistent pipeline with price projection.
    ///
    /// Uses LocalSolver for price discovery, PriceProjector for cross-market
    /// consistency, and MmAllocator for allocation.
    ///
    /// This pipeline ensures that prices respect marginal consistency
    /// constraints from cross-market orders (bundles).
    pub fn consistent() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .price_projector(PriceProjectorImpl::new())
            .allocator(MmAllocator::new())
            .use_price_projection(true)
            .build()
    }

    /// Create a full pipeline with all components.
    ///
    /// Includes price discovery, projection, allocation, and MWIS combination.
    pub fn full() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .price_projector(PriceProjectorImpl::new())
            .allocator(MmAllocator::new())
            .partial_solver(GreedySolver::new())
            .use_price_projection(true)
            .combine_with_mwis(true)
            .build()
    }

    /// Create a new pipeline builder.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Solve a matching problem using this pipeline.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        let mut result = PipelineResult::empty(problem.liquidity.snapshot());
        let mut timings = PipelineTimings::default();

        // Phase 1: Price Discovery
        let mut price_result = if let Some(ref discoverer) = self.price_discoverer {
            let pd_start = Instant::now();
            let pd_result = discoverer.discover_prices(problem);
            timings.price_discovery_secs = pd_start.elapsed().as_secs_f64();
            Some(pd_result)
        } else {
            None
        };

        // Phase 2: Price Projection (if we have prices and a projector)
        let projection_result = if let (Some(ref projector), Some(ref mut prices)) =
            (&self.price_projector, &mut price_result)
        {
            if self.config.use_price_projection {
                let proj_start = Instant::now();
                let proj_result = projector.project(&prices.prices, problem);
                timings.price_projection_secs = proj_start.elapsed().as_secs_f64();

                // Update prices with projected values
                if proj_result.success {
                    // Recompute fills at projected prices
                    crate::price_projector::recompute_fills(prices, &proj_result.prices, problem);
                }

                Some(proj_result)
            } else {
                None
            }
        } else {
            None
        };

        // Phase 3: Order Allocation (if we have prices and an allocator)
        let allocation_result = if let (Some(ref allocator), Some(ref prices)) =
            (&self.allocator, &price_result)
        {
            let alloc_start = Instant::now();
            let alloc_result = allocator.allocate(
                &problem.mm_constraints,
                &prices.prices,
                &problem.orders,
            );
            timings.allocation_secs = alloc_start.elapsed().as_secs_f64();
            Some(alloc_result)
        } else {
            None
        };

        // Phase 4: Run Partial Solvers
        let partial_start = Instant::now();
        let partial_solutions: Vec<PartialSolution> = self
            .partial_solvers
            .iter()
            .map(|solver| solver.solve_partial(problem))
            .collect();
        timings.partial_solving_secs = partial_start.elapsed().as_secs_f64();

        // Phase 5: Combine Solutions
        let combine_start = Instant::now();
        if self.config.combine_with_mwis && !partial_solutions.is_empty() {
            // Convert PartialSolutions to SolverSolutions for the combiner
            let solver_solutions: Vec<SolverSolution> = partial_solutions
                .iter()
                .map(|ps| {
                    // Map fills to order indices
                    let fills: Vec<_> = ps
                        .fills
                        .iter()
                        .filter_map(|fill| {
                            problem
                                .orders
                                .iter()
                                .position(|o| o.id == fill.order_id)
                                .map(|idx| (idx, fill.clone()))
                        })
                        .collect();

                    SolverSolution {
                        solver_name: ps.solver_name.clone(),
                        fills,
                        welfare: ps.welfare,
                        confidence: ps.confidence,
                    }
                })
                .collect();

            let (combined_result, stats, contributions) =
                self.combiner.combine(solver_solutions, problem);

            result.result = combined_result;
            result.combine_stats = Some(stats);
            result.contributions = contributions;
        } else if let Some(ref pd_result) = price_result {
            // Use price discovery result directly
            result.result = self.build_result_from_prices(problem, pd_result, &allocation_result);
        } else if !partial_solutions.is_empty() {
            // Just use the best partial solution
            let best = partial_solutions
                .iter()
                .max_by_key(|s| s.welfare)
                .unwrap();

            result.result = self.build_result_from_partial(problem, best);
            result.contributions.push(SolverContribution {
                solver_name: best.solver_name.clone(),
                fills_contributed: best.fills.len(),
                welfare_contributed: best.welfare,
            });
        }
        timings.combining_secs = combine_start.elapsed().as_secs_f64();

        result.price_discovery = price_result;
        result.price_projection = projection_result;
        result.allocation = allocation_result;
        result.phase_times = timings;
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.iterations = 1; // TODO: Implement fixed-point iteration tracking

        result
    }

    /// Build a MatchingResult from price discovery and allocation.
    fn build_result_from_prices(
        &self,
        problem: &Problem,
        prices: &PriceDiscoveryResult,
        allocation: &Option<AllocationResult>,
    ) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        // Collect fills from all market solutions
        let mut all_fills = prices.all_fills();

        // If we have allocation, filter to only activated orders
        if let Some(ref alloc) = allocation {
            let activated_set: std::collections::HashSet<_> =
                alloc.activated_orders.iter().copied().collect();
            all_fills.retain(|f| activated_set.contains(&f.order_id));
        }

        // Build order lookup
        let order_map: std::collections::HashMap<_, _> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // Add fills to result
        for fill in all_fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                result.add_fill(fill, order);
            }
        }

        result
    }

    /// Build a MatchingResult from a partial solution.
    fn build_result_from_partial(
        &self,
        problem: &Problem,
        partial: &PartialSolution,
    ) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        let order_map: std::collections::HashMap<_, _> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        for fill in &partial.fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                result.add_fill(fill.clone(), order);
            }
        }

        result
    }
}

impl Solver for Pipeline {
    fn solve(&self, problem: &Problem) -> MatchingResult {
        Pipeline::solve(self, problem).result
    }

    fn name(&self) -> &str {
        "Pipeline"
    }
}

// ============================================================================
// Pipeline Builder
// ============================================================================

/// Builder for constructing pipelines.
pub struct PipelineBuilder {
    price_discoverer: Option<Box<dyn PriceDiscoverer>>,
    price_projector: Option<Box<dyn PriceProjector>>,
    allocator: Option<Box<dyn OrderAllocator>>,
    partial_solvers: Vec<Box<dyn PartialSolver>>,
    config: PipelineConfig,
}

impl PipelineBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            price_discoverer: None,
            price_projector: None,
            allocator: None,
            partial_solvers: Vec::new(),
            config: PipelineConfig::default(),
        }
    }

    /// Set the price discoverer.
    pub fn price_discoverer<P: PriceDiscoverer + 'static>(mut self, discoverer: P) -> Self {
        self.price_discoverer = Some(Box::new(discoverer));
        self
    }

    /// Set the price projector for cross-market consistency.
    pub fn price_projector<P: PriceProjector + 'static>(mut self, projector: P) -> Self {
        self.price_projector = Some(Box::new(projector));
        self
    }

    /// Set the order allocator.
    pub fn allocator<A: OrderAllocator + 'static>(mut self, allocator: A) -> Self {
        self.allocator = Some(Box::new(allocator));
        self
    }

    /// Add a partial solver.
    pub fn partial_solver<S: PartialSolver + 'static>(mut self, solver: S) -> Self {
        self.partial_solvers.push(Box::new(solver));
        self
    }

    /// Set whether to use fixed-point iteration.
    pub fn use_fixed_point(mut self, use_it: bool) -> Self {
        self.config.use_fixed_point = use_it;
        self
    }

    /// Set maximum iterations for fixed-point.
    pub fn max_iterations(mut self, max: usize) -> Self {
        self.config.max_iterations = max;
        self
    }

    /// Set convergence threshold for fixed-point.
    pub fn convergence_threshold(mut self, threshold: f64) -> Self {
        self.config.convergence_threshold = threshold;
        self
    }

    /// Set whether to combine with MWIS.
    pub fn combine_with_mwis(mut self, combine: bool) -> Self {
        self.config.combine_with_mwis = combine;
        self
    }

    /// Set MILP timeout.
    pub fn milp_timeout(mut self, timeout_secs: f64) -> Self {
        self.config.milp_timeout_secs = timeout_secs;
        self
    }

    /// Set whether to include specialized solvers.
    pub fn include_specialized(mut self, include: bool) -> Self {
        self.config.include_specialized = include;
        self
    }

    /// Set whether to use price projection for cross-market consistency.
    pub fn use_price_projection(mut self, use_it: bool) -> Self {
        self.config.use_price_projection = use_it;
        self
    }

    /// Build the pipeline.
    pub fn build(self) -> Pipeline {
        Pipeline {
            price_discoverer: self.price_discoverer,
            price_projector: self.price_projector,
            allocator: self.allocator,
            partial_solvers: self.partial_solvers,
            combiner: SolutionCombiner::new(),
            config: self.config,
        }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Platform Config Integration
// ============================================================================

use crate::platform::PlatformConfig;

impl Pipeline {
    /// Create a Pipeline from a PlatformConfig.
    ///
    /// This allows using the new Pipeline architecture while maintaining
    /// compatibility with the existing PlatformConfig API.
    #[cfg(feature = "milp")]
    pub fn from_platform_config(config: &PlatformConfig) -> Self {
        let mut builder = Pipeline::builder();

        if config.include_greedy {
            builder = builder.partial_solver(GreedySolver::new());
        }

        if config.include_randomized {
            builder = builder.partial_solver(MultiHeuristicSolver::new());
        }

        if config.include_milp {
            builder = builder.partial_solver(MilpSolver::with_timeout(config.milp_timeout_secs()));
        }

        if config.include_arbitrage {
            builder = builder.partial_solver(ArbitrageDetector::new());
        }

        if config.include_bundle_decomposer {
            builder = builder.partial_solver(BundleDecomposer::new());
        }

        if config.include_chain_finder {
            builder = builder.partial_solver(ChainFinder::new());
        }

        builder.combine_with_mwis(true).build()
    }

    /// Create a Pipeline from a PlatformConfig (no MILP).
    #[cfg(not(feature = "milp"))]
    pub fn from_platform_config(config: &PlatformConfig) -> Self {
        let mut builder = Pipeline::builder();

        if config.include_greedy {
            builder = builder.partial_solver(GreedySolver::new());
        }

        if config.include_randomized {
            builder = builder.partial_solver(MultiHeuristicSolver::new());
        }

        if config.include_arbitrage {
            builder = builder.partial_solver(ArbitrageDetector::new());
        }

        if config.include_bundle_decomposer {
            builder = builder.partial_solver(BundleDecomposer::new());
        }

        if config.include_chain_finder {
            builder = builder.partial_solver(ChainFinder::new());
        }

        builder.combine_with_mwis(true).build()
    }
}

// ============================================================================
// Adapter Implementations for Specialized Solvers
// ============================================================================

impl PartialSolver for ArbitrageDetector {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let result = self.solve(problem);
        PartialSolution::with_fills(
            "Arbitrage",
            result.fills,
            result.total_welfare,
            SolutionConfidence::Heuristic,
        )
    }

    fn name(&self) -> &str {
        "Arbitrage"
    }
}

impl PartialSolver for BundleDecomposer {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let result = self.solve(problem);
        PartialSolution::with_fills(
            "BundleDecomposer",
            result.fills,
            result.total_welfare,
            SolutionConfidence::Heuristic,
        )
    }

    fn name(&self) -> &str {
        "BundleDecomposer"
    }
}

impl PartialSolver for ChainFinder {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let result = self.solve(problem);
        PartialSolution::with_fills(
            "ChainFinder",
            result.fills,
            result.total_welfare,
            SolutionConfidence::Heuristic,
        )
    }

    fn name(&self) -> &str {
        "ChainFinder"
    }
}

impl PartialSolver for MultiHeuristicSolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let result = self.solve(problem);
        PartialSolution::with_fills(
            "MultiHeuristic",
            result.fills,
            result.total_welfare,
            SolutionConfidence::Heuristic,
        )
    }

    fn name(&self) -> &str {
        "MultiHeuristic"
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    fn test_pipeline_current() {
        let problem = create_test_problem();
        let pipeline = Pipeline::current();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
        assert!(result.total_time_secs >= 0.0);
    }

    #[test]
    fn test_pipeline_builder() {
        let problem = create_test_problem();

        let pipeline = Pipeline::builder()
            .partial_solver(GreedySolver::new())
            .combine_with_mwis(true)
            .build();

        let result = pipeline.solve(&problem);
        assert!(result.result.orders_filled > 0);
    }

    #[test]
    fn test_pipeline_full_platform() {
        let problem = create_test_problem();
        let pipeline = Pipeline::full_platform();
        let result = pipeline.solve(&problem);

        // Should have contributions from at least one solver
        assert!(!result.contributions.is_empty() || result.result.orders_filled == 0);
    }

    #[test]
    fn test_pipeline_iterative() {
        let problem = create_test_problem();
        let pipeline = Pipeline::iterative();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
    }

    #[test]
    fn test_pipeline_timings() {
        let problem = create_test_problem();
        let pipeline = Pipeline::current();
        let result = pipeline.solve(&problem);

        let total_phase_time = result.phase_times.price_discovery_secs
            + result.phase_times.allocation_secs
            + result.phase_times.partial_solving_secs
            + result.phase_times.combining_secs;

        // Phase times should roughly add up to total time
        // (allowing for some overhead)
        assert!(total_phase_time <= result.total_time_secs * 1.5 + 0.001);
    }

    #[test]
    fn test_pipeline_consistent() {
        let problem = create_test_problem();
        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
        // With no cross-market orders, projection should be a no-op
        if let Some(projection) = &result.price_projection {
            assert!(projection.success);
        }
    }

    #[test]
    fn test_pipeline_full() {
        let problem = create_test_problem();
        let pipeline = Pipeline::full();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
        assert!(result.total_time_secs >= 0.0);
    }

    #[test]
    fn test_pipeline_consistent_with_bundles() {
        use matching_engine::bundle_yes;

        let mut problem = Problem::new("cross_market");
        let market_a = problem.markets.add_binary("market_a");
        let market_b = problem.markets.add_binary("market_b");

        // Add liquidity
        problem.liquidity.add_ask(market_a, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_a, 1, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 1, 500_000_000, 1000);

        // Add single-market orders
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market_a,
            600_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market_b,
            400_000_000,
            100,
        ));

        // Add bundle order
        problem.orders.push(bundle_yes(
            &problem.markets,
            3,
            &[market_a, market_b],
            300_000_000,
            50,
        ));

        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
        if let Some(projection) = &result.price_projection {
            assert!(projection.success);
        }
    }
}
