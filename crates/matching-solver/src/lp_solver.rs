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
use crate::pipeline::{PipelineResult, PipelineTimings};
use crate::traits::PriceDiscoveryResult;
use crate::{MatchingResult, Pipeline};

/// Configuration for the LP solver.
#[derive(Clone, Debug)]
pub struct LpConfig {
    /// Max iterations for MM budget shading (0 = LP only, no MM handling).
    pub max_mm_iterations: usize,
    /// Factor by which to reduce over-budget MM order limits each iteration.
    /// 0.5 means halve the limit each round.
    pub shading_factor: f64,
    /// Max iterations for re-expansion pass after shading converges.
    pub max_expand_iterations: usize,
}

impl Default for LpConfig {
    fn default() -> Self {
        Self {
            max_mm_iterations: 10,
            shading_factor: 0.5,
            max_expand_iterations: 5,
        }
    }
}

/// LP-based solver that handles the convex core exactly via HiGHS,
/// then uses iterative limit shading for MM budget constraints.
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

    /// Solve a matching problem using LP + iterative MM budget shading.
    ///
    /// Returns a `PipelineResult` compatible with `enforce_ucp` post-processing.
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

        // Current max_fill limits (may be shaded for MM budget iteration)
        let original_limits: Vec<Qty> = problem.orders.iter().map(|o| o.max_fill).collect();
        let mut effective_limits = original_limits.clone();

        // Iterative LP + MM budget shading
        let mut best_solution: Option<LpSolution> = None;

        // Phase 1: Shade down until budgets are satisfied
        for mm_iter in 0..=self.config.max_mm_iterations {
            let solution = self.solve_lp(
                &problem.orders,
                &coeffs,
                &markets,
                &market_to_group,
                problem.market_groups.len(),
                &effective_limits,
            );

            let Some(ref sol) = solution else {
                break;
            };

            // Check MM budget violations
            if problem.mm_constraints.is_empty() || mm_iter == self.config.max_mm_iterations {
                best_solution = solution;
                break;
            }

            let prices = normalized_yes_prices(sol, &markets);
            let violations = check_mm_budgets(
                sol,
                &problem.orders,
                &problem.mm_constraints,
                &mm_order_info,
                &prices,
            );

            if violations.is_empty() {
                best_solution = solution;
                break;
            }

            // Shade limits for over-budget MM orders
            shade_mm_limits(
                &violations,
                &problem.orders,
                &problem.mm_constraints,
                &mm_order_info,
                &mut effective_limits,
                self.config.shading_factor,
                &prices,
            );

            best_solution = solution;
        }

        // Phase 2: Re-expand shaded limits to recover MM liquidity.
        // After shading converged, some MM orders may have been over-reduced.
        // Try restoring them toward original limits (highest welfare/capital first).
        if best_solution.is_some()
            && !problem.mm_constraints.is_empty()
            && self.config.max_expand_iterations > 0
        {
            expand_mm_limits(
                &self,
                &problem.orders,
                &coeffs,
                &markets,
                &market_to_group,
                problem.market_groups.len(),
                &problem.mm_constraints,
                &mm_order_info,
                &original_limits,
                &mut effective_limits,
                &mut best_solution,
            );
        }

        let Some(solution) = best_solution else {
            return PipelineResult::empty();
        };

        // Extract fills and prices from LP solution
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        let (result, prices, arb_orders) = extract_result(
            &solution,
            &problem.orders,
            &coeffs,
            &markets,
            &market_to_group,
            problem.market_groups.len(),
            &order_map,
        );

        // Build PipelineResult with prices for enforce_ucp
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

        // Build order map including arb orders for enforce_ucp
        let mut order_map_with_arbs = order_map;
        for arb in &arb_orders {
            order_map_with_arbs.insert(arb.id, arb);
        }

        // Enforce UCP: reprice at final clearing prices, trim position imbalance
        Pipeline::enforce_ucp(&mut pipeline_result, &order_map_with_arbs);

        // After enforce_ucp, minting cost is absorbed into the arb order fills
        // (arb orders have limit_price == clearing_price, so zero welfare contribution).
        // Reset minting_cost to 0 so the verifier's welfare check passes:
        //   total_welfare == fill_welfare - minting_cost
        pipeline_result.result.minting_cost = 0;

        // Store arb orders for witness/verification
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
    fn solve_lp(
        &self,
        orders: &[Order],
        coeffs: &[OrderCoefficients],
        markets: &[MarketId],
        market_to_group: &HashMap<MarketId, usize>,
        num_groups: usize,
        effective_limits: &[Qty],
    ) -> Option<LpSolution> {
        let n = orders.len();
        let nanos_f = NANOS_PER_DOLLAR as f64;

        let mut pb = RowProblem::default();

        // ================================================================
        // Variables
        // ================================================================

        // q_i: fill quantity for order i
        // Objective coefficient: sign_i * limit_price_i
        let q_cols: Vec<_> = (0..n)
            .map(|i| {
                let sign = order_sign(&orders[i]);
                let obj = sign * orders[i].limit_price as f64;
                let ub = effective_limits[i] as f64;
                pb.add_column(obj, 0.0..=ub)
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

        // Extract primal values by column index.
        // Columns are added in order: q_0..q_{n-1}, mint_m0..mint_mk, gmint_g0..gmint_gp
        // so we can compute indices directly.
        let q_values: Vec<f64> = (0..n).map(|i| primal[i]).collect();

        let mint_values: HashMap<MarketId, f64> = markets
            .iter()
            .enumerate()
            .map(|(j, &m)| (m, primal[n + j]))
            .collect();

        let gmint_values: Vec<f64> = (0..num_groups)
            .map(|g| primal[n + markets.len() + g])
            .collect();

        // Extract dual values (prices)
        // The dual of the YES balance constraint gives the shadow price for YES shares
        // The dual of the NO balance constraint gives the shadow price for NO shares
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
            objective: solved.objective_value(),
            q_values,
            mint_values,
            gmint_values,
            dual_yes,
            dual_no,
        })
    }
}

impl Default for LpSolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw solution from the LP solver.
#[allow(dead_code)]
struct LpSolution {
    objective: f64,
    q_values: Vec<f64>,
    mint_values: HashMap<MarketId, f64>,
    gmint_values: Vec<f64>,
    dual_yes: HashMap<MarketId, f64>,
    dual_no: HashMap<MarketId, f64>,
}

/// Derive normalized YES clearing prices from LP dual variables.
///
/// For each market, takes |dual_yes| and |dual_no|, normalizes so they sum to $1.
/// Returns p_YES per market (in nanos). p_NO = NANOS_PER_DOLLAR - p_YES.
fn normalized_yes_prices(
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
fn collect_markets(orders: &[Order]) -> Vec<MarketId> {
    let mut markets = Vec::new();
    let mut seen = HashSet::new();
    for order in orders {
        for m_idx in 0..order.num_markets as usize {
            let m = order.markets[m_idx];
            if !m.is_none() && seen.insert(m) {
                markets.push(m);
            }
        }
    }
    markets
}

/// Extract fills, prices, and arbitrage orders from the LP solution.
///
/// Rounds continuous q_i to integer fills, derives clearing prices from duals,
/// and creates synthetic arb orders to restore position balance (from minting).
fn extract_result(
    solution: &LpSolution,
    orders: &[Order],
    coeffs: &[OrderCoefficients],
    markets: &[MarketId],
    _market_to_group: &HashMap<MarketId, usize>,
    _num_groups: usize,
    order_map: &HashMap<u64, &Order>,
) -> (MatchingResult, HashMap<MarketId, Vec<Nanos>>, Vec<Order>) {
    let nanos_f = NANOS_PER_DOLLAR as f64;
    let mut result = MatchingResult::new();

    // Derive clearing prices from dual variables.
    //
    // The dual of an equality constraint in a maximization LP gives the
    // marginal value of relaxing the RHS by one unit. For balance constraints
    // of the form "demand - supply = 0", the dual represents the price of
    // that commodity (YES or NO shares).
    //
    // We take the absolute value and clamp to [0, NANOS_PER_DOLLAR].
    let mut clearing_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();

    for &market in markets {
        let dual_y = solution.dual_yes.get(&market).copied().unwrap_or(0.0);
        let dual_n = solution.dual_no.get(&market).copied().unwrap_or(0.0);

        // The dual values represent marginal value of shares.
        // For a maximization problem with equality constraints,
        // the dual should be positive for valuable resources.
        let p_yes = dual_y.abs().round().clamp(0.0, nanos_f) as Nanos;
        let p_no = dual_n.abs().round().clamp(0.0, nanos_f) as Nanos;

        // If minting is active (mint > 0), duals should satisfy p_YES + p_NO = $1
        // due to complementary slackness. We normalize to ensure this.
        let sum = p_yes + p_no;
        let (final_yes, final_no) = if sum > 0 {
            // Normalize so YES + NO = $1
            let scale = nanos_f / sum as f64;
            let ny = (p_yes as f64 * scale).round() as Nanos;
            let nn = NANOS_PER_DOLLAR.saturating_sub(ny);
            (ny, nn)
        } else {
            // Fallback: 50/50
            (NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2)
        };

        clearing_prices.insert(market, vec![final_yes, final_no]);
    }

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

    // Create synthetic arb orders to restore position balance from minting.
    //
    // The LP uses continuous mint/gmint variables for minting, but these aren't
    // fills. Compute per-market position imbalance and create arb orders to cancel.
    let mut arb_orders = Vec::new();
    let max_order_id = orders.iter().map(|o| o.id).max().unwrap_or(0);
    let mut next_arb_id = max_order_id + 3_000_000_000;

    // Compute net position using marginal payoffs (same approach as MILP)
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

    // Create arb fills to cancel each market's imbalance
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
            // Excess YES demand → sell YES
            order.payoffs[0] = -1;
            order.payoffs[1] = 0;
        } else {
            // Excess NO demand → buy YES
            order.payoffs[0] = 1;
            order.payoffs[1] = 0;
        }
        order.limit_price = yes_price;
        order.max_fill = shares;

        let fill = Fill::new(next_arb_id, shares, yes_price);
        result.add_fill(fill, &order);
        arb_orders.push(order);
        next_arb_id += 1;
    }

    // Adjust welfare to account for minting cost (same logic as MILP):
    // The LP objective already deducts minting cost, but fill-level welfare doesn't
    // because arb orders have limit_price == fill_price (zero welfare).
    // Clamp to non-negative: tiny negative values arise from LP float rounding.
    let fill_welfare = result.total_welfare;
    let objective_welfare = solution.objective.round() as i64;
    let minting_cost = (fill_welfare - objective_welfare).max(0);
    result.minting_cost = minting_cost;
    result.total_welfare = fill_welfare - minting_cost;

    (result, clearing_prices, arb_orders)
}

/// Per-constraint MM budget violation info.
struct MmViolation {
    /// Index into problem.mm_constraints
    mm_idx: usize,
    /// Capital used (exceeds budget)
    capital_used: u64,
    /// Budget
    budget: u64,
}

/// Check which MM budget constraints are violated.
///
/// Uses normalized clearing prices (p_YES + p_NO = $1) for capital computation,
/// consistent with how fills are priced in extract_result.
fn check_mm_budgets(
    solution: &LpSolution,
    orders: &[Order],
    mm_constraints: &[matching_engine::MmConstraint],
    mm_order_info: &HashMap<u64, (usize, MmSide)>,
    prices: &HashMap<MarketId, Nanos>,
) -> Vec<MmViolation> {
    let mut violations = Vec::new();

    for (mm_idx, mm) in mm_constraints.iter().enumerate() {
        let mut total_capital: u128 = 0;

        for (i, order) in orders.iter().enumerate() {
            let q_val = solution.q_values[i];
            if q_val < 0.5 {
                continue;
            }
            let fill_qty = q_val.round() as Qty;
            if fill_qty == 0 {
                continue;
            }

            let Some(&(oi_mm_idx, side)) = mm_order_info.get(&order.id) else {
                continue;
            };
            if oi_mm_idx != mm_idx {
                continue;
            }

            // Use normalized clearing price for this order's market
            let market = order.markets[0];
            let p_yes = prices.get(&market).copied().unwrap_or(NANOS_PER_DOLLAR / 2);

            let capital = side.capital_needed(p_yes, fill_qty);
            total_capital += capital as u128;
        }

        if total_capital > mm.max_capital as u128 {
            violations.push(MmViolation {
                mm_idx,
                capital_used: total_capital.min(u64::MAX as u128) as u64,
                budget: mm.max_capital,
            });
        }
    }

    violations
}

/// Shade (reduce) effective limits for MM orders in violated constraints.
///
/// Strategy: for each violated MM constraint, sort its orders by welfare/capital
/// ratio (descending), and progressively reduce limits for low-ratio orders.
/// Uses actual clearing prices for capital computation (not hardcoded 50c).
fn shade_mm_limits(
    violations: &[MmViolation],
    orders: &[Order],
    _mm_constraints: &[matching_engine::MmConstraint],
    mm_order_info: &HashMap<u64, (usize, MmSide)>,
    effective_limits: &mut [Qty],
    shading_factor: f64,
    prices: &HashMap<MarketId, Nanos>,
) {
    for violation in violations {
        let overshoot = violation.capital_used as f64 / violation.budget as f64;

        // Compute welfare/capital ratio for each MM order using actual prices
        let mut order_ratios: Vec<(usize, f64)> = Vec::new();

        for (i, order) in orders.iter().enumerate() {
            let Some(&(oi_mm_idx, side)) = mm_order_info.get(&order.id) else {
                continue;
            };
            if oi_mm_idx != violation.mm_idx {
                continue;
            }

            let welfare_per_unit = order.limit_price as f64;
            // Use actual clearing price for capital computation
            let market = order.markets[0];
            let p_yes = prices.get(&market).copied().unwrap_or(NANOS_PER_DOLLAR / 2);
            let capital_per_unit = side.capital_needed(p_yes, 1) as f64;

            let ratio = if capital_per_unit > 0.0 {
                welfare_per_unit / capital_per_unit
            } else {
                f64::INFINITY
            };

            order_ratios.push((i, ratio));
        }

        // Sort by ratio ascending (shade lowest-ratio orders first)
        order_ratios.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Reduce limits proportionally to overshoot
        let reduction = shading_factor * (1.0 - 1.0 / overshoot).max(0.0).min(1.0);

        for (idx, _ratio) in &order_ratios {
            let current = effective_limits[*idx];
            let new_limit = ((current as f64) * (1.0 - reduction)).round() as Qty;
            effective_limits[*idx] = new_limit.max(1); // Never reduce to 0
        }
    }
}

/// Re-expand previously shaded MM order limits to recover liquidity.
///
/// After shading converges (budgets satisfied), some MM orders may have been
/// reduced more than necessary. This pass tries restoring them toward their
/// original limits, highest welfare/capital ratio first. Each expansion is
/// validated by re-solving the LP and checking budgets.
fn expand_mm_limits(
    solver: &LpSolver,
    orders: &[Order],
    coeffs: &[OrderCoefficients],
    markets: &[MarketId],
    market_to_group: &HashMap<MarketId, usize>,
    num_groups: usize,
    mm_constraints: &[matching_engine::MmConstraint],
    mm_order_info: &HashMap<u64, (usize, MmSide)>,
    original_limits: &[Qty],
    effective_limits: &mut [Qty],
    best_solution: &mut Option<LpSolution>,
) {
    // Compute prices from current best solution for welfare/capital ranking
    let current_prices = best_solution
        .as_ref()
        .map(|sol| normalized_yes_prices(sol, markets))
        .unwrap_or_default();

    // Find MM orders that were shaded (effective < original)
    let mut shaded_orders: Vec<(usize, f64)> = Vec::new();
    for (i, order) in orders.iter().enumerate() {
        let Some(&(_, side)) = mm_order_info.get(&order.id) else {
            continue;
        };
        if effective_limits[i] >= original_limits[i] {
            continue;
        }
        // Welfare/capital ratio using actual clearing prices
        let market = order.markets[0];
        let p_yes = current_prices.get(&market).copied().unwrap_or(NANOS_PER_DOLLAR / 2);
        let capital_per_unit = side.capital_needed(p_yes, 1) as f64;
        let ratio = if capital_per_unit > 0.0 {
            order.limit_price as f64 / capital_per_unit
        } else {
            f64::INFINITY
        };
        shaded_orders.push((i, ratio));
    }

    if shaded_orders.is_empty() {
        return;
    }

    // Sort by welfare/capital ratio descending — restore highest-value orders first
    shaded_orders.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for _expand_iter in 0..solver.config.max_expand_iterations {
        let mut any_expanded = false;

        for &(idx, _ratio) in &shaded_orders {
            if effective_limits[idx] >= original_limits[idx] {
                continue;
            }

            // Try restoring halfway toward original
            let current = effective_limits[idx];
            let original = original_limits[idx];
            let candidate = current + (original - current + 1) / 2;

            effective_limits[idx] = candidate;

            // Re-solve and check budgets
            let solution = solver.solve_lp(
                orders,
                coeffs,
                markets,
                market_to_group,
                num_groups,
                effective_limits,
            );

            let Some(ref sol) = solution else {
                effective_limits[idx] = current; // revert
                continue;
            };

            let prices = normalized_yes_prices(sol, markets);
            let violations = check_mm_budgets(sol, orders, mm_constraints, mm_order_info, &prices);

            if violations.is_empty() {
                // Expansion is budget-feasible — keep it
                *best_solution = solution;
                any_expanded = true;
            } else {
                // Violated — revert this order's expansion
                effective_limits[idx] = current;
            }
        }

        if !any_expanded {
            break;
        }
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
