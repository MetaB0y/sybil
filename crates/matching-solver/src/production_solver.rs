//! Named production retained-cash clearing policy.
//!
//! The concrete composition stays here so production callers depend on a
//! stable intent-level type instead of assembling research building blocks.

use matching_engine::Problem;

use crate::{ExactComponentSolver, PacingBundleSolver, PipelineResult, Solver};

/// Production retained-cash solver selected by the frozen promotion protocol.
///
/// Balanced economically independent components are solved separately; all
/// connected and strongly unbalanced books use the same pacing-bundle solver
/// monolithically. Both routes optimize the same retained-cash objective and
/// cross the same integer landing and verifier boundary.
pub struct ProductionSolver {
    inner: ExactComponentSolver<PacingBundleSolver>,
}

impl ProductionSolver {
    pub fn new() -> Self {
        Self {
            inner: ExactComponentSolver::new(PacingBundleSolver::new()),
        }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        self.inner.solve(problem)
    }
}

impl Default for ProductionSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for ProductionSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        ProductionSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "ProductionRetainedCash"
    }
}

#[cfg(test)]
mod tests {
    use matching_engine::{Problem, simple_no_buy, simple_yes_buy};

    use super::*;

    #[test]
    fn facade_uses_the_promoted_retained_cash_path() {
        let mut problem = Problem::new("production-facade");
        let market = problem.markets.add_binary("market");
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            1_000,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets,
            2,
            market,
            500_000_000,
            1_000,
        ));

        let solver = ProductionSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(solver.name(), "ProductionRetainedCash");
        assert_eq!(result.diagnostics.algorithm, "pacing-bundle");
        assert!(!result.result.fills.is_empty());
    }
}
