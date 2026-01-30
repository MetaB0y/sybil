//! Matching solver for prediction market FBA (Frequent Batch Auction).
//!
//! # Architecture
//!
//! The solver operates in phases:
//! 1. **Price Discovery** (`LocalSolver`): Find clearing prices per market
//! 2. **Negrisk Arbitrage** (`NegriskSolver`): Exploit price inconsistencies
//! 3. **MM Allocation** (`MmAllocator`): Respect market maker budget constraints
//! 4. **Arbitrage Detection** (`ArbitrageDetector`): Find remaining opportunities
//!
//! # Quick Start
//!
//! ```ignore
//! use matching_solver::Pipeline;
//!
//! let pipeline = Pipeline::with_negrisk();
//! let result = pipeline.solve(&problem);
//! ```

// Internal modules
pub mod benchmark;
pub(crate) mod combiner;
pub mod greedy;
pub mod local_solver;
pub mod mm_allocator;
pub mod pipeline;
pub(crate) mod specialized;
pub mod traits;
pub mod verifier;
pub mod viz;

#[cfg(feature = "milp")]
pub mod milp;

// === Public API ===

// Core solvers
pub use greedy::GreedySolver;

// New architecture components
pub use local_solver::{
    solve_all_markets_parallel, solve_market_lp, LocalSolver, MarketSolution,
};
pub use mm_allocator::{AllocationResult as MmAllocationResult, MmAllocation, MmAllocator};

// Experimentation platform traits
pub use traits::{
    matching_result_to_partial, AllocationResult, OrderAllocator, PartialSolution, PartialSolver,
    PriceDiscoverer, PriceDiscoveryResult,
};

// Pipeline system
pub use pipeline::{
    IterationStats, Pipeline, PipelineBuilder, PipelineConfig, PipelineResult, PipelineTimings,
};

// Visualization
pub use viz::VizSnapshot;

// Benchmark harness
pub use benchmark::{compare_to_baseline, BenchmarkHarness, BenchmarkResults, BenchmarkRun};

// Result verification (for ZK proof integration)
pub use verifier::{verify, verify_strict, VerificationResult, Verifier, Violation, ViolationKind};

#[cfg(feature = "milp")]
pub use milp::{MilpConfig, MilpResult, MilpSolver, SolveStatus};

use serde::Serialize;

use matching_engine::{Fill, LiquidityPool, Order, Problem};

/// Result of solving a matching problem.
#[derive(Clone, Debug, Serialize)]
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
