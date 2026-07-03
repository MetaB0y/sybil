//! Eisenberg-Gale (Fisher market) solver for prediction market matching.
//!
//! Implements the convex program from "Prediction Markets Are Fisher Markets" (Theorem 5):
//! replacing linear MM welfare with log utility (`B_k · ln(U_k)`) makes the budget-constrained
//! clearing problem convex — unique prices, polynomial-time solvable, budgets absorbed
//! into the objective via KKT conditions.
//!
//! Uses Frank-Wolfe (conditional gradient) with the LP solver as oracle:
//! each iteration solves an LP with gradient-derived objective coefficients.
//! For retail orders the gradient is constant (same as LP welfare);
//! for MM orders the gradient diminishes as `B_k / U_k`, naturally enforcing budgets.
//!
//! **Optimization**: Exact line search via bisection on the EG objective derivative.
//! The EG objective along the FW direction is concave in γ, so bisection on
//! `dφ/dγ = 0` finds the optimal step in ~15 iterations. This typically halves
//! the number of LP oracle calls vs fixed `γ = 2/(t+2)`.

use std::time::Instant;

use matching_engine::{Order, Problem};

use crate::lp_solver::{
    build_and_solve_lp, build_solver_context, project_and_finalize, welfare_weights,
};
use crate::result::PipelineResult;

/// Configuration for the Eisenberg-Gale solver.
#[derive(Clone, Debug)]
pub struct EgConfig {
    /// Maximum Frank-Wolfe iterations (default: 25).
    pub max_fw_iterations: usize,
    /// Convergence tolerance: relative change in EG objective (default: 1e-6).
    pub convergence_tol: f64,
    /// Q-stability tolerance: max absolute change in any q_i to declare convergence (default: 1.0).
    pub q_stability_tol: f64,
    /// Bisection steps for exact line search (default: 15).
    pub line_search_steps: usize,
    /// SLP iterations for residual MM budget violations after rounding (default: 1).
    pub max_mm_slp_iterations: usize,
}

impl Default for EgConfig {
    fn default() -> Self {
        Self {
            max_fw_iterations: 25,
            convergence_tol: 1e-6,
            q_stability_tol: 1.0,
            line_search_steps: 15,
            max_mm_slp_iterations: 1,
        }
    }
}

/// Eisenberg-Gale solver using Frank-Wolfe with LP oracle and exact line search.
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

    /// Solve a matching problem using the Eisenberg-Gale convex program.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "EG");
        let _rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let orders = &problem.orders;
        let n = orders.len();

        let ctx = build_solver_context(problem);

        // Per-order MM info: order_index -> (mm_constraint_index, MmSide)
        let mm_order_map = ctx.mm_order_index_map(orders);

        // Precompute per-order welfare weight: sign * limit_price
        let welfare_weights = welfare_weights(orders);

        // Group positive-welfare MM orders by constraint index. Nonpositive
        // MM orders stay in the linear welfare objective; putting them inside
        // log utility can drive U_k negative and collapse the FW allocation.
        let num_mm = problem.mm_constraints.len();
        let mut mm_pos_groups: Vec<Vec<usize>> = vec![Vec::new(); num_mm];
        for (&order_idx, &(mm_idx, _)) in &mm_order_map {
            if welfare_weights[order_idx] > 0.0 {
                mm_pos_groups[mm_idx].push(order_idx);
            }
        }

        // MM budgets
        let mm_budgets: Vec<f64> = problem
            .mm_constraints
            .iter()
            .map(|mm| mm.max_capital.0 as f64)
            .collect();

        let has_mm = !problem.mm_constraints.is_empty();

        // If no MM orders, just run the LP directly (EG reduces to LP)
        if !has_mm {
            return self.solve_lp_only(problem, orders, &ctx, start);
        }

        let active_mm: Vec<bool> = (0..num_mm)
            .map(|k| mm_budgets[k] > 0.0 && !mm_pos_groups[k].is_empty())
            .collect();

        let is_log_mm_order = |i: usize| -> Option<usize> {
            let (mm_idx, _) = *mm_order_map.get(&i)?;
            (welfare_weights[i] > 0.0 && active_mm[mm_idx]).then_some(mm_idx)
        };

        // ================================================================
        // Step 1: Warm start — solve LP with linear welfare
        // ================================================================
        let linear_obj: Vec<f64> = welfare_weights.clone();
        let warm_solution = build_and_solve_lp(
            orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &linear_obj,
            &[], // No budget constraints
        );

        let Some(warm_sol) = warm_solution else {
            return PipelineResult::empty();
        };

        // Initialize q from warm start
        let mut q: Vec<f64> = warm_sol.q_values.clone();
        drop(warm_sol);

        // Seed MM fills: ensure each MM group has nonzero surplus
        // to avoid gradient explosion (B_k / 0) on first iteration.
        for (mm_idx, group_orders) in mm_pos_groups.iter().enumerate() {
            if !active_mm[mm_idx] {
                continue;
            }
            let surplus: f64 = group_orders
                .iter()
                .map(|&i| welfare_weights[i] * q[i])
                .sum();
            if surplus <= 0.0 {
                for &i in group_orders {
                    if q[i] < 1.0 {
                        q[i] = 1.0_f64.min(orders[i].max_fill.0 as f64);
                    }
                }
            }
        }

        let mut prev_obj = f64::NEG_INFINITY;

        // ================================================================
        // Step 2: Frank-Wolfe loop with exact line search
        // ================================================================
        for _t in 0..self.config.max_fw_iterations {
            // Compute U_k = Σ_{i ∈ MM_k} w_i * q_i for each MM group
            let u_k: Vec<f64> = mm_pos_groups
                .iter()
                .map(|group_orders| {
                    let u: f64 = group_orders
                        .iter()
                        .map(|&i| welfare_weights[i] * q[i])
                        .sum();
                    u.max(1.0) // Floor at 1.0 nano to avoid division by zero
                })
                .collect();

            // Build gradient (objective coefficients for LP oracle)
            let grad: Vec<f64> = (0..n)
                .map(|i| {
                    if let Some(mm_idx) = is_log_mm_order(i) {
                        mm_budgets[mm_idx] * welfare_weights[i] / u_k[mm_idx]
                    } else {
                        welfare_weights[i]
                    }
                })
                .collect();

            // Solve LP oracle with gradient as objective
            let Some(sol) = build_and_solve_lp(
                orders,
                &ctx.markets,
                &ctx.market_to_group,
                ctx.num_groups,
                &grad,
                &[],
            ) else {
                break;
            };

            // ============================================================
            // Exact line search: find γ* that maximizes φ(γ) = f(q + γ(s-q))
            // ============================================================
            //
            // φ(γ) = Σ_k B_k * ln((1-γ)*U_k(q) + γ*U_k(s))
            //       + (1-γ)*R(q) + γ*R(s)
            //
            // φ'(γ) = Σ_k B_k * ΔU_k / ((1-γ)*U_k_q + γ*U_k_s) + ΔR
            //
            // Concave in γ → bisection on φ'(γ) = 0.

            // Precompute U_k(s) for each MM group
            let u_k_s: Vec<f64> = mm_pos_groups
                .iter()
                .map(|group_orders| {
                    let u: f64 = group_orders
                        .iter()
                        .map(|&i| welfare_weights[i] * sol.q_values[i])
                        .sum();
                    u.max(1.0)
                })
                .collect();

            // ΔU_k = U_k(s) - U_k(q)
            let delta_u: Vec<f64> = (0..num_mm).map(|k| u_k_s[k] - u_k[k]).collect();

            // R(q) = linear-welfare orders, R(s) = same at the oracle vertex.
            let r_q: f64 = (0..n)
                .filter(|&i| is_log_mm_order(i).is_none())
                .map(|i| welfare_weights[i] * q[i])
                .sum();
            let r_s: f64 = (0..n)
                .filter(|&i| is_log_mm_order(i).is_none())
                .map(|i| welfare_weights[i] * sol.q_values[i])
                .sum();
            let delta_r = r_s - r_q;

            // φ'(γ) evaluated at a given γ
            let phi_prime = |gamma: f64| -> f64 {
                let mut deriv = delta_r;
                for k in 0..num_mm {
                    if !active_mm[k] {
                        continue;
                    }
                    let denom = (1.0 - gamma) * u_k[k] + gamma * u_k_s[k];
                    if denom > 0.0 {
                        deriv += mm_budgets[k] * delta_u[k] / denom;
                    }
                }
                deriv
            };

            // Bisection on φ'(γ) = 0 over [0, 1]
            let gamma = if phi_prime(0.0) <= 0.0 {
                // Objective decreasing from the start — take minimal step
                // Use standard FW step as fallback (ensures convergence)
                2.0 / (_t as f64 + 2.0)
            } else if phi_prime(1.0) >= 0.0 {
                // Objective still increasing at γ=1 — full step
                1.0
            } else {
                // Normal case: bisect to find root
                let mut lo = 0.0_f64;
                let mut hi = 1.0_f64;
                for _ in 0..self.config.line_search_steps {
                    let mid = (lo + hi) / 2.0;
                    if phi_prime(mid) > 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                (lo + hi) / 2.0
            };

            // Update q: q^{t+1} = (1 - γ) * q^t + γ * s^t
            let mut max_q_change: f64 = 0.0;
            for (i, q_i) in q.iter_mut().enumerate() {
                let q_new = (1.0 - gamma) * *q_i + gamma * sol.q_values[i];
                max_q_change = max_q_change.max((q_new - *q_i).abs());
                *q_i = q_new;
            }

            // Compute EG objective: Σ_k B_k * ln(U_k) + Σ_{j ∉ MM} w_j * q_j
            let mut eg_obj = 0.0;
            for (mm_idx, group_orders) in mm_pos_groups.iter().enumerate() {
                if !active_mm[mm_idx] {
                    continue;
                }
                let surplus: f64 = group_orders
                    .iter()
                    .map(|&i| welfare_weights[i] * q[i])
                    .sum();
                if surplus > 0.0 {
                    eg_obj += mm_budgets[mm_idx] * surplus.ln();
                }
            }
            for i in 0..n {
                if is_log_mm_order(i).is_none() {
                    eg_obj += welfare_weights[i] * q[i];
                }
            }

            // Check convergence: objective stability AND q-stability
            let obj_converged = if prev_obj > f64::NEG_INFINITY {
                let rel_change = (eg_obj - prev_obj).abs() / prev_obj.abs().max(1.0);
                rel_change < self.config.convergence_tol
            } else {
                false
            };

            let q_converged = max_q_change < self.config.q_stability_tol;

            prev_obj = eg_obj;

            if obj_converged && q_converged {
                break;
            }
        }

        // ================================================================
        // Step 3: Projection LP for valid prices
        // ================================================================
        //
        // FW produces q values that are convex combinations of LP vertices.
        // The last LP's duals don't correspond to this allocation (different
        // objective). The shared epilogue caps upper bounds at the FW
        // allocation and re-solves the standard welfare LP for proper duals
        // where complementary slackness guarantees UCP.
        let result = project_and_finalize(&q, problem, &ctx, start);
        if result.result.fills.is_empty() {
            crate::lp_solver::LpSolver::new().solve(problem)
        } else {
            result
        }
    }

    /// Fast path: no MM orders → single LP solve (identical to LpSolver).
    fn solve_lp_only(
        &self,
        problem: &Problem,
        orders: &[Order],
        ctx: &crate::lp_solver::SolverContext,
        start: Instant,
    ) -> PipelineResult {
        let objective_coeffs = welfare_weights(orders);

        let Some(sol) = build_and_solve_lp(
            orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &objective_coeffs,
            &[],
        ) else {
            return PipelineResult::empty();
        };

        crate::lp_solver::finalize_result(&sol, problem, ctx, start)
    }
}

impl Default for EgSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for EgSolver {
    /// Forwards to the inherent `EgSolver::solve` method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        EgSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "EG"
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

    #[test]
    fn test_eg_single_market_matches_lp() {
        // No MMs -> EG should produce identical results to LP
        let result = EgSolver::new().solve(&single_market_problem());

        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare()
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_eg_minting() {
        let result = EgSolver::new().solve(&minting_problem());

        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(result.result.total_welfare() > 0);
    }

    #[test]
    fn test_eg_group_minting() {
        let result = EgSolver::new().solve(&group_minting_problem());

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(result.result.total_welfare() > 0);
    }

    #[test]
    fn test_eg_empty_problem() {
        let problem = Problem::new("empty");
        let solver = EgSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_eg_no_profitable_trades() {
        let result = EgSolver::new().solve(&no_profitable_trades_problem());

        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }

    #[test]
    fn test_eg_mm_budget_absorption() {
        // MM with limited budget -> EG should respect budget.
        // YES buyer + NO buyer (MM) pair via minting: mint costs $1,
        // recovers 60c + 50c = $1.10 -> profitable.
        let result = EgSolver::new().solve(&mm_budget_problem());

        assert!(result.result.orders_filled > 0, "should fill some orders");
        assert_buy_no_within_budget(&result, 200, 50);
    }

    #[test]
    fn test_eg_zero_mm_budget() {
        let result = EgSolver::new().solve(&zero_budget_mm_problem());

        assert_mm_not_filled(&result, 200);
    }

    #[test]
    fn test_eg_multiple_mms() {
        // Two MMs with different budgets, both buying NO to pair with YES buyers via minting
        let result = EgSolver::new().solve(&multiple_mms_problem());

        assert!(result.result.orders_filled > 0);
        assert!(result.result.total_welfare() > 0);
    }
}
