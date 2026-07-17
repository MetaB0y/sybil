//! Certified retained-cash clearing with a fully corrective pacing bundle.
//!
//! The retained-cash utility has the variational representation
//!
//! `psi_B(U) = min_{0 < alpha <= 1} alpha * U - B * ln(alpha)`.
//!
//! Fixing the MM pacing vector `alpha` leaves the ordinary matching LP. Each
//! LP solution is therefore a cutting plane for the convex pacing dual and a
//! feasible atom for the primal. This solver retains those atoms and fully
//! corrects their convex weights with pairwise exact line searches. The
//! recovered mixture is primal-feasible, while one final matching-LP call
//! supplies a global upper bound. Their difference is a genuine retained-cash
//! objective certificate.

use std::time::{Duration, Instant};

use matching_engine::{NANOS_PER_DOLLAR, Problem, SHARE_SCALE};

use crate::lp_solver::{
    LinearOracleBackend, MatchingLpOracle, build_solver_context, project_and_finalize,
    support_and_finalize_target_with_objective,
};
use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};
use crate::retained_cash_solver::{ObjectiveModel, landed_quantities, landing_l1_ratio};

/// Configuration for the experimental fully corrective pacing-bundle solver.
#[derive(Clone, Debug)]
pub struct PacingBundleConfig {
    /// Fixed-pacing matching oracle.
    pub linear_oracle: LinearOracleBackend,
    /// Maximum new matching-LP atoms. One final oracle call still certifies
    /// the current mixture when this cap is reached.
    pub max_iterations: usize,
    /// Maximum cheap pairwise correction steps after adding each atom.
    pub max_master_iterations: usize,
    /// Absolute global primal--dual gap tolerance in objective nanodollars.
    pub gap_abs_nanos: f64,
    /// Relative global gap tolerance against the current primal objective.
    pub gap_rel: f64,
    /// Absolute tolerance for the restricted bundle master.
    pub master_gap_abs_nanos: f64,
    /// Relative tolerance for the restricted bundle master.
    pub master_gap_rel: f64,
    /// Bisection steps for exact one-dimensional concave line searches.
    pub line_search_steps: usize,
}

impl Default for PacingBundleConfig {
    fn default() -> Self {
        Self {
            linear_oracle: LinearOracleBackend::Highs,
            max_iterations: 100,
            max_master_iterations: 1_000,
            gap_abs_nanos: 1_000_000.0,
            // Landing needs a near-stationary target, not merely a small
            // objective error: a loose supporting gap can be close in value
            // while far from the exact optimal face on degenerate books.
            gap_rel: 1e-8,
            master_gap_abs_nanos: 1_000.0,
            master_gap_rel: 1e-9,
            line_search_steps: 48,
        }
    }
}

/// Experimental low-dimensional solver over one pacing factor per MM.
pub struct PacingBundleSolver {
    config: PacingBundleConfig,
}

impl PacingBundleSolver {
    pub fn new() -> Self {
        Self {
            config: PacingBundleConfig::default(),
        }
    }

    pub fn with_config(config: PacingBundleConfig) -> Self {
        Self { config }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "PacingBundle");
        let rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::failure(
                "pacing-bundle",
                TerminationStatus::UnsupportedInput,
                format!("rejected {rejected_orders} unsupported orders"),
                start.elapsed().as_secs_f64(),
            );
        }

        let ctx = build_solver_context(problem);
        let model = ObjectiveModel::new(problem, &ctx);
        if !model.has_reduced_form_orders() {
            let mut result = crate::LpSolver::new().solve(problem);
            result.diagnostics.algorithm = "pacing-bundle".to_string();
            result.diagnostics.status = TerminationStatus::Delegated;
            result.diagnostics.message =
                Some("no MM retained-cash terms; objective reduces to LP".into());
            return result;
        }

        let mut oracle_orders = problem.orders.clone();
        model.disable_zero_budget_orders(&mut oracle_orders);
        let Some(mut oracle) = MatchingLpOracle::new(
            self.config.linear_oracle,
            &oracle_orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
        ) else {
            return PipelineResult::failure(
                "pacing-bundle",
                TerminationStatus::NumericalFailure,
                format!(
                    "failed to construct the {:?} oracle",
                    self.config.linear_oracle
                ),
                start.elapsed().as_secs_f64(),
            );
        };

        let zero = vec![0.0; problem.orders.len()];
        let mut master = BundleMaster::new(BundleAtom::from_allocation(&model, zero));
        let mut best_upper_bound = f64::INFINITY;
        let mut oracle_calls = 0usize;
        let mut atoms_added = 0usize;
        let mut master_iterations = 0usize;
        let mut oracle_time = Duration::ZERO;
        let mut master_time = Duration::ZERO;
        let mut converged = false;
        let mut numerical_failure = None;

        for iteration in 0..=self.config.max_iterations {
            let alpha = model.pacing_factors(&master.utilities);
            let coefficients = model.oracle_coefficients_from_alpha(&alpha);
            let oracle_started = Instant::now();
            let oracle_solution = oracle.solve(&coefficients);
            oracle_time += oracle_started.elapsed();
            let Some(oracle_solution) = oracle_solution else {
                numerical_failure = Some("HiGHS oracle failed".to_string());
                break;
            };
            oracle_calls += 1;

            let atom = BundleAtom::from_allocation(&model, oracle_solution.q_values);
            let current_score =
                model.affine_score(&master.utilities, master.linear_component, &alpha);
            let Some(oracle_upper_bound) = oracle_solution.objective_upper_bound_dollars else {
                numerical_failure = Some("HiGHS oracle did not supply a dual upper bound".into());
                break;
            };
            let oracle_upper_score =
                oracle_upper_bound * NANOS_PER_DOLLAR as f64 / SHARE_SCALE as f64;
            let objective = master.objective(&model);
            let oracle_gap = (oracle_upper_score - current_score).max(0.0);
            best_upper_bound = best_upper_bound.min(objective + oracle_gap);
            let certified_gap = (best_upper_bound - objective).max(0.0);
            let tolerance = self
                .config
                .gap_abs_nanos
                .max(self.config.gap_rel * objective.abs().max(1.0));
            if certified_gap <= tolerance {
                converged = true;
                break;
            }
            if iteration == self.config.max_iterations {
                break;
            }

            if master.add_atom(atom) {
                atoms_added += 1;
            }
            let master_started = Instant::now();
            let correction = master.correct(&model, &self.config);
            master_time += master_started.elapsed();
            master_iterations += correction.iterations;
            if !correction.progressed && correction.gap > correction.tolerance {
                numerical_failure = Some(format!(
                    "bundle master stalled with restricted gap {} above tolerance {}",
                    correction.gap, correction.tolerance
                ));
                break;
            }
        }

        if oracle_calls == 0 {
            return PipelineResult::failure(
                "pacing-bundle",
                TerminationStatus::NumericalFailure,
                numerical_failure.unwrap_or_else(|| "oracle produced no iterate".to_string()),
                start.elapsed().as_secs_f64(),
            );
        }

        let q = master.recover_allocation();
        let objective = model.objective_from_components(&master.utilities, master.linear_component);
        let last_gap = (best_upper_bound - objective).max(0.0);
        let mut result = if converged && numerical_failure.is_none() {
            let final_alpha = model.pacing_factors(&master.utilities);
            let projection_objective = model.oracle_coefficients_from_alpha(&final_alpha);
            support_and_finalize_target_with_objective(
                &q,
                problem,
                &ctx,
                &projection_objective,
                start,
            )
        } else {
            project_and_finalize(&q, problem, &ctx, start)
        };
        if result.diagnostics.status == TerminationStatus::PostProcessingFailure {
            let previous = result.diagnostics.message.take().unwrap_or_default();
            result.diagnostics.message = Some(format!(
                "{previous}; bundle objective={objective}, gap={last_gap}, atoms={}, finite_q={}",
                master.active_atoms(),
                q.iter().all(|value| value.is_finite()),
            ));
        } else {
            let integer_landing_budget_trimmed = result.diagnostics.integer_landing_budget_trimmed;
            let landed_q = landed_quantities(problem, &result);
            let landed_objective =
                model.objective_for_landed_fills(&landed_q, &result.result.fills);
            let status = if numerical_failure.is_some() {
                TerminationStatus::NumericalFailure
            } else if converged {
                TerminationStatus::Converged
            } else {
                TerminationStatus::IterationLimit
            };
            let message = numerical_failure.unwrap_or_else(|| {
                format!(
                    "{} active atoms; {master_iterations} pairwise master steps; {:?} oracle {:.6}s; master {:.6}s",
                    master.active_atoms(),
                    self.config.linear_oracle,
                    oracle_time.as_secs_f64(),
                    master_time.as_secs_f64(),
                )
            });
            result.diagnostics = SolverDiagnostics {
                algorithm: "pacing-bundle".to_string(),
                status,
                iterations: Some(atoms_added),
                convergence_metric: last_gap.is_finite().then_some(last_gap),
                objective_value: Some(objective),
                optimality_gap: last_gap.is_finite().then_some(last_gap),
                oracle_calls: Some(oracle_calls),
                integer_landing_loss: Some((objective - landed_objective).max(0.0)),
                integer_landing_l1_ratio: landing_l1_ratio(&q, &landed_q),
                integer_landing_budget_trimmed,
                master_iterations: Some(master_iterations),
                active_atoms: Some(master.active_atoms()),
                oracle_time_secs: Some(oracle_time.as_secs_f64()),
                master_time_secs: Some(master_time.as_secs_f64()),
                message: Some(message),
                ..Default::default()
            };
        }
        result
    }
}

impl Default for PacingBundleSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for PacingBundleSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        PacingBundleSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "PacingBundle"
    }
}

#[derive(Clone)]
struct BundleAtom {
    allocation: Vec<f64>,
    utilities: Vec<f64>,
    linear_component: f64,
}

impl BundleAtom {
    fn from_allocation(model: &ObjectiveModel<'_>, allocation: Vec<f64>) -> Self {
        let (utilities, linear_component) = model.allocation_components(&allocation);
        Self {
            allocation,
            utilities,
            linear_component,
        }
    }

    fn same_summary(&self, other: &Self) -> bool {
        let scale = self
            .linear_component
            .abs()
            .max(other.linear_component.abs())
            .max(1.0);
        (self.linear_component - other.linear_component).abs() <= 1e-11 * scale
            && self.utilities.iter().zip(&other.utilities).all(|(a, b)| {
                let scale = a.abs().max(b.abs()).max(1.0);
                (a - b).abs() <= 1e-11 * scale
            })
    }
}

struct BundleMaster {
    atoms: Vec<BundleAtom>,
    weights: Vec<f64>,
    utilities: Vec<f64>,
    linear_component: f64,
}

struct CorrectionResult {
    iterations: usize,
    gap: f64,
    tolerance: f64,
    progressed: bool,
}

impl BundleMaster {
    fn new(initial: BundleAtom) -> Self {
        Self {
            utilities: initial.utilities.clone(),
            linear_component: initial.linear_component,
            atoms: vec![initial],
            weights: vec![1.0],
        }
    }

    fn add_atom(&mut self, atom: BundleAtom) -> bool {
        if self.atoms.iter().any(|known| known.same_summary(&atom)) {
            return false;
        }
        self.atoms.push(atom);
        self.weights.push(0.0);
        true
    }

    fn objective(&self, model: &ObjectiveModel<'_>) -> f64 {
        model.objective_from_components(&self.utilities, self.linear_component)
    }

    fn active_atoms(&self) -> usize {
        self.weights
            .iter()
            .filter(|&&weight| weight > 1e-12)
            .count()
    }

    fn correct(
        &mut self,
        model: &ObjectiveModel<'_>,
        config: &PacingBundleConfig,
    ) -> CorrectionResult {
        let mut iterations = 0usize;
        let mut progressed = false;
        let mut gap = f64::INFINITY;
        let mut tolerance = f64::INFINITY;

        for _ in 0..config.max_master_iterations {
            let alpha = model.pacing_factors(&self.utilities);
            let current_score = model.affine_score(&self.utilities, self.linear_component, &alpha);
            let scores: Vec<_> = self
                .atoms
                .iter()
                .map(|atom| model.affine_score(&atom.utilities, atom.linear_component, &alpha))
                .collect();
            let toward = scores
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .expect("bundle always has an atom");
            gap = (scores[toward] - current_score).max(0.0);
            tolerance = config
                .master_gap_abs_nanos
                .max(config.master_gap_rel * self.objective(model).abs().max(1.0));
            if gap <= tolerance {
                break;
            }

            let away = self
                .weights
                .iter()
                .enumerate()
                .filter(|(_, weight)| **weight > 1e-14)
                .min_by(|left, right| scores[left.0].total_cmp(&scores[right.0]))
                .map(|(index, _)| index)
                .expect("bundle weights sum to one");
            if toward == away {
                break;
            }

            let delta_utilities: Vec<_> = self.atoms[toward]
                .utilities
                .iter()
                .zip(&self.atoms[away].utilities)
                .map(|(&toward, &away)| toward - away)
                .collect();
            let delta_linear =
                self.atoms[toward].linear_component - self.atoms[away].linear_component;
            let gamma = model.segment_argmax(
                &self.utilities,
                &delta_utilities,
                delta_linear,
                self.weights[away],
                config.line_search_steps,
            );
            if gamma <= 1e-15 {
                break;
            }

            self.weights[toward] += gamma;
            self.weights[away] -= gamma;
            if self.weights[away] < 1e-14 {
                self.weights[away] = 0.0;
            }
            self.recompute_summary();
            iterations += 1;
            progressed = true;
        }

        CorrectionResult {
            iterations,
            gap,
            tolerance,
            progressed,
        }
    }

    fn recompute_summary(&mut self) {
        self.utilities.fill(0.0);
        self.linear_component = 0.0;
        let mut weight_sum = 0.0;
        for (atom, &weight) in self.atoms.iter().zip(&self.weights) {
            if weight == 0.0 {
                continue;
            }
            weight_sum += weight;
            self.linear_component += weight * atom.linear_component;
            for (total, value) in self.utilities.iter_mut().zip(&atom.utilities) {
                *total += weight * value;
            }
        }
        if (weight_sum - 1.0).abs() > 1e-12 && weight_sum > 0.0 {
            for weight in &mut self.weights {
                *weight /= weight_sum;
            }
            self.recompute_summary();
        }
    }

    fn recover_allocation(&self) -> Vec<f64> {
        let mut allocation = vec![0.0; self.atoms[0].allocation.len()];
        for (atom, &weight) in self.atoms.iter().zip(&self.weights) {
            if weight == 0.0 {
                continue;
            }
            for (total, value) in allocation.iter_mut().zip(&atom.allocation) {
                *total += weight * value;
            }
        }
        allocation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, Nanos, shares_to_qty, simple_no_buy,
        simple_yes_buy,
    };

    fn tight_budget_problem() -> Problem {
        let mut problem = Problem::new("pacing_bundle_tight");
        let market = problem.markets.add_binary("market");
        problem.orders.push(simple_no_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            shares_to_qty(100).0,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            200,
            market,
            500_000_000,
            shares_to_qty(100).0,
        ));
        let mut mm = MmConstraint::new(MmId::new(1), Nanos(10 * NANOS_PER_DOLLAR));
        mm.add_order(200, MmSide::BuyYes);
        problem.mm_constraints.push(mm);
        problem
    }

    #[test]
    fn pacing_variational_identity_matches_reduced_utility() {
        for (budget, utility) in [(10.0, 4.0), (10.0, 10.0), (10.0, 25.0)] {
            let alpha: f64 = if utility <= budget {
                1.0
            } else {
                budget / utility
            };
            let dual = alpha * utility - budget * alpha.ln();
            assert!(
                (dual - crate::retained_cash_solver::retained_cash_utility(budget, utility)).abs()
                    <= 1e-12
            );
        }
    }

    #[test]
    fn tight_budget_converges_with_a_global_certificate() {
        let problem = tight_budget_problem();
        let result = PacingBundleSolver::new().solve(&problem);

        assert_eq!(
            result.diagnostics.status,
            TerminationStatus::Converged,
            "{:?}",
            result.diagnostics
        );
        assert!(result.diagnostics.optimality_gap.unwrap() <= 1_000_000.0);
        let landed = crate::retained_cash_objective_for_fills(&problem, &result.result.fills);
        let upper = result.diagnostics.objective_value.unwrap()
            + result.diagnostics.optimality_gap.unwrap();
        assert!(landed <= upper + 1.0, "landed={landed}, upper={upper}");
        assert!(result.diagnostics.active_atoms.unwrap() >= 1);
        let fill = result
            .result
            .fills
            .iter()
            .find(|fill| fill.order_id == 200)
            .expect("MM receives a budget-limited fill");
        assert!(
            MmSide::BuyYes
                .capital_needed(fill.fill_price, fill.fill_qty)
                .0
                <= 10 * NANOS_PER_DOLLAR
        );
    }

    #[test]
    fn bundle_matches_frank_wolfe_objective_on_tight_book() {
        let problem = tight_budget_problem();
        let bundle = PacingBundleSolver::new().solve(&problem);
        let frank_wolfe = crate::RetainedCashSolver::new().solve(&problem);
        let bundle_objective =
            crate::retained_cash_objective_for_fills(&problem, &bundle.result.fills);
        let fw_objective =
            crate::retained_cash_objective_for_fills(&problem, &frank_wolfe.result.fills);
        assert!((bundle_objective - fw_objective).abs() <= NANOS_PER_DOLLAR as f64);
    }
}
