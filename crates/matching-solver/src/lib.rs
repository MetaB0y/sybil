//! Matching solver for prediction market FBA (Frequent Batch Auction).
//!
//! Solves the welfare-maximizing order matching problem via convex programs:
//! - **LP** (`lp_solver`): Linear program via HiGHS with entropy smoothing
//! - **EG** (`eg_solver`): Eisenberg-Gale / Fisher market formulation
//! - **Conic** (`conic_solver`): Conic EG via Clarabel
//! - **MILP** (`milp`): Mixed-integer via SCIP (exact with timeout)

// Internal modules
pub mod coefficients;
pub mod result;
pub mod verifier;
pub mod viz;

#[cfg(feature = "milp")]
pub mod milp;

#[cfg(feature = "lp")]
pub mod lp_solver;

#[cfg(feature = "lp")]
pub mod eg_solver;

#[cfg(feature = "conic")]
pub mod conic_solver;

// === Public API ===

// Result types
pub use result::{
    CombineStats, IterationStats, PipelineResult, PipelineTimings, PriceDiscoveryResult,
    SolverContribution, UcpStats,
};

// Visualization
pub use viz::VizSnapshot;

// Result verification (for ZK proof integration)
pub use verifier::{verify, verify_strict, VerificationResult, Verifier, Violation, ViolationKind};

#[cfg(feature = "milp")]
pub use milp::{MilpConfig, MilpResult, MilpSolver, MmBudgetMode, SolveStatus};

#[cfg(feature = "lp")]
pub use lp_solver::{LpConfig, LpSolver};

#[cfg(feature = "lp")]
pub use eg_solver::{EgConfig, EgSolver};

#[cfg(feature = "conic")]
pub use conic_solver::{ConicConfig, ConicSolver};

use serde::Serialize;

use matching_engine::{Fill, Order};

/// Result of solving a matching problem.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MatchingResult {
    /// Orders that were filled
    pub fills: Vec<Fill>,
    /// Total welfare achieved (fill-level surplus minus minting cost).
    pub total_welfare: i64,
    /// Cost of share creation (minting) not captured in fill-level welfare.
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
