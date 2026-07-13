//! Legacy `IterLpSolver` compatibility wrapper.
//!
//! The former damped multiplier fixed point had no convergence proof and used
//! a multiplier/objective inconsistent with the retained-cash KKT system. New
//! code should use [`crate::RetainedCashSolver`] directly. Keeping this thin
//! wrapper avoids silently changing old CLI/configuration inputs into the
//! production LP while ensuring they execute the paper-aligned algorithm.

use matching_engine::Problem;

use crate::{PipelineResult, RetainedCashConfig, RetainedCashSolver, Solver};

#[derive(Clone, Debug)]
pub struct IterLpConfig {
    pub max_iterations: usize,
    pub mu_tol: f64,
    /// Retained only for configuration compatibility; generalized
    /// Frank--Wolfe uses a certified line search and does not use damping.
    pub damping: f64,
}

impl Default for IterLpConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            mu_tol: 1e-7,
            damping: 0.0,
        }
    }
}

pub struct IterLpSolver {
    config: IterLpConfig,
}

impl IterLpSolver {
    pub fn new() -> Self {
        Self {
            config: IterLpConfig::default(),
        }
    }

    pub fn with_config(config: IterLpConfig) -> Self {
        Self { config }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let mut result = RetainedCashSolver::with_config(RetainedCashConfig {
            max_iterations: self.config.max_iterations,
            gap_rel: self.config.mu_tol,
            ..Default::default()
        })
        .solve(problem);
        let compatibility_note = format!(
            "legacy IterLP name executes retained-cash-fw; obsolete damping={} ignored",
            self.config.damping
        );
        result.diagnostics.message = Some(match result.diagnostics.message.take() {
            Some(message) => format!("{compatibility_note}; {message}"),
            None => compatibility_note,
        });
        result
    }
}

impl Default for IterLpSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for IterLpSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        IterLpSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "RetainedCashFW (legacy IterLP alias)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{mm_budget_problem, single_market_problem};

    #[test]
    fn compatibility_wrapper_is_explicit() {
        let result = IterLpSolver::new().solve(&single_market_problem());
        assert_eq!(result.diagnostics.algorithm, "retained-cash-fw");
        assert!(
            result
                .diagnostics
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("legacy IterLP")
        );
    }

    #[test]
    fn compatibility_wrapper_respects_mm_budget() {
        let problem = mm_budget_problem();
        let result = IterLpSolver::new().solve(&problem);
        assert!(result.result.total_welfare() >= 0);
    }
}
