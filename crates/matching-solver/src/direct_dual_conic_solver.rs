//! Exact exponential-cone solve of the joint price–pacing dual.
//!
//! Unlike [`crate::ConicSolver`], which optimizes fill quantities directly,
//! this formulation makes market prices and MM pacing factors primal
//! variables. Per-order hinge epigraphs encode demand. Their dual multipliers
//! provide a feasible continuous fill target, which still passes through the
//! shared supporting-price and integer-landing boundary.

use std::collections::HashMap;
use std::time::Instant;

use clarabel::algebra::*;
use clarabel::solver::*;

use matching_engine::{NANOS_PER_DOLLAR, Problem, SHARE_SCALE};

use crate::lp_solver::{build_solver_context, support_and_finalize_target_with_objective};
use crate::price_pacing_dual::PriceDualOracle;
use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};
use crate::retained_cash_solver::ObjectiveModel;

#[derive(Clone, Debug)]
pub struct DirectDualConicConfig {
    pub max_iter: u32,
    pub tol: f64,
    pub verbose: bool,
    pub time_limit: f64,
}

impl Default for DirectDualConicConfig {
    fn default() -> Self {
        Self {
            max_iter: 200,
            tol: 1e-8,
            verbose: false,
            time_limit: 30.0,
        }
    }
}

pub struct DirectDualConicSolver {
    config: DirectDualConicConfig,
}

impl DirectDualConicSolver {
    pub fn new() -> Self {
        Self {
            config: DirectDualConicConfig::default(),
        }
    }

    pub fn with_config(config: DirectDualConicConfig) -> Self {
        Self { config }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "direct-dual-conic");
        let rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::failure(
                "direct-dual-conic",
                TerminationStatus::UnsupportedInput,
                format!("rejected {rejected_orders} unsupported orders"),
                start.elapsed().as_secs_f64(),
            );
        }

        let orders = &problem.orders;
        let n = orders.len();
        let ctx = build_solver_context(problem);
        let m = ctx.markets.len();
        let model = ObjectiveModel::new(problem, &ctx);

        let active_mms: Vec<_> = model
            .budgets()
            .iter()
            .enumerate()
            .filter_map(|(mm_index, &budget)| {
                (budget > 0.0 && !model.mm_orders(mm_index).is_empty()).then_some(mm_index)
            })
            .collect();
        if active_mms.is_empty() {
            let mut result = crate::LpSolver::new().solve(problem);
            result.diagnostics.algorithm = "direct-dual-conic".into();
            result.diagnostics.status = TerminationStatus::Delegated;
            result.diagnostics.message =
                Some("no active log-utility MMs; objective reduces to LP".into());
            return result;
        }
        let active_to_local: HashMap<_, _> = active_mms
            .iter()
            .enumerate()
            .map(|(local, &global)| (global, local))
            .collect();
        let k = active_mms.len();

        // x = [YES prices, pacing alpha, log(alpha), hinge epigraphs]
        let price_offset = 0;
        let alpha_offset = m;
        let log_offset = m + k;
        let hinge_offset = m + 2 * k;
        let dimension = m + 2 * k + n;

        let nanos = NANOS_PER_DOLLAR as f64;
        let share_scale = SHARE_SCALE as f64;
        let mut objective = vec![0.0; dimension];
        for (index, order) in orders.iter().enumerate() {
            objective[hinge_offset + index] = order.max_fill.0 as f64 / share_scale;
        }
        for (local, &global) in active_mms.iter().enumerate() {
            objective[log_offset + local] = -model.budgets()[global] / nanos;
        }

        let num_exp_rows = 3 * k;
        let num_hinge_rows = n;
        let num_hinge_nonnegative_rows = n;
        let num_alpha_bound_rows = 2 * k;
        let num_price_bound_rows = 2 * m;
        let num_group_rows = ctx.num_groups;
        let num_nonnegative_rows = num_hinge_rows
            + num_hinge_nonnegative_rows
            + num_alpha_bound_rows
            + num_price_bound_rows
            + num_group_rows;
        let total_rows = num_exp_rows + num_nonnegative_rows;

        let mut rows = Vec::new();
        let mut columns = Vec::new();
        let mut values = Vec::new();
        let mut right_hand_side = vec![0.0; total_rows];

        // (log(alpha), 1, alpha) in the exponential cone.
        for local in 0..k {
            let base = 3 * local;
            rows.push(base);
            columns.push(log_offset + local);
            values.push(-1.0);
            right_hand_side[base + 1] = 1.0;
            rows.push(base + 2);
            columns.push(alpha_offset + local);
            values.push(-1.0);
        }

        let zero_alpha = vec![0.0; problem.mm_constraints.len()];
        let base_coefficients = model.oracle_coefficients_from_alpha(&zero_alpha);
        let market_index: HashMap<_, _> = ctx
            .markets
            .iter()
            .enumerate()
            .map(|(index, &market)| (market, index))
            .collect();

        let hinge_base = num_exp_rows;
        for (index, order) in orders.iter().enumerate() {
            let row = hinge_base + index;
            let market = market_index[&order.markets[0]];
            let price_coefficient = (order.payoffs[1] - order.payoffs[0]) as f64;
            rows.push(row);
            columns.push(price_offset + market);
            values.push(price_coefficient);
            rows.push(row);
            columns.push(hinge_offset + index);
            values.push(-1.0);
            if let Some(global_mm) = model.mm_index(index)
                && let Some(&local_mm) = active_to_local.get(&global_mm)
            {
                rows.push(row);
                columns.push(alpha_offset + local_mm);
                values.push(model.mm_value(index) / nanos);
            }
            let base = base_coefficients[index] / nanos - order.payoffs[1] as f64;
            right_hand_side[row] = -base;
        }

        let mut row = hinge_base + num_hinge_rows;
        // t_i >= 0.
        for index in 0..n {
            rows.push(row);
            columns.push(hinge_offset + index);
            values.push(-1.0);
            row += 1;
        }
        // 0 <= alpha <= 1.
        for local in 0..k {
            rows.push(row);
            columns.push(alpha_offset + local);
            values.push(1.0);
            right_hand_side[row] = 1.0;
            row += 1;
            rows.push(row);
            columns.push(alpha_offset + local);
            values.push(-1.0);
            row += 1;
        }
        // 0 <= p_yes <= 1.
        for market in 0..m {
            rows.push(row);
            columns.push(price_offset + market);
            values.push(1.0);
            right_hand_side[row] = 1.0;
            row += 1;
            rows.push(row);
            columns.push(price_offset + market);
            values.push(-1.0);
            row += 1;
        }
        // Categorical no-arbitrage: sum p_yes <= 1.
        let mut grouped_markets = vec![Vec::new(); ctx.num_groups];
        for (market, &group) in &ctx.market_to_group {
            if let Some(&index) = market_index.get(market) {
                grouped_markets[group].push(index);
            }
        }
        for group in grouped_markets {
            for market in group {
                rows.push(row);
                columns.push(price_offset + market);
                values.push(1.0);
            }
            right_hand_side[row] = 1.0;
            row += 1;
        }
        debug_assert_eq!(row, total_rows);

        let p_matrix = CscMatrix::zeros((dimension, dimension));
        let a_matrix = CscMatrix::new_from_triplets(total_rows, dimension, rows, columns, values);
        let mut cones = Vec::with_capacity(k + 1);
        for _ in 0..k {
            cones.push(ExponentialConeT());
        }
        cones.push(NonnegativeConeT(num_nonnegative_rows));
        let settings = DefaultSettings {
            verbose: self.config.verbose,
            max_iter: self.config.max_iter,
            time_limit: self.config.time_limit,
            tol_gap_abs: self.config.tol,
            tol_gap_rel: self.config.tol,
            tol_feas: self.config.tol,
            max_step_fraction: 0.8,
            ..DefaultSettings::default()
        };
        let mut solver = match DefaultSolver::new(
            &p_matrix,
            &objective,
            &a_matrix,
            &right_hand_side,
            &cones,
            settings,
        ) {
            Ok(solver) => solver,
            Err(error) => {
                return PipelineResult::failure(
                    "direct-dual-conic",
                    TerminationStatus::NumericalFailure,
                    format!("Clarabel setup failed: {error:?}"),
                    start.elapsed().as_secs_f64(),
                );
            }
        };
        solver.solve();

        let status = solver.solution.status;
        let iterations = solver.solution.iterations as usize;
        let primal_residual = solver.solution.r_prim;
        let dual_residual = solver.solution.r_dual;
        if !matches!(status, SolverStatus::Solved | SolverStatus::AlmostSolved) {
            let mut failure = PipelineResult::failure(
                "direct-dual-conic",
                TerminationStatus::NumericalFailure,
                format!("Clarabel terminated with {status:?}"),
                start.elapsed().as_secs_f64(),
            );
            failure.diagnostics.iterations = Some(iterations);
            failure.diagnostics.primal_residual =
                primal_residual.is_finite().then_some(primal_residual);
            failure.diagnostics.dual_residual = dual_residual.is_finite().then_some(dual_residual);
            return failure;
        }

        let solution = &solver.solution;
        let mut alpha = vec![0.0; problem.mm_constraints.len()];
        for (local, &global) in active_mms.iter().enumerate() {
            alpha[global] = solution.x[alpha_offset + local].clamp(f64::MIN_POSITIVE, 1.0);
        }
        let mut yes_prices = solution.x[price_offset..price_offset + m].to_vec();
        let price_oracle =
            PriceDualOracle::new(orders, &ctx.markets, &ctx.market_to_group, ctx.num_groups)
                .expect("supported problem has a price dual");
        price_oracle
            .project_prices(&mut yes_prices)
            .expect("matching price dimensions");
        let objective_upper = price_oracle
            .joint_objective_at(&model, &alpha, &yes_prices)
            .expect("projected direct dual point is feasible");

        let mut q_values = Vec::with_capacity(n);
        for (index, order) in orders.iter().enumerate() {
            let zero_budget_mm = model
                .mm_index(index)
                .is_some_and(|mm_index| model.budgets()[mm_index] <= 0.0);
            let shares = if zero_budget_mm {
                0.0
            } else {
                solution.z[hinge_base + index]
                    .max(0.0)
                    .min(order.max_fill.0 as f64 / share_scale)
            };
            q_values.push(shares * share_scale);
        }
        let (utilities, linear) = model.allocation_components(&q_values);
        let primal_objective = model.objective_from_components(&utilities, linear);
        let certified_gap = (objective_upper - primal_objective).max(0.0);

        let final_alpha = model.pacing_factors(&utilities);
        let projection_objective = model.oracle_coefficients_from_alpha(&final_alpha);
        let mut result = support_and_finalize_target_with_objective(
            &q_values,
            problem,
            &ctx,
            &projection_objective,
            start,
        );
        if result.diagnostics.status != TerminationStatus::PostProcessingFailure {
            let integer_landing_budget_trimmed = result.diagnostics.integer_landing_budget_trimmed;
            let landed_q = crate::retained_cash_solver::landed_quantities(problem, &result);
            let landed_objective =
                model.objective_for_landed_fills(&landed_q, &result.result.fills);
            result.diagnostics = SolverDiagnostics {
                algorithm: "direct-dual-conic".into(),
                status: TerminationStatus::Converged,
                iterations: Some(iterations),
                objective_value: primal_objective.is_finite().then_some(primal_objective),
                optimality_gap: certified_gap.is_finite().then_some(certified_gap),
                integer_landing_loss: Some((primal_objective - landed_objective).max(0.0)),
                integer_landing_l1_ratio: crate::retained_cash_solver::landing_l1_ratio(
                    &q_values, &landed_q,
                ),
                integer_landing_budget_trimmed,
                primal_residual: primal_residual.is_finite().then_some(primal_residual),
                dual_residual: dual_residual.is_finite().then_some(dual_residual),
                message: Some(format!(
                    "Clarabel status: {status:?}; exact projected dual upper={objective_upper}"
                )),
                ..Default::default()
            };
        }
        result
    }
}

impl Default for DirectDualConicSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for DirectDualConicSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        DirectDualConicSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "DirectDualConic"
    }
}

#[cfg(test)]
mod tests {
    use matching_engine::{MmConstraint, MmId, MmSide, Nanos};

    use super::*;
    use crate::test_fixtures::{
        group_minting_problem, mm_budget_problem, multiple_mms_problem, zero_budget_mm_problem,
    };

    fn assert_success(problem: &Problem) {
        let result = DirectDualConicSolver::new().solve(problem);
        assert_eq!(
            result.diagnostics.status,
            TerminationStatus::Converged,
            "{:?}",
            result.diagnostics.message,
        );
        let objective = result.diagnostics.objective_value.unwrap();
        let gap = result.diagnostics.optimality_gap.unwrap();
        assert!(objective.is_finite());
        assert!(gap >= 0.0 && gap.is_finite());
    }

    #[test]
    fn solves_tight_budget_book() {
        assert_success(&mm_budget_problem());
    }

    #[test]
    fn solves_multiple_market_makers() {
        assert_success(&multiple_mms_problem());
    }

    #[test]
    fn solves_categorical_group() {
        let mut problem = group_minting_problem();
        let mut mm = MmConstraint::new(MmId(1), Nanos(NANOS_PER_DOLLAR));
        mm.add_order(1, MmSide::BuyYes);
        problem.mm_constraints.push(mm);
        assert_success(&problem);
    }

    #[test]
    fn keeps_zero_budget_mm_unfilled() {
        let problem = zero_budget_mm_problem();
        let result = DirectDualConicSolver::new().solve(&problem);
        assert!(
            result
                .result
                .fills
                .iter()
                .all(|fill| fill.order_id != 200 || fill.fill_qty.0 == 0)
        );
    }
}
