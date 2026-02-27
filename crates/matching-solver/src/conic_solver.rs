//! Conic EG solver using Clarabel.rs interior-point solver.
//!
//! Solves the Eisenberg-Gale convex program directly via exponential cones,
//! replacing the Frank-Wolfe iterative approach in `eg_solver.rs` with a
//! single interior-point solve.
//!
//! For each MM with positive budget B_k, models `t_k ≤ ln(V_k)` using a
//! 3-dimensional exponential cone constraint where `V_k = Σ L_i q_i + s_k`
//! (utility + retained cash). Everything else is linear.
//!
//! The full program:
//!   max  Σ_k [B_k · ln(V_k) − s_k] + Σ_{j∉MM} w_j q_j − minting_cost
//! which Clarabel solves as a minimization after negation.

use std::collections::HashMap;
use std::time::Instant;

use clarabel::algebra::*;
use clarabel::solver::*;

use matching_engine::{MarketId, MmSide, Order, Problem, NANOS_PER_DOLLAR};

use crate::coefficients::{order_sign, precompute_coefficients, OrderCoefficients};
use crate::lp_solver::{
    build_and_solve_lp, collect_markets, create_position_arbs, extract_result, recompute_welfare,
    trim_mm_budget_overflows,
};
use crate::result::{PipelineResult, PipelineTimings, PriceDiscoveryResult};
use crate::MatchingResult;

/// Configuration for the conic EG solver.
#[derive(Clone, Debug)]
pub struct ConicConfig {
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
            max_iter: 200,
            tol: 1e-8,
            verbose: false,
            time_limit: 30.0,
        }
    }
}

/// Conic EG solver: one Clarabel interior-point solve instead of 25 LP oracle calls.
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
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let orders = &problem.orders;
        let n = orders.len();

        // Precompute per-order coefficients (c_yes, c_no, alpha, beta)
        let coeffs: Vec<OrderCoefficients> = orders
            .iter()
            .map(|o| precompute_coefficients(o))
            .collect();

        let markets = collect_markets(orders);
        let num_markets = markets.len();

        // Map market -> group index
        let market_to_group: HashMap<MarketId, usize> = problem
            .market_groups
            .iter()
            .enumerate()
            .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
            .collect();
        let num_groups = problem.market_groups.len();

        // MM order info: order_id -> (mm_constraint_index, MmSide)
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
            .filter_map(|(i, o)| mm_order_info_by_id.get(&o.id).map(|&info| (i, info)))
            .collect();

        // Per-order welfare weight: sign × limit_price
        let welfare_weights: Vec<f64> = orders
            .iter()
            .map(|o| order_sign(o) * o.limit_price as f64)
            .collect();

        // Active MMs: only those with positive budget get exp cone constraints
        let active_mms: Vec<usize> = (0..problem.mm_constraints.len())
            .filter(|&k| problem.mm_constraints[k].max_capital > 0)
            .collect();
        let num_active_mm = active_mms.len();

        // Map global MM index -> local (active) index
        let active_mm_to_local: HashMap<usize, usize> = active_mms
            .iter()
            .enumerate()
            .map(|(local, &global)| (global, local))
            .collect();

        // Group MM orders by active MM local index.
        // Only orders with POSITIVE welfare weights participate in V_k (exp cone).
        // Negative-weight orders (sellers) are treated as retail (linear welfare).
        // This ensures V_k > 0, which the exp cone requires.
        let mut mm_groups: Vec<Vec<usize>> = vec![Vec::new(); num_active_mm];
        for (&order_idx, &(mm_idx, _)) in &mm_order_map {
            if let Some(&local) = active_mm_to_local.get(&mm_idx) {
                if welfare_weights[order_idx] > 0.0 {
                    mm_groups[local].push(order_idx);
                }
                // Negative-weight orders treated as retail (linear welfare in objective)
            }
        }

        // MM budgets for active MMs
        let mm_budgets: Vec<f64> = active_mms
            .iter()
            .map(|&k| problem.mm_constraints[k].max_capital as f64)
            .collect();

        // ================================================================
        // Variable layout
        // ================================================================
        //
        // x = [q_0..q_{n-1}, s_0..s_{K-1}, t_0..t_{K-1}, mint_0..mint_{M-1}, gmint_0..gmint_{G-1}]
        //
        // q_i:     fill quantities          (n vars)
        // s_k:     retained cash per MM     (K vars, ≥ 0)
        // t_k:     log-utility epigraph     (K vars, free)
        // mint_m:  per-market minting       (M vars, free)
        // gmint_g: group minting            (G vars, ≥ 0)

        // Filter out MMs with no positive-weight orders (they get no exp cone)
        let cone_mms: Vec<usize> = (0..num_active_mm)
            .filter(|&kk| !mm_groups[kk].is_empty())
            .collect();
        let k = cone_mms.len(); // number of MMs that actually get exp cone constraints
        let m = num_markets;
        let g = num_groups;
        let d = n + 2 * k + m + g;

        // Remap cone_mms index to get budgets/groups for exp-cone MMs only
        let cone_mm_budgets: Vec<f64> = cone_mms.iter().map(|&kk| mm_budgets[kk]).collect();
        let cone_mm_groups: Vec<&Vec<usize>> = cone_mms.iter().map(|&kk| &mm_groups[kk]).collect();

        let q_offset = 0;
        let s_offset = n;
        let t_offset = n + k;
        let mint_offset = n + 2 * k;
        let gmint_offset = n + 2 * k + m;

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
        // After solving, we multiply ZeroCone duals by NANOS to recover
        // clearing prices in nanos.

        let nanos_f = NANOS_PER_DOLLAR as f64;

        // ================================================================
        // Objective (Clarabel minimizes, scaled by 1/NANOS)
        // ================================================================
        //
        // Variable substitution: t_k' = α_k · t_k where α_k = B_k/NANOS.
        // This makes the objective coefficient on t_k' equal to -1 instead
        // of -α_k, eliminating the ~1e4 ratio between budget and welfare
        // coefficients that kills interior-point conditioning.
        //
        // min: -Σ t_k' + Σ s_k' - Σ (w_j/S) q_j + Σ mint_m + Σ gmint_g

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
                // Positive-weight MM orders: welfare captured via B_k * ln(V_k)
                obj[q_offset + i] = 0.0;
            } else {
                // Retail orders AND negative-weight MM orders: linear welfare
                obj[q_offset + i] = -welfare_weights[i] / nanos_f;
            }
        }
        for kk in 0..k {
            obj[s_offset + kk] = 1.0; // s_k' in dollars
            obj[t_offset + kk] = -1.0; // t_k' = α_k · t_k, so coeff is -1
        }
        for mm in 0..m {
            obj[mint_offset + mm] = 1.0; // $1 per mint
        }
        for gg in 0..g {
            obj[gmint_offset + gg] = 1.0; // $1 per group mint
        }

        // P: all zeros (no quadratic term)
        let p_mat = CscMatrix::zeros((d, d));

        // ================================================================
        // Constraints: Ax + s_cone = b, s_cone ∈ K
        // ================================================================

        let num_exp_rows = 3 * k;
        let num_balance_rows = 2 * m;
        let num_bound_rows = 2 * n + k + g;
        let total_rows = num_exp_rows + num_balance_rows + num_bound_rows;

        // Build A in COO (triplet) format
        let mut tri_row: Vec<usize> = Vec::new();
        let mut tri_col: Vec<usize> = Vec::new();
        let mut tri_val: Vec<f64> = Vec::new();
        let mut b_vec = vec![0.0_f64; total_rows];

        // --- Block 1: Exponential cones (3 rows per active MM) ---
        //
        // After substitution t_k' = α_k · t_k, the exp cone models:
        //   exp(t_k'/α_k) ≤ V_k/NANOS  ⟺  t_k'/α_k ≤ ln(V_k/NANOS)
        //
        // Slack variables: (s₁, s₂, s₃) ∈ K_exp with s₂·exp(s₁/s₂) ≤ s₃
        //   Row 0: s₁ = t_k'/α_k       (A has -1/α_k at t_k', b = 0)
        //   Row 1: s₂ = 1              (A all zeros, b = 1)
        //   Row 2: s₃ = V_k/NANOS      (same as before)

        for kk in 0..k {
            let row_base = 3 * kk;

            // Row 0: slack = t_k'/α_k
            tri_row.push(row_base);
            tri_col.push(t_offset + kk);
            tri_val.push(-1.0 / alpha_k[kk]);

            // Row 1: slack = 1
            b_vec[row_base + 1] = 1.0;

            // Row 2: slack = V_k/NANOS (only positive-weight orders)
            for &order_idx in cone_mm_groups[kk] {
                tri_row.push(row_base + 2);
                tri_col.push(q_offset + order_idx);
                tri_val.push(-welfare_weights[order_idx] / nanos_f);
            }
            tri_row.push(row_base + 2);
            tri_col.push(s_offset + kk);
            tri_val.push(-1.0); // s_k' already in dollars
        }

        // --- Block 2: Position balance (ZeroCone, 2 rows per market) ---
        //
        // YES: Σ c_yes_i q_i - mint_m - gmint_g = 0
        // NO:  Σ c_no_i q_i  - mint_m            = 0

        let balance_base = num_exp_rows;
        for (m_idx, &market) in markets.iter().enumerate() {
            let yes_row = balance_base + 2 * m_idx;
            let no_row = balance_base + 2 * m_idx + 1;

            // YES balance
            for i in 0..n {
                if let Some(&c_y) = coeffs[i].c_yes.get(&market) {
                    if c_y.abs() > 1e-12 {
                        tri_row.push(yes_row);
                        tri_col.push(q_offset + i);
                        tri_val.push(c_y);
                    }
                }
            }
            tri_row.push(yes_row);
            tri_col.push(mint_offset + m_idx);
            tri_val.push(-1.0);
            if let Some(&g_idx) = market_to_group.get(&market) {
                tri_row.push(yes_row);
                tri_col.push(gmint_offset + g_idx);
                tri_val.push(-1.0);
            }

            // NO balance
            for i in 0..n {
                if let Some(&c_n) = coeffs[i].c_no.get(&market) {
                    if c_n.abs() > 1e-12 {
                        tri_row.push(no_row);
                        tri_col.push(q_offset + i);
                        tri_val.push(c_n);
                    }
                }
            }
            tri_row.push(no_row);
            tri_col.push(mint_offset + m_idx);
            tri_val.push(-1.0);
        }

        // --- Block 3: Variable bounds (NonnegativeCone) ---
        //
        // q_i ≤ max_fill, q_i ≥ 0, s_k' ≥ 0, gmint_g ≥ 0

        let bound_base = num_exp_rows + num_balance_rows;
        let mut bound_row = bound_base;

        // q_i ≤ max_fill → slack = max_fill - q_i ≥ 0
        for i in 0..n {
            tri_row.push(bound_row);
            tri_col.push(q_offset + i);
            tri_val.push(1.0);
            b_vec[bound_row] = orders[i].max_fill as f64;
            bound_row += 1;
        }

        // q_i ≥ 0 → slack = q_i ≥ 0
        for i in 0..n {
            tri_row.push(bound_row);
            tri_col.push(q_offset + i);
            tri_val.push(-1.0);
            bound_row += 1;
        }

        // s_k' ≥ 0
        for kk in 0..k {
            tri_row.push(bound_row);
            tri_col.push(s_offset + kk);
            tri_val.push(-1.0);
            bound_row += 1;
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
        if num_balance_rows > 0 {
            cones.push(ZeroConeT(num_balance_rows));
        }
        if num_bound_rows > 0 {
            cones.push(NonnegativeConeT(num_bound_rows));
        }

        // Build sparse A matrix from triplets
        let a_mat =
            CscMatrix::new_from_triplets(total_rows, d, tri_row, tri_col, tri_val);

        // ================================================================
        // Solve
        // ================================================================

        let settings = DefaultSettings {
            verbose: self.config.verbose,
            max_iter: self.config.max_iter,
            time_limit: self.config.time_limit,
            tol_gap_abs: 1e-6,
            tol_gap_rel: 1e-6,
            tol_feas: 1e-6,
            static_regularization_constant: 1e-6,
            dynamic_regularization_delta: 2e-5,
            equilibrate_max_scaling: 1e6,
            equilibrate_max_iter: 20,
            ..DefaultSettings::default()
        };

        let Ok(mut solver) =
            DefaultSolver::new(&p_mat, &obj, &a_mat, &b_vec, &cones, settings)
        else {
            return PipelineResult::empty();
        };

        solver.solve();

        match solver.solution.status {
            SolverStatus::Solved | SolverStatus::AlmostSolved => {}
            _ => return PipelineResult::empty(),
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
        // slackness guarantees UCP. Position balance holds from the LP
        // constraints. Budget absorption holds from the EG structure.

        let q_values: Vec<f64> = (0..n).map(|i| x[q_offset + i].max(0.0)).collect();

        let mut projected_orders: Vec<Order> = orders.to_vec();
        for i in 0..n {
            let conic_fill = q_values[i].round().max(0.0) as u64;
            projected_orders[i].max_fill = conic_fill.min(orders[i].max_fill);
        }

        let projection_obj: Vec<f64> = welfare_weights.clone();
        let Some(final_sol) = build_and_solve_lp(
            &projected_orders,
            &coeffs,
            &markets,
            &market_to_group,
            num_groups,
            &projection_obj,
            &[],
        ) else {
            return PipelineResult::empty();
        };

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let (mut result, prices) = extract_result(&final_sol, orders, &coeffs, &markets);

        // Budget trim: integer rounding breaks KKT budget absorption.
        if !problem.mm_constraints.is_empty() {
            trim_mm_budget_overflows(
                &mut result,
                &problem.mm_constraints,
                &mm_order_info_by_id,
            );
        }

        // Arb orders record the minting operations (mint variables from the LP).
        let max_order_id = orders.iter().map(|o| o.id).max().unwrap_or(0);
        let arb_orders = create_position_arbs(&mut result, &order_map, &prices, max_order_id);

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
        });
        pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
        pipeline_result.phase_times = PipelineTimings {
            price_discovery_secs: start.elapsed().as_secs_f64(),
            ..Default::default()
        };
        pipeline_result.group_minting_arb_orders = arb_orders;

        // Gate: negative welfare → return empty
        if pipeline_result.result.total_welfare < 0 {
            pipeline_result.result = MatchingResult::new();
        }

        pipeline_result
    }
}

impl Default for ConicSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        outcome_sell, simple_no_buy, simple_yes_buy, MarketGroup, MmConstraint, MmId,
        NANOS_PER_DOLLAR,
    };

    #[test]
    fn test_conic_single_market() {
        let mut problem = Problem::new("conic_single");
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

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_conic_minting() {
        let mut problem = Problem::new("conic_minting");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 500_000_000, 100,
        ));

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(
            result.result.orders_filled, 2,
            "both orders should fill via minting"
        );
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_conic_group_minting() {
        let mut problem = Problem::new("conic_group_mint");
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

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_conic_empty_problem() {
        let problem = Problem::new("empty");
        let solver = ConicSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_conic_no_profitable_trades() {
        let mut problem = Problem::new("no_profit");
        let market = problem.markets.add_binary("market");

        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 300_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 300_000_000, 100,
        ));

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        assert_eq!(result.result.orders_filled, 0, "should not fill unprofitable minting");
    }

    #[test]
    fn test_conic_mm_budget() {
        let mut problem = Problem::new("conic_mm_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c, 500 shares
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 500,
        ));

        // MM buying NO at 50c, 1000 shares, budget $50
        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = ConicSolver::new();
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
    fn test_conic_multiple_mms() {
        let mut problem = Problem::new("conic_multi_mm");
        let market = problem.markets.add_binary("market");

        // YES buyers
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 1000,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets, 2, market, 550_000_000, 1000,
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

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.orders_filled > 0);
        assert!(result.result.total_welfare > 0);
    }

    #[test]
    fn test_conic_zero_budget_mm() {
        let mut problem = Problem::new("conic_zero_budget");
        let market = problem.markets.add_binary("market");

        // YES buyer + NO buyer pair via minting
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));
        problem.orders.push(simple_no_buy(
            &problem.markets, 100, market, 500_000_000, 100,
        ));

        // MM with zero budget
        let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
        problem.orders.push(mm_order);

        let mut mm = MmConstraint::new(MmId(1), 0);
        mm.add_order(200, MmSide::BuyNo);
        problem.mm_constraints.push(mm);

        let solver = ConicSolver::new();
        let result = solver.solve(&problem);

        // Zero-budget MM should get zero fills
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 200);
        assert!(
            mm_fill.is_none() || mm_fill.unwrap().fill_qty == 0,
            "zero-budget MM should not be filled"
        );
    }

    #[cfg(feature = "lp")]
    #[test]
    fn test_conic_matches_lp_no_mm() {
        use crate::lp_solver::LpSolver;

        // No MMs → conic should produce identical results to LP
        let mut problem = Problem::new("conic_vs_lp");
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
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 400_000_000, 50,
        ));

        let lp_result = LpSolver::new().solve(&problem);
        let conic_result = ConicSolver::new().solve(&problem);

        // Welfare should match (within rounding tolerance)
        let welfare_diff = (lp_result.result.total_welfare - conic_result.result.total_welfare).abs();
        assert!(
            welfare_diff <= 2 * NANOS_PER_DOLLAR as i64,
            "welfare should match: LP={}, Conic={}, diff={}",
            lp_result.result.total_welfare,
            conic_result.result.total_welfare,
            welfare_diff
        );

        // Both should produce fills
        assert!(
            conic_result.result.orders_filled > 0,
            "conic should produce fills"
        );
    }
}
