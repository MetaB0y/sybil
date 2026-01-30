//! Core traits for the solver experimentation platform.
//!
//! This module defines the trait hierarchy that enables:
//! - Swappable solver components
//! - Easy experimentation with different combinations
//! - Clean separation between price discovery, allocation, and partial solving
//!
//! # Trait Hierarchy
//!
//! ```text
//! PriceDiscoverer ─── discovers clearing prices for markets
//! OrderAllocator ──── allocates budget-constrained orders given prices
//! PartialSolver ───── produces partial solutions for MWIS combination
//! LpContributor ───── (future) contributes constraints to a unified LP
//! ```

use std::collections::HashMap;

use serde::Serialize;

use matching_engine::{Fill, MarketId, MmConstraint, Nanos, Order, Problem, Qty};

use crate::combiner::SolutionConfidence;
use crate::local_solver::MarketSolution;

// ============================================================================
// Price Discovery
// ============================================================================

/// Result of price discovery across markets.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PriceDiscoveryResult {
    /// Clearing prices per outcome for each market.
    /// Maps MarketId -> Vec<Nanos> where index is outcome.
    pub prices: HashMap<MarketId, Vec<Nanos>>,

    /// Per-market solutions with fills computed at clearing prices.
    pub market_solutions: HashMap<MarketId, MarketSolution>,

    /// Total welfare from all market solutions.
    pub total_welfare: i64,

    /// Total number of fills across all markets.
    pub total_fills: usize,
}

impl PriceDiscoveryResult {
    /// Create an empty price discovery result.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add a market solution to the result.
    pub fn add_market_solution(&mut self, solution: MarketSolution) {
        self.total_welfare += solution.welfare;
        self.total_fills += solution.fills.len();
        self.prices
            .insert(solution.market_id, solution.prices.clone());
        self.market_solutions.insert(solution.market_id, solution);
    }

    /// Get all fills from all market solutions.
    pub fn all_fills(&self) -> Vec<Fill> {
        self.market_solutions
            .values()
            .flat_map(|s| s.fills.iter().cloned())
            .collect()
    }
}

/// Discovers clearing prices for markets.
///
/// A `PriceDiscoverer` analyzes a problem and determines equilibrium prices
/// for each market. This is the first phase in a typical solve pipeline.
///
/// # Implementors
///
/// - `LocalSolver`: Per-market clearing with price normalization
/// - Future: Global LP-based price discovery
pub trait PriceDiscoverer: Send + Sync {
    /// Discover clearing prices for all markets in the problem.
    fn discover_prices(&self, problem: &Problem) -> PriceDiscoveryResult;

    /// Name of this price discoverer.
    fn name(&self) -> &str;
}

// ============================================================================
// Order Allocation
// ============================================================================

/// Result of order allocation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AllocationResult {
    /// Order IDs that should be activated (filled).
    pub activated_orders: Vec<u64>,

    /// Total welfare from activated orders.
    pub total_welfare: i64,

    /// Number of iterations used (for fixed-point algorithms).
    pub iterations: usize,

    /// Per-MM allocation details (if applicable).
    pub mm_allocations: Vec<crate::mm_allocator::MmAllocation>,
}

/// Allocates budget-constrained orders given prices.
///
/// An `OrderAllocator` takes clearing prices and determines which orders
/// should be filled, respecting budget constraints (e.g., MM budgets).
///
/// # Implementors
///
/// - `MmAllocator`: Lagrangian relaxation for MM budget allocation
pub trait OrderAllocator: Send + Sync {
    /// Allocate orders given constraints, prices, and actual fills.
    ///
    /// # Arguments
    /// * `constraints` - MM constraints with budget limits
    /// * `prices` - Clearing prices per outcome per market
    /// * `orders` - All orders in the problem
    /// * `fills` - Actual fills from price discovery (order_id -> (price, qty))
    fn allocate(
        &self,
        constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        orders: &[Order],
        fills: &HashMap<u64, (Nanos, Qty)>,
    ) -> AllocationResult;

    /// Name of this allocator.
    fn name(&self) -> &str;
}

// ============================================================================
// Partial Solving
// ============================================================================

/// A partial solution that can be combined with others via MWIS.
#[derive(Clone, Debug, Serialize)]
pub struct PartialSolution {
    /// Name of the solver that produced this.
    pub solver_name: String,

    /// Fills proposed by this solver.
    pub fills: Vec<Fill>,

    /// Total welfare achieved.
    pub welfare: i64,

    /// Confidence level of this solution.
    pub confidence: SolutionConfidence,
}

impl PartialSolution {
    /// Create a new empty partial solution.
    pub fn new(solver_name: impl Into<String>) -> Self {
        Self {
            solver_name: solver_name.into(),
            fills: Vec::new(),
            welfare: 0,
            confidence: SolutionConfidence::Heuristic,
        }
    }

    /// Create a partial solution with fills.
    pub fn with_fills(
        solver_name: impl Into<String>,
        fills: Vec<Fill>,
        welfare: i64,
        confidence: SolutionConfidence,
    ) -> Self {
        Self {
            solver_name: solver_name.into(),
            fills,
            welfare,
            confidence,
        }
    }

    /// Check if this solution is empty.
    pub fn is_empty(&self) -> bool {
        self.fills.is_empty()
    }
}

/// Produces partial solutions for MWIS combination.
///
/// A `PartialSolver` runs a solving strategy and produces a set of fills
/// that can be combined with other partial solutions via MWIS.
///
/// # Implementors
///
/// - `ArbitrageDetector`: Bundle/spread matching
/// - `MilpSolver`: Optimal (time-limited) MILP
/// - Specialized solvers: Arbitrage, Bundle decomposition, etc.
pub trait PartialSolver: Send + Sync {
    /// Solve the problem and return a partial solution.
    fn solve_partial(&self, problem: &Problem) -> PartialSolution;

    /// Name of this solver.
    fn name(&self) -> &str;

    /// Confidence level this solver typically produces.
    fn confidence(&self) -> SolutionConfidence {
        SolutionConfidence::Heuristic
    }
}

// ============================================================================
// Adapter Utilities
// ============================================================================

/// Convert a MatchingResult to a PartialSolution.
pub fn matching_result_to_partial(
    result: &crate::MatchingResult,
    solver_name: &str,
    confidence: SolutionConfidence,
) -> PartialSolution {
    PartialSolution::with_fills(
        solver_name,
        result.fills.clone(),
        result.total_welfare,
        confidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_discovery_result() {
        let mut result = PriceDiscoveryResult::empty();
        assert!(result.prices.is_empty());
        assert_eq!(result.total_welfare, 0);

        let solution = MarketSolution::empty(MarketId::new(1), 2);
        result.add_market_solution(solution);

        assert!(result.prices.contains_key(&MarketId::new(1)));
    }

    #[test]
    fn test_partial_solution() {
        let solution = PartialSolution::new("test");
        assert!(solution.is_empty());
        assert_eq!(solution.solver_name, "test");
        assert_eq!(solution.confidence, SolutionConfidence::Heuristic);
    }

    #[test]
    fn test_allocation_result() {
        let result = AllocationResult::default();
        assert!(result.activated_orders.is_empty());
        assert_eq!(result.total_welfare, 0);
    }
}
