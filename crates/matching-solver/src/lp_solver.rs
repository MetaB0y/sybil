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

use matching_engine::{
    Fill, MarketId, MmSide, NANOS_PER_DOLLAR, Nanos, Order, Problem, Qty, minting_cost_from_fills,
};

use crate::MatchingResult;
use crate::result::{PipelineResult, PipelineTimings, PriceDiscoveryResult};
use crate::solver::order_sign;

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

        let supported = crate::solver::filter_supported_problem(problem, "LP");
        let _rejected_orders = supported.rejected_orders;
        let problem = supported.problem.as_ref();
        if problem.orders.is_empty() {
            return PipelineResult::empty();
        }

        let ctx = build_solver_context(problem);

        // Pre-group MM orders by constraint for efficient iteration
        let mm_constraint_orders: Vec<Vec<(usize, MmSide)>> = {
            let mut by_mm = vec![Vec::new(); problem.mm_constraints.len()];
            for (i, order) in problem.orders.iter().enumerate() {
                if let Some(&(mm_idx, side)) = ctx.mm_order_info.get(&order.id) {
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
                &ctx.markets,
                &ctx.market_to_group,
                ctx.num_groups,
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
            let prices = normalized_yes_prices(&sol, &ctx.markets);
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

        finalize_result(&solution, problem, &ctx, start)
    }

    /// Build and solve the core LP using HiGHS.
    ///
    /// Returns the raw LP solution (primal + dual values) or None if infeasible.
    /// `budget_rows` contains linearized MM budget constraints: each entry is
    /// (terms: [(order_index, capital_per_unit)], budget_nanos_f64).
    fn solve_lp(
        &self,
        orders: &[Order],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
        budget_rows: &[(Vec<(usize, f64)>, f64)],
    ) -> Option<LpSolution> {
        // Default welfare objective: sign_i * limit_price_i
        let objective_coeffs = welfare_weights(orders);
        build_and_solve_lp(
            orders,
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

impl crate::Solver for LpSolver {
    /// Forwards to the inherent `LpSolver::solve` method.
    /// Explicit path needed to disambiguate from this trait method.
    fn solve(&self, problem: &Problem) -> PipelineResult {
        LpSolver::solve(self, problem)
    }
    fn name(&self) -> &str {
        "LP"
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
/// All orders must be single-market binary orders.
///
/// `objective_coeffs[i]` is the objective coefficient for order i's fill variable.
/// `budget_rows` contains linearized MM budget constraints (empty for EG solver).
pub(crate) fn build_and_solve_lp(
    orders: &[Order],
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
            let ub = orders[i].max_fill.0 as f64;
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
    // For single-market binary orders, the payoff vector directly gives
    // the per-market coefficients:
    //   c_yes = payoffs[0] (YES outcome payoff)
    //   c_no  = payoffs[1] (NO outcome payoff)

    // Track row indices for dual extraction
    let mut yes_row_indices: HashMap<MarketId, usize> = HashMap::new();
    let mut no_row_indices: HashMap<MarketId, usize> = HashMap::new();
    let mut row_count = 0usize;

    for &market in markets {
        // YES balance
        let mut yes_terms: Vec<(highs::Col, f64)> = Vec::new();
        for i in 0..n {
            if orders[i].markets[0] == market {
                let c_y = orders[i].payoffs[0] as f64;
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
            if orders[i].markets[0] == market {
                let c_n = orders[i].payoffs[1] as f64;
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

        let p_yes_raw = dual_y.abs().round().clamp(0.0, nanos_f) as u64;
        let p_no_raw = dual_n.abs().round().clamp(0.0, nanos_f) as u64;

        let sum = p_yes_raw + p_no_raw;
        let p_yes = if sum > 0 {
            let scale = nanos_f / sum as f64;
            Nanos((p_yes_raw as f64 * scale).round() as u64)
        } else {
            // No price signal — use 50/50 as neutral default.
            // Only happens when no orders touch the market.
            Nanos(NANOS_PER_DOLLAR / 2)
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

/// Extract real order fills and clearing prices from the LP solution.
///
/// Rounds continuous q_i to integer fills and derives clearing prices from
/// duals. Minting/group-minting variables are settled later by the sequencer's
/// MINT account; they are never represented as synthetic fills.
pub(crate) fn extract_result(
    solution: &LpSolution,
    orders: &[Order],
    markets: &[MarketId],
) -> (MatchingResult, HashMap<MarketId, Vec<Nanos>>) {
    let mut result = MatchingResult::new();

    // Derive clearing prices from LP duals (YES and NO per market)
    let yes_prices = normalized_yes_prices(solution, markets);
    let clearing_prices: HashMap<MarketId, Vec<Nanos>> = yes_prices
        .iter()
        .map(|(&m, &p_yes)| {
            (
                m,
                vec![p_yes, Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))],
            )
        })
        .collect();

    // Extract fills from primal solution
    for (i, order) in orders.iter().enumerate() {
        let q_val = solution.q_values[i];
        if q_val < 0.5 {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        let fill_qty = Qty(q_val.round() as u64);
        if fill_qty == Qty::ZERO {
            result.orders_unfilled_liquidity += 1;
            continue;
        }

        // For single-market binary orders, fill price is simply:
        // - YES side (payoffs[0] != 0): p_yes
        // - NO side (payoffs[1] != 0, payoffs[0] == 0): NANOS - p_yes
        let market = order.markets[0];
        let p_yes = clearing_prices
            .get(&market)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Nanos(0));
        let fill_price = if order.payoffs[0] != 0 {
            p_yes
        } else {
            Nanos(NANOS_PER_DOLLAR.saturating_sub(p_yes.0))
        };

        let fill = Fill::new(order.id, fill_qty, fill_price);
        result.add_fill(fill, order);
    }

    // Welfare is recomputed from scratch after all post-processing.
    (result, clearing_prices)
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
                let q = Qty(solution.q_values[i].round() as u64);
                if q == Qty::ZERO {
                    return 0;
                }
                let p_yes = prices
                    .get(&orders[i].markets[0])
                    .copied()
                    .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                side.capital_needed(p_yes, q).0 as u128
            })
            .sum();

        if total_capital > mm.max_capital.0 as u128 {
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
                        .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                    let cpu = side.capital_needed(p_yes, Qty(1)).0 as f64;
                    (cpu > 0.0).then_some((i, cpu))
                })
                .collect();
            (terms, mm.max_capital.0 as f64)
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
            if oi_mm_idx != mm_idx || fill.fill_qty == Qty::ZERO {
                continue;
            }
            mm_fills.push((fi, side.capital_needed(fill.fill_price, fill.fill_qty).0));
        }

        let total_capital: u128 = mm_fills.iter().map(|&(_, c)| c as u128).sum();
        if total_capital <= mm.max_capital.0 as u128 {
            continue;
        }

        // Over budget — trim smallest fills first (least disruptive)
        mm_fills.sort_by_key(|&(_, cap)| cap);

        let mut remaining = total_capital;
        for &(fi, _) in &mm_fills {
            if remaining <= mm.max_capital.0 as u128 {
                break;
            }
            let fill = &result.fills[fi];
            let Some(&(_, side)) = mm_order_info.get(&fill.order_id) else {
                continue;
            };
            let trim = trim_qty_to_fit_budget(
                side,
                fill.fill_price,
                fill.fill_qty.0,
                remaining,
                mm.max_capital.0 as u128,
            );
            if trim == 0 {
                continue;
            }

            let fill_price = fill.fill_price;
            let old_qty = result.fills[fi].fill_qty;
            let old_capital = side.capital_needed(fill_price, old_qty).0 as u128;
            result.fills[fi].fill_qty.0 -= trim;
            let new_capital = side.capital_needed(fill_price, result.fills[fi].fill_qty).0 as u128;
            remaining = remaining - old_capital + new_capital;
        }
    }

    result.fills.retain(|f| f.fill_qty.0 > 0);
}

fn trim_qty_to_fit_budget(
    side: MmSide,
    fill_price: Nanos,
    fill_qty: u64,
    remaining_capital: u128,
    budget: u128,
) -> u64 {
    if remaining_capital <= budget || fill_qty == 0 {
        return 0;
    }

    let old_capital = side.capital_needed(fill_price, Qty(fill_qty)).0 as u128;
    if old_capital == 0 {
        return 0;
    }

    if remaining_capital - old_capital > budget {
        return fill_qty;
    }

    let mut lo = 1;
    let mut hi = fill_qty;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let new_qty = fill_qty - mid;
        let new_capital = side.capital_needed(fill_price, Qty(new_qty)).0 as u128;
        let after_trim = remaining_capital - old_capital + new_capital;
        if after_trim <= budget {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    lo
}

pub(crate) fn trim_zero_price_minting(
    result: &mut MatchingResult,
    order_map: &HashMap<u64, &Order>,
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) {
    let mut diff_by_market: HashMap<MarketId, i128> = HashMap::new();
    for fill in &result.fills {
        let Some(&order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let diff_coeff = outcome_diff_coeff(order);
        if diff_coeff == 0 {
            continue;
        }
        *diff_by_market.entry(order.markets[0]).or_insert(0) +=
            diff_coeff as i128 * fill.fill_qty.0 as i128;
    }

    for (market, diff) in diff_by_market {
        let Some(trim_direction) = zero_price_mint_direction(market, diff, clearing_prices) else {
            continue;
        };

        let mut remaining = diff.unsigned_abs();
        let mut candidates: Vec<(usize, u64)> = result
            .fills
            .iter()
            .enumerate()
            .filter_map(|(fill_idx, fill)| {
                let &order = order_map.get(&fill.order_id)?;
                if order.markets[0] != market || outcome_diff_coeff(order) != trim_direction {
                    return None;
                }
                Some((fill_idx, fill.fill_qty.0))
            })
            .collect();
        candidates.sort_by_key(|&(_, qty)| qty);

        for (fill_idx, qty) in candidates {
            if remaining == 0 {
                break;
            }
            let trim = if remaining > qty as u128 {
                qty
            } else {
                remaining as u64
            };
            result.fills[fill_idx].fill_qty.0 -= trim;
            remaining -= trim as u128;
        }
    }

    result.fills.retain(|fill| fill.fill_qty.0 > 0);
}

fn zero_price_mint_direction(
    market: MarketId,
    diff: i128,
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Option<i8> {
    if diff == 0 {
        return None;
    }

    let prices = clearing_prices.get(&market);
    let missing_or_zero = |outcome: usize| {
        prices
            .and_then(|market_prices| market_prices.get(outcome))
            .copied()
            .unwrap_or(Nanos(0))
            == Nanos(0)
    };

    if diff > 0 && missing_or_zero(0) {
        Some(1)
    } else if diff < 0 && missing_or_zero(1) {
        Some(-1)
    } else {
        None
    }
}

fn outcome_diff_coeff(order: &Order) -> i8 {
    order.payoffs[0].saturating_sub(order.payoffs[1])
}

/// Per-order welfare weight in the objective: sign × limit price.
///
/// Buyers contribute `+limit_price`, sellers `-limit_price`. This is the
/// linear welfare coefficient shared by every LP-family objective.
pub(crate) fn welfare_weight(order: &Order) -> f64 {
    order_sign(order) * order.limit_price.0 as f64
}

/// Per-order welfare weights (`sign × limit price`) for all orders, in order.
pub(crate) fn welfare_weights(orders: &[Order]) -> Vec<f64> {
    orders.iter().map(welfare_weight).collect()
}

/// Build the MM order map `order_id → (mm_constraint_index, MmSide)`.
///
/// Shared by [`build_solver_context`] and the decomposed solver's global
/// budget-trimming pass.
pub(crate) fn build_mm_order_info(problem: &Problem) -> HashMap<u64, (usize, MmSide)> {
    problem
        .mm_constraints
        .iter()
        .enumerate()
        .flat_map(|(mm_idx, mm)| {
            mm.order_ids
                .iter()
                .filter_map(move |&oid| mm.order_sides.get(&oid).map(|&side| (oid, (mm_idx, side))))
        })
        .collect()
}

/// Common setup shared across all LP-family solvers: collect markets,
/// build market-to-group mapping, build MM order info.
pub(crate) struct SolverContext {
    pub markets: Vec<MarketId>,
    pub market_to_group: HashMap<MarketId, usize>,
    pub num_groups: usize,
    pub mm_order_info: HashMap<u64, (usize, MmSide)>,
}

impl SolverContext {
    /// Per-order MM info keyed by order *index*: `order_index → (mm_idx, side)`.
    ///
    /// Convenience view over [`Self::mm_order_info`] for solvers that iterate
    /// orders positionally (EG, IterLP, Conic).
    pub(crate) fn mm_order_index_map(&self, orders: &[Order]) -> HashMap<usize, (usize, MmSide)> {
        orders
            .iter()
            .enumerate()
            .filter_map(|(i, o)| self.mm_order_info.get(&o.id).map(|&info| (i, info)))
            .collect()
    }
}

/// Build the common context from a Problem.
pub(crate) fn build_solver_context(problem: &Problem) -> SolverContext {
    let markets = collect_markets(&problem.orders);
    let market_to_group: HashMap<MarketId, usize> = problem
        .market_groups
        .iter()
        .enumerate()
        .flat_map(|(g_idx, group)| group.markets.iter().map(move |&m| (m, g_idx)))
        .collect();
    SolverContext {
        markets,
        market_to_group,
        num_groups: problem.market_groups.len(),
        mm_order_info: build_mm_order_info(problem),
    }
}

/// Common post-processing shared across all LP-family solvers.
///
/// After the core solving phase (LP, Frank-Wolfe, conic, or μ-iteration),
/// all solvers share this finalization: extract real order fills from the LP
/// solution, trim MM budget overflows, recompute welfare, and gate on
/// non-negative welfare.
pub(crate) fn finalize_result(
    solution: &LpSolution,
    problem: &Problem,
    ctx: &SolverContext,
    start: Instant,
) -> PipelineResult {
    let orders = &problem.orders;
    let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
    let (mut result, prices) = extract_result(solution, orders, &ctx.markets);

    trim_mm_budget_overflows(&mut result, &problem.mm_constraints, &ctx.mm_order_info);
    trim_zero_price_minting(&mut result, &order_map, &prices);
    recompute_welfare(&mut result, &order_map);

    let mut pipeline_result = PipelineResult::empty();
    pipeline_result.result = result;
    pipeline_result.price_discovery = Some(PriceDiscoveryResult {
        prices,
        total_fills: pipeline_result.result.fills.len(),
        total_welfare: pipeline_result.result.total_welfare(),
    });
    pipeline_result.total_time_secs = start.elapsed().as_secs_f64();
    pipeline_result.phase_times = PipelineTimings {
        price_discovery_secs: start.elapsed().as_secs_f64(),
        ..Default::default()
    };

    pipeline_result
}

/// Shared projection-LP epilogue for the EG, IterLP, and Conic solvers.
///
/// Their core phase (Frank-Wolfe, μ-iteration, or conic interior point)
/// produces a continuous allocation whose duals don't yield valid clearing
/// prices. This caps each order's `max_fill` at the ceiled core allocation,
/// re-solves the standard welfare LP for exact prices, and finalizes — so the
/// LP's complementary slackness guarantees a uniform clearing price.
///
/// `allocation[i]` is the core-phase fill for order `i` (in the same order as
/// `problem.orders`); it is ceiled as an integer upper bound and clamped to
/// `[0, max_fill]`.
pub(crate) fn project_and_finalize(
    allocation: &[f64],
    problem: &Problem,
    ctx: &SolverContext,
    start: Instant,
) -> PipelineResult {
    let orders = &problem.orders;

    let mut projected_orders: Vec<Order> = orders.to_vec();
    for (i, order) in projected_orders.iter_mut().enumerate() {
        let core_fill = if allocation[i] <= 1e-9 {
            0
        } else {
            allocation[i].ceil() as u64
        };
        order.max_fill = Qty(core_fill.min(orders[i].max_fill.0));
    }

    let projection_obj = welfare_weights(orders);
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

    finalize_result(&final_sol, problem, ctx, start)
}

/// Recompute welfare, volume, and fill count from scratch.
pub(crate) fn recompute_welfare(result: &mut MatchingResult, order_map: &HashMap<u64, &Order>) {
    result.gross_welfare = 0;
    result.total_quantity_filled = 0;
    result.orders_filled = 0;
    for fill in &result.fills {
        if let Some(&order) = order_map.get(&fill.order_id) {
            result.gross_welfare += order.gross_welfare_contribution(fill.fill_qty);
        }
        result.total_quantity_filled += fill.fill_qty.0;
        result.orders_filled += 1;
    }
    result.minting_cost = minting_cost_from_fills(order_map.values().copied(), &result.fills);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{
        group_minting_problem, minting_problem, no_profitable_trades_problem, single_market_problem,
    };

    #[test]
    fn test_lp_single_market() {
        let result = LpSolver::new().solve(&single_market_problem());

        assert!(
            result.result.total_welfare() > 0,
            "should produce positive welfare, got {}",
            result.result.total_welfare()
        );
        assert!(result.result.orders_filled > 0, "should fill some orders");
    }

    #[test]
    fn test_lp_minting() {
        let result = LpSolver::new().solve(&minting_problem());

        assert!(
            result.result.orders_filled == 2,
            "both orders should fill via minting, got {}",
            result.result.orders_filled
        );
        assert!(
            result.result.total_welfare() > 0,
            "minting should produce positive welfare"
        );
    }

    #[test]
    fn test_lp_group_minting() {
        let problem = group_minting_problem();
        let result = LpSolver::new().solve(&problem);

        assert!(
            result.result.orders_filled >= 3,
            "should fill all 3 via group minting, filled {}",
            result.result.orders_filled
        );
        assert!(
            result
                .result
                .fills
                .iter()
                .all(|fill| problem.orders.iter().any(|order| order.id == fill.order_id)),
            "LP finalizer must not leak synthetic minting/arb fills into block output"
        );
        assert!(
            result.result.total_welfare() > 0,
            "group minting should produce positive welfare, got {}",
            result.result.total_welfare()
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
        // Should not fill because minting costs $1 but only recovers $0.60.
        let result = LpSolver::new().solve(&no_profitable_trades_problem());

        assert_eq!(
            result.result.orders_filled, 0,
            "should not fill unprofitable minting"
        );
    }
}
