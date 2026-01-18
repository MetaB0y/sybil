//! Solvers for the NP-hard matching problem.
//!
//! This crate provides multiple solver implementations:
//!
//! - [`GreedySolver`]: Fast heuristic that processes orders by welfare potential
//! - [`RandomizedGreedySolver`]: Multiple shuffled greedy runs, returns best
//! - [`MilpSolver`]: Optimal via MILP (requires `milp` feature)
//! - [`CompositeSolver`]: Combines specialized solvers with problem decomposition
//!
//! # Solver Composition
//!
//! The [`composition`] module provides infrastructure for:
//! - Problem analysis and decomposition into clusters
//! - Partial solution merging with conflict resolution
//! - Routing sub-problems to appropriate solvers
//!
//! # Specialized Solvers
//!
//! The [`specialized`] module provides:
//! - [`ArbitrageDetector`]: Finds riskless profit opportunities
//! - [`ConditionalEvaluator`]: Handles price-triggered orders

pub mod composition;
pub mod greedy;
pub mod randomized;
pub mod specialized;

#[cfg(feature = "milp")]
pub mod milp;

#[cfg(feature = "lp-validation")]
pub mod upper_bound;

pub use greedy::GreedySolver;
pub use randomized::RandomizedGreedySolver;

#[cfg(feature = "milp")]
pub use milp::MilpSolver;

// Composition exports
pub use composition::{
    ClusterInfo, CompositeSolver, Decomposer, MarketGraph, PartialSolution, ProblemAnalysis,
    SolutionConfidence, SolutionMerger, SolverBuilder, SubProblem,
};

// Specialized solver exports
pub use specialized::{ArbitrageDetector, ConditionalEvaluator};

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
