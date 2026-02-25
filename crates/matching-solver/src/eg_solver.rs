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

use std::collections::HashMap;
use std::time::Instant;

use matching_engine::{MarketId, MmSide, Nanos, Order, Problem, NANOS_PER_DOLLAR};

use crate::coefficients::{order_sign, precompute_coefficients, OrderCoefficients};
use crate::lp_solver::{
    build_and_solve_lp, collect_markets, create_position_arbs, extract_result,
    normalized_yes_prices, recompute_welfare, trim_mm_budget_overflows,
};
use crate::pipeline::{PipelineResult, PipelineTimings};
use crate::traits::PriceDiscoveryResult;
use crate::MatchingResult;

/// Configuration for the Eisenberg-Gale solver.
#[derive(Clone, Debug)]
pub struct EgConfig {
    /// Maximum Frank-Wolfe iterations (default: 50).
    pub max_fw_iterations: usize,
    /// Convergence tolerance: relative change in EG objective (default: 1e-6).
    pub convergence_tol: f64,
    /// Price stability tolerance: max price change between iterations in nanos (default: 0.001 * NANOS_PER_DOLLAR).
    pub price_stability_tol: f64,
    /// SLP iterations for residual MM budget violations after rounding (default: 1).
    pub max_mm_slp_iterations: usize,
}

impl Default for EgConfig {
    fn default() -> Self {
        Self {
            max_fw_iterations: 50,
            convergence_tol: 1e-6,
            price_stability_tol: 0.001 * NANOS_PER_DOLLAR as f64,
            max_mm_slp_iterations: 1,
        }
    }
}

/// Eisenberg-Gale solver using Frank-Wolfe with LP oracle.
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

        let orders = &problem.orders;
        let n = orders.len();

        // Precompute coefficients for all orders
        let coeffs: Vec<OrderCoefficients> = orders
            .iter()
            .map(|o| precompute_coefficients(o))
            .collect();

        // Collect all markets
        let markets = collect_markets(orders);

        // Build market -> group index mapping
        let market_to_group: HashMap<MarketId, usize> = problem
            .market_groups
            .iter()
            .enumerate()
            .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
            .collect();

        // Build MM order info: order_index -> (mm_constraint_index, MmSide)
        let mm_order_info_by_id: HashMap<u64, (usize, MmSide)> = problem
            .mm_constraints
            .iter()
            .enumerate()
            .flat_map(|(mm_idx, mm)| {
                mm.order_ids.iter().filter_map(move |&oid| {
                    mm.order_sides.get(&oid).map(|&side| (oid, (mm_idx, side)))
                })
            })
            .collect();

        // Per-order MM info: order_index -> (mm_constraint_index, MmSide)
        let mm_order_map: HashMap<usize, (usize, MmSide)> = orders
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                mm_order_info_by_id
                    .get(&o.id)
                    .map(|&info| (i, info))
            })
            .collect();

        // Group MM orders by constraint index
        let num_mm = problem.mm_constraints.len();
        let mut mm_groups: Vec<Vec<usize>> = vec![Vec::new(); num_mm];
        for (&order_idx, &(mm_idx, _)) in &mm_order_map {
            mm_groups[mm_idx].push(order_idx);
        }

        // Precompute per-order welfare weight: sign * limit_price
        let welfare_weights: Vec<f64> = orders
            .iter()
            .map(|o| order_sign(o) * o.limit_price as f64)
            .collect();

        // MM budgets
        let mm_budgets: Vec<f64> = problem
            .mm_constraints
            .iter()
            .map(|mm| mm.max_capital as f64)
            .collect();

        let has_mm = !problem.mm_constraints.is_empty();

        // If no MM orders, just run the LP directly (EG reduces to LP)
        if !has_mm {
            return self.solve_lp_only(
                problem, orders, &coeffs, &markets, &market_to_group, start,
            );
        }

        // ================================================================
        // Step 1: Warm start — solve LP with linear welfare
        // ================================================================
        let linear_obj: Vec<f64> = welfare_weights.clone();
        let warm_solution = build_and_solve_lp(
            orders,
            &coeffs,
            &markets,
            &market_to_group,
            problem.market_groups.len(),
            &linear_obj,
            &[], // No budget constraints
        );

        let Some(warm_sol) = warm_solution else {
            return PipelineResult::empty();
        };

        // Initialize q from warm start
        let mut q: Vec<f64> = warm_sol.q_values.clone();

        // Seed MM fills: ensure each MM group has nonzero surplus
        // to avoid gradient explosion (B_k / 0) on first iteration.
        for (mm_idx, group_orders) in mm_groups.iter().enumerate() {
            if mm_budgets[mm_idx] == 0.0 {
                continue;
            }
            let surplus: f64 = group_orders
                .iter()
                .map(|&i| welfare_weights[i] * q[i])
                .sum();
            if surplus <= 0.0 {
                // Seed each MM order with a tiny fill
                for &i in group_orders {
                    if q[i] < 1.0 {
                        q[i] = 1.0_f64.min(orders[i].max_fill as f64);
                    }
                }
            }
        }

        let mut prev_obj = f64::NEG_INFINITY;
        let mut prev_prices: HashMap<MarketId, Nanos> = HashMap::new();
        // warm_sol is consumed; we don't need its duals (projection LP gets proper duals)
        drop(warm_sol);

        // ================================================================
        // Step 2: Frank-Wolfe loop
        // ================================================================
        for t in 0..self.config.max_fw_iterations {
            // Compute U_k = Σ_{i ∈ MM_k} w_i * q_i for each MM group
            let mut u_k: Vec<f64> = vec![0.0; num_mm];
            for (mm_idx, group_orders) in mm_groups.iter().enumerate() {
                u_k[mm_idx] = group_orders
                    .iter()
                    .map(|&i| welfare_weights[i] * q[i])
                    .sum();
                // Floor at 1.0 (1 nano) to avoid division by zero
                if u_k[mm_idx] < 1.0 {
                    u_k[mm_idx] = 1.0;
                }
            }

            // Build gradient (objective coefficients for LP oracle)
            let grad: Vec<f64> = (0..n)
                .map(|i| {
                    if let Some(&(mm_idx, _)) = mm_order_map.get(&i) {
                        // MM order: grad = B_k * w_i / U_k
                        mm_budgets[mm_idx] * welfare_weights[i] / u_k[mm_idx]
                    } else {
                        // Retail order: grad = w_i (constant)
                        welfare_weights[i]
                    }
                })
                .collect();

            // Solve LP oracle with gradient as objective
            let lp_sol = build_and_solve_lp(
                orders,
                &coeffs,
                &markets,
                &market_to_group,
                problem.market_groups.len(),
                &grad,
                &[], // No budget constraints — EG handles them
            );

            let Some(sol) = lp_sol else {
                break;
            };

            // Frank-Wolfe step size: γ = 2 / (t + 2)
            let gamma = 2.0 / (t as f64 + 2.0);

            // Update q: q^{t+1} = (1 - γ) * q^t + γ * s^t
            for i in 0..n {
                q[i] = (1.0 - gamma) * q[i] + gamma * sol.q_values[i];
            }

            // Compute EG objective: Σ_k B_k * ln(U_k) + Σ_{j ∉ MM} w_j * q_j
            let mut eg_obj = 0.0;

            // MM log-utility terms
            for (mm_idx, group_orders) in mm_groups.iter().enumerate() {
                if mm_budgets[mm_idx] == 0.0 {
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

            // Retail linear welfare terms
            for i in 0..n {
                if !mm_order_map.contains_key(&i) {
                    eg_obj += welfare_weights[i] * q[i];
                }
            }

            // Check convergence: objective + price stability
            let obj_converged = if prev_obj > f64::NEG_INFINITY {
                let rel_change = (eg_obj - prev_obj).abs()
                    / (prev_obj.abs().max(1.0));
                rel_change < self.config.convergence_tol
            } else {
                false
            };

            // Extract prices from LP duals for price stability check
            let current_prices = normalized_yes_prices(&sol, &markets);
            let price_converged = if !prev_prices.is_empty() {
                let max_delta: f64 = markets
                    .iter()
                    .map(|m| {
                        let prev = prev_prices.get(m).copied().unwrap_or(0) as f64;
                        let curr = current_prices.get(m).copied().unwrap_or(0) as f64;
                        (prev - curr).abs()
                    })
                    .fold(0.0, f64::max);
                max_delta < self.config.price_stability_tol
            } else {
                false
            };

            prev_obj = eg_obj;
            prev_prices = current_prices;

            if obj_converged && price_converged {
                break;
            }
        }

        // ================================================================
        // Step 3: Projection LP for valid prices
        // ================================================================
        //
        // FW produces q values that are convex combinations of LP vertices.
        // The last LP's duals don't correspond to this allocation (different
        // objective). Solve one final LP with standard welfare objective but
        // with upper bounds capped at the FW allocation. This gives proper
        // duals where complementary slackness guarantees UCP.

        let projection_obj: Vec<f64> = welfare_weights.clone();

        // Create projected orders with FW-derived upper bounds
        let mut projected_orders: Vec<Order> = orders.to_vec();
        for i in 0..n {
            let fw_fill = q[i].round().max(0.0) as u64;
            projected_orders[i].max_fill = fw_fill.min(orders[i].max_fill);
        }

        let final_solution = build_and_solve_lp(
            &projected_orders,
            &coeffs,
            &markets,
            &market_to_group,
            problem.market_groups.len(),
            &projection_obj,
            &[], // No budget constraints — EG already handled them
        );

        let Some(final_sol) = final_solution else {
            return PipelineResult::empty();
        };

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let (mut result, prices) = extract_result(&final_sol, orders, &coeffs, &markets);

        // Budget trim: integer rounding breaks KKT budget absorption.
        // This is always needed, not just when violations are detected.
        if has_mm {
            trim_mm_budget_overflows(
                &mut result,
                &problem.mm_constraints,
                &mm_order_info_by_id,
            );
        }

        // Create arb orders after all post-processing
        let max_order_id = orders.iter().map(|o| o.id).max().unwrap_or(0);
        let arb_orders = create_position_arbs(&mut result, &order_map, &prices, max_order_id);

        // Recompute welfare from scratch
        let mut order_map_with_arbs = order_map;
        for arb in &arb_orders {
            order_map_with_arbs.insert(arb.id, arb);
        }
        recompute_welfare(&mut result, &order_map_with_arbs);

        // Build PipelineResult
        let mut pipeline_result = PipelineResult::empty();
        pipeline_result.result = result;
        pipeline_result.price_discovery = Some(PriceDiscoveryResult {
            prices,
            total_fills: pipeline_result.result.fills.len(),
            total_welfare: pipeline_result.result.total_welfare,
            market_solutions: HashMap::new(),
        });
        pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
        pipeline_result.phase_times = PipelineTimings {
            price_discovery_secs: start.elapsed().as_secs_f64(),
            ..Default::default()
        };
        pipeline_result.group_minting_arb_orders = arb_orders;

        // Gate: if total welfare is negative, return empty
        if pipeline_result.result.total_welfare < 0 {
            pipeline_result.result = MatchingResult::new();
        }

        pipeline_result
    }

    /// Fast path: no MM orders → single LP solve (identical to LpSolver).
    fn solve_lp_only(
        &self,
        problem: &Problem,
        orders: &[Order],
        coeffs: &[OrderCoefficients],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        start: Instant,
    ) -> PipelineResult {
        let objective_coeffs: Vec<f64> = orders
            .iter()
            .map(|o| order_sign(o) * o.limit_price as f64)
            .collect();

        let solution = build_and_solve_lp(
            orders,
            coeffs,
            markets,
            market_to_group,
            problem.market_groups.len(),
            &objective_coeffs,
            &[],
        );

        let Some(sol) = solution else {
            return PipelineResult::empty();
        };

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let (mut result, prices) = extract_result(&sol, orders, coeffs, markets);

        let max_order_id = orders.iter().map(|o| o.id).max().unwrap_or(0);
        let arb_orders = create_position_arbs(&mut result, &order_map, &prices, max_order_id);

        let mut order_map_with_arbs = order_map;
        for arb in &arb_orders {
            order_map_with_arbs.insert(arb.id, arb);
        }
        recompute_welfare(&mut result, &order_map_with_arbs);

        let mut pipeline_result = PipelineResult::empty();
        pipeline_result.result = result;
        pipeline_result.price_discovery = Some(PriceDiscoveryResult {
            prices,
            total_fills: pipeline_result.result.fills.len(),
            total_welfare: pipeline_result.result.total_welfare,
            market_solutions: HashMap::new(),
        });
        pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
        pipeline_result.phase_times = PipelineTimings {
            price_discovery_secs: start.elapsed().as_secs_f64(),
            ..Default::default()
        };
        pipeline_result.group_minting_arb_orders = arb_orders;

        if pipeline_result.result.total_welfare < 0 {
            pipeline_result.result = MatchingResult::new();
        }

        pipeline_result
    }
}

impl Default for EgSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        bundle_yes, outcome_sell, simple_no_buy, simple_yes_buy, MarketGroup, MmConstraint, MmId,
    };

    #[test]
    fn test_eg_single_market_matches_lp() {
        // No MMs → EG should produce identical results to LP
        let mut problem = Problem::new("eg_single");
        let market = problem.markets.add_binary("market");

        problem.orders.push(outcome_sell(
            &problem.markets, 100, market, 0, 500_000_000, 1000,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets, 101, market, 1, 500_000_000, 1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_eg_minting() {
        let mut problem = Problem::new("eg_minting");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 500_000_000, 100,
        ));

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_eg_group_minting() {
        let mut problem = Problem::new("eg_group_mint");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        let m2 = problem.markets.add_binary("C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 3, m2, 300_000_000, 100));

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(result.result.total_welfare > 0);
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
        let mut problem = Problem::new("no_profit");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 300_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 300_000_000, 100,
        ));

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(result.result.orders_filled, 0, "should not fill unprofitable minting");
    }

    #[test]
    fn test_eg_mm_budget_absorption() {
        // MM with limited budget — EG should respect budget.
        // YES buyer + NO buyer (MM) pair via minting: mint costs $1,
        // recovers 60c + 50c = $1.10 → profitable.
        let mut problem = Problem::new("eg_mm_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c, 500 shares
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 500,
        ));

        // MM buying NO at 50c, 1000 shares, budget $50
        // BuyNo capital = (1 - p_yes) * qty
        let mm_order = simple_no_buy(
            &problem.markets, 200, market, 500_000_000, 1000,
        );
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR); // $50 budget
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        // Should fill something
        assert!(result.result.orders_filled > 0, "should fill some orders");

        // Check MM budget not exceeded
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 200);
        if let Some(fill) = mm_fill {
            let capital = MmSide::BuyNo.capital_needed(fill.fill_price, fill.fill_qty);
            assert!(
                capital <= 50 * NANOS_PER_DOLLAR + NANOS_PER_DOLLAR / 100, // 1% tolerance for rounding
                "MM capital {} should not exceed budget {}",
                capital,
                50 * NANOS_PER_DOLLAR
            );
        }
    }

    #[test]
    fn test_eg_zero_mm_budget() {
        let mut problem = Problem::new("eg_zero_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer + NO buyer pair via minting
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 100, market, 500_000_000, 100,
        ));

        // MM with zero budget (also wants NO)
        let mm_order = simple_no_buy(
            &problem.markets, 200, market, 500_000_000, 1000,
        );
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 0);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        // Zero-budget MM should get zero fills
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 200);
        assert!(
            mm_fill.is_none() || mm_fill.unwrap().fill_qty == 0,
            "zero-budget MM should not be filled"
        );
    }

    #[test]
    fn test_eg_bundle_orders() {
        let mut problem = Problem::new("eg_bundle");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        problem.orders.push(bundle_yes(
            &problem.markets, 1, &[market_a, market_b], 400_000_000, 100,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets, 10, market_a, 0, 150_000_000, 200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets, 11, market_b, 0, 150_000_000, 200,
        ));

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill bundle + sellers");
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_eg_multiple_mms() {
        // Two MMs with different budgets, both buying NO to pair with YES buyers via minting
        let mut problem = Problem::new("eg_multi_mm");
        let market = problem.markets.add_binary("market");

        // YES buyers
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets, 2, market, 550_000_000, 1000,
        ));

        // MM1: buys NO at 45c, budget $100
        let mm1_order = simple_no_buy(
            &problem.markets, 200, market, 450_000_000, 2000,
        );
        problem.orders.push(mm1_order);
        let mut mm1 = MmConstraint::new(MmId(1), 100 * NANOS_PER_DOLLAR);
        mm1.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm1);

        // MM2: buys NO at 50c, budget $50
        let mm2_order = simple_no_buy(
            &problem.markets, 300, market, 500_000_000, 2000,
        );
        problem.orders.push(mm2_order);
        let mut mm2 = MmConstraint::new(MmId(2), 50 * NANOS_PER_DOLLAR);
        mm2.add_order(300, MmSide::BuyNo);
        problem.mm_constraints.push(mm2);

        let solver = EgSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.orders_filled > 0);
        assert!(result.result.total_welfare > 0);
    }
}
