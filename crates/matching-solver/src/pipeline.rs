//! Pipeline configuration for FBA solving.
//!
//! The pipeline provides a flexible way to combine solver components:
//! - Price discovery (LocalSolver)
//! - Price projection (PriceProjector)
//! - Order allocation (MmAllocator)
//! - Arbitrage detection (ArbitrageDetector)
//!
//! # Example
//!
//! ```ignore
//! let pipeline = Pipeline::consistent();
//! let result = pipeline.solve(&problem);
//! ```

use std::time::Instant;

use matching_engine::Problem;

use crate::combiner::{
    CombineStats, SolutionCombiner, SolutionConfidence, SolverContribution, SolverSolution,
};
use crate::greedy::GreedySolver;
use crate::local_solver::LocalSolver;
use crate::mm_allocator::MmAllocator;
use crate::price_projector::PriceProjector as PriceProjectorImpl;
use crate::specialized::ArbitrageDetector;
use crate::traits::{
    AllocationResult, OrderAllocator, PartialSolution, PartialSolver, PriceDiscoverer,
    PriceDiscoveryResult, PriceProjectionResult, PriceProjector,
};
use crate::{MatchingResult, Solver};

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

    /// Whether to use price projection for cross-market consistency.
    pub use_price_projection: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            use_fixed_point: false,
            max_iterations: 5,
            convergence_threshold: 0.01,
            combine_with_mwis: false,
            milp_timeout_secs: 1.0,
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

    /// Per-iteration stats for convergence analysis.
    pub iteration_stats: Vec<IterationStats>,

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

/// Stats for a single fixed-point iteration.
#[derive(Clone, Debug, Default)]
pub struct IterationStats {
    /// Iteration number (1-indexed).
    pub iteration: usize,
    /// Total welfare after this iteration.
    pub welfare: i64,
    /// Total volume (shares) after this iteration.
    pub volume: u64,
    /// Total fills after this iteration.
    pub fills: usize,
    /// Welfare delta from previous iteration.
    pub welfare_delta: i64,
    /// Volume delta from previous iteration.
    pub volume_delta: u64,
    /// Fills delta from previous iteration.
    pub fills_delta: usize,
    /// Breakdown: fills from price discovery.
    pub price_discovery_fills: usize,
    /// Breakdown: fills from bundle matching.
    pub bundle_fills: usize,
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
            iteration_stats: Vec::new(),
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
    /// Uses LocalSolver for price discovery and MmAllocator for allocation.
    pub fn current() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .allocator(MmAllocator::new())
            .build()
    }

    /// Create a full platform pipeline with all solvers.
    #[cfg(feature = "milp")]
    pub fn full_platform() -> Self {
        Self::builder()
            .partial_solver(GreedySolver::new())
            .partial_solver(MilpSolver::with_timeout(1.0))
            .partial_solver(ArbitrageDetector::new())
            .combine_with_mwis(true)
            .build()
    }

    /// Create a full platform pipeline without MILP feature.
    #[cfg(not(feature = "milp"))]
    pub fn full_platform() -> Self {
        Self::builder()
            .partial_solver(GreedySolver::new())
            .partial_solver(ArbitrageDetector::new())
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
    pub fn consistent() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .price_projector(PriceProjectorImpl::new())
            .allocator(MmAllocator::new())
            .use_price_projection(true)
            .build()
    }

    /// Create a full pipeline with all components (sequential, no MWIS).
    ///
    /// Runs solvers in sequence:
    /// 1. LocalSolver for price discovery on single-market orders
    /// 2. PriceProjector for multi-outcome consistency
    /// 3. MmAllocator for MM budget constraints
    /// 4. ArbitrageDetector for bundle matching
    ///
    /// Each phase consumes liquidity, subsequent phases work on remaining.
    pub fn full() -> Self {
        Self::builder()
            .price_discoverer(LocalSolver::new())
            .price_projector(PriceProjectorImpl::new())
            .allocator(MmAllocator::new())
            .partial_solver(ArbitrageDetector::new())
            .use_price_projection(true)
            .use_fixed_point(true)
            .max_iterations(5)
            .combine_with_mwis(false) // Sequential, not MWIS
            .build()
    }

    /// Create a new pipeline builder.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Solve a matching problem using this pipeline.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        if self.config.use_fixed_point {
            self.solve_sequential(problem)
        } else {
            self.solve_single_pass(problem)
        }
    }

    /// Sequential solving with fixed-point iteration.
    ///
    /// Runs phases in order, each consuming liquidity:
    /// 1. Price Discovery (LocalSolver) - fills single-market orders
    /// 2. Price Projection - adjusts prices for consistency
    /// 3. MM Allocation - activates MM orders within budget
    /// 4. Partial Solvers (ArbitrageDetector) - fills bundles
    /// Repeats until convergence or max iterations.
    fn solve_sequential(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        let mut result = PipelineResult::empty(problem.liquidity.snapshot());
        let mut timings = PipelineTimings::default();

        let mut prev_welfare = 0i64;
        let mut prev_volume = 0u64;
        let mut prev_fills = 0usize;
        let mut iterations = 0;

        // Create a mutable problem with remaining liquidity
        let mut remaining_liquidity = problem.liquidity.snapshot();

        // Track orders that have already been filled
        let mut filled_order_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // Track fills for this iteration
            let mut iter_price_discovery_fills = 0usize;
            let mut iter_bundle_fills = 0usize;

            // Filter out already-filled orders
            let remaining_orders: Vec<_> = problem
                .orders
                .iter()
                .filter(|o| !filled_order_ids.contains(&o.id))
                .cloned()
                .collect();

            // Create problem view with remaining liquidity and unfilled orders
            let iter_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                liquidity: remaining_liquidity.clone(),
                orders: remaining_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            // Phase 1: Price Discovery
            let price_result = if let Some(ref discoverer) = self.price_discoverer {
                let pd_start = Instant::now();
                let pd_result = discoverer.discover_prices(&iter_problem);
                timings.price_discovery_secs += pd_start.elapsed().as_secs_f64();
                Some(pd_result)
            } else {
                None
            };

            // Phase 2: Price Projection
            let projection_result = if let (Some(ref projector), Some(ref prices)) =
                (&self.price_projector, &price_result)
            {
                if self.config.use_price_projection {
                    let proj_start = Instant::now();
                    let proj_result = projector.project(&prices.prices, &iter_problem);
                    timings.price_projection_secs += proj_start.elapsed().as_secs_f64();
                    Some(proj_result)
                } else {
                    None
                }
            } else {
                None
            };

            // Phase 3: MM Allocation
            let allocation_result = if let (Some(ref allocator), Some(ref prices)) =
                (&self.allocator, &price_result)
            {
                let alloc_start = Instant::now();
                let alloc_result = allocator.allocate(
                    &iter_problem.mm_constraints,
                    &prices.prices,
                    &iter_problem.orders,
                );
                timings.allocation_secs += alloc_start.elapsed().as_secs_f64();
                Some(alloc_result)
            } else {
                None
            };

            // Build result from price discovery + allocation
            if let Some(ref pd_result) = price_result {
                let iter_result =
                    self.build_result_from_prices(&iter_problem, pd_result, &allocation_result);

                // Consume liquidity for filled orders and track filled order IDs
                for fill in &iter_result.fills {
                    if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                        self.consume_order_liquidity(order, fill.fill_qty, &mut remaining_liquidity);
                        filled_order_ids.insert(fill.order_id);
                        iter_price_discovery_fills += 1;
                    }
                }

                // Merge into result
                for fill in iter_result.fills {
                    if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                        result.result.add_fill(fill, order);
                    }
                }
            }

            // Phase 4: Run Partial Solvers (e.g., ArbitrageDetector for bundles)
            let partial_start = Instant::now();
            for solver in &self.partial_solvers {
                // Filter out already-filled orders for partial solvers too
                let partial_orders: Vec<_> = problem
                    .orders
                    .iter()
                    .filter(|o| !filled_order_ids.contains(&o.id))
                    .cloned()
                    .collect();

                // Create problem with current remaining liquidity and unfilled orders
                let partial_problem = Problem {
                    name: problem.name.clone(),
                    markets: problem.markets.clone(),
                    liquidity: remaining_liquidity.clone(),
                    orders: partial_orders,
                    mm_constraints: problem.mm_constraints.clone(),
                    market_groups: problem.market_groups.clone(),
                };

                let partial_result = solver.solve_partial(&partial_problem);

                // Add fills, consume liquidity, and track filled order IDs
                for fill in partial_result.fills {
                    if let Some(order) = problem.orders.iter().find(|o| o.id == fill.order_id) {
                        self.consume_order_liquidity(order, fill.fill_qty, &mut remaining_liquidity);
                        filled_order_ids.insert(fill.order_id);
                        result.result.add_fill(fill, order);
                        iter_bundle_fills += 1;

                        result.contributions.push(SolverContribution {
                            solver_name: solver.name().to_string(),
                            fills_contributed: 1,
                            welfare_contributed: order.welfare_contribution(0, 1), // Approx
                        });
                    }
                }
            }
            timings.partial_solving_secs += partial_start.elapsed().as_secs_f64();

            // Store last iteration's metadata
            result.price_discovery = price_result;
            result.price_projection = projection_result;
            result.allocation = allocation_result;

            // Calculate current totals
            let current_welfare = result.result.total_welfare;
            let current_volume: u64 = result.result.fills.iter().map(|f| f.fill_qty).sum();
            let current_fills = result.result.fills.len();

            // Track iteration stats
            result.iteration_stats.push(IterationStats {
                iteration: iterations,
                welfare: current_welfare,
                volume: current_volume,
                fills: current_fills,
                welfare_delta: current_welfare - prev_welfare,
                volume_delta: current_volume.saturating_sub(prev_volume),
                fills_delta: current_fills.saturating_sub(prev_fills),
                price_discovery_fills: iter_price_discovery_fills,
                bundle_fills: iter_bundle_fills,
            });

            // Check convergence
            let welfare_delta = current_welfare - prev_welfare;
            let converged = welfare_delta.abs() as f64 / (prev_welfare.abs() as f64 + 1.0)
                < self.config.convergence_threshold;

            prev_welfare = current_welfare;
            prev_volume = current_volume;
            prev_fills = current_fills;

            if converged && iter > 0 {
                break;
            }
        }

        result.result.remaining_liquidity = remaining_liquidity;
        result.phase_times = timings;
        result.total_time_secs = start.elapsed().as_secs_f64();
        result.iterations = iterations;

        result
    }

    /// Single-pass solving (original behavior).
    fn solve_single_pass(&self, problem: &Problem) -> PipelineResult {
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

        // Phase 2: Price Projection
        let projection_result = if let (Some(ref projector), Some(ref mut prices)) =
            (&self.price_projector, &mut price_result)
        {
            if self.config.use_price_projection {
                let proj_start = Instant::now();
                let proj_result = projector.project(&prices.prices, problem);
                timings.price_projection_secs = proj_start.elapsed().as_secs_f64();

                if proj_result.success {
                    crate::price_projector::recompute_fills(prices, &proj_result.prices, problem);
                }

                Some(proj_result)
            } else {
                None
            }
        } else {
            None
        };

        // Phase 3: Order Allocation
        let allocation_result =
            if let (Some(ref allocator), Some(ref prices)) = (&self.allocator, &price_result) {
                let alloc_start = Instant::now();
                let alloc_result =
                    allocator.allocate(&problem.mm_constraints, &prices.prices, &problem.orders);
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
            let solver_solutions: Vec<SolverSolution> = partial_solutions
                .iter()
                .map(|ps| {
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
            result.result = self.build_result_from_prices(problem, pd_result, &allocation_result);
        } else if !partial_solutions.is_empty() {
            let best = partial_solutions.iter().max_by_key(|s| s.welfare).unwrap();

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
        result.iterations = 1;

        result
    }

    /// Consume liquidity for an order fill.
    fn consume_order_liquidity(
        &self,
        order: &matching_engine::Order,
        qty: matching_engine::Qty,
        liquidity: &mut matching_engine::LiquidityPool,
    ) {
        if order.num_markets == 1 {
            // For single-market orders, consume from the appropriate book
            let market = order.markets[0];
            // Determine outcome from payoffs (simplified: assume outcome 0 if positive payoff at state 0)
            let outcome = if order.payoffs[0] > 0 { 0 } else { 1 };
            if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
                book.consume_asks(qty, order.limit_price);
            }
        } else {
            // For bundles, consume from joint liquidity
            let joint_outcome = self.build_joint_outcome_for_order(order);
            if let Some(joint_book) = liquidity.joint_book_get_mut(&joint_outcome) {
                joint_book.consume_asks(qty, order.limit_price);
            }
        }
    }

    /// Build a JointOutcome from a bundle order.
    fn build_joint_outcome_for_order(
        &self,
        order: &matching_engine::Order,
    ) -> matching_engine::JointOutcome {
        let mut legs = Vec::new();
        for market_idx in 0..order.num_markets as usize {
            let market = order.markets[market_idx];
            if market.is_none() {
                continue;
            }
            // Determine outcome from payoffs
            // For bundle YES orders, state 0 (all YES) has positive payoff
            let outcome = if order.payoffs[0] > 0 { 0 } else { 1 };
            legs.push((market, outcome));
        }
        matching_engine::JointOutcome::new(legs)
    }

    /// Build a MatchingResult from price discovery and allocation.
    fn build_result_from_prices(
        &self,
        problem: &Problem,
        prices: &PriceDiscoveryResult,
        allocation: &Option<AllocationResult>,
    ) -> MatchingResult {
        let mut result = MatchingResult::new(problem.liquidity.snapshot());

        let mut all_fills = prices.all_fills();

        if let Some(ref alloc) = allocation {
            let activated_set: std::collections::HashSet<_> =
                alloc.activated_orders.iter().copied().collect();
            all_fills.retain(|f| activated_set.contains(&f.order_id));
        }

        let order_map: std::collections::HashMap<_, _> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

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
// Adapter Implementations for Solvers
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

        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

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
    fn test_pipeline_consistent() {
        let problem = create_test_problem();
        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        assert!(result.price_discovery.is_some());
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

        problem.liquidity.add_ask(market_a, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_a, 1, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 1, 500_000_000, 1000);

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
