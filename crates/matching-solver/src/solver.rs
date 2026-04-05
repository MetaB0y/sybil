//! Unified solver interface for prediction market matching.

use matching_engine::Problem;

use crate::PipelineResult;

/// Unified solver trait. All solvers (LP, EG, Conic, IterLP, MILP, Decomposed)
/// implement this trait, making them injectable and interchangeable.
///
/// For solvers with richer return types (e.g., `MilpSolver::solve_with_status`),
/// the concrete type provides additional methods beyond this trait.
pub trait Solver: Send + Sync {
    /// Solve a matching problem, returning fills, clearing prices, and timing.
    fn solve(&self, problem: &Problem) -> PipelineResult;

    /// Human-readable solver name for logging and diagnostics.
    fn name(&self) -> &str;
}
