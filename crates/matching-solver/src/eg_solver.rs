//! Legacy `EgSolver` compatibility wrapper.
//!
//! The former implementation optimized the no-cash `B ln U` ablation, used
//! an infeasible fill-seeding step, omitted minting cost from line search, and
//! forced a step when the directional derivative was non-positive. The
//! retained-cash program is the supported paper objective and now owns the
//! Frank--Wolfe implementation. A no-cash comparison remains available through
//! [`crate::ConicSolver`] in `Fisher` mode.

use matching_engine::Problem;

use crate::{PipelineResult, RetainedCashConfig, RetainedCashSolver, Solver};

#[derive(Clone, Debug)]
pub struct EgConfig {
    pub max_fw_iterations: usize,
    pub convergence_tol: f64,
    /// Retained for old configuration files; certified-gap convergence does
    /// not use allocation-stability stopping.
    pub q_stability_tol: f64,
    pub line_search_steps: usize,
    /// Retained for old configuration files; integer landing performs the
    /// protocol budget check and does not run a hidden SLP fallback.
    pub max_mm_slp_iterations: usize,
}

impl Default for EgConfig {
    fn default() -> Self {
        Self {
            max_fw_iterations: 100,
            convergence_tol: 1e-7,
            q_stability_tol: 0.0,
            line_search_steps: 48,
            max_mm_slp_iterations: 0,
        }
    }
}

pub struct EgSolver {
    config: EgConfig,
}

impl EgSolver {
    pub fn new() -> Self {
        Self {
            config: EgConfig::default(),
        }
    }

    pub fn with_config(config: EgConfig) -> Self {
        Self { config }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let mut result = RetainedCashSolver::with_config(RetainedCashConfig {
            max_iterations: self.config.max_fw_iterations,
            gap_rel: self.config.convergence_tol,
            line_search_steps: self.config.line_search_steps,
            ..Default::default()
        })
        .solve(problem);
        let compatibility_note = format!(
            "legacy EG name executes retained-cash-fw; q_stability_tol={} and max_mm_slp_iterations={} ignored",
            self.config.q_stability_tol, self.config.max_mm_slp_iterations
        );
        result.diagnostics.message = Some(match result.diagnostics.message.take() {
            Some(message) => format!("{compatibility_note}; {message}"),
            None => compatibility_note,
        });
        result
    }
}

impl Default for EgSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for EgSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        EgSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "RetainedCashFW (legacy EG alias)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{mm_budget_problem, single_market_problem};

    #[test]
    fn compatibility_wrapper_is_explicit() {
        let result = EgSolver::new().solve(&single_market_problem());
        assert_eq!(result.diagnostics.algorithm, "retained-cash-fw");
        assert!(
            result
                .diagnostics
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("legacy EG")
        );
    }

    #[test]
    fn compatibility_wrapper_returns_nonnegative_welfare() {
        let result = EgSolver::new().solve(&mm_budget_problem());
        assert!(result.result.total_welfare() >= 0);
    }
}
