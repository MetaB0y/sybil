//! Matching solver for prediction market FBA (Frequent Batch Auction).
//!
//! # Architecture
//!
//! The solver operates in phases:
//! 1. **Price Discovery** (`LocalSolver`): Find clearing prices per market
//! 2. **Price Projection** (`PriceProjector`): Ensure cross-market consistency
//! 3. **MM Allocation** (`MmAllocator`): Respect market maker budget constraints
//! 4. **Arbitrage Detection** (`ArbitrageDetector`): Find remaining opportunities
//!
//! # Quick Start
//!
//! ```ignore
//! use matching_solver::Pipeline;
//!
//! let pipeline = Pipeline::consistent();
//! let result = pipeline.solve(&problem);
//! ```

// Internal modules
pub(crate) mod combiner;
pub mod greedy;
pub mod local_solver;
pub mod mm_allocator;
pub mod price_projector;
pub(crate) mod specialized;
pub mod traits;
pub mod pipeline;
pub mod benchmark;

#[cfg(feature = "milp")]
pub mod milp;

// === Public API ===

// Core solvers
pub use greedy::GreedySolver;

// New architecture components
pub use local_solver::{LocalSolver, LocalSolverConfig, MarketSolution, solve_all_markets_parallel, solve_market_lp};
pub use mm_allocator::{MmAllocator, AllocatorConfig, AllocationResult as MmAllocationResult, MmAllocation};

// Experimentation platform traits
pub use traits::{
    PriceDiscoverer, PriceDiscoveryResult,
    PriceProjector, PriceProjectionResult,
    OrderAllocator, AllocationResult,
    PartialSolver, PartialSolution,
    matching_result_to_partial,
};

// Price projector implementation
pub use price_projector::{
    PriceProjector as PriceProjectorImpl,
    ProjectorConfig,
    ProjectionResult,
    JointOutcome,
    MarginalViolation,
    recompute_fills,
};

// Pipeline system
pub use pipeline::{Pipeline, PipelineBuilder, PipelineConfig, PipelineResult, PipelineTimings};

// Benchmark harness
pub use benchmark::{BenchmarkHarness, BenchmarkResults, BenchmarkRun, compare_to_baseline};

#[cfg(feature = "milp")]
pub use milp::{MilpConfig, MilpResult, MilpSolver, SolveStatus};

use matching_engine::{LiquidityPool, Order, Fill, Problem};

/// Result of solving a matching problem.
#[derive(Clone, Debug)]
pub struct MatchingResult {
    /// Orders that were filled
    pub fills: Vec<Fill>,
    /// Total welfare achieved
    pub total_welfare: i64,
    /// Number of orders filled (at least partially)
    pub orders_filled: usize,
    /// Number of orders unfilled due to liquidity exhaustion
    pub orders_unfilled_liquidity: usize,
    /// Number of orders unfilled due to all-or-none constraints
    pub orders_unfilled_aon: usize,
    /// Total quantity filled across all orders
    pub total_quantity_filled: u64,
    /// Remaining liquidity after matching
    pub remaining_liquidity: LiquidityPool,
}

impl MatchingResult {
    pub fn new(remaining_liquidity: LiquidityPool) -> Self {
        Self {
            fills: Vec::new(),
            total_welfare: 0,
            orders_filled: 0,
            orders_unfilled_liquidity: 0,
            orders_unfilled_aon: 0,
            total_quantity_filled: 0,
            remaining_liquidity,
        }
    }

    pub fn add_fill(&mut self, fill: Fill, order: &Order) {
        self.total_welfare += fill.welfare(order);
        self.total_quantity_filled += fill.fill_qty;
        if fill.fill_qty > 0 {
            self.orders_filled += 1;
        }
        self.fills.push(fill);
    }
}

/// Trait for matching solvers.
pub trait Solver {
    fn solve(&self, problem: &Problem) -> MatchingResult;
    fn name(&self) -> &str;
}
