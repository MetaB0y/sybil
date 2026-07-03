//! Iterative LP solver: fixed-point iteration with EG μ-boosted MM weights.
//!
//! At the EG optimum, each MM k has KKT multiplier μ_k = min(1, B_k/U_k).
//! The optimal fills solve an LP with objective `w_j × (1 + μ_k)` for
//! positive-welfare MM orders, `w_j` otherwise. Since μ_k depends on the
//! solution, we iterate: solve LP → compute μ → re-solve.
//!
//! Same welfare quality as the conic solver, LP-level robustness.
//! Note: this is NOT the "Augmented LP" from `eg-conic.typ` (which is a
//! custom interior-point solver with Woodbury rank-K correction).

use std::collections::HashMap;
use std::time::Instant;

use matching_engine::{MmSide, Order, Problem};

use crate::lp_solver::{build_and_solve_lp, build_solver_context, finalize_result, order_sign};
use crate::result::PipelineResult;

/// Configuration for the iterative LP solver.
#[derive(Clone, Debug)]
pub struct IterLpConfig {
    /// Maximum μ-update iterations (default: 15).
    pub max_iterations: usize,
    /// Convergence tolerance on max |Δμ_k| (default: 1e-4).
    pub mu_tol: f64,
    /// Damping factor for μ updates (default: 0.6).
    pub damping: f64,
}

impl Default for IterLpConfig {
    fn default() -> Self {
        Self {
            max_iterations: 15,
            mu_tol: 1e-4,
            damping: 0.6,
        }
    }
}

/// Iterative LP solver: fixed-point iteration with EG μ-boosted MM weights.
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

    /// Solve a matching problem using iterative μ-boosted LPs.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let supported = crate::solver::filter_supported_problem(problem, "IterLP");
        let _rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let orders = &problem.orders;
        let n = orders.len();

        // No MMs → delegate to plain LP (fast path)
        if problem.mm_constraints.is_empty() {
            return crate::lp_solver::LpSolver::new().solve(problem);
        }

        let ctx = build_solver_context(problem);

        // Per-order MM info: order_index -> (mm_constraint_index, MmSide)
        let mm_order_map: HashMap<usize, (usize, MmSide)> = orders
            .iter()
            .enumerate()
            .filter_map(|(i, o)| ctx.mm_order_info.get(&o.id).map(|&info| (i, info)))
            .collect();

        // Per-order welfare weight: sign × limit_price
        let welfare_weights: Vec<f64> = orders
            .iter()
            .map(|o| order_sign(o) * o.limit_price as f64)
            .collect();

        let num_mm = problem.mm_constraints.len();

        // Group positive-welfare MM orders by constraint index.
        // Only positive-welfare orders participate in U_k (same convention as conic solver).
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
            .map(|mm| mm.max_capital as f64)
            .collect();

        // Identify active MMs (positive budget AND at least one positive-welfare order)
        let active_mm: Vec<bool> = (0..num_mm)
            .map(|k| mm_budgets[k] > 0.0 && !mm_pos_groups[k].is_empty())
            .collect();

        // Initialize μ_k = 0 for all k
        let mut mu: Vec<f64> = vec![0.0; num_mm];

        // Track best solution by EG objective
        let mut best_q: Option<Vec<f64>> = None;
        let mut best_eg_obj = f64::NEG_INFINITY;

        for iter in 0..self.config.max_iterations {
            // Build objective: w_j(1 + μ_k) for positive-welfare MM orders, w_j for rest
            let objective_coeffs: Vec<f64> = (0..n)
                .map(|i| {
                    if let Some(&(mm_idx, _)) = mm_order_map.get(&i) {
                        if welfare_weights[i] > 0.0 && active_mm[mm_idx] {
                            welfare_weights[i] * (1.0 + mu[mm_idx])
                        } else {
                            welfare_weights[i]
                        }
                    } else {
                        welfare_weights[i]
                    }
                })
                .collect();

            // Solve LP (no budget constraints — μ handles them)
            let Some(sol) = build_and_solve_lp(
                orders,
                &ctx.markets,
                &ctx.market_to_group,
                ctx.num_groups,
                &objective_coeffs,
                &[],
            ) else {
                break;
            };

            // Compute U_k = Σ w_j q_j over positive-welfare MM orders, floor at 1.0
            let u_k: Vec<f64> = mm_pos_groups
                .iter()
                .map(|group| {
                    let u: f64 = group
                        .iter()
                        .map(|&i| welfare_weights[i] * sol.q_values[i])
                        .sum();
                    u.max(1.0) // Floor at 1.0 nano to avoid division by zero
                })
                .collect();

            // Compute EG objective: Σ_k B_k ln(U_k) + Σ_{j∉MM} w_j q_j
            let mut eg_obj = 0.0_f64;
            for k in 0..num_mm {
                if mm_budgets[k] > 0.0 && u_k[k] > 0.0 {
                    eg_obj += mm_budgets[k] * u_k[k].ln();
                }
            }
            for (i, &w) in welfare_weights.iter().enumerate() {
                if !mm_order_map.contains_key(&i) {
                    eg_obj += w * sol.q_values[i];
                }
            }

            // Track best solution
            if eg_obj > best_eg_obj {
                best_eg_obj = eg_obj;
                best_q = Some(sol.q_values.clone());
            }

            // Update μ_k
            let mut max_delta: f64 = 0.0;
            for k in 0..num_mm {
                if !active_mm[k] {
                    continue;
                }

                // s_k = max(0, B_k - U_k): slack (over-capitalized MMs)
                let s_k = (mm_budgets[k] - u_k[k]).max(0.0);
                // μ_k_new = B_k / (U_k + s_k)
                let mu_new = mm_budgets[k] / (u_k[k] + s_k);

                // Damped update
                let mu_updated = (1.0 - self.config.damping) * mu[k] + self.config.damping * mu_new;

                max_delta = max_delta.max((mu_updated - mu[k]).abs());
                mu[k] = mu_updated;
            }

            // Check convergence
            if iter > 0 && max_delta < self.config.mu_tol {
                break;
            }
        }

        let Some(converged_q) = best_q else {
            return PipelineResult::empty();
        };

        // ================================================================
        // Projection LP: cap max_fill at converged allocation, solve
        // standard welfare LP for exact prices
        // ================================================================

        let mut projected_orders: Vec<Order> = orders.to_vec();
        for i in 0..n {
            let aug_fill = converged_q[i].round().max(0.0) as u64;
            projected_orders[i].max_fill = aug_fill.min(orders[i].max_fill);
        }

        let projection_obj: Vec<f64> = welfare_weights.clone();
        let Some(final_sol) = build_and_solve_lp(
            &projected_orders,
            &ctx.markets,
            &ctx.market_to_group,
            ctx.num_groups,
            &projection_obj,
            &[],
        ) else {
            return PipelineResult::empty();
        };

        finalize_result(&final_sol, problem, &ctx, start)
    }
}

impl Default for IterLpSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Solver for IterLpSolver {
    /// Forwards to the inherent `IterLpSolver::solve` method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        IterLpSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "IterLP"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        outcome_sell, shares_to_qty, simple_no_buy, simple_yes_buy, MarketGroup, MmConstraint,
        MmId, NANOS_PER_DOLLAR,
    };

    fn dollars(nanos: i64) -> f64 {
        nanos as f64 / NANOS_PER_DOLLAR as f64
    }

    #[test]
    fn test_aug_lp_single_market() {
        let mut problem = Problem::new("aug_lp_single");
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

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_aug_lp_minting() {
        let mut problem = Problem::new("aug_lp_minting");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 500_000_000, 100));

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_aug_lp_group_minting() {
        let mut problem = Problem::new("aug_lp_group_mint");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        let m2 = problem.markets.add_binary("C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m2, 300_000_000, 100));

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_aug_lp_empty_problem() {
        let problem = Problem::new("empty");
        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_aug_lp_no_profitable_trades() {
        let mut problem = Problem::new("no_profit");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            300_000_000,
            100,
        ));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 300_000_000, 100));

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }

    #[test]
    fn test_aug_lp_mm_budget() {
        let mut problem = Problem::new("aug_lp_mm_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c, 500 shares
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            500,
        ));

        // MM buying NO at 50c, 1000 shares, budget $50
        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.orders_filled > 0, "should fill some orders");

        // Check MM budget not exceeded
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 200);
        if let Some(fill) = mm_fill {
            let capital = MmSide::BuyNo.capital_needed(fill.fill_price, fill.fill_qty);
            assert!(
                capital <= 50 * NANOS_PER_DOLLAR + NANOS_PER_DOLLAR / 100,
                "MM capital {} should not exceed budget {}",
                capital,
                50 * NANOS_PER_DOLLAR
            );
        }
    }

    #[test]
    fn test_aug_lp_zero_budget_mm() {
        let mut problem = Problem::new("aug_lp_zero_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer + NO buyer pair via minting
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets,
            100,
            market,
            500_000_000,
            100,
        ));

        // MM with zero budget
        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 0);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        // Zero-budget MM should get zero fills
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 200);
        assert!(
            mm_fill.is_none() || mm_fill.unwrap().fill_qty == 0,
            "zero-budget MM should not be filled"
        );
    }

    #[test]
    fn test_aug_lp_multiple_mms() {
        let mut problem = Problem::new("aug_lp_multi_mm");
        let market = problem.markets.add_binary("market");

        // YES buyers
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market,
            600_000_000,
            1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market,
            550_000_000,
            1000,
        ));

        // MM1: buys NO at 45c, budget $100
        let mm1_order = simple_no_buy(&problem.markets, 200, market, 450_000_000, 2000);
        problem.orders.push(mm1_order);
        let mut mm1 = MmConstraint::new(MmId(1), 100 * NANOS_PER_DOLLAR);
        mm1.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm1);

        // MM2: buys NO at 50c, budget $50
        let mm2_order = simple_no_buy(&problem.markets, 300, market, 500_000_000, 2000);
        problem.orders.push(mm2_order);
        let mut mm2 = MmConstraint::new(MmId(2), 50 * NANOS_PER_DOLLAR);
        mm2.add_order(300, MmSide::BuyNo);
        problem.mm_constraints.push(mm2);

        let solver = IterLpSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.orders_filled > 0);
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_aug_lp_matches_lp_no_mm() {
        use crate::lp_solver::LpSolver;

        // No MMs → AugLP should produce identical results to LP
        let mut problem = Problem::new("aug_lp_vs_lp");
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
        let aug_result = IterLpSolver::new().solve(&problem);

        // Welfare should match exactly (delegates to LP)
        assert_eq!(
            lp_result.result.total_welfare, aug_result.result.total_welfare,
            "AugLP should produce identical welfare to LP when no MMs"
        );
        assert_eq!(
            lp_result.result.orders_filled, aug_result.result.orders_filled,
            "AugLP should produce identical fills to LP when no MMs"
        );
    }

    /// Demonstrate scenario where LP's SLP linearization underestimates MM capital,
    /// leading to over-allocation → aggressive trim → welfare loss.
    ///
    /// Setup: MM buys NO with tight budget. Without budget, p_yes is high (≈80-90c),
    /// so BuyNo capital/unit = (1-p_yes) ≈ 10-20c looks cheap. LP linearizes at
    /// these prices, allows many MM fills. But budget forces fewer fills, which
    /// drops p_yes toward 50c, making capital/unit ≈ 50c — 3-5x more expensive.
    /// LP's single SLP iteration can't track this price shift.
    #[test]
    fn test_auglp_vs_lp_tight_budget_price_shift() {
        use crate::lp_solver::LpSolver;

        let mut problem = Problem::new("price_shift");
        let market = problem.markets.add_binary("m");

        // Demand curve: 20 YES buyers from 90c down to 52c, 100 shares each
        // Total YES demand: 2000 shares
        for i in 0..20u64 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                900_000_000 - i * 20_000_000,
                shares_to_qty(100),
            ));
        }

        // MM buying NO at 60c, huge capacity, tiny budget ($3)
        // BuyNo capital = (1 - p_yes) * qty
        //   At p_yes=85c: capital/unit = 15c → budget $3 = 20 fills
        //   At p_yes=55c: capital/unit = 45c → budget $3 = 6 fills
        // LP linearizes at high p_yes, thinks 20 fills are fine.
        // Actual equilibrium is at lower p_yes where only ~6 fills fit.
        let mm_order = simple_no_buy(
            &problem.markets,
            200,
            market,
            600_000_000,
            shares_to_qty(50_000),
        );
        problem.orders.push(mm_order);
        let mut mm = MmConstraint::new(MmId(1), 3 * NANOS_PER_DOLLAR);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        // Retail NO sellers at moderate prices (alternative liquidity)
        for i in 0..10u64 {
            problem.orders.push(outcome_sell(
                &problem.markets,
                300 + i,
                market,
                1,
                350_000_000 + i * 15_000_000, // 35c to 48.5c
                shares_to_qty(100),
            ));
        }

        let lp_result = LpSolver::new().solve(&problem);
        let aug_result = IterLpSolver::new().solve(&problem);

        let lp_w = lp_result.result.total_welfare;
        let aug_w = aug_result.result.total_welfare;

        eprintln!("Tight budget price-shift scenario:");
        eprintln!(
            "  LP:    welfare=${:.2}, fills={}",
            dollars(lp_w),
            lp_result.result.orders_filled,
        );
        eprintln!(
            "  AugLP: welfare=${:.2}, fills={}",
            dollars(aug_w),
            aug_result.result.orders_filled,
        );
        eprintln!(
            "  Gap:   {:.1}%",
            (aug_w - lp_w) as f64 / lp_w.max(1) as f64 * 100.0
        );

        // Both should produce valid results
        assert!(lp_w > 0, "LP should find a solution");
        assert!(aug_w > 0, "AugLP should find a solution");

        // AugLP should massively outperform LP here.
        // LP's SLP linearizes BuyNo capital at high p_yes, underestimates cost,
        // then trim destroys almost all fills. AugLP iterates μ to find the
        // allocation that correctly uses retail NO sellers.
        assert!(
            aug_w > lp_w * 10,
            "AugLP (${:.2}) should far exceed LP (${:.2}) with tight budget + price shift",
            dollars(aug_w),
            dollars(lp_w),
        );
    }

    /// Multiple MMs with very different budgets competing for liquidity.
    /// LP treats them identically (same welfare weight), over-fills the
    /// low-budget MM, then trim destroys those fills. AugLP's μ-boost
    /// correctly favors the high-budget MM from the start.
    #[test]
    fn test_auglp_vs_lp_competing_mms() {
        use crate::lp_solver::LpSolver;

        let mut problem = Problem::new("competing_mms");
        let market = problem.markets.add_binary("m");

        // YES demand
        for i in 0..15u64 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                850_000_000 - i * 20_000_000, // 85c to 57c
                200,
            ));
        }

        // MM1: tiny budget ($2), buys NO at 55c
        let mm1_order = simple_no_buy(&problem.markets, 200, market, 550_000_000, 50_000);
        problem.orders.push(mm1_order);
        let mut mm1 = MmConstraint::new(MmId(1), 2 * NANOS_PER_DOLLAR);
        mm1.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm1);

        // MM2: medium budget ($20), buys NO at 50c
        let mm2_order = simple_no_buy(&problem.markets, 300, market, 500_000_000, 50_000);
        problem.orders.push(mm2_order);
        let mut mm2 = MmConstraint::new(MmId(2), 20 * NANOS_PER_DOLLAR);
        mm2.add_order(300, MmSide::BuyNo);
        problem.mm_constraints.push(mm2);

        // MM3: large budget ($200), buys NO at 45c
        let mm3_order = simple_no_buy(&problem.markets, 400, market, 450_000_000, 50_000);
        problem.orders.push(mm3_order);
        let mut mm3 = MmConstraint::new(MmId(3), 200 * NANOS_PER_DOLLAR);
        mm3.add_order(400, MmSide::BuyNo);
        problem.mm_constraints.push(mm3);

        // Retail NO sellers (fallback liquidity)
        for i in 0..5u64 {
            problem.orders.push(outcome_sell(
                &problem.markets,
                500 + i,
                market,
                1,
                300_000_000 + i * 20_000_000,
                150,
            ));
        }

        let lp_result = LpSolver::new().solve(&problem);
        let aug_result = IterLpSolver::new().solve(&problem);

        let lp_w = lp_result.result.total_welfare;
        let aug_w = aug_result.result.total_welfare;

        eprintln!("Competing MMs scenario:");
        eprintln!(
            "  LP:    welfare=${:.2}, fills={}",
            dollars(lp_w),
            lp_result.result.orders_filled,
        );
        eprintln!(
            "  AugLP: welfare=${:.2}, fills={}",
            dollars(aug_w),
            aug_result.result.orders_filled,
        );
        eprintln!(
            "  Gap:   {:.1}%",
            (aug_w - lp_w) as f64 / lp_w.max(1) as f64 * 100.0
        );

        assert!(lp_w > 0);
        assert!(aug_w > 0);
    }

    /// Multi-market scenario where MMs span markets.
    /// Budget constraint interacts with group minting, creating
    /// cross-market price distortion that LP's SLP misses.
    #[test]
    fn test_auglp_vs_lp_multi_market_mm() {
        use crate::lp_solver::LpSolver;

        let mut problem = Problem::new("multi_market_mm");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        let m2 = problem.markets.add_binary("C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        let markets = [m0, m1, m2];
        let prices = [700_000_000u64, 600_000_000, 500_000_000]; // 70c, 60c, 50c

        // YES buyers across 3 markets
        let mut oid = 1u64;
        for (mi, &m) in markets.iter().enumerate() {
            for j in 0..8u64 {
                problem.orders.push(simple_yes_buy(
                    &problem.markets,
                    oid,
                    m,
                    prices[mi] + 100_000_000 - j * 10_000_000, // spread around base
                    150,
                ));
                oid += 1;
            }
        }

        // MM1 with tight budget ($5), buys NO across all 3 markets
        let mm1_ids: Vec<u64> = (200..203).collect();
        for (mi, &m) in markets.iter().enumerate() {
            let mm_order = simple_no_buy(
                &problem.markets,
                mm1_ids[mi],
                m,
                450_000_000 + mi as u64 * 20_000_000,
                10_000,
            );
            problem.orders.push(mm_order);
        }
        let mut mm1 = MmConstraint::new(MmId(1), 5 * NANOS_PER_DOLLAR);
        for &id in &mm1_ids {
            mm1.add_order(id, MmSide::BuyNo);
        }
        problem.mm_constraints.push(mm1);

        // MM2 with larger budget ($80), also buys NO across markets
        let mm2_ids: Vec<u64> = (300..303).collect();
        for (mi, &m) in markets.iter().enumerate() {
            let mm_order = simple_no_buy(
                &problem.markets,
                mm2_ids[mi],
                m,
                400_000_000 + mi as u64 * 15_000_000,
                10_000,
            );
            problem.orders.push(mm_order);
        }
        let mut mm2 = MmConstraint::new(MmId(2), 80 * NANOS_PER_DOLLAR);
        for &id in &mm2_ids {
            mm2.add_order(id, MmSide::BuyNo);
        }
        problem.mm_constraints.push(mm2);

        let lp_result = LpSolver::new().solve(&problem);
        let aug_result = IterLpSolver::new().solve(&problem);

        let lp_w = lp_result.result.total_welfare;
        let aug_w = aug_result.result.total_welfare;

        eprintln!("Multi-market MM scenario:");
        eprintln!(
            "  LP:    welfare=${:.2}, fills={}",
            dollars(lp_w),
            lp_result.result.orders_filled,
        );
        eprintln!(
            "  AugLP: welfare=${:.2}, fills={}",
            dollars(aug_w),
            aug_result.result.orders_filled,
        );
        eprintln!(
            "  Gap:   {:.1}%",
            (aug_w - lp_w) as f64 / lp_w.max(1) as f64 * 100.0
        );

        assert!(lp_w > 0);
        assert!(aug_w > 0);
    }

    /// Sweep MM budget and compare LP vs AugLP welfare.
    /// At extreme budget constraints, LP's linearization error peaks.
    #[test]
    fn test_auglp_budget_sweep() {
        use crate::lp_solver::LpSolver;

        let budgets_dollars = [1u64, 2, 5, 10, 25, 50, 100, 250, 500, 1000];

        eprintln!("\nBudget sweep (LP vs AugLP):");
        eprintln!(
            "{:>10} {:>12} {:>12} {:>8} {:>8}",
            "Budget", "LP", "AugLP", "Gap%", "Winner"
        );

        for &budget in &budgets_dollars {
            let mut problem = Problem::new("sweep");
            let market = problem.markets.add_binary("m");

            // YES demand: 30 buyers, 90c down to 52c
            for i in 0..30u64 {
                problem.orders.push(simple_yes_buy(
                    &problem.markets,
                    i + 1,
                    market,
                    900_000_000 - i * 14_000_000,
                    100,
                ));
            }

            // MM buying NO at 55c, huge capacity, variable budget
            let mm_order = simple_no_buy(&problem.markets, 200, market, 550_000_000, 100_000);
            problem.orders.push(mm_order);
            let mut mm = MmConstraint::new(MmId(1), budget * NANOS_PER_DOLLAR);
            mm.add_order(200, MmSide::BuyNo);
            problem.mm_constraints.push(mm);

            // Retail NO sellers
            for i in 0..8u64 {
                problem.orders.push(outcome_sell(
                    &problem.markets,
                    300 + i,
                    market,
                    1,
                    300_000_000 + i * 20_000_000,
                    200,
                ));
            }

            let lp_w = LpSolver::new().solve(&problem).result.total_welfare;
            let aug_w = IterLpSolver::new().solve(&problem).result.total_welfare;
            let gap = if lp_w > 0 {
                (aug_w - lp_w) as f64 / lp_w as f64 * 100.0
            } else if aug_w > 0 {
                100.0
            } else {
                0.0
            };
            let winner = if aug_w > lp_w {
                "AugLP"
            } else if lp_w > aug_w {
                "LP"
            } else {
                "Tie"
            };

            eprintln!(
                "${:>9} ${:>10.2} ${:>10.2} {:>+7.1}% {:>8}",
                budget,
                dollars(lp_w),
                dollars(aug_w),
                gap,
                winner,
            );
        }
    }
}
