//! Matching solver for prediction market FBA (Frequent Batch Auction).
//!
//! # Architecture
//!
//! The solver operates in phases:
//! 1. **Price Discovery** (`LocalSolver`): Find clearing prices per market
//! 2. **Negrisk Arbitrage** (`NegriskSolver`): Exploit price inconsistencies
//! 3. **MM Allocation** (`MmAllocator`): Respect market maker budget constraints
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
pub mod coefficients;
pub mod dual_master;
pub mod fill_extraction;
pub mod local_solver;
pub mod mm_allocator;
pub mod pipeline;
pub(crate) mod specialized;
pub mod traits;
pub mod verifier;
pub mod group_minting;
pub mod smoothed_solver;
pub mod joint_solver;
pub mod viz;

#[cfg(feature = "milp")]
pub mod milp;

#[cfg(feature = "lp")]
pub mod lp_solver;

// === Public API ===

// New architecture components
pub use local_solver::{LocalSolver, MarketSolution};
pub use mm_allocator::{MmAllocation, MmAllocator};

// Experimentation platform traits
pub use traits::{
    matching_result_to_partial, AllocationResult, OrderAllocator, PartialSolution, PartialSolver,
    PriceDiscoverer, PriceDiscoveryResult,
};

// Specialized solvers
pub use specialized::MultiMarketSolver;

// Dual decomposition
pub use dual_master::{DualConfig, DualMaster, DualResult, DualState, StepDecay};

// Smoothed gradient solver
pub use smoothed_solver::SmoothedSolver;

// Joint group solver
pub use joint_solver::JointGroupSolver;

// Pipeline system
pub use pipeline::{
    IterationStats, Pipeline, PipelineBuilder, PipelineConfig, PipelineResult, PipelineTimings,
    UcpStats,
};

// Visualization
pub use viz::VizSnapshot;

// Benchmark harness
pub use benchmark::{compare_to_baseline, BenchmarkHarness, BenchmarkResults, BenchmarkRun};

// Result verification (for ZK proof integration)
pub use verifier::{verify, verify_strict, VerificationResult, Verifier, Violation, ViolationKind};

#[cfg(feature = "milp")]
pub use milp::{MilpConfig, MilpResult, MilpSolver, MmBudgetMode, SolveStatus};

#[cfg(feature = "lp")]
pub use lp_solver::{LpConfig, LpSolver};

use serde::Serialize;

use matching_engine::{Fill, Order, Problem};

/// Result of solving a matching problem.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MatchingResult {
    /// Orders that were filled
    pub fills: Vec<Fill>,
    /// Total welfare achieved (fill-level surplus minus minting cost).
    pub total_welfare: i64,
    /// Cost of share creation (minting) not captured in fill-level welfare.
    ///
    /// For heuristic solvers this is 0 because arb order limits embed the cost.
    /// For MILP this equals `fill_welfare - objective_welfare` (the gap from
    /// group minting when Σp < $1 for some groups).
    pub minting_cost: i64,
    /// Number of orders filled (at least partially)
    pub orders_filled: usize,
    /// Number of orders unfilled due to liquidity exhaustion
    pub orders_unfilled_liquidity: usize,
    /// Total quantity filled across all orders
    pub total_quantity_filled: u64,
}

impl MatchingResult {
    pub fn new() -> Self {
        Self {
            fills: Vec::new(),
            total_welfare: 0,
            minting_cost: 0,
            orders_filled: 0,
            orders_unfilled_liquidity: 0,
            total_quantity_filled: 0,
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
