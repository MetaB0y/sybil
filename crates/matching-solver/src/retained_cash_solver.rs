//! Certified retained-cash clearing by generalized Frank--Wolfe.
//!
//! This solver optimizes the paper's reduced-form objective
//!
//! `sum_k psi_Bk(U_k(q)) + retail(q) - C_0(D(q))`,
//!
//! where `psi_B(U) = U` below the budget (up to an irrelevant constant) and
//! `B * (1 + ln(U / B))` above it.  The smooth MM term is linearized while
//! retail welfare and the zero-temperature minting cost remain inside a HiGHS
//! LP oracle.  The resulting generalized Frank--Wolfe gap is a certificate of
//! continuous objective suboptimality, not an iterate-stability heuristic.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use matching_engine::{NANOS_PER_DOLLAR, Problem, SHARE_SCALE};

use crate::lp_solver::{
    ReusableLpOracle, SolverContext, build_solver_context, project_and_finalize,
    support_and_finalize_target_with_objective, welfare_weights,
};
use crate::result::{PipelineResult, SolverDiagnostics, TerminationStatus};

/// Configuration for retained-cash generalized Frank--Wolfe.
#[derive(Clone, Debug)]
pub struct RetainedCashConfig {
    /// Maximum allocation updates. One final oracle call is still made to
    /// certify the returned iterate when this cap is reached.
    pub max_iterations: usize,
    /// Absolute certified-gap tolerance in nanos of objective value.
    pub gap_abs_nanos: f64,
    /// Relative certified-gap tolerance against the current objective scale.
    pub gap_rel: f64,
    /// Bisection steps for the exact one-dimensional concave line search.
    pub line_search_steps: usize,
}

impl Default for RetainedCashConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            gap_abs_nanos: 1_000_000.0, // $0.001
            gap_rel: 1e-5,
            line_search_steps: 48,
        }
    }
}

/// Paper-aligned retained-cash solver with a generalized Frank--Wolfe gap.
pub struct RetainedCashSolver {
    config: RetainedCashConfig,
}

impl RetainedCashSolver {
    pub fn new() -> Self {
        Self {
            config: RetainedCashConfig::default(),
        }
    }

    pub fn with_config(config: RetainedCashConfig) -> Self {
        Self { config }
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "RetainedCashFW");
        let rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::failure(
                "retained-cash-fw",
                TerminationStatus::UnsupportedInput,
                format!("rejected {rejected_orders} unsupported orders"),
                start.elapsed().as_secs_f64(),
            );
        }

        let ctx = build_solver_context(problem);
        let model = ObjectiveModel::new(problem, &ctx);

        if !model.has_reduced_form_orders() {
            let mut result = crate::LpSolver::new().solve(problem);
            result.diagnostics.algorithm = "retained-cash-fw".to_string();
            result.diagnostics.status = TerminationStatus::Delegated;
            result.diagnostics.message =
                Some("no positive-welfare MM orders; retained-cash objective reduces to LP".into());
            return result;
        }

        // Zero is feasible, avoids the no-cash log singularity, and has the
        // economically correct slack-capital gradient alpha=1 for B>0.
        let mut q = vec![0.0; problem.orders.len()];
        let mut objective = model.objective(&q);
        let mut last_gap = f64::INFINITY;
        let mut oracle_calls = 0usize;
        let mut oracle_time = Duration::ZERO;
        let mut updates = 0usize;
        let mut converged = false;
        let mut oracle_failed = false;

        let mut oracle_orders = problem.orders.clone();
        model.disable_zero_budget_orders(&mut oracle_orders);
        let Some(mut oracle) = ReusableLpOracle::new(
            &oracle_orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &[],
        ) else {
            return PipelineResult::failure(
                "retained-cash-fw",
                TerminationStatus::NumericalFailure,
                "failed to construct the HiGHS oracle",
                start.elapsed().as_secs_f64(),
            );
        };

        for iteration in 0..=self.config.max_iterations {
            let u_q = model.utilities(&q);
            let alpha_q = model.pacing_factors(&u_q);
            let oracle_objective = model.oracle_coefficients_from_alpha(&alpha_q);

            let oracle_started = Instant::now();
            let oracle_solution = oracle.solve(&oracle_objective);
            oracle_time += oracle_started.elapsed();
            let Some(oracle_solution) = oracle_solution else {
                oracle_failed = true;
                break;
            };
            oracle_calls += 1;

            let s = &oracle_solution.q_values;
            let g_q = model.linear_component(&q);
            let g_s = model.linear_component(s);
            let current_score = model.affine_score(&u_q, g_q, &alpha_q);
            let Some(oracle_upper_bound) = oracle_solution.objective_upper_bound_dollars else {
                oracle_failed = true;
                break;
            };
            let oracle_upper_score =
                oracle_upper_bound * NANOS_PER_DOLLAR as f64 / SHARE_SCALE as f64;
            last_gap = (oracle_upper_score - current_score).max(0.0);

            let tolerance = self
                .config
                .gap_abs_nanos
                .max(self.config.gap_rel * objective.abs().max(1.0));
            if last_gap <= tolerance {
                converged = true;
                break;
            }
            if iteration == self.config.max_iterations {
                break;
            }

            let u_s = model.utilities(s);
            let delta_u: Vec<f64> = u_s
                .iter()
                .zip(&u_q)
                .map(|(right, left)| right - left)
                .collect();
            let delta_g = g_s - g_q;
            let derivative = |gamma: f64| {
                let mut value = delta_g;
                for k in 0..u_q.len() {
                    let u = u_q[k] + gamma * delta_u[k];
                    value += model.pacing_factor(k, u) * delta_u[k];
                }
                value
            };

            // Concavity makes the directional derivative non-increasing.
            // Unlike the legacy EG path, a non-positive derivative at zero is
            // a stopping condition; it is never replaced by a forced step.
            let mut gamma = if derivative(0.0) <= 0.0 {
                converged = true;
                last_gap = 0.0;
                break;
            } else if derivative(1.0) >= 0.0 {
                1.0
            } else {
                let mut low = 0.0;
                let mut high = 1.0;
                for _ in 0..self.config.line_search_steps {
                    let mid = (low + high) / 2.0;
                    if derivative(mid) > 0.0 {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                (low + high) / 2.0
            };

            let previous_objective = objective;
            let mut candidate = convex_combination(&q, s, gamma);
            let mut candidate_objective = model.objective(&candidate);

            // Protect monotonicity against floating-point bisection noise. This
            // is an Armijo-style step reduction within the same algorithm, not
            // a cross-solver fallback.
            for _ in 0..24 {
                if candidate_objective + 1e-6 >= previous_objective {
                    break;
                }
                gamma /= 2.0;
                candidate = convex_combination(&q, s, gamma);
                candidate_objective = model.objective(&candidate);
            }
            if candidate_objective + 1e-6 < previous_objective {
                oracle_failed = true;
                break;
            }

            q = candidate;
            objective = candidate_objective;
            updates += 1;
        }

        if oracle_calls == 0 {
            return PipelineResult::failure(
                "retained-cash-fw",
                TerminationStatus::NumericalFailure,
                "HiGHS oracle failed before producing an iterate",
                start.elapsed().as_secs_f64(),
            );
        }

        // Land on protocol integer quantities without inventing fills: each
        // order is capped by ceil(q_i), and the ordinary welfare LP chooses a
        // verifier-supported point inside those caps. This LP is the explicit
        // integer-grid/pricing epilogue, not an alternative core solver.
        let mut result = if converged && !oracle_failed {
            let final_utilities = model.utilities(&q);
            let final_alpha = model.pacing_factors(&final_utilities);
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
                "{previous}; core objective={objective}, gap={last_gap}, finite_q={}",
                q.iter().all(|value| value.is_finite()),
            ));
        } else {
            let integer_landing_budget_trimmed = result.diagnostics.integer_landing_budget_trimmed;
            let landed_q = landed_quantities(problem, &result);
            let landed_objective =
                model.objective_for_landed_fills(&landed_q, &result.result.fills);
            result.diagnostics = SolverDiagnostics {
                algorithm: "retained-cash-fw".to_string(),
                status: if oracle_failed {
                    TerminationStatus::NumericalFailure
                } else if converged {
                    TerminationStatus::Converged
                } else {
                    TerminationStatus::IterationLimit
                },
                iterations: Some(updates),
                convergence_metric: last_gap.is_finite().then_some(last_gap),
                objective_value: Some(objective),
                optimality_gap: last_gap.is_finite().then_some(last_gap),
                oracle_calls: Some(oracle_calls),
                oracle_time_secs: Some(oracle_time.as_secs_f64()),
                integer_landing_loss: Some((objective - landed_objective).max(0.0)),
                integer_landing_l1_ratio: landing_l1_ratio(&q, &landed_q),
                integer_landing_budget_trimmed,
                message: Some(
                    "objective/gap/landing loss are continuous retained-cash nanodollars"
                        .to_string(),
                ),
                ..Default::default()
            };
        }
        result
    }
}

impl Default for RetainedCashSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for RetainedCashSolver {
    fn solve(&self, problem: &Problem) -> PipelineResult {
        RetainedCashSolver::solve(self, problem)
    }

    fn name(&self) -> &str {
        "RetainedCashFW"
    }
}

fn convex_combination(left: &[f64], right: &[f64], gamma: f64) -> Vec<f64> {
    left.iter()
        .zip(right)
        .map(|(l, r)| (1.0 - gamma) * l + gamma * r)
        .collect()
}

pub(crate) fn landed_quantities(problem: &Problem, result: &PipelineResult) -> Vec<f64> {
    let fills: HashMap<u64, u64> = result
        .result
        .fills
        .iter()
        .map(|fill| (fill.order_id, fill.fill_qty.0))
        .collect();
    problem
        .orders
        .iter()
        .map(|order| fills.get(&order.id).copied().unwrap_or(0) as f64)
        .collect()
}

pub(crate) fn landing_l1_ratio(target: &[f64], landed: &[f64]) -> Option<f64> {
    let numerator: f64 = target
        .iter()
        .zip(landed)
        .map(|(&left, &right)| (left - right).abs())
        .sum();
    let denominator: f64 = target
        .iter()
        .zip(landed)
        .map(|(&left, &right)| left.abs().max(right.abs()))
        .sum();
    (denominator > 0.0).then_some(numerator / denominator)
}

/// Evaluate the shifted retained-cash objective on landed protocol fills.
///
/// The core objective uses the LP's zero-temperature minting term. Finalizers
/// may trim individual fills after price discovery, so landed fills instead
/// use the protocol's signed mint/burn cost derived from their actual uniform
/// prices. The value is therefore comparable across landed solver outputs on
/// the same problem and budget.
pub fn retained_cash_objective_for_fills(
    problem: &Problem,
    fills: &[matching_engine::Fill],
) -> f64 {
    let ctx = build_solver_context(problem);
    let model = ObjectiveModel::new(problem, &ctx);
    let fill_map: HashMap<_, _> = fills
        .iter()
        .map(|fill| (fill.order_id, fill.fill_qty.0))
        .collect();
    let q: Vec<_> = problem
        .orders
        .iter()
        .map(|order| fill_map.get(&order.id).copied().unwrap_or(0) as f64)
        .collect();
    model.objective_for_landed_fills(&q, fills)
}

/// Instance-specific first welfare-gap bound from the paper, evaluated at an
/// unconstrained LP allocation: sum_k [Delta_k - B_k ln(1 + Delta_k/B_k)].
pub fn retained_cash_welfare_gap_bound_for_fills(
    problem: &Problem,
    unconstrained_fills: &[matching_engine::Fill],
) -> f64 {
    let ctx = build_solver_context(problem);
    let model = ObjectiveModel::new(problem, &ctx);
    let fill_map: HashMap<_, _> = unconstrained_fills
        .iter()
        .map(|fill| (fill.order_id, fill.fill_qty.0))
        .collect();
    let q: Vec<_> = problem
        .orders
        .iter()
        .map(|order| fill_map.get(&order.id).copied().unwrap_or(0) as f64)
        .collect();
    model
        .utilities(&q)
        .iter()
        .zip(&model.budgets)
        .map(|(&utility, &budget)| {
            let delta = (utility - budget).max(0.0);
            if delta == 0.0 {
                0.0
            } else if budget <= 0.0 {
                delta
            } else {
                delta - budget * (1.0 + delta / budget).ln()
            }
        })
        .sum()
}

/// Shifted reduced-form utility. The omitted `B ln B - B` constant has no
/// effect on allocation and keeps reported objectives finite and interpretable.
pub(crate) fn retained_cash_utility(budget: f64, utility: f64) -> f64 {
    if budget <= 0.0 {
        0.0
    } else if utility <= budget {
        utility.max(0.0)
    } else {
        budget * (1.0 + (utility / budget).ln())
    }
}

pub(crate) struct ObjectiveModel<'a> {
    problem: &'a Problem,
    ctx: &'a SolverContext,
    welfare_weights: Vec<f64>,
    /// Non-negative MM values after the paper's buy/sell reduction. A sell at
    /// L is a complementary-outcome buy at 1-L.
    mm_values: Vec<f64>,
    log_mm_by_order: Vec<Option<usize>>,
    mm_groups: Vec<Vec<usize>>,
    budgets: Vec<f64>,
    market_index: HashMap<matching_engine::MarketId, usize>,
}

impl<'a> ObjectiveModel<'a> {
    pub(crate) fn new(problem: &'a Problem, ctx: &'a SolverContext) -> Self {
        let welfare_weights = welfare_weights(&problem.orders);
        let mm_order_map = ctx.mm_order_index_map(&problem.orders);
        let mut log_mm_by_order = vec![None; problem.orders.len()];
        let mut mm_values = vec![0.0; problem.orders.len()];
        let mut mm_groups = vec![Vec::new(); problem.mm_constraints.len()];
        for (order_index, (mm_index, _)) in mm_order_map {
            log_mm_by_order[order_index] = Some(mm_index);
            mm_values[order_index] = if welfare_weights[order_index] >= 0.0 {
                welfare_weights[order_index]
            } else {
                NANOS_PER_DOLLAR as f64 + welfare_weights[order_index]
            };
            mm_groups[mm_index].push(order_index);
        }
        // `mm_order_index_map` is a HashMap convenience view. Canonicalize its
        // iteration order before any floating-point utility sums so process-
        // randomized hash seeds cannot move a solve across its gap tolerance.
        for group in &mut mm_groups {
            group.sort_unstable();
        }
        let budgets = problem
            .mm_constraints
            .iter()
            .map(|mm| mm.max_capital.0 as f64)
            .collect();
        let market_index = ctx
            .markets
            .iter()
            .enumerate()
            .map(|(index, market)| (*market, index))
            .collect();
        Self {
            problem,
            ctx,
            welfare_weights,
            mm_values,
            log_mm_by_order,
            mm_groups,
            budgets,
            market_index,
        }
    }

    pub(crate) fn has_reduced_form_orders(&self) -> bool {
        self.mm_groups.iter().any(|group| !group.is_empty())
    }

    pub(crate) fn disable_zero_budget_orders(&self, orders: &mut [matching_engine::Order]) {
        for (group, &budget) in self.mm_groups.iter().zip(&self.budgets) {
            if budget > 0.0 {
                continue;
            }
            for &order_index in group {
                orders[order_index].max_fill = matching_engine::Qty::ZERO;
            }
        }
    }

    pub(crate) fn utilities(&self, q: &[f64]) -> Vec<f64> {
        self.mm_groups
            .iter()
            .map(|group| {
                group.iter().map(|&i| self.mm_values[i] * q[i]).sum::<f64>() / SHARE_SCALE as f64
            })
            .collect()
    }

    pub(crate) fn pacing_factor(&self, mm_index: usize, utility: f64) -> f64 {
        let budget = self.budgets[mm_index];
        if budget <= 0.0 {
            0.0
        } else if utility <= budget || utility <= 0.0 {
            1.0
        } else {
            budget / utility
        }
    }

    pub(crate) fn pacing_factors(&self, utilities: &[f64]) -> Vec<f64> {
        utilities
            .iter()
            .enumerate()
            .map(|(k, &utility)| self.pacing_factor(k, utility))
            .collect()
    }

    pub(crate) fn oracle_coefficients_from_alpha(&self, alpha: &[f64]) -> Vec<f64> {
        self.welfare_weights
            .iter()
            .enumerate()
            .map(|(i, &weight)| {
                self.log_mm_by_order[i]
                    .map(|mm_index| {
                        let sell_correction = if weight < 0.0 {
                            NANOS_PER_DOLLAR as f64
                        } else {
                            0.0
                        };
                        alpha[mm_index] * self.mm_values[i] - sell_correction
                    })
                    .unwrap_or(weight)
            })
            .collect()
    }

    pub(crate) fn objective(&self, q: &[f64]) -> f64 {
        self.objective_from_components(&self.utilities(q), self.linear_component(q))
    }

    pub(crate) fn objective_from_components(&self, utilities: &[f64], linear: f64) -> f64 {
        let mm = utilities
            .iter()
            .enumerate()
            .map(|(k, &utility)| retained_cash_utility(self.budgets[k], utility))
            .sum::<f64>();
        mm + linear
    }

    pub(crate) fn linear_component(&self, q: &[f64]) -> f64 {
        self.non_mint_linear_component(q) - self.minting_cost(q)
    }

    fn non_mint_linear_component(&self, q: &[f64]) -> f64 {
        let retail = self
            .welfare_weights
            .iter()
            .enumerate()
            .filter(|(i, _)| self.log_mm_by_order[*i].is_none())
            .map(|(i, weight)| weight * q[i] / SHARE_SCALE as f64)
            .sum::<f64>();
        let sell_reduction_correction = self
            .welfare_weights
            .iter()
            .enumerate()
            .filter(|(i, weight)| self.log_mm_by_order[*i].is_some() && **weight < 0.0)
            .map(|(i, _)| NANOS_PER_DOLLAR as f64 * q[i] / SHARE_SCALE as f64)
            .sum::<f64>();
        retail - sell_reduction_correction
    }

    pub(crate) fn objective_for_landed_fills(
        &self,
        q: &[f64],
        fills: &[matching_engine::Fill],
    ) -> f64 {
        let mm = self
            .utilities(q)
            .iter()
            .enumerate()
            .map(|(k, &utility)| retained_cash_utility(self.budgets[k], utility))
            .sum::<f64>();
        let protocol_minting_cost =
            matching_engine::minting_cost_from_fills(self.problem.orders.iter(), fills) as f64;
        mm + self.non_mint_linear_component(q) - protocol_minting_cost
    }

    pub(crate) fn affine_score(&self, utilities: &[f64], linear: f64, alpha: &[f64]) -> f64 {
        linear
            + utilities
                .iter()
                .zip(alpha)
                .map(|(&utility, &pacing)| utility * pacing)
                .sum::<f64>()
    }

    pub(crate) fn segment_argmax(
        &self,
        utilities: &[f64],
        delta_utilities: &[f64],
        delta_linear: f64,
        max_step: f64,
        line_search_steps: usize,
    ) -> f64 {
        let derivative = |gamma: f64| {
            delta_linear
                + utilities
                    .iter()
                    .zip(delta_utilities)
                    .enumerate()
                    .map(|(k, (&utility, &delta))| {
                        self.pacing_factor(k, utility + gamma * delta) * delta
                    })
                    .sum::<f64>()
        };

        if max_step <= 0.0 || derivative(0.0) <= 0.0 {
            return 0.0;
        }
        if derivative(max_step) >= 0.0 {
            return max_step;
        }

        let mut low = 0.0;
        let mut high = max_step;
        for _ in 0..line_search_steps {
            let mid = (low + high) / 2.0;
            if derivative(mid) > 0.0 {
                low = mid;
            } else {
                high = mid;
            }
        }
        (low + high) / 2.0
    }

    fn minting_cost(&self, q: &[f64]) -> f64 {
        let mut yes = vec![0.0; self.ctx.markets.len()];
        let mut no = vec![0.0; self.ctx.markets.len()];
        for (i, order) in self.problem.orders.iter().enumerate() {
            let Some(&market_index) = self.market_index.get(&order.markets[0]) else {
                continue;
            };
            yes[market_index] += order.payoffs[0] as f64 * q[i];
            no[market_index] += order.payoffs[1] as f64 * q[i];
        }

        let mut mint_quantity = 0.0;
        let mut group_max_diff = vec![0.0_f64; self.ctx.num_groups];
        for (index, market) in self.ctx.markets.iter().enumerate() {
            if let Some(&group) = self.ctx.market_to_group.get(market) {
                mint_quantity += no[index];
                group_max_diff[group] = group_max_diff[group].max(yes[index] - no[index]);
            } else {
                mint_quantity += yes[index].max(no[index]);
            }
        }
        for max_diff in group_max_diff {
            mint_quantity += max_diff;
        }

        mint_quantity * NANOS_PER_DOLLAR as f64 / SHARE_SCALE as f64
    }
}

/// Evaluate the LP's zero-temperature complete-set cost on landed quantities.
///
/// Comparing this quantity with settlement-derived cash cost tests whether the
/// landed prices still support the allocation. A large discrepancy catches a
/// particularly dangerous class of post-price fill mutation that ordinary
/// feasibility and limit checks do not expose.
pub fn zero_temperature_minting_cost_for_fills(
    problem: &Problem,
    fills: &[matching_engine::Fill],
) -> f64 {
    let ctx = build_solver_context(problem);
    let model = ObjectiveModel::new(problem, &ctx);
    let fill_map: HashMap<_, _> = fills
        .iter()
        .map(|fill| (fill.order_id, fill.fill_qty.0))
        .collect();
    let q: Vec<_> = problem
        .orders
        .iter()
        .map(|order| fill_map.get(&order.id).copied().unwrap_or(0) as f64)
        .collect();
    model.minting_cost(&q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        MmConstraint, MmId, MmSide, Nanos, Qty, notional_nanos, outcome_buy, outcome_sell,
        shares_to_qty, simple_no_buy, simple_yes_buy,
    };
    use matching_scenarios::{
        FlashLiquidityConfig, ScenarioConfig, generate_flash_liquidity_scenario, generate_scenario,
    };

    fn tight_budget_problem(budget_dollars: u64) -> Problem {
        let mut problem = Problem::new("retained_cash_tight");
        let market = problem.markets.add_binary("market");
        problem.orders.push(simple_no_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            shares_to_qty(100).0,
        ));
        problem.orders.push(outcome_buy(
            &problem.markets,
            200,
            market,
            0,
            500_000_000,
            shares_to_qty(100).0,
        ));
        let mut mm = MmConstraint::new(MmId::new(1), Nanos(budget_dollars * NANOS_PER_DOLLAR));
        mm.add_order(200, MmSide::BuyYes);
        problem.mm_constraints.push(mm);
        problem
    }

    fn tight_sell_budget_problem(budget_dollars: u64) -> Problem {
        let mut problem = Problem::new("retained_cash_tight_sell");
        let market = problem.markets.add_binary("market");
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            shares_to_qty(100).0,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            200,
            market,
            0,
            500_000_000,
            shares_to_qty(100).0,
        ));
        let mut mm = MmConstraint::new(MmId::new(1), Nanos(budget_dollars * NANOS_PER_DOLLAR));
        mm.add_order(200, MmSide::SellYes);
        problem.mm_constraints.push(mm);
        problem
    }

    fn calibrate_budgets_from_unconstrained_lp(problem: &mut Problem, fraction: f64) {
        let mut unconstrained = problem.clone();
        unconstrained.mm_constraints.clear();
        let unconstrained = crate::LpSolver::new().solve(&unconstrained);
        let unconstrained_q: HashMap<_, _> = unconstrained
            .result
            .fills
            .iter()
            .map(|fill| (fill.order_id, fill.fill_qty))
            .collect();
        let order_map: HashMap<_, _> = problem
            .orders
            .iter()
            .map(|order| (order.id, order))
            .collect();
        for mm in &mut problem.mm_constraints {
            let limit_value = mm
                .order_ids
                .iter()
                .map(|order_id| {
                    let order = order_map[order_id];
                    let value = if order.is_seller() {
                        Nanos(NANOS_PER_DOLLAR - order.limit_price.0)
                    } else {
                        order.limit_price
                    };
                    notional_nanos(
                        value,
                        unconstrained_q.get(order_id).copied().unwrap_or(Qty::ZERO),
                    )
                    .0
                })
                .sum::<u64>();
            mm.max_capital = Nanos((limit_value as f64 * fraction).round() as u64);
        }
    }

    fn wide_range_problem(seed: u64) -> Problem {
        let mut problem = generate_scenario(ScenarioConfig {
            seed,
            num_markets: 30,
            market_group_probability: 0.65,
            num_orders: 3_000,
            order_size_min: 1,
            order_size_max: 10_000,
            order_size_power: 3.5,
            retail_buy_probability: 0.65,
            liquidity_scarcity: 0.4,
            hot_market_fraction: 0.1,
            hot_order_probability: 0.75,
            liquidity_depth_levels: 6,
            liquidity_dispersion: 1.5,
            num_mms: 2,
            mm_budget_min: 100,
            mm_budget_max: 100_000,
            mm_spread_bps: 75,
            mm_capacity_multiplier: 50,
            mm_market_coverage_fraction: 1.0,
            mm_market_coverage_max: 30,
        });
        calibrate_budgets_from_unconstrained_lp(&mut problem, 0.25);
        problem
    }

    #[test]
    fn shifted_utility_is_affine_then_logarithmic() {
        let budget = 10.0;
        assert_eq!(retained_cash_utility(budget, 4.0), 4.0);
        assert_eq!(retained_cash_utility(budget, budget), budget);
        assert!(retained_cash_utility(budget, 20.0) < 20.0);
    }

    #[test]
    fn tight_budget_converges_with_a_certificate() {
        let problem = tight_budget_problem(10);
        let result = RetainedCashSolver::new().solve(&problem);

        assert_eq!(
            result.diagnostics.status,
            TerminationStatus::Converged,
            "{:?}",
            result.diagnostics
        );
        assert!(result.diagnostics.optimality_gap.unwrap() <= 1_000_000.0);
        let fill = result
            .result
            .fills
            .iter()
            .find(|fill| fill.order_id == 200)
            .expect("MM should provide a budget-limited amount");
        let capital = MmSide::BuyYes.capital_needed(fill.fill_price, fill.fill_qty);
        assert!(capital.0 <= 10 * NANOS_PER_DOLLAR);
    }

    #[test]
    fn slack_budget_recovers_the_lp_welfare() {
        let problem = tight_budget_problem(1_000);
        let retained = RetainedCashSolver::new().solve(&problem);
        let lp = crate::LpSolver::new().solve(&problem);

        assert_eq!(retained.diagnostics.status, TerminationStatus::Converged);
        assert_eq!(retained.result.total_welfare(), lp.result.total_welfare());
        assert_eq!(
            retained_cash_objective_for_fills(&problem, &retained.result.fills).round() as i64,
            retained.result.total_welfare()
        );
    }

    #[test]
    fn mm_sell_is_paced_as_a_complementary_buy() {
        let problem = tight_sell_budget_problem(10);
        let result = RetainedCashSolver::new().solve(&problem);

        assert_eq!(result.diagnostics.status, TerminationStatus::Converged);
        let fill = result
            .result
            .fills
            .iter()
            .find(|fill| fill.order_id == 200)
            .expect("MM ask should receive a budget-limited fill");
        let capital = MmSide::SellYes.capital_needed(fill.fill_price, fill.fill_qty);
        assert!(capital.0 <= 10 * NANOS_PER_DOLLAR);
        assert!(fill.fill_qty < shares_to_qty(100));

        let ctx = build_solver_context(&problem);
        let model = ObjectiveModel::new(&problem, &ctx);
        let q = vec![0.0, shares_to_qty(10).0 as f64];
        assert_eq!(model.utilities(&q)[0], 5.0 * NANOS_PER_DOLLAR as f64);
    }

    #[test]
    fn utility_uses_protocol_share_scale() {
        let problem = tight_budget_problem(10);
        let ctx = build_solver_context(&problem);
        let model = ObjectiveModel::new(&problem, &ctx);
        let q = vec![0.0, shares_to_qty(10).0 as f64];
        assert_eq!(model.utilities(&q)[0], 5.0 * NANOS_PER_DOLLAR as f64);
    }

    #[test]
    fn flash_reference_landing_preserves_the_certified_core() {
        let mut problem = generate_flash_liquidity_scenario(FlashLiquidityConfig {
            seed: 16_400,
            num_markets: 4,
            opportunities_per_market: 2,
            num_mms: 1,
            quantity_min_shares: 10,
            quantity_max_shares: 30,
            initial_budget_dollars: 1_000,
        });
        calibrate_budgets_from_unconstrained_lp(&mut problem, 0.25);

        let result = RetainedCashSolver::new().solve(&problem);
        assert_eq!(result.diagnostics.status, TerminationStatus::Converged);
        let objective = result.diagnostics.objective_value.unwrap();
        let landing_loss = result.diagnostics.integer_landing_loss.unwrap();
        assert!(
            landing_loss <= objective.abs() * 0.01,
            "landing lost {landing_loss} of a {objective} certified core"
        );
    }

    #[test]
    fn supported_wide_range_landings_preserve_minting_duality() {
        for seed in [16_200, 16_201, 16_202, 16_204] {
            let problem = wide_range_problem(seed);
            let result = crate::PacingBundleSolver::with_config(crate::PacingBundleConfig {
                max_iterations: 200,
                gap_rel: 1e-8,
                ..Default::default()
            })
            .solve(&problem);
            assert_ne!(
                result.diagnostics.status,
                TerminationStatus::PostProcessingFailure,
                "seed {seed}: {:?}",
                result.diagnostics,
            );
            let objective = result.diagnostics.objective_value.unwrap();
            let landing_loss = result.diagnostics.integer_landing_loss.unwrap();
            assert!(
                landing_loss <= objective.abs() * 1e-3,
                "seed {seed} landing lost {}% of the core objective",
                landing_loss / objective.abs() * 100.0,
            );
            let zero_temperature =
                zero_temperature_minting_cost_for_fills(&problem, &result.result.fills);
            let duality_gap = (zero_temperature - result.result.minting_cost as f64).abs();
            assert!(
                duality_gap <= 50_000_000.0,
                "seed {seed} landed minting duality gap was ${}",
                duality_gap / NANOS_PER_DOLLAR as f64,
            );
        }
    }

    #[test]
    fn unsupported_wide_range_integer_face_fails_explicitly() {
        let problem = wide_range_problem(16_203);
        let result = crate::PacingBundleSolver::with_config(crate::PacingBundleConfig {
            max_iterations: 200,
            gap_rel: 1e-8,
            ..Default::default()
        })
        .solve(&problem);

        assert_eq!(
            result.diagnostics.status,
            TerminationStatus::PostProcessingFailure
        );
        assert_eq!(result.diagnostics.algorithm, "target-support-lp");
        assert!(
            result
                .diagnostics
                .message
                .as_deref()
                .is_some_and(|message| message
                    .contains("no integer candidate was supported by primary minting prices")),
            "unexpected diagnostic: {:?}",
            result.diagnostics,
        );
    }

    #[test]
    fn nearest_face_landing_is_available_on_multi_mm_flash_book() {
        let mut problem = generate_flash_liquidity_scenario(FlashLiquidityConfig {
            seed: 18_303,
            num_markets: 40,
            opportunities_per_market: 25,
            num_mms: 8,
            quantity_min_shares: 5,
            quantity_max_shares: 250,
            initial_budget_dollars: 25_000,
        });
        calibrate_budgets_from_unconstrained_lp(&mut problem, 0.25);

        let result = crate::PacingBundleSolver::new().solve(&problem);
        assert_ne!(
            result.diagnostics.status,
            TerminationStatus::PostProcessingFailure,
            "{:?}",
            result.diagnostics,
        );
        assert!(!result.result.fills.is_empty());
    }

    #[test]
    fn support_gate_precedes_hard_budget_projection_on_large_book() {
        let mut problem = Problem::new("support_before_budget_projection");
        let market = problem.markets.add_binary("market");
        problem.orders.extend([
            simple_yes_buy(&problem.markets, 1, market, 600_000_000, 1),
            simple_no_buy(&problem.markets, 2, market, 600_000_000, 1),
            outcome_sell(&problem.markets, 3, market, 0, 400_000_000, 1),
            outcome_sell(&problem.markets, 4, market, 1, 400_000_000, 1),
            simple_no_buy(&problem.markets, 5, market, NANOS_PER_DOLLAR, 1_000_000_000),
            simple_yes_buy(&problem.markets, 6, market, NANOS_PER_DOLLAR, 1_000_000_000),
        ]);
        let mut mm = MmConstraint::new(MmId::new(1), Nanos(500_000_000_000_000));
        mm.add_order(5, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        for result in [
            RetainedCashSolver::new().solve(&problem),
            crate::PacingBundleSolver::new().solve(&problem),
        ] {
            assert_ne!(
                result.diagnostics.status,
                TerminationStatus::PostProcessingFailure,
                "{:?}",
                result.diagnostics,
            );
            let zero_temperature =
                zero_temperature_minting_cost_for_fills(&problem, &result.result.fills);
            let support_gap = (zero_temperature - result.result.minting_cost as f64).abs();
            assert!(
                support_gap <= 50_000_000.0,
                "landed support gap was ${}",
                support_gap / NANOS_PER_DOLLAR as f64,
            );
            let mm_fill = result.result.fills.iter().find(|fill| fill.order_id == 5);
            let capital = mm_fill
                .map(|fill| {
                    MmSide::BuyNo
                        .capital_needed(fill.fill_price, fill.fill_qty)
                        .0
                })
                .unwrap_or(0);
            assert!(capital <= 500_000_000_000_000);
        }
    }
}
