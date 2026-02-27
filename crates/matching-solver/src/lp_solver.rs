//! LP-based solver for prediction market matching.
//!
//! Formulates the welfare-maximizing matching problem as a Linear Program:
//! - Variables: fill quantities, per-market minting, group minting
//! - Constraints: YES/NO position balance per market, quantity bounds
//! - Objective: maximize total welfare (limit_price × quantity for buyers, minus for sellers)
//!   minus minting cost ($1 per mint)
//!
//! Prices emerge from LP duality: the dual of the YES balance constraint for market m
//! gives p_YES_m, and the dual of the NO constraint gives p_NO_m. When minting is active,
//! p_YES + p_NO = $1 automatically. When group minting is active, Σ p_YES = $1.
//!
//! MM budget constraints (bilinear: price × quantity) are handled iteratively by
//! re-solving the LP with tightened order limits until budgets are satisfied.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use highs::{HighsModelStatus, RowProblem, Sense};

use matching_engine::{Fill, MarketId, MmSide, Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR};

use crate::coefficients::{order_sign, precompute_coefficients, OrderCoefficients};
use crate::result::{PipelineResult, PipelineTimings, PriceDiscoveryResult};
use crate::MatchingResult;

/// Configuration for the LP solver.
#[derive(Clone, Debug)]
pub struct LpConfig {
    /// Max SLP iterations for MM budget linearization (0 = LP only, no MM handling).
    pub max_mm_iterations: usize,
}

impl Default for LpConfig {
    fn default() -> Self {
        Self {
            max_mm_iterations: 1,
        }
    }
}

/// LP-based solver that handles the convex core exactly via HiGHS,
/// then uses SLP (sequential LP) for MM budget constraints.
pub struct LpSolver {
    config: LpConfig,
}

impl LpSolver {
    pub fn new() -> Self {
        Self {
            config: LpConfig::default(),
        }
    }

    pub fn with_config(config: LpConfig) -> Self {
        Self { config }
    }

    /// Solve a matching problem using LP + SLP for MM budgets.
    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        // Precompute coefficients for all orders
        let coeffs: Vec<OrderCoefficients> = problem
            .orders
            .iter()
            .map(|o| precompute_coefficients(o))
            .collect();

        // Collect all markets
        let markets = collect_markets(&problem.orders);

        // Build market -> group index mapping
        let market_to_group: HashMap<MarketId, usize> = problem
            .market_groups
            .iter()
            .enumerate()
            .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
            .collect();

        // Build MM order info: order_id -> (mm_constraint_index, MmSide)
        let mm_order_info: HashMap<u64, (usize, MmSide)> = problem
            .mm_constraints
            .iter()
            .enumerate()
            .flat_map(|(mm_idx, mm)| {
                mm.order_ids.iter().filter_map(move |&oid| {
                    mm.order_sides.get(&oid).map(|&side| (oid, (mm_idx, side)))
                })
            })
            .collect();

        // Pre-group MM orders by constraint for efficient iteration
        let mm_constraint_orders: Vec<Vec<(usize, MmSide)>> = {
            let mut by_mm = vec![Vec::new(); problem.mm_constraints.len()];
            for (i, order) in problem.orders.iter().enumerate() {
                if let Some(&(mm_idx, side)) = mm_order_info.get(&order.id) {
                    by_mm[mm_idx].push((i, side));
                }
            }
            by_mm
        };

        // Sequential LP: solve without budgets, then add linearized budget
        // constraints and re-solve until budgets are satisfied.
        let mut budget_rows: Vec<(Vec<(usize, f64)>, f64)> = Vec::new();
        let mut best_solution: Option<LpSolution> = None;

        for slp_iter in 0..=self.config.max_mm_iterations {
            let solution = self.solve_lp(
                &problem.orders,
                &coeffs,
                &markets,
                &market_to_group,
                problem.market_groups.len(),
                &budget_rows,
            );

            let Some(sol) = solution else {
                break;
            };
            // No MM constraints or final iteration → keep solution and stop
            if problem.mm_constraints.is_empty() || slp_iter == self.config.max_mm_iterations {
                best_solution = Some(sol);
                break;
            }

            // Check MM budget violations at current prices
            let prices = normalized_yes_prices(&sol, &markets);
            let violated = has_mm_budget_violations(
                &sol,
                &problem.orders,
                &problem.mm_constraints,
                &mm_constraint_orders,
                &prices,
            );

            if !violated {
                best_solution = Some(sol);
                break;
            }

            // Linearize budget constraints at current prices and re-solve.
            // For each MM: Σ capital_per_unit_i(p) × q_i ≤ Budget_k
            budget_rows = linearize_mm_budgets(
                &problem.orders,
                &problem.mm_constraints,
                &mm_constraint_orders,
                &prices,
            );

            best_solution = Some(sol);
        }

        let Some(solution) = best_solution else {
            return PipelineResult::empty();
        };

        // The LP guarantees UCP via duality (complementary slackness):
        //   - filled orders have non-negative surplus (fill_price ≤ limit_price)
        //   - prices satisfy p_YES + p_NO = $1 (from mint variable stationarity)
        //   - group prices satisfy Σp ≤ $1 (from gmint variables)
        // No need for enforce_ucp — it was designed for the old decomposition pipeline.
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        let (mut result, prices) = extract_result(
            &solution,
            &problem.orders,
            &coeffs,
            &markets,
        );

        // Trim any tiny MM budget overflows from integer rounding.
        if !problem.mm_constraints.is_empty() {
            trim_mm_budget_overflows(
                &mut result,
                &problem.mm_constraints,
                &mm_order_info,
            );
        }

        // Create arb orders AFTER all post-processing (including MM trim)
        // so position balance accounts for the final fill set.
        let max_order_id = problem.orders.iter().map(|o| o.id).max().unwrap_or(0);
        let arb_orders = create_position_arbs(&mut result, &order_map, &prices, max_order_id);

        // Recompute welfare from scratch — incremental tracking is error-prone
        // after trim + arb modifications. Arb fills contribute 0 welfare
        // (limit == price), so minting_cost = 0 is consistent.
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

        // Gate: if total welfare is negative, return empty
        if pipeline_result.result.total_welfare < 0 {
            pipeline_result.result = MatchingResult::new();
        }

        pipeline_result
    }

    /// Build and solve the core LP using HiGHS.
    ///
    /// Returns the raw LP solution (primal + dual values) or None if infeasible.
    /// `budget_rows` contains linearized MM budget constraints: each entry is
    /// (terms: [(order_index, capital_per_unit)], budget_nanos_f64).
    fn solve_lp(
        &self,
        orders: &[Order],
        coeffs: &[OrderCoefficients],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
        budget_rows: &[(Vec<(usize, f64)>, f64)],
    ) -> Option<LpSolution> {
        // Default welfare objective: sign_i * limit_price_i
        let objective_coeffs: Vec<f64> = orders
            .iter()
            .map(|o| order_sign(o) * o.limit_price as f64)
            .collect();
        build_and_solve_lp(
            orders,
            coeffs,
            markets,
            market_to_group,
            num_groups,
            &objective_coeffs,
            budget_rows,
        )
    }
}

impl Default for LpSolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw solution from the LP solver: primal fill quantities + dual prices.
pub(crate) struct LpSolution {
    pub(crate) q_values: Vec<f64>,
    pub(crate) dual_yes: HashMap<MarketId, f64>,
    pub(crate) dual_no: HashMap<MarketId, f64>,
}

/// Build and solve an LP with custom objective coefficients.
///
/// This is the LP oracle used by both the LP solver (linear welfare) and
/// the EG solver (Frank-Wolfe gradient). The constraints (position balance,
/// quantity bounds, minting) are the same; only the objective varies.
///
/// `objective_coeffs[i]` is the objective coefficient for order i's fill variable.
/// `budget_rows` contains linearized MM budget constraints (empty for EG solver).
pub(crate) fn build_and_solve_lp(
    orders: &[Order],
    coeffs: &[OrderCoefficients],
    markets: &[MarketId],
    market_to_group: &HashMap<MarketId, usize>,
    num_groups: usize,
    objective_coeffs: &[f64],
    budget_rows: &[(Vec<(usize, f64)>, f64)],
) -> Option<LpSolution> {
    let n = orders.len();
    let nanos_f = NANOS_PER_DOLLAR as f64;

    let mut pb = RowProblem::default();

    // ================================================================
    // Variables
    // ================================================================

    // q_i: fill quantity for order i
    let q_cols: Vec<_> = (0..n)
        .map(|i| {
            let ub = orders[i].max_fill as f64;
            pb.add_column(objective_coeffs[i], 0.0..=ub)
        })
        .collect();

    // mint_m: per-market minting (free variable, can be negative for burning)
    // Objective: -NANOS_PER_DOLLAR (minting costs $1 per unit)
    //
    // YES balance: Σ c_yes_i * q_i = mint_m + gmint_g
    // NO balance:  Σ c_no_i * q_i = mint_m
    //
    // mint_m is free (positive=minting, negative=burning).
    let big = 1e15_f64;
    let mint_cols: HashMap<MarketId, _> = markets
        .iter()
        .map(|&m| {
            let col = pb.add_column(-nanos_f, -big..=big);
            (m, col)
        })
        .collect();

    // gmint_g: group-level minting for each market group
    // Objective: -NANOS_PER_DOLLAR per unit
    // gmint_g >= 0 (group minting only, no group burning)
    let gmint_cols: Vec<_> = (0..num_groups)
        .map(|_| pb.add_column(-nanos_f, 0.0..))
        .collect();

    // ================================================================
    // Constraints: YES and NO balance per market
    // ================================================================
    //
    // For each market m:
    //   YES: Σ_i c_yes_i_m * q_i - mint_m - gmint_g(m) = 0
    //   NO:  Σ_i c_no_i_m  * q_i - mint_m = 0

    // Track row indices for dual extraction
    let mut yes_row_indices: HashMap<MarketId, usize> = HashMap::new();
    let mut no_row_indices: HashMap<MarketId, usize> = HashMap::new();
    let mut row_count = 0usize;

    for &market in markets {
        // YES balance
        let mut yes_terms: Vec<(highs::Col, f64)> = Vec::new();
        for i in 0..n {
            if let Some(&c_y) = coeffs[i].c_yes.get(&market) {
                if c_y.abs() > 1e-12 {
                    yes_terms.push((q_cols[i], c_y));
                }
            }
        }
        if let Some(&mint_col) = mint_cols.get(&market) {
            yes_terms.push((mint_col, -1.0));
        }
        if let Some(&g_idx) = market_to_group.get(&market) {
            yes_terms.push((gmint_cols[g_idx], -1.0));
        }
        pb.add_row(0.0..=0.0, &yes_terms);
        yes_row_indices.insert(market, row_count);
        row_count += 1;

        // NO balance
        let mut no_terms: Vec<(highs::Col, f64)> = Vec::new();
        for i in 0..n {
            if let Some(&c_n) = coeffs[i].c_no.get(&market) {
                if c_n.abs() > 1e-12 {
                    no_terms.push((q_cols[i], c_n));
                }
            }
        }
        if let Some(&mint_col) = mint_cols.get(&market) {
            no_terms.push((mint_col, -1.0));
        }
        pb.add_row(0.0..=0.0, &no_terms);
        no_row_indices.insert(market, row_count);
        row_count += 1;
    }

    // ================================================================
    // Linearized MM budget constraints
    // ================================================================
    // Σ capital_per_unit_i × q_i ≤ Budget_k
    // where capital_per_unit_i is computed at previous iteration's prices.

    for (terms, budget) in budget_rows {
        let row_terms: Vec<(highs::Col, f64)> = terms
            .iter()
            .map(|&(order_idx, coeff)| (q_cols[order_idx], coeff))
            .collect();
        pb.add_row(..=*budget, &row_terms);
    }

    // ================================================================
    // Solve
    // ================================================================

    let mut model = pb.optimise(Sense::Maximise);
    model.make_quiet();

    let solved = model.solve();

    match solved.status() {
        HighsModelStatus::Optimal | HighsModelStatus::ObjectiveBound => {}
        HighsModelStatus::Infeasible => return None,
        _ => return None,
    }

    let solution = solved.get_solution();
    let primal = solution.columns();
    let dual_rows = solution.dual_rows();

    let q_values: Vec<f64> = (0..n).map(|i| primal[i]).collect();

    // Extract dual values (prices)
    let mut dual_yes: HashMap<MarketId, f64> = HashMap::new();
    let mut dual_no: HashMap<MarketId, f64> = HashMap::new();

    for &market in markets {
        if let Some(&row_idx) = yes_row_indices.get(&market) {
            dual_yes.insert(market, dual_rows[row_idx]);
        }
        if let Some(&row_idx) = no_row_indices.get(&market) {
            dual_no.insert(market, dual_rows[row_idx]);
        }
    }

    Some(LpSolution {
        q_values,
        dual_yes,
        dual_no,
    })
}

/// Derive normalized YES clearing prices from LP dual variables.
///
/// For each market, takes |dual_yes| and |dual_no|, normalizes so they sum to $1.
/// Returns p_YES per market (in nanos). p_NO = NANOS_PER_DOLLAR - p_YES.
pub(crate) fn normalized_yes_prices(
    solution: &LpSolution,
    markets: &[MarketId],
) -> HashMap<MarketId, Nanos> {
    let nanos_f = NANOS_PER_DOLLAR as f64;
    let mut prices = HashMap::new();

    for &market in markets {
        let dual_y = solution.dual_yes.get(&market).copied().unwrap_or(0.0);
        let dual_n = solution.dual_no.get(&market).copied().unwrap_or(0.0);

        let p_yes_raw = dual_y.abs().round().clamp(0.0, nanos_f) as Nanos;
        let p_no_raw = dual_n.abs().round().clamp(0.0, nanos_f) as Nanos;

        let sum = p_yes_raw + p_no_raw;
        let p_yes = if sum > 0 {
            let scale = nanos_f / sum as f64;
            (p_yes_raw as f64 * scale).round() as Nanos
        } else {
            // No price signal — use 50/50 as neutral default.
            // Only happens when no orders touch the market.
            NANOS_PER_DOLLAR / 2
        };

        prices.insert(market, p_yes);
    }

    prices
}

/// Collect all unique markets from active orders.
pub(crate) fn collect_markets(orders: &[Order]) -> Vec<MarketId> {
    let mut seen = HashSet::new();
    orders
        .iter()
        .flat_map(|o| &o.markets[..o.num_markets as usize])
        .filter(|m| !m.is_none() && seen.insert(**m))
        .copied()
        .collect()
}

/// Extract fills and clearing prices from the LP solution.
///
/// Rounds continuous q_i to integer fills, derives clearing prices from duals.
/// Does NOT create arb orders — those are added after all post-processing
/// (MM budget trim) so position balance accounts for final fills.
pub(crate) fn extract_result(
    solution: &LpSolution,
    orders: &[Order],
    coeffs: &[OrderCoefficients],
    markets: &[MarketId],
) -> (MatchingResult, HashMap<MarketId, Vec<Nanos>>) {
    let mut result = MatchingResult::new();

    // Derive clearing prices from LP duals (YES and NO per market)
    let yes_prices = normalized_yes_prices(solution, markets);
    let clearing_prices: HashMap<MarketId, Vec<Nanos>> = yes_prices
        .iter()
        .map(|(&m, &p_yes)| (m, vec![p_yes, NANOS_PER_DOLLAR.saturating_sub(p_yes)]))
        .collect();

    // Extract fills from primal solution
    for (i, order) in orders.iter().enumerate() {
        let q_val = solution.q_values[i];
        if q_val < 0.5 {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        let fill_qty = q_val.round() as Qty;
        if fill_qty == 0 {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        // Compute fill price using alpha/beta formula (same as MILP):
        // eff_price = |Σ_m alpha_m * p_m + beta|
        let eff_price: f64 = coeffs[i]
            .alpha
            .iter()
            .map(|(m, &a)| {
                let p = clearing_prices
                    .get(m)
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0) as f64;
                a * p
            })
            .sum::<f64>()
            + coeffs[i].beta;
        let fill_price = eff_price.abs().round().max(0.0) as Nanos;

        let fill = Fill::new(order.id, fill_qty, fill_price);
        result.add_fill(fill, order);
    }

    // Welfare is recomputed from scratch after all post-processing (trim + arbs).
    (result, clearing_prices)
}

/// Create synthetic arb orders to restore position balance.
///
/// Computes per-market net position from all fills, then creates arb orders
/// (and fills) to zero out any imbalance from minting or integer rounding.
pub(crate) fn create_position_arbs(
    result: &mut MatchingResult,
    order_map: &HashMap<u64, &Order>,
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    max_order_id: u64,
) -> Vec<Order> {
    let mut arb_orders = Vec::new();
    let mut next_arb_id = max_order_id + 3_000_000_000;

    // Compute net position per market from all current fills
    let mut net_position: HashMap<MarketId, i64> = HashMap::new();
    for fill in &result.fills {
        if fill.fill_qty == 0 {
            continue;
        }
        let Some(&order) = order_map.get(&fill.order_id) else {
            continue;
        };
        for (market_id, normalized) in order.marginal_payoffs_i64() {
            *net_position.entry(market_id).or_insert(0) += normalized * fill.fill_qty as i64;
        }
    }

    for (&market, &net) in &net_position {
        if net == 0 {
            continue;
        }
        let shares = net.unsigned_abs();
        let yes_price = clearing_prices
            .get(&market)
            .and_then(|p| p.first().copied())
            .unwrap_or(0);

        let mut order = Order::new(next_arb_id);
        order.markets[0] = market;
        order.num_markets = 1;
        order.num_states = 2;
        if net > 0 {
            order.payoffs[0] = -1; // Sell YES to offset excess demand
            order.payoffs[1] = 0;
        } else {
            order.payoffs[0] = 1; // Buy YES to offset excess supply
            order.payoffs[1] = 0;
        }
        order.limit_price = yes_price;
        order.max_fill = shares;

        let fill = Fill::new(next_arb_id, shares, yes_price);
        result.add_fill(fill, &order);
        arb_orders.push(order);
        next_arb_id += 1;
    }

    arb_orders
}

/// Check whether any MM budget constraint is violated at current LP solution prices.
pub(crate) fn has_mm_budget_violations(
    solution: &LpSolution,
    orders: &[Order],
    mm_constraints: &[matching_engine::MmConstraint],
    mm_constraint_orders: &[Vec<(usize, MmSide)>],
    prices: &HashMap<MarketId, Nanos>,
) -> bool {
    for (mm_idx, mm) in mm_constraints.iter().enumerate() {
        let total_capital: u128 = mm_constraint_orders[mm_idx]
            .iter()
            .map(|&(i, side)| {
                let q = solution.q_values[i].round() as Qty;
                if q == 0 {
                    return 0;
                }
                let p_yes = prices
                    .get(&orders[i].markets[0])
                    .copied()
                    .unwrap_or(NANOS_PER_DOLLAR / 2);
                side.capital_needed(p_yes, q) as u128
            })
            .sum();

        if total_capital > mm.max_capital as u128 {
            return true;
        }
    }

    false
}

/// Build linearized MM budget constraints from current clearing prices.
///
/// For each MM constraint, produces a row: Σ capital_per_unit_i × q_i ≤ Budget.
/// The capital_per_unit is computed at the given prices (fixed for this LP iteration).
/// This linearizes the bilinear p×q constraint, enabling the LP to enforce budgets directly.
pub(crate) fn linearize_mm_budgets(
    orders: &[Order],
    mm_constraints: &[matching_engine::MmConstraint],
    mm_constraint_orders: &[Vec<(usize, MmSide)>],
    prices: &HashMap<MarketId, Nanos>,
) -> Vec<(Vec<(usize, f64)>, f64)> {
    mm_constraints
        .iter()
        .enumerate()
        .map(|(mm_idx, mm)| {
            let terms: Vec<(usize, f64)> = mm_constraint_orders[mm_idx]
                .iter()
                .filter_map(|&(i, side)| {
                    let p_yes = prices
                        .get(&orders[i].markets[0])
                        .copied()
                        .unwrap_or(NANOS_PER_DOLLAR / 2);
                    let cpu = side.capital_needed(p_yes, 1) as f64;
                    (cpu > 0.0).then_some((i, cpu))
                })
                .collect();
            (terms, mm.max_capital as f64)
        })
        .collect()
}

/// Trim MM fills to fix tiny budget overflows from integer rounding.
///
/// The SLP enforces budgets at linearized prices, but rounding continuous q_i
/// to integers can push capital usage slightly over budget. Trims the minimum
/// number of fill units to satisfy all budgets. Welfare is recomputed separately.
pub(crate) fn trim_mm_budget_overflows(
    result: &mut MatchingResult,
    mm_constraints: &[matching_engine::MmConstraint],
    mm_order_info: &HashMap<u64, (usize, MmSide)>,
) {
    for (mm_idx, mm) in mm_constraints.iter().enumerate() {
        let mut mm_fills: Vec<(usize, u64)> = Vec::new(); // (fill_index, capital)

        for (fi, fill) in result.fills.iter().enumerate() {
            let Some(&(oi_mm_idx, side)) = mm_order_info.get(&fill.order_id) else {
                continue;
            };
            if oi_mm_idx != mm_idx || fill.fill_qty == 0 {
                continue;
            }
            mm_fills.push((fi, side.capital_needed(fill.fill_price, fill.fill_qty)));
        }

        let total_capital: u128 = mm_fills.iter().map(|&(_, c)| c as u128).sum();
        if total_capital <= mm.max_capital as u128 {
            continue;
        }

        // Over budget — trim smallest fills first (least disruptive)
        mm_fills.sort_by_key(|&(_, cap)| cap);

        let mut remaining = total_capital;
        for &(fi, _) in &mm_fills {
            if remaining <= mm.max_capital as u128 {
                break;
            }
            let fill = &result.fills[fi];
            let Some(&(_, side)) = mm_order_info.get(&fill.order_id) else {
                continue;
            };
            let cpu = side.capital_needed(fill.fill_price, 1) as u128;
            if cpu == 0 {
                continue;
            }
            let overflow = remaining - mm.max_capital as u128;
            let trim = ((overflow + cpu - 1) / cpu).min(result.fills[fi].fill_qty as u128) as u64;
            remaining -= side.capital_needed(fill.fill_price, trim) as u128;
            result.fills[fi].fill_qty -= trim;
        }
    }

    result.fills.retain(|f| f.fill_qty > 0);
}

/// Recompute welfare, volume, and fill count from scratch.
pub(crate) fn recompute_welfare(result: &mut MatchingResult, order_map: &HashMap<u64, &Order>) {
    result.total_welfare = 0;
    result.total_quantity_filled = 0;
    result.orders_filled = 0;
    result.minting_cost = 0;
    for fill in &result.fills {
        if let Some(&order) = order_map.get(&fill.order_id) {
            result.total_welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
        }
        result.total_quantity_filled += fill.fill_qty;
        result.orders_filled += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, simple_no_buy, outcome_sell, MarketGroup};

    #[test]
    fn test_lp_single_market() {
        let mut problem = Problem::new("lp_single");
        let market = problem.markets.add_binary("market");

        // YES seller at 50c, 1000 shares
        problem.orders.push(outcome_sell(
            &problem.markets, 100, market, 0, 500_000_000, 1000,
        ));
        // NO seller at 50c, 1000 shares
        problem.orders.push(outcome_sell(
            &problem.markets, 101, market, 1, 500_000_000, 1000,
        ));
        // YES buyer at 60c, 100 shares
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));

        let solver = LpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_lp_minting() {
        let mut problem = Problem::new("lp_minting");
        let market = problem.markets.add_binary("market");

        // YES buyer at 60c
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 600_000_000, 100,
        ));
        // NO buyer at 50c
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 500_000_000, 100,
        ));

        let solver = LpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled == 2,
            "both orders should fill via minting, got {}",
            result.result.orders_filled
        );
        assert!(
            result.result.total_welfare > 0,
            "minting should produce positive welfare"
        );
    }

    #[test]
    fn test_lp_group_minting() {
        let mut problem = Problem::new("lp_group_mint");
        let m0 = problem.markets.add_binary("A");
        let m1 = problem.markets.add_binary("B");
        let m2 = problem.markets.add_binary("C");

        let mut group = MarketGroup::new("Election");
        group.add_market(m0);
        group.add_market(m1);
        group.add_market(m2);
        problem.add_market_group(group);

        // YES buyers at prices that sum to > $1 (profitable negrisk)
        problem.orders.push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));
        problem.orders.push(simple_yes_buy(&problem.markets, 3, m2, 300_000_000, 100));

        let solver = LpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(
            result.result.total_welfare > 0,
            "group minting should produce positive welfare, got {}",
            result.result.total_welfare
        );
    }

    #[test]
    fn test_lp_empty_problem() {
        let problem = Problem::new("empty");
        let solver = LpSolver::new();
        let result = solver.solve(&problem);
        assert_eq!(result.result.orders_filled, 0);
    }

    #[test]
    fn test_lp_no_profitable_trades() {
        let mut problem = Problem::new("no_profit");
        let market = problem.markets.add_binary("market");

        // YES buyer at 30c
        problem.orders.push(simple_yes_buy(
            &problem.markets, 1, market, 300_000_000, 100,
        ));
        // NO buyer at 30c → sum = 60c < $1, not profitable to mint
        problem.orders.push(simple_no_buy(
            &problem.markets, 2, market, 300_000_000, 100,
        ));
        // No sellers → only minting possible, but it costs $1 and only returns 60c

        let solver = LpSolver::new();
        let result = solver.solve(&problem);

        // Should not fill because minting costs $1 but only recovers $0.60
        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }

    #[test]
    fn test_lp_bundle_orders() {
        let mut problem = Problem::new("lp_bundle");
        let market_a = problem.markets.add_binary("A");
        let market_b = problem.markets.add_binary("B");

        // Bundle buyer: wants all YES at 40c
        problem.orders.push(matching_engine::bundle_yes(
            &problem.markets, 1, &[market_a, market_b], 400_000_000, 100,
        ));

        // Individual YES sellers on each market
        problem.orders.push(outcome_sell(
            &problem.markets, 10, market_a, 0, 150_000_000, 200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets, 11, market_b, 0, 150_000_000, 200,
        ));

        let solver = LpSolver::new();
        let result = solver.solve(&problem);

        assert!(
            result.result.orders_filled > 0,
            "should fill bundle + sellers"
        );
        assert!(
            result.result.total_welfare > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare
        );
    }
}
