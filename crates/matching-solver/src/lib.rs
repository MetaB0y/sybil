//! Solvers for the NP-hard matching problem.
//!
//! This crate provides multiple solver implementations:
//!
//! - [`GreedySolver`]: Fast heuristic that processes orders by welfare potential
//! - [`MultiHeuristicSolver`]: Tries multiple sorting strategies, returns best
//! - [`MilpSolver`]: Optimal via MILP (requires `milp` feature)
//! - [`SolverPlatform`]: Production-ready platform combining all solvers
//!
//! # Quick Start
//!
//! ```ignore
//! use matching_solver::{Solver, GreedySolver};
//!
//! let solver = GreedySolver::new();
//! let result = solver.solve(&problem);
//! println!("Welfare: {}, Filled: {}", result.total_welfare, result.orders_filled);
//! ```
//!
//! For optimal solutions (with time budget):
//!
//! ```ignore
//! use matching_solver::{SolverPlatform, PlatformConfig};
//!
//! let platform = SolverPlatform::with_config(PlatformConfig {
//!     total_time_budget_ms: 5000,
//!     ..Default::default()
//! });
//! let result = platform.solve(&problem);
//! ```

// Internal modules
pub(crate) mod combiner;
pub mod greedy;
pub mod local_solver;
pub mod mm_allocator;
pub mod platform;
pub mod randomized;
pub(crate) mod specialized;

#[cfg(feature = "milp")]
pub mod milp;

// === Public API ===

// Core solvers
pub use greedy::GreedySolver;
pub use randomized::{MultiHeuristicSolver, RandomizedGreedySolver};
pub use platform::{PlatformConfig, PlatformResult, SolverPlatform};

// New architecture components
pub use local_solver::{LocalSolver, LocalSolverConfig, MarketSolution, solve_all_markets_parallel};
pub use mm_allocator::{MmAllocator, AllocatorConfig, AllocationResult, MmAllocation};

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
