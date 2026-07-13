//! Conic EG solver using Clarabel.rs interior-point solver.
//!
//! Supports three objective modes via [`ObjectiveMode`]:
//!
//! - **QuasiFisher** (default): `max Σ [B_k ln(V_k + s_k) - s_k] + Σ w_j q_j`
//!   Cash variable s_k ensures μ_k ≤ 1 (no forced negative-welfare fills).
//! - **Fisher**: `max Σ B_k ln(V_k) + Σ w_j q_j`
//!   No cash variable — MMs may get negative-welfare fills.
//! - **Linear**: Delegates to [`LpSolver`](crate::lp_solver::LpSolver).
//!
//! For each MM with positive budget B_k, models
//! `t_k ≤ B_k ln((V_k + s_k) / B_k)` with the canonical perspective
//! exponential-cone triple `(t_k, B_k, V_k + s_k)`. Everything else is linear.

use std::collections::HashMap;
use std::time::Instant;

use clarabel::algebra::*;
use clarabel::solver::*;

use matching_engine::{NANOS_PER_DOLLAR, Problem, SHARE_SCALE};

use crate::lp_solver::{
    build_solver_context, project_and_finalize, project_and_finalize_with_objective,
    welfare_weights,
};
use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};
use crate::retained_cash_solver::ObjectiveModel;

/// Objective mode for the conic solver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjectiveMode {
    /// Linear welfare: max Σ w_j q_j. Delegates to LpSolver.
    Linear,
    /// Fisher market: max Σ B_k ln(U_k) + Σ_{j∉MM} w_j q_j.
    /// No cash variable — MMs may get negative-welfare fills.
    Fisher,
    /// Quasi-linear Fisher: max Σ [B_k ln(U_k + s_k) - s_k] + Σ_{j∉MM} w_j q_j.
    /// Cash variable ensures μ_k ≤ 1, no forced negative-welfare fills.
    #[default]
    QuasiFisher,
}

/// Configuration for the conic EG solver.
#[derive(Clone, Debug)]
pub struct ConicConfig {
    /// Objective mode (default: QuasiFisher).
    pub mode: ObjectiveMode,
    /// LMSR smoothing temperature (default: 0.0). b>0 not yet implemented.
    pub temperature: f64,
    /// Maximum solver iterations (default: 200).
    pub max_iter: u32,
    /// Convergence tolerance (default: 1e-8).
    pub tol: f64,
    /// Print solver output (default: false).
    pub verbose: bool,
    /// Time limit in seconds (default: 30.0).
    pub time_limit: f64,
}

impl Default for ConicConfig {
    fn default() -> Self {
        Self {
            mode: ObjectiveMode::default(),
            temperature: 0.0,
            max_iter: 200,
            tol: 1e-8,
            verbose: false,
            time_limit: 30.0,
        }
    }
}

/// Independent exponential-cone reference solved by Clarabel.
pub struct ConicSolver {
    config: ConicConfig,
}

impl ConicSolver {
    pub fn new() -> Self {
        Self {
            config: ConicConfig::default(),
        }
    }

    pub fn with_config(config: ConicConfig) -> Self {
        Self { config }
    }

    /// Solve a matching problem using the conic EG formulation.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        assert!(
            self.config.temperature == 0.0,
            "LMSR smoothing (temperature > 0) is not yet implemented"
        );

        // Linear mode: delegate to LpSolver
        #[cfg(feature = "lp")]
        if self.config.mode == ObjectiveMode::Linear {
            let mut result = crate::lp_solver::LpSolver::new().solve(problem);
            result.diagnostics.algorithm = "conic-linear".to_string();
            result.diagnostics.status = TerminationStatus::Delegated;
            result.diagnostics.message = Some("linear mode is implemented by LpSolver".into());
            return result;
        }
        #[cfg(not(feature = "lp"))]
        if self.config.mode == ObjectiveMode::Linear {
            panic!("ObjectiveMode::Linear requires the `lp` feature");
        }

        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "Conic");
        let rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::failure(
                "conic",
                TerminationStatus::UnsupportedInput,
                format!("rejected {rejected_orders} unsupported orders"),
                start.elapsed().as_secs_f64(),
            );
        }

        let orders = &problem.orders;
        let n = orders.len();

        let ctx = build_solver_context(problem);
        let num_markets = ctx.markets.len();

        // Per-order MM info: order_index -> (mm_constraint_index, MmSide)
        let mm_order_map = ctx.mm_order_index_map(orders);

        // Per-order welfare weight: sign × limit_price
        let welfare_weights = welfare_weights(orders);

        // Active MMs: only those with positive budget get exp cone constraints
        let active_mms: Vec<usize> = (0..problem.mm_constraints.len())
            .filter(|&k| problem.mm_constraints[k].max_capital.0 > 0)
            .collect();
        let num_active_mm = active_mms.len();

        // Map global MM index -> local (active) index
        let active_mm_to_local: HashMap<usize, usize> = active_mms
            .iter()
            .enumerate()
            .map(|(local, &global)| (global, local))
            .collect();

        // Group MM orders by active MM local index. MM sells use the paper's
        // buy/sell reduction: sell an outcome at L == buy its complement at
        // 1-L, plus a linear complete-set correction in the original
        // coordinates. Thus every MM valuation entering V_k is non-negative.
        let mut mm_groups: Vec<Vec<usize>> = vec![Vec::new(); num_active_mm];
        for (&order_idx, &(mm_idx, _)) in &mm_order_map {
            if let Some(&local) = active_mm_to_local.get(&mm_idx) {
                mm_groups[local].push(order_idx);
            }
        }
        let mm_values: Vec<f64> = welfare_weights
            .iter()
            .map(|&weight| {
                if weight >= 0.0 {
                    weight
                } else {
                    NANOS_PER_DOLLAR as f64 + weight
                }
            })
            .collect();

        // MM budgets for active MMs
        let mm_budgets: Vec<f64> = active_mms
            .iter()
            .map(|&k| problem.mm_constraints[k].max_capital.0 as f64)
            .collect();

        // ================================================================
        // Variable layout (parameterized by mode)
        // ================================================================
        //
        // QuasiFisher: x = [q, s, t, mint, gmint]
        // Fisher:      x = [q, t, mint, gmint]
        //
        // Independent binary markets retain one free minting-epigraph
        // variable M_m with M_m >= D_yes and M_m >= D_no. For a mutually
        // exclusive group, translation equivariance gives the smaller exact
        // form sum_m D_no_m + max(0, max_m(D_yes_m-D_no_m)), represented by
        // one nonnegative gmint variable and one inequality per member.

        // Filter out MMs with no positive-weight orders (they get no exp cone)
        let cone_mms: Vec<usize> = (0..num_active_mm)
            .filter(|&kk| !mm_groups[kk].is_empty())
            .collect();
        let k = cone_mms.len(); // number of MMs that actually get exp cone constraints
        let m = num_markets;
        let g = ctx.num_groups;
        let independent_markets: Vec<_> = ctx
            .markets
            .iter()
            .copied()
            .filter(|market| !ctx.market_to_group.contains_key(market))
            .collect();
        let u = independent_markets.len();
        let independent_mint_index: HashMap<_, _> = independent_markets
            .iter()
            .enumerate()
            .map(|(index, &market)| (market, index))
            .collect();

        let has_cash = matches!(self.config.mode, ObjectiveMode::QuasiFisher);
        let num_cash_vars = if has_cash { k } else { 0 };
        let d = n + num_cash_vars + k + u + g;

        if k == 0 {
            let mut result = crate::lp_solver::LpSolver::new().solve(problem);
            result.diagnostics.algorithm = "conic".to_string();
            result.diagnostics.status = TerminationStatus::Delegated;
            result.diagnostics.message =
                Some("no active log-utility MMs; objective reduces to LP".into());
            return result;
        }

        // Remap cone_mms index to get budgets/groups for exp-cone MMs only
        let cone_mm_budgets: Vec<f64> = cone_mms.iter().map(|&kk| mm_budgets[kk]).collect();
        let cone_mm_groups: Vec<&Vec<usize>> = cone_mms.iter().map(|&kk| &mm_groups[kk]).collect();

        let q_offset = 0;
        let s_offset = n; // only meaningful if has_cash
        let t_offset = n + num_cash_vars;
        let mint_offset = n + num_cash_vars + k;
        let gmint_offset = mint_offset + u;

        // ================================================================
        // Scaling
        // ================================================================
        //
        // All monetary values are in nanos (1e9 per dollar). This creates
        // ~1e9 condition numbers which break interior point solvers.
        //
        // We scale the objective by 1/NANOS so all coefficients become O(1).
        // The variable s_k (retained cash) is also rescaled: s_k' = s_k/NANOS,
        // and the exp cone V_k row is scaled by 1/NANOS so L_i → L_i/NANOS.
        //
        // The final supporting LP, rather than Clarabel's approximate duals,
        // recovers protocol clearing prices in nanos.

        let nanos_f = NANOS_PER_DOLLAR as f64;
        let share_scale_f = SHARE_SCALE as f64;

        // ================================================================
        // Objective (Clarabel minimizes, scaled by 1/NANOS)
        // ================================================================
        //
        // q and group-mint variables are measured in whole shares,
        // not protocol quantity ticks. This keeps their typical magnitude
        // three orders smaller while preserving the exact model under a
        // linear change of variables.
        //
        // The log epigraph uses the perspective form
        // `(t_k, B_k, U_k + s_k) in K_exp`. This keeps the objective
        // coefficient and the structural matrix at unit scale; putting
        // `t_k / B_k` in the matrix is equivalent mathematically but becomes
        // ill-conditioned when budgets span several orders of magnitude.
        //
        // min: -Σ t_k + Σ s_k' - Σ (w_j/NANOS) q_j + C_0(D)
        //
        // Independent markets use explicit mint epigraphs. Grouped markets
        // contribute Σ D_no in q coefficients plus one group epigraph.

        // α_k = B_k / NANOS (budget in dollars) for each exp-cone MM
        let alpha_k: Vec<f64> = cone_mm_budgets.iter().map(|&b| b / nanos_f).collect();
        // Set of orders that participate in exp cone (positive-weight MM orders)
        let mm_cone_orders: std::collections::HashSet<usize> = cone_mm_groups
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect();

        let mut obj = vec![0.0_f64; d];

        for i in 0..n {
            if mm_cone_orders.contains(&i) {
                // MM buys are captured entirely by B_k ln(V_k). In original
                // coordinates an MM sell additionally contributes -$1 per
                // share; together with value 1-L inside V_k this recovers -L
                // exactly on the slack-budget branch.
                obj[q_offset + i] = if welfare_weights[i] < 0.0 { 1.0 } else { 0.0 };
            } else {
                // Non-MM orders retain ordinary linear welfare.
                obj[q_offset + i] = -welfare_weights[i] / nanos_f;
            }
            if ctx.market_to_group.contains_key(&orders[i].markets[0]) {
                obj[q_offset + i] += orders[i].payoffs[1] as f64;
            }
        }
        for kk in 0..k {
            if has_cash {
                obj[s_offset + kk] = 1.0; // s_k' in dollars
            }
            obj[t_offset + kk] = -1.0;
        }
        for mm in 0..u {
            obj[mint_offset + mm] = 1.0; // dollars per complete set
        }
        for gg in 0..g {
            obj[gmint_offset + gg] = 1.0; // $1 per full-share mint
        }

        // P: all zeros (no quadratic term)
        let p_mat = CscMatrix::zeros((d, d));

        // ================================================================
        // Constraints: Ax + s_cone = b, s_cone ∈ K
        // ================================================================

        let num_exp_rows = 3 * k;
        let num_mint_rows = 2 * u + (m - u);
        let num_bound_rows = 2 * n + num_cash_vars + g;
        let num_nonnegative_rows = num_mint_rows + num_bound_rows;
        let total_rows = num_exp_rows + num_nonnegative_rows;

        // Build A in COO (triplet) format
        let mut tri_row: Vec<usize> = Vec::new();
        let mut tri_col: Vec<usize> = Vec::new();
        let mut tri_val: Vec<f64> = Vec::new();
        let mut b_vec = vec![0.0_f64; total_rows];

        // --- Block 1: Exponential cones (3 rows per active MM) ---
        //
        // The perspective exp cone models
        //
        //   B_k * exp(t_k / B_k) <= V_k,
        //
        // or `t_k <= B_k ln(V_k / B_k)`. The omitted `B_k ln(B_k)`
        // constant does not affect the allocation.
        //
        // Slack variables: (s₁, s₂, s₃) ∈ K_exp with s₂·exp(s₁/s₂) ≤ s₃
        //   Row 0: s₁ = t_k
        //   Row 1: s₂ = B_k/NANOS
        //   Row 2: s₃ = V_k/NANOS

        for kk in 0..k {
            let row_base = 3 * kk;

            // Row 0: slack = t_k
            tri_row.push(row_base);
            tri_col.push(t_offset + kk);
            tri_val.push(-1.0);

            // Row 1: slack = B_k in dollars
            b_vec[row_base + 1] = alpha_k[kk];

            // Row 2: slack = V_k/NANOS (only positive-weight orders)
            // QuasiFisher: V_k = Σ L_i q_i + s_k
            // Fisher:      V_k = Σ L_i q_i
            for &order_idx in cone_mm_groups[kk] {
                tri_row.push(row_base + 2);
                tri_col.push(q_offset + order_idx);
                tri_val.push(-mm_values[order_idx] / nanos_f);
            }
            if has_cash {
                tri_row.push(row_base + 2);
                tri_col.push(s_offset + kk);
                tri_val.push(-1.0); // s_k' already in dollars
            }
        }

        // --- Block 2: zero-temperature minting epigraph inequalities ---
        // Ax + slack = 0, slack >= 0, hence Ax <= 0.

        let mint_base = num_exp_rows;
        let mut mint_row = mint_base;
        let mut orders_by_market: HashMap<_, Vec<usize>> = HashMap::new();
        for (index, order) in orders.iter().enumerate() {
            orders_by_market
                .entry(order.markets[0])
                .or_default()
                .push(index);
        }
        for &market in &ctx.markets {
            let market_orders = orders_by_market
                .get(&market)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if let Some(&mint_index) = independent_mint_index.get(&market) {
                for outcome in 0..2 {
                    for &i in market_orders {
                        let payoff = orders[i].payoffs[outcome] as f64;
                        if payoff.abs() > 1e-12 {
                            tri_row.push(mint_row);
                            tri_col.push(q_offset + i);
                            tri_val.push(payoff);
                        }
                    }
                    tri_row.push(mint_row);
                    tri_col.push(mint_offset + mint_index);
                    tri_val.push(-1.0);
                    mint_row += 1;
                }
            } else if let Some(&g_idx) = ctx.market_to_group.get(&market) {
                for &i in market_orders {
                    let difference = (orders[i].payoffs[0] - orders[i].payoffs[1]) as f64;
                    if difference.abs() > 1e-12 {
                        tri_row.push(mint_row);
                        tri_col.push(q_offset + i);
                        tri_val.push(difference);
                    }
                }
                tri_row.push(mint_row);
                tri_col.push(gmint_offset + g_idx);
                tri_val.push(-1.0);
                mint_row += 1;
            }
        }
        debug_assert_eq!(mint_row, mint_base + num_mint_rows);

        // --- Block 3: Variable bounds (NonnegativeCone) ---
        //
        // q_i ≤ max_fill, q_i ≥ 0, s_k' ≥ 0, gmint_g ≥ 0

        let bound_base = num_exp_rows + num_mint_rows;
        let mut bound_row = bound_base;

        // q_i ≤ max_fill (in whole shares) → slack = max_fill - q_i ≥ 0
        for (i, order) in orders.iter().enumerate() {
            tri_row.push(bound_row);
            tri_col.push(q_offset + i);
            tri_val.push(1.0);
            let zero_budget_mm = mm_order_map
                .get(&i)
                .is_some_and(|(mm_index, _)| problem.mm_constraints[*mm_index].max_capital.0 == 0);
            b_vec[bound_row] = if zero_budget_mm {
                0.0
            } else {
                order.max_fill.0 as f64 / share_scale_f
            };
            bound_row += 1;
        }

        // q_i ≥ 0 → slack = q_i ≥ 0
        for i in 0..n {
            tri_row.push(bound_row);
            tri_col.push(q_offset + i);
            tri_val.push(-1.0);
            bound_row += 1;
        }

        // s_k' ≥ 0 (QuasiFisher only)
        if has_cash {
            for kk in 0..k {
                tri_row.push(bound_row);
                tri_col.push(s_offset + kk);
                tri_val.push(-1.0);
                bound_row += 1;
            }
        }

        // gmint_g ≥ 0
        for gg in 0..g {
            tri_row.push(bound_row);
            tri_col.push(gmint_offset + gg);
            tri_val.push(-1.0);
            bound_row += 1;
        }

        // ================================================================
        // Cone specification
        // ================================================================

        let mut cones: Vec<SupportedConeT<f64>> = Vec::new();
        for _ in 0..k {
            cones.push(ExponentialConeT());
        }
        if num_nonnegative_rows > 0 {
            cones.push(NonnegativeConeT(num_nonnegative_rows));
        }

        // Build sparse A matrix from triplets
        let a_mat = CscMatrix::new_from_triplets(total_rows, d, tri_row, tri_col, tri_val);

        // ================================================================
        // Solve
        // ================================================================

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

        let mut solver = match DefaultSolver::new(&p_mat, &obj, &a_mat, &b_vec, &cones, settings) {
            Ok(solver) => solver,
            Err(error) => {
                return PipelineResult::failure(
                    "conic",
                    TerminationStatus::NumericalFailure,
                    format!("Clarabel setup failed: {error:?}"),
                    start.elapsed().as_secs_f64(),
                );
            }
        };

        solver.solve();

        let solver_status = solver.solution.status;
        let iterations = solver.solution.iterations as usize;
        // The perspective cone omits one allocation-independent +B_k per MM
        // from the shifted retained-cash objective used elsewhere.
        let conic_objective_nanos =
            (-solver.solution.obj_val + alpha_k.iter().sum::<f64>()) * nanos_f;
        let conic_gap_nanos =
            (solver.solution.obj_val - solver.solution.obj_val_dual).abs() * nanos_f;
        let primal_residual = solver.solution.r_prim;
        let dual_residual = solver.solution.r_dual;
        match solver_status {
            SolverStatus::Solved | SolverStatus::AlmostSolved => {}
            _ => {
                let mut failure = PipelineResult::failure(
                    "conic",
                    TerminationStatus::NumericalFailure,
                    format!("Clarabel terminated with {solver_status:?}"),
                    start.elapsed().as_secs_f64(),
                );
                failure.diagnostics.iterations = Some(iterations);
                failure.diagnostics.objective_value = conic_objective_nanos
                    .is_finite()
                    .then_some(conic_objective_nanos);
                failure.diagnostics.optimality_gap =
                    conic_gap_nanos.is_finite().then_some(conic_gap_nanos);
                failure.diagnostics.primal_residual =
                    primal_residual.is_finite().then_some(primal_residual);
                failure.diagnostics.dual_residual =
                    dual_residual.is_finite().then_some(dual_residual);
                return failure;
            }
        }

        // ================================================================
        // Extract solution
        // ================================================================

        let x = &solver.solution.x;

        // ================================================================
        // Projection LP for exact prices
        // ================================================================
        //
        // The conic solver gives optimal fills but approximate duals.
        // Solve a final LP with max_fill capped at the conic allocation.
        // The LP's duals give exact clearing prices where complementary
        // slackness guarantees UCP. The minting epigraph holds in the LP
        // constraints. Budget absorption holds from the EG structure.

        let q_values: Vec<f64> = (0..n)
            .map(|i| x[q_offset + i].max(0.0) * share_scale_f)
            .collect();

        let mut result = if self.config.mode == ObjectiveMode::QuasiFisher {
            let model = ObjectiveModel::new(problem, &ctx);
            let final_utilities = model.utilities(&q_values);
            let final_alpha = model.pacing_factors(&final_utilities);
            let projection_objective = model.oracle_coefficients_from_alpha(&final_alpha);
            project_and_finalize_with_objective(
                &q_values,
                problem,
                &ctx,
                &projection_objective,
                start,
            )
        } else {
            project_and_finalize(&q_values, problem, &ctx, start)
        };
        if result.diagnostics.status != TerminationStatus::PostProcessingFailure {
            result.diagnostics = SolverDiagnostics {
                algorithm: match self.config.mode {
                    ObjectiveMode::Linear => "conic-linear",
                    ObjectiveMode::Fisher => "conic-fisher",
                    ObjectiveMode::QuasiFisher => "conic-quasi-fisher",
                }
                .to_string(),
                status: TerminationStatus::Converged,
                iterations: Some(iterations),
                objective_value: conic_objective_nanos
                    .is_finite()
                    .then_some(conic_objective_nanos),
                optimality_gap: conic_gap_nanos.is_finite().then_some(conic_gap_nanos),
                primal_residual: primal_residual.is_finite().then_some(primal_residual),
                dual_residual: dual_residual.is_finite().then_some(dual_residual),
                message: Some(format!("Clarabel status: {solver_status:?}")),
                ..Default::default()
            };
        }
        result
    }
}

impl Default for ConicSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for ConicSolver {
    /// Forwards to the inherent `ConicSolver::solve` method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        ConicSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "Conic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{
        assert_buy_no_within_budget, assert_mm_not_filled, group_minting_problem, minting_problem,
        mm_budget_problem, multiple_mms_problem, no_profitable_trades_problem,
        single_market_problem, zero_budget_mm_problem,
    };
    use matching_engine::{
        MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, Nanos, outcome_sell, simple_no_buy,
        simple_yes_buy,
    };

    #[test]
    fn test_conic_single_market() {
        let result = ConicSolver::new().solve(&single_market_problem());

        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare()
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_conic_minting() {
        let result = ConicSolver::new().solve(&minting_problem());

        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(result.result.total_welfare() > 0);
    }

    #[test]
    fn test_conic_group_minting() {
        let result = ConicSolver::new().solve(&group_minting_problem());

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(result.result.total_welfare() > 0);
    }

    #[test]
    fn test_conic_empty_problem() {
        let problem = Problem::new("empty");
        let solver = ConicSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_conic_failure_is_not_silently_replaced_by_lp() {
        let problem = mm_budget_problem();
        let solver = ConicSolver::with_config(ConicConfig {
            max_iter: 0,
            ..Default::default()
        });
        let result = solver.solve(&problem);

        assert_eq!(
            result.diagnostics.status,
            TerminationStatus::NumericalFailure
        );
        assert!(result.result.fills.is_empty());
        assert!(
            !crate::LpSolver::new()
                .solve(&problem)
                .result
                .fills
                .is_empty()
        );
    }

    #[test]
    fn test_conic_no_profitable_trades() {
        let result = ConicSolver::new().solve(&no_profitable_trades_problem());

        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }

    #[test]
    fn test_conic_mm_budget() {
        let problem = mm_budget_problem();
        let result = ConicSolver::new().solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill some orders");
        assert_buy_no_within_budget(&result, 200, 50);

        let landed = crate::retained_cash_objective_for_fills(&problem, &result.result.fills);
        let continuous = result.diagnostics.objective_value.expect("conic objective");
        let gap = result.diagnostics.optimality_gap.expect("conic gap");
        assert!(
            landed <= continuous + gap + NANOS_PER_DOLLAR as f64,
            "landed={landed} continuous={continuous} gap={gap}",
        );
    }

    #[test]
    fn test_conic_multiple_mms() {
        let result = ConicSolver::new().solve(&multiple_mms_problem());

        assert!(result.result.orders_filled > 0);
        assert!(result.result.total_welfare() > 0);
    }

    #[test]
    fn test_conic_zero_budget_mm() {
        let result = ConicSolver::new().solve(&zero_budget_mm_problem());

        assert_mm_not_filled(&result, 200);
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_conic_matches_lp_no_mm() {
        use crate::lp_solver::LpSolver;

        // No MMs → conic should produce identical results to LP
        let mut problem = Problem::new("conic_vs_lp");
        let market = problem.markets.add_binary("market");

        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            500_000_000,
            1000,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            101,
            market,
            1,
            500_000_000,
            1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 400_000_000, 50));

        let lp_result = LpSolver::new().solve(&problem);
        let conic_result = ConicSolver::new().solve(&problem);

        // Welfare should match (within rounding tolerance)
        let welfare_diff =
            (lp_result.result.total_welfare() - conic_result.result.total_welfare()).abs();
        assert!(
            welfare_diff <= 2 * NANOS_PER_DOLLAR as i64,
            "welfare should match: LP={}, Conic={}, diff={}",
            lp_result.result.total_welfare(),
            conic_result.result.total_welfare(),
            welfare_diff
        );

        // Both should produce fills
        assert!(
            conic_result.result.orders_filled > 0,
            "conic should produce fills"
        );
    }

    #[test]
    fn test_conic_fisher_mode() {
        let mut problem = Problem::new("conic_fisher");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            500,
        ));

        // MM buying NO at 50c, budget $50
        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), Nanos(50 * NANOS_PER_DOLLAR));
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = ConicSolver::with_config(ConicConfig {
            mode: ObjectiveMode::Fisher,
            ..Default::default()
        });
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled > 0,
            "Fisher mode should fill orders"
        );
        assert!(
            result.result.total_welfare() > 0,
            "Fisher mode should have positive welfare"
        );
    }

    #[test]
    fn test_conic_fisher_vs_quasi_fisher() {
        // QuasiFisher welfare >= Fisher welfare (cash variable can only help)
        let mut problem = Problem::new("fisher_vs_quasi");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            500,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market,
            550_000_000,
            300,
        ));

        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 2000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), Nanos(100 * NANOS_PER_DOLLAR));
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let fisher = ConicSolver::with_config(ConicConfig {
            mode: ObjectiveMode::Fisher,
            ..Default::default()
        });
        let quasi = ConicSolver::with_config(ConicConfig {
            mode: ObjectiveMode::QuasiFisher,
            ..Default::default()
        });

        let fisher_result = fisher.solve(&problem);
        let quasi_result = quasi.solve(&problem);

        // QuasiFisher has strictly more degrees of freedom (s_k ≥ 0),
        // so its welfare should be >= Fisher welfare (within tolerance)
        assert!(
            quasi_result.result.total_welfare()
                >= fisher_result.result.total_welfare() - NANOS_PER_DOLLAR as i64,
            "QuasiFisher welfare ({}) should be >= Fisher welfare ({})",
            quasi_result.result.total_welfare(),
            fisher_result.result.total_welfare(),
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_conic_linear_delegates_to_lp() {
        use crate::lp_solver::LpSolver;

        let mut problem = Problem::new("conic_linear");
        let market = problem.markets.add_binary("market");

        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            500_000_000,
            1000,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            101,
            market,
            1,
            500_000_000,
            1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 400_000_000, 50));

        let lp_result = LpSolver::new().solve(&problem);
        let linear_result = ConicSolver::with_config(ConicConfig {
            mode: ObjectiveMode::Linear,
            ..Default::default()
        })
        .solve(&problem);

        // Linear mode delegates to LP, so results should be identical
        assert_eq!(
            lp_result.result.total_welfare(),
            linear_result.result.total_welfare(),
            "Linear mode should produce identical welfare to LP"
        );
        assert_eq!(
            lp_result.result.orders_filled, linear_result.result.orders_filled,
            "Linear mode should produce identical fills to LP"
        );
    }
}
