//! Matching solver for prediction market FBA (Frequent Batch Auction).
//!
//! Solves the welfare-maximizing order matching problem via convex programs:
//! - **LP** (`lp_solver`): Linear program via HiGHS with MM budget shading
//! - **Production** (`production_solver`): exact-component fully corrective retained cash
//! - **RC-FW** (`retained_cash_solver`): independent certified reference
//! - **Pacing bundle** (`pacing_bundle_solver`): fully corrective retained-cash core
//! - **Exact components** (`exact_components`): exact connectivity-aware router
//! - **Conic** (`conic_solver`): Conic EG via Clarabel
//! - **Direct dual** (`direct_dual_conic_solver`): price-side Clarabel reference
//! - **MILP** (`milp`): Mixed-integer via SCIP (exact with timeout)

// Internal modules
pub mod result;
pub mod solver;
pub mod viz;

#[cfg(feature = "milp")]
pub mod milp;

#[cfg(feature = "retained-cash")]
mod lp_solver;

#[cfg(feature = "retained-cash")]
mod component_assembly;

#[cfg(feature = "conic")]
pub mod conic_solver;

#[cfg(feature = "conic")]
pub mod direct_dual_conic_solver;

#[cfg(feature = "retained-cash")]
mod price_pacing_dual;

#[cfg(feature = "retained-cash")]
pub mod retained_cash_solver;

#[cfg(feature = "retained-cash")]
pub mod pacing_bundle_solver;

#[cfg(feature = "lp")]
pub mod decomposed;

#[cfg(feature = "retained-cash")]
pub mod exact_components;

#[cfg(feature = "retained-cash")]
mod production_solver;

#[cfg(all(test, feature = "retained-cash"))]
pub(crate) mod test_fixtures;

// === Public API ===

// Result types
pub use result::{
    PipelineResult, PipelineTimings, PriceDiscoveryResult, SolverDiagnostics, TerminationStatus,
};

// Solver trait
pub use solver::Solver;

// Visualization
pub use viz::VizSnapshot;

#[cfg(feature = "milp")]
pub use milp::{MilpConfig, MilpResult, MilpSolver, MmBudgetMode, SolveStatus};

#[cfg(feature = "lp")]
pub use lp_solver::{LpConfig, LpSolver};

#[cfg(feature = "retained-cash")]
pub use lp_solver::LinearOracleBackend;

#[cfg(feature = "retained-cash")]
pub use retained_cash_solver::{
    RetainedCashConfig, RetainedCashSolver, retained_cash_objective_for_fills,
    retained_cash_welfare_gap_bound_for_fills, zero_temperature_minting_cost_for_fills,
};

#[cfg(feature = "retained-cash")]
pub use pacing_bundle_solver::{PacingBundleConfig, PacingBundleSolver};

#[cfg(feature = "retained-cash")]
pub use production_solver::ProductionSolver;

#[cfg(feature = "conic")]
pub use conic_solver::{ConicConfig, ConicSolver, ObjectiveMode};

#[cfg(feature = "conic")]
pub use direct_dual_conic_solver::{DirectDualConicConfig, DirectDualConicSolver};

#[cfg(feature = "lp")]
pub use decomposed::DecomposedSolver;

#[cfg(feature = "retained-cash")]
pub use exact_components::{ExactComponentSolver, ExactComponentStats, exact_component_stats};

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use matching_engine::{Fill, Order, net_welfare};

/// Result of solving a matching problem.
#[derive(Clone, Debug, Default)]
pub struct MatchingResult {
    /// Orders that were filled
    pub fills: Vec<Fill>,
    /// Gross order-value objective before protocol minting cost.
    pub gross_welfare: i64,
    /// Signed complete-set cost: positive for minting, negative for burning.
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
            gross_welfare: 0,
            minting_cost: 0,
            orders_filled: 0,
            orders_unfilled_liquidity: 0,
            total_quantity_filled: 0,
        }
    }

    /// Total welfare under the protocol convention: gross order value net of
    /// the signed settlement-derived mint/burn cost.
    pub fn total_welfare(&self) -> i64 {
        net_welfare(self.gross_welfare, self.minting_cost)
    }

    pub fn add_fill(&mut self, fill: Fill, order: &Order) {
        self.gross_welfare += order.gross_welfare_contribution(fill.fill_qty);
        self.total_quantity_filled += fill.fill_qty.0;
        if fill.fill_qty.0 > 0 {
            self.orders_filled += 1;
        }
        self.fills.push(fill);
    }
}

impl Serialize for MatchingResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("MatchingResult", 7)?;
        state.serialize_field("fills", &self.fills)?;
        state.serialize_field("gross_welfare", &self.gross_welfare)?;
        state.serialize_field("minting_cost", &self.minting_cost)?;
        state.serialize_field("total_welfare", &self.total_welfare())?;
        state.serialize_field("orders_filled", &self.orders_filled)?;
        state.serialize_field("orders_unfilled_liquidity", &self.orders_unfilled_liquidity)?;
        state.serialize_field("total_quantity_filled", &self.total_quantity_filled)?;
        state.end()
    }
}
