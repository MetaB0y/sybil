//! Named production retained-cash clearing policy.
//!
//! The concrete policy stays here so production callers depend on a stable
//! intent-level type instead of selecting research building blocks.

use matching_engine::Problem;

use crate::{PacingBundleSolver, PipelineResult, Solver};

/// Production retained-cash solver selected by the frozen promotion and
/// adversarial-connectivity protocols.
///
/// The pacing bundle always solves the complete book. Exact component routing
/// remains an explicit opt-in accelerator, but is not part of the security
/// baseline: an admission-sized MM bundle can cheaply connect the whole book.
pub struct ProductionSolver {
    inner: PacingBundleSolver,
}

impl ProductionSolver {
    pub fn new() -> Self {
        Self {
            inner: PacingBundleSolver::new(),
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
    fn facade_uses_the_monolithic_promoted_retained_cash_path() {
        let mut problem = Problem::new("production-facade");
        let markets = [
            problem.markets.add_binary("market-a"),
            problem.markets.add_binary("market-b"),
        ];
        let mut order_id = 1;
        for market in markets {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                order_id,
                market,
                600_000_000,
                1_000,
            ));
            order_id += 1;
            problem.orders.push(simple_no_buy(
                &problem.markets,
                order_id,
                market,
                500_000_000,
                1_000,
            ));
            order_id += 1;
        }

        let solver = ProductionSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(solver.name(), "ProductionRetainedCash");
        // Two balanced independent markets would trigger the exact router.
        assert_eq!(result.diagnostics.algorithm, "pacing-bundle");
        assert!(!result.result.fills.is_empty());
    }
}
