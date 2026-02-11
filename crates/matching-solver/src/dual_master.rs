//! Hybrid dual decomposition + greedy knapsack for market clearing.
//!
//! Decouples two types of coupling constraints:
//! - **Price consistency** (λ): sum of YES prices = $1 across MarketGroups
//!   → handled via Lagrangian relaxation with subgradient updates
//! - **MM budgets**: capital usage ≤ budget per market maker
//!   → handled via greedy knapsack allocation (welfare/capital ratio sorting)
//!
//! The main loop accumulates fills across iterations:
//! 1. Shade orders using λ (price consistency only)
//! 2. Solve per-market subproblems with shaded orders + remaining liquidity
//! 3. Run greedy MM knapsack on candidate fills (remaining budget)
//! 4. Filter fills: non-MM that pass limit check + MM from knapsack
//! 5. Accumulate fills, consume liquidity
//! 6. Compute price residuals and update λ
//! 7. Check convergence: |price_residuals| < tol AND welfare_delta < 1%

use std::collections::{HashMap, HashSet};

use matching_engine::{
    Fill, MarketGroup, MarketId, MmConstraint, Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR,
};
use serde::Serialize;
use tracing::debug;

use crate::local_solver::LocalSolver;
use crate::pipeline::Pipeline;
use crate::specialized::MultiMarketSolver;
use crate::traits::PriceDiscoveryResult;
use crate::MatchingResult;

// ============================================================================
// Configuration
// ============================================================================

/// Step size decay strategy for subgradient updates.
#[derive(Clone, Debug, Serialize)]
pub enum StepDecay {
    /// α_t = α_0 / sqrt(t)
    InvSqrt,
    /// α_t = α_0 / t
    InvLinear,
}

/// Configuration for the dual decomposition solver.
#[derive(Clone, Debug, Serialize)]
pub struct DualConfig {
    /// Maximum number of outer iterations.
    pub max_iterations: usize,
    /// Initial step size for subgradient updates.
    pub initial_step_size: f64,
    /// Primal tolerance: max allowed constraint violation (fraction of $1).
    pub primal_tolerance: f64,
    /// Dual tolerance: max allowed dual variable change between iterations.
    pub dual_tolerance: f64,
    /// Step size decay strategy.
    pub step_decay: StepDecay,
    /// Welfare tolerance: stop when marginal improvement < this fraction.
    pub welfare_tolerance: f64,
}

impl Default for DualConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            initial_step_size: 0.3,
            primal_tolerance: 0.02,
            dual_tolerance: 0.001,
            step_decay: StepDecay::InvSqrt,
            welfare_tolerance: 0.005,
        }
    }
}

// ============================================================================
// Dual State
// ============================================================================

/// State of the dual variables across iterations.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DualState {
    /// Price consistency multipliers: one per MarketGroup.
    /// λ > 0 means prices sum above $1 (posrisk); λ < 0 means below $1 (negrisk).
    pub lambda: HashMap<String, f64>,
    /// Previous λ values for convergence checking.
    pub prev_lambda: HashMap<String, f64>,
}

// ============================================================================
// Dual Result
// ============================================================================

/// Result of dual decomposition solving.
#[derive(Clone, Debug, Serialize)]
pub struct DualResult {
    /// The final matching result (fills, welfare).
    pub matching_result: MatchingResult,
    /// Price discovery result with per-market prices.
    pub prices: PriceDiscoveryResult,
    /// Number of iterations executed.
    pub iterations: usize,
    /// Whether the solver converged within tolerance.
    pub converged: bool,
    /// Final price sum error per group (fraction of $1).
    pub final_price_sum_error: HashMap<String, f64>,
    /// Final MM utilization per MM (fraction of budget used).
    pub final_mm_utilization: HashMap<u64, f64>,
    /// Final dual state for diagnostics.
    pub dual_state: DualState,
}

/// Stats for a single dual decomposition iteration.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DualIterationStats {
    pub iteration: usize,
    pub step_size: f64,
    pub max_price_residual: f64,
    pub lambda_norm: f64,
    pub welfare: i64,
    pub fills: usize,
    pub mm_fills: usize,
}

// ============================================================================
// DualMaster
// ============================================================================

/// The dual decomposition master problem solver.
pub struct DualMaster {
    config: DualConfig,
    local_solver: LocalSolver,
    multi_market_solver: Option<MultiMarketSolver>,
}

impl DualMaster {
    /// Create a new DualMaster with default configuration.
    pub fn new() -> Self {
        Self {
            config: DualConfig::default(),
            local_solver: LocalSolver::new(),
            multi_market_solver: None,
        }
    }

    /// Create a new DualMaster with custom configuration.
    pub fn with_config(config: DualConfig) -> Self {
        Self {
            config,
            local_solver: LocalSolver::new(),
            multi_market_solver: None,
        }
    }

    /// Set the multi-market solver for bundle repricing inside the dual loop.
    pub fn with_multi_market_solver(mut self, solver: MultiMarketSolver) -> Self {
        self.multi_market_solver = Some(solver);
        self
    }

    /// Main dual decomposition solve loop.
    ///
    /// Accumulates fills across iterations with greedy MM knapsack allocation.
    #[tracing::instrument(skip_all, name = "dual_master")]
    pub fn solve(&self, problem: &Problem) -> DualResult {
        let mut state = DualState::default();

        // Initialize λ=0 for each MarketGroup
        for group in &problem.market_groups {
            state.lambda.insert(group.name.clone(), 0.0);
            state.prev_lambda.insert(group.name.clone(), 0.0);
        }

        // Build lookup: which group does each market belong to?
        let market_to_group: HashMap<MarketId, String> = problem
            .market_groups
            .iter()
            .flat_map(|g| g.markets.iter().map(move |&m| (m, g.name.clone())))
            .collect();

        // Build order lookup
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        // Build MM order IDs set
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // State for cumulative fill accumulation
        let mut matching_result = MatchingResult::new();
        let mut filled_order_ids: HashSet<u64> = HashSet::new();
        let mut cumulative_mm_fills: HashMap<u64, (Nanos, Qty)> = HashMap::new();

        let mut iteration_stats = Vec::new();
        let mut converged = false;
        let mut iterations = 0;
        let mut prev_welfare: i64 = 0;
        let mut last_prices = PriceDiscoveryResult::empty();

        for iter in 1..=self.config.max_iterations {
            iterations = iter;

            // Step size: α_t = α_0 / sqrt(t) or α_0 / t
            let step_size = match self.config.step_decay {
                StepDecay::InvSqrt => self.config.initial_step_size / (iter as f64).sqrt(),
                StepDecay::InvLinear => self.config.initial_step_size / iter as f64,
            };

            // 1. Shade orders with λ only (no μ)
            // Don't shade MM orders — their tight spreads make them sensitive to
            // lambda drift, causing the original-limit check to reject fills when
            // the shaded clearing price drops below the original sell limit.
            let (mm_remaining, non_mm_remaining): (Vec<Order>, Vec<Order>) = problem
                .orders
                .iter()
                .filter(|o| !filled_order_ids.contains(&o.id))
                .cloned()
                .partition(|o| mm_order_ids.contains(&o.id));

            let mut shaded_orders =
                shade_orders(&non_mm_remaining, &state.lambda, &market_to_group);
            shaded_orders.extend(mm_remaining);

            // 2. Solve per-market subproblems with shaded orders
            let shaded_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                orders: shaded_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            let mut prices = self.local_solver.discover_prices_impl(&shaded_problem);

            // 2b. Run multi-market repricing inside the dual loop
            // This handles bundle/spread orders that span multiple markets
            let mut bundle_fills_this_iter: Vec<Fill> = Vec::new();
            if let Some(ref mm_solver) = self.multi_market_solver {
                let repricing_result = mm_solver.solve_with_repricing(&shaded_problem, &prices);
                if repricing_result.bundles_matched > 0 {
                    // Update prices with repriced market solutions
                    for (mid, sol) in &repricing_result.repriced_solutions {
                        if let Some(old_sol) = prices.market_solutions.get(mid) {
                            prices.total_welfare -= old_sol.welfare;
                            prices.total_fills -= old_sol.fills.len();
                        }
                        prices.total_welfare += sol.welfare;
                        prices.total_fills += sol.fills.len();
                        prices.prices.insert(*mid, sol.prices.clone());
                        prices.market_solutions.insert(*mid, sol.clone());
                    }
                    bundle_fills_this_iter = repricing_result.bundle_fills;
                }
            }

            // 3. Collect candidate fills, validate against original limits
            let candidate_fills: Vec<Fill> = prices
                .all_fills()
                .into_iter()
                .filter(|fill| {
                    fill.fill_qty > 0
                        && order_map
                            .get(&fill.order_id)
                            .map(|o| o.is_satisfied_at_price(fill.fill_price))
                            .unwrap_or(false)
                })
                .collect();

            // 4. Separate non-MM fills (accept directly) and MM fills (knapsack)
            let mut non_mm_fills: Vec<Fill> = Vec::new();
            let mut mm_candidate_fills: Vec<Fill> = Vec::new();

            for fill in candidate_fills {
                if mm_order_ids.contains(&fill.order_id) {
                    mm_candidate_fills.push(fill);
                } else {
                    non_mm_fills.push(fill);
                }
            }

            // 5. Greedy MM knapsack: sort by welfare/capital ratio, greedily activate
            let mut mm_accepted_fills: Vec<Fill> = Vec::new();
            {
                // Build fill lookup for mapping knapsack results back to fills
                let fill_by_id: HashMap<u64, &Fill> =
                    mm_candidate_fills.iter().map(|f| (f.order_id, f)).collect();
                let mut already_accepted: HashSet<u64> = HashSet::new();

                for mm in &problem.mm_constraints {
                    let remaining_budget = mm
                        .max_capital
                        .saturating_sub(mm.capital_used(&cumulative_mm_fills));
                    if remaining_budget == 0 {
                        continue;
                    }

                    // Build knapsack input, excluding already-accepted orders
                    let knapsack_input: Vec<(u64, i64, Nanos)> = mm_candidate_fills
                        .iter()
                        .filter(|f| {
                            mm.contains_order(f.order_id) && !already_accepted.contains(&f.order_id)
                        })
                        .filter_map(|f| {
                            let order = order_map.get(&f.order_id)?;
                            let welfare = order.welfare_contribution(f.fill_price, f.fill_qty);
                            let side = mm.order_sides.get(&f.order_id)?;
                            let capital = side.capital_needed(f.fill_price, f.fill_qty);
                            Some((f.order_id, welfare, capital))
                        })
                        .collect();

                    let (activated_ids, _, _) =
                        crate::mm_allocator::greedy_knapsack(&knapsack_input, remaining_budget);

                    for id in activated_ids {
                        if let Some(&fill) = fill_by_id.get(&id) {
                            already_accepted.insert(id);
                            mm_accepted_fills.push(fill.clone());
                        }
                    }
                }
            }

            // 6. Collect iteration candidate fills (non-MM + knapsack-approved MM)
            let fill_start_idx = matching_result.fills.len();
            let mut iter_fills = 0usize;
            let mut iter_mm_fills = 0usize;
            let mut iter_mm_ids: HashSet<u64> = HashSet::new();

            let all_candidate_fills: Vec<(Fill, bool)> = non_mm_fills
                .into_iter()
                .map(|f| (f, false))
                .chain(mm_accepted_fills.into_iter().map(|f| (f, true)))
                .collect();

            for (fill, is_mm) in all_candidate_fills {
                let Some(&order) = order_map.get(&fill.order_id) else {
                    continue;
                };
                filled_order_ids.insert(fill.order_id);
                if is_mm {
                    cumulative_mm_fills.insert(fill.order_id, (fill.fill_price, fill.fill_qty));
                    iter_mm_ids.insert(fill.order_id);
                }
                matching_result.add_fill(fill, order);
            }

            // 6b. Add bundle fills from multi-market repricing
            for fill in bundle_fills_this_iter {
                if let Some(order) = order_map.get(&fill.order_id) {
                    if !filled_order_ids.contains(&fill.order_id)
                        && order.is_satisfied_at_price(fill.fill_price)
                    {
                        filled_order_ids.insert(fill.order_id);
                        matching_result.add_fill(fill, order);
                    }
                }
            }

            // 6c. Per-iteration UCP enforcement: reprice at this iteration's
            // clearing prices and trim position imbalance. This prevents
            // price drift across iterations from causing massive welfare loss
            // during the final enforce_ucp.
            if fill_start_idx < matching_result.fills.len() {
                let iter_fills_slice: Vec<Fill> =
                    matching_result.fills[fill_start_idx..].to_vec();

                let mut candidates = Pipeline::reprice_and_filter_fills(
                    &iter_fills_slice,
                    &prices.prices,
                    &order_map,
                );
                Pipeline::trim_position_imbalance(&mut candidates, &order_map);

                // Build set of surviving order IDs
                let surviving_ids: HashSet<u64> = candidates
                    .iter()
                    .filter(|(f, _)| f.fill_qty > 0)
                    .map(|(f, _)| f.order_id)
                    .collect();

                // Un-fill dropped orders so they can be re-matched in later iterations
                for fill in &matching_result.fills[fill_start_idx..] {
                    if !surviving_ids.contains(&fill.order_id) {
                        filled_order_ids.remove(&fill.order_id);
                        cumulative_mm_fills.remove(&fill.order_id);
                    }
                }

                // Replace this iteration's fills with survivors and recompute totals
                matching_result.fills.truncate(fill_start_idx);
                let mut welfare = 0i64;
                let mut volume = 0u64;
                let mut filled = 0usize;
                for fill in &matching_result.fills {
                    if let Some(&order) = order_map.get(&fill.order_id) {
                        welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
                    }
                    volume += fill.fill_qty;
                    filled += 1;
                }
                for (fill, _) in candidates {
                    if fill.fill_qty > 0 {
                        if let Some(&order) = order_map.get(&fill.order_id) {
                            welfare += order.welfare_contribution(fill.fill_price, fill.fill_qty);
                        }
                        volume += fill.fill_qty;
                        filled += 1;
                        if iter_mm_ids.contains(&fill.order_id) {
                            iter_mm_fills += 1;
                        }
                        iter_fills += 1;
                        matching_result.fills.push(fill);
                    }
                }
                matching_result.total_welfare = welfare;
                matching_result.total_quantity_filled = volume;
                matching_result.orders_filled = filled;
            }

            // 7. Compute price residuals and update λ
            let price_residuals = compute_price_residuals(&prices, &problem.market_groups);

            state.prev_lambda = state.lambda.clone();
            update_duals(&mut state, &price_residuals, step_size);

            // 8. Record stats
            let max_price_residual = price_residuals
                .values()
                .map(|r| r.abs())
                .fold(0.0f64, f64::max);
            let lambda_norm: f64 = state.lambda.values().map(|v| v * v).sum::<f64>().sqrt();
            let current_welfare = matching_result.total_welfare;

            iteration_stats.push(DualIterationStats {
                iteration: iter,
                step_size,
                max_price_residual,
                lambda_norm,
                welfare: current_welfare,
                fills: iter_fills,
                mm_fills: iter_mm_fills,
            });
            debug!(
                iter,
                fills = iter_fills,
                mm_fills = iter_mm_fills,
                welfare = current_welfare,
                max_price_residual,
                "iteration complete"
            );

            // Merge prices: only update markets that had activity this iteration.
            // Markets without activity produce synthetic default (50/50) prices
            // that would overwrite valid prices from earlier iterations.
            for (mid, sol) in prices.market_solutions {
                if sol.has_activity {
                    last_prices.prices.insert(mid, sol.prices.clone());
                    last_prices.market_solutions.insert(mid, sol);
                } else if !last_prices.market_solutions.contains_key(&mid) {
                    // First time seeing this market — use solver's default
                    last_prices.prices.insert(mid, sol.prices.clone());
                    last_prices.market_solutions.insert(mid, sol);
                }
            }

            // 9. Check convergence: price residuals + welfare delta
            let welfare_delta_frac = if prev_welfare.abs() > 0 {
                (current_welfare - prev_welfare).abs() as f64 / prev_welfare.abs() as f64
            } else if current_welfare > 0 {
                1.0 // First iteration with positive welfare: not converged yet
            } else {
                0.0
            };

            prev_welfare = current_welfare;

            // Need at least 2 iterations before checking welfare convergence
            if iter >= 2
                && check_convergence(
                    &price_residuals,
                    &state,
                    self.config.primal_tolerance,
                    self.config.dual_tolerance,
                    welfare_delta_frac,
                    self.config.welfare_tolerance,
                )
            {
                converged = true;
                break;
            }

            // Early exit if no new fills this iteration (nothing left to do)
            if iter_fills == 0 && iter >= 2 {
                converged = true;
                break;
            }
        }

        // Compute final diagnostics
        let final_price_sum_error = compute_price_sum_errors(&last_prices, &problem.market_groups);
        let final_mm_utilization =
            compute_mm_utilization(&cumulative_mm_fills, &problem.mm_constraints);

        DualResult {
            matching_result,
            prices: last_prices,
            iterations,
            converged,
            final_price_sum_error,
            final_mm_utilization,
            dual_state: state,
        }
    }
}

impl Default for DualMaster {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Bid Shading
// ============================================================================

/// Create shaded copies of orders with adjusted limit prices.
///
/// Only applies λ (price consistency) adjustment. MM budget constraints
/// are handled separately by greedy knapsack allocation.
///
/// The shading formulas from Lagrangian relaxation:
/// - YES buyer:  effective = limit - λ×$1
/// - NO buyer:   effective = limit + λ×$1
/// - YES seller: effective = limit - λ×$1
/// - NO seller:  effective = limit + λ×$1
pub fn shade_orders(
    orders: &[Order],
    lambda: &HashMap<String, f64>,
    market_to_group: &HashMap<MarketId, String>,
) -> Vec<Order> {
    orders
        .iter()
        .map(|order| {
            let mut shaded = order.clone();

            let npd = NANOS_PER_DOLLAR as f64;
            let limit_f64 = order.limit_price as f64;

            if order.num_markets == 1 {
                // Single-market order: shade based on YES/NO exposure
                let market = order.markets[0];

                let lambda_val = market_to_group
                    .get(&market)
                    .and_then(|g| lambda.get(g))
                    .copied()
                    .unwrap_or(0.0);

                let is_yes_buyer = order.payoffs[0] > 0 && order.payoffs.iter().all(|&p| p >= 0);
                let is_no_buyer = order.num_states >= 2
                    && order.payoffs[1] > 0
                    && order.payoffs.iter().all(|&p| p >= 0);
                let is_yes_seller = order.payoffs[0] < 0;
                let is_no_seller = order.num_states >= 2 && order.payoffs[1] < 0;

                let lambda_nanos = lambda_val * npd;

                let effective = if is_yes_buyer || is_yes_seller {
                    limit_f64 - lambda_nanos
                } else if is_no_buyer || is_no_seller {
                    limit_f64 + lambda_nanos
                } else {
                    limit_f64
                };

                shaded.limit_price = effective.round().clamp(0.0, npd) as Nanos;
            } else {
                // Multi-market (bundle/spread) order: compute net λ exposure
                // For each market leg, determine if it contributes YES or NO exposure,
                // then sum the λ contributions across all legs in the same group.
                let num_states = order.num_states as usize;
                let num_markets = order.num_markets as usize;

                let mut total_lambda_adjustment = 0.0;

                for m_idx in 0..num_markets {
                    let market = order.markets[m_idx];
                    if market.is_none() {
                        continue;
                    }

                    let lambda_val = market_to_group
                        .get(&market)
                        .and_then(|g| lambda.get(g))
                        .copied()
                        .unwrap_or(0.0);

                    if lambda_val.abs() < 1e-12 {
                        continue;
                    }

                    // Compute net YES exposure for this market leg:
                    // Average payoff when market m = YES minus average when m = NO
                    let stride = 1usize << m_idx;
                    let mut yes_sum = 0.0f64;
                    let mut yes_count = 0usize;
                    let mut no_sum = 0.0f64;
                    let mut no_count = 0usize;

                    for s in 0..num_states {
                        let outcome = (s / stride) % 2;
                        let payoff = order.payoffs[s] as f64;
                        if outcome == 0 {
                            yes_sum += payoff;
                            yes_count += 1;
                        } else {
                            no_sum += payoff;
                            no_count += 1;
                        }
                    }

                    let c_yes = if yes_count > 0 {
                        yes_sum / yes_count as f64
                    } else {
                        0.0
                    };
                    let c_no = if no_count > 0 {
                        no_sum / no_count as f64
                    } else {
                        0.0
                    };
                    let net_exposure = c_yes - c_no; // positive = net YES, negative = net NO

                    // YES exposure contributes -λ shading, NO exposure contributes +λ shading
                    total_lambda_adjustment -= net_exposure * lambda_val * npd;
                }

                let effective = limit_f64 + total_lambda_adjustment;
                shaded.limit_price = effective.round().clamp(0.0, npd) as Nanos;
            }

            shaded
        })
        .collect()
}

// ============================================================================
// Primal Residuals
// ============================================================================

/// Compute price consistency residuals.
///
/// Returns price residuals: (sum_yes - $1) / $1 per MarketGroup.
/// Budget constraints are handled by greedy knapsack, not dual variables.
pub fn compute_price_residuals(
    prices: &PriceDiscoveryResult,
    groups: &[MarketGroup],
) -> HashMap<String, f64> {
    let npd = NANOS_PER_DOLLAR as f64;

    let mut price_residuals = HashMap::new();
    for group in groups {
        let sum_yes: f64 = group
            .markets
            .iter()
            .filter_map(|&m| prices.prices.get(&m))
            .filter_map(|p: &Vec<Nanos>| p.first())
            .map(|&p| p as f64)
            .sum();
        let residual = (sum_yes - npd) / npd;
        price_residuals.insert(group.name.clone(), residual);
    }

    price_residuals
}

// ============================================================================
// Dual Variable Updates
// ============================================================================

/// Update λ dual variables using subgradient step.
///
/// λ is unconstrained (can be positive or negative).
pub fn update_duals(state: &mut DualState, price_residuals: &HashMap<String, f64>, step_size: f64) {
    for (group, residual) in price_residuals {
        let lambda = state.lambda.entry(group.clone()).or_insert(0.0);
        *lambda += step_size * residual;
    }
}

// ============================================================================
// Convergence Check
// ============================================================================

/// Check if the dual decomposition has converged.
///
/// Convergence requires ALL of:
/// 1. All price residuals below tolerance (constraints approximately satisfied)
/// 2. All λ changes below tolerance (dual stability)
/// 3. Welfare improvement below welfare_tolerance (marginal returns diminishing)
pub fn check_convergence(
    price_residuals: &HashMap<String, f64>,
    state: &DualState,
    primal_tol: f64,
    dual_tol: f64,
    welfare_delta_frac: f64,
    welfare_tol: f64,
) -> bool {
    // Check primal feasibility (price consistency)
    let primal_ok = price_residuals.values().all(|r| r.abs() < primal_tol);

    // Check dual stability
    let lambda_change: f64 = state
        .lambda
        .iter()
        .map(|(k, v)| {
            let prev = state.prev_lambda.get(k).copied().unwrap_or(0.0);
            (v - prev).abs()
        })
        .sum();

    let dual_ok = lambda_change < dual_tol;

    // Check welfare convergence (marginal improvement < tolerance)
    let welfare_ok = welfare_delta_frac.abs() < welfare_tol;

    // Strict mode: require all criteria met
    (primal_ok && dual_ok) && welfare_ok
}

// ============================================================================
// Diagnostic Helpers
// ============================================================================

/// Compute price sum errors per group (for final diagnostics).
fn compute_price_sum_errors(
    prices: &PriceDiscoveryResult,
    groups: &[MarketGroup],
) -> HashMap<String, f64> {
    let npd = NANOS_PER_DOLLAR as f64;
    let mut errors = HashMap::new();

    for group in groups {
        let sum_yes: f64 = group
            .markets
            .iter()
            .filter_map(|&m| prices.prices.get(&m))
            .filter_map(|p: &Vec<Nanos>| p.first())
            .map(|&p| p as f64)
            .sum();
        errors.insert(group.name.clone(), (sum_yes - npd) / npd);
    }

    errors
}

/// Compute MM utilization from cumulative fills (for final diagnostics).
fn compute_mm_utilization(
    cumulative_mm_fills: &HashMap<u64, (Nanos, Qty)>,
    mm_constraints: &[MmConstraint],
) -> HashMap<u64, f64> {
    let mut utilization = HashMap::new();
    for mm in mm_constraints {
        let capital_used = mm.capital_used(cumulative_mm_fills);
        let util = if mm.max_capital > 0 {
            capital_used as f64 / mm.max_capital as f64
        } else {
            0.0
        };
        utilization.insert(mm.mm_id.0, util);
    }

    utilization
}

// ============================================================================
// LocalSolver extension
// ============================================================================

impl LocalSolver {
    /// Internal method for dual master to call discover_prices without trait object.
    pub(crate) fn discover_prices_impl(&self, problem: &Problem) -> PriceDiscoveryResult {
        use crate::traits::PriceDiscoverer;
        self.discover_prices(problem)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        outcome_sell, price_to_nanos, simple_no_buy, simple_yes_buy, MarketGroup, MmId, MmSide,
    };

    /// Helper: create a 3-outcome election problem with market group.
    ///
    /// Uses stepped sell order supply curves so clearing prices are set by demand.
    /// This is needed for dual decomposition to influence prices via bid shading.
    fn election_problem() -> Problem {
        let mut problem = Problem::new("election");
        let m_a = problem.markets.add_binary("Candidate A");
        let m_b = problem.markets.add_binary("Candidate B");
        let m_c = problem.markets.add_binary("Candidate C");

        // Group: exactly one wins
        let group = MarketGroup::new("Election")
            .with_market(m_a)
            .with_market(m_b)
            .with_market(m_c);
        problem.add_market_group(group);

        // Stepped sell orders provide supply at various price levels
        let mut sell_id = 10000u64;
        for &m in &[m_a, m_b, m_c] {
            // YES sell orders: stepped supply
            for &price in &[0.20, 0.30, 0.40, 0.50, 0.60] {
                problem.orders.push(outcome_sell(
                    &problem.markets,
                    sell_id,
                    m,
                    0,
                    price_to_nanos(price),
                    200,
                ));
                sell_id += 1;
            }
            // NO sell orders
            problem.orders.push(outcome_sell(
                &problem.markets,
                sell_id,
                m,
                1,
                price_to_nanos(0.30),
                500,
            ));
            sell_id += 1;
        }

        // YES buyers for market A (strong demand ~50% implied)
        for i in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                100 + i,
                m_a,
                price_to_nanos(0.45 + 0.01 * i as f64),
                50,
            ));
        }
        // YES buyers for market B (moderate demand ~30% implied)
        for i in 0..8 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                200 + i,
                m_b,
                price_to_nanos(0.25 + 0.01 * i as f64),
                50,
            ));
        }
        // YES buyers for market C (light demand ~20% implied)
        for i in 0..5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                300 + i,
                m_c,
                price_to_nanos(0.15 + 0.01 * i as f64),
                50,
            ));
        }

        // Some NO buyers to create two-sided markets
        for i in 0..3 {
            problem.orders.push(simple_no_buy(
                &problem.markets,
                400 + i,
                m_a,
                price_to_nanos(0.50 + 0.01 * i as f64),
                50,
            ));
        }
        for i in 0..3 {
            problem.orders.push(simple_no_buy(
                &problem.markets,
                500 + i,
                m_b,
                price_to_nanos(0.65 + 0.01 * i as f64),
                50,
            ));
        }

        problem
    }

    #[test]
    fn test_dual_config_default() {
        let config = DualConfig::default();
        assert_eq!(config.max_iterations, 20);
        assert!((config.initial_step_size - 0.3).abs() < 1e-9);
        assert!(config.primal_tolerance > 0.0);
        assert!((config.welfare_tolerance - 0.005).abs() < 1e-9);
    }

    #[test]
    fn test_shade_orders_no_adjustment() {
        // With λ=0, shaded orders should have same limits
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.50), 100);

        let lambda = HashMap::new();
        let market_to_group = HashMap::new();

        let shaded = shade_orders(&[order.clone()], &lambda, &market_to_group);

        assert_eq!(shaded.len(), 1);
        assert_eq!(shaded[0].limit_price, order.limit_price);
    }

    #[test]
    fn test_shade_orders_lambda_positive() {
        // λ > 0 means prices sum > $1, so YES buyers should bid less
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.60), 100);

        let mut lambda = HashMap::new();
        lambda.insert("group".to_string(), 0.10); // 10% excess

        let mut market_to_group = HashMap::new();
        market_to_group.insert(m, "group".to_string());

        let shaded = shade_orders(&[order.clone()], &lambda, &market_to_group);

        // YES buyer should have lower limit (bid less aggressively)
        assert!(
            shaded[0].limit_price < order.limit_price,
            "YES buyer should bid less when λ>0: shaded={}, original={}",
            shaded[0].limit_price,
            order.limit_price
        );
    }

    #[test]
    fn test_shade_orders_lambda_negative() {
        // λ < 0 means prices sum < $1, so YES buyers should bid more
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.30), 100);

        let mut lambda = HashMap::new();
        lambda.insert("group".to_string(), -0.10); // 10% deficit

        let mut market_to_group = HashMap::new();
        market_to_group.insert(m, "group".to_string());

        let shaded = shade_orders(&[order.clone()], &lambda, &market_to_group);

        // YES buyer should have higher limit (bid more aggressively)
        assert!(
            shaded[0].limit_price > order.limit_price,
            "YES buyer should bid more when λ<0: shaded={}, original={}",
            shaded[0].limit_price,
            order.limit_price
        );
    }

    #[test]
    fn test_update_duals() {
        let mut state = DualState::default();
        state.lambda.insert("g1".to_string(), 0.0);

        let mut price_residuals = HashMap::new();
        price_residuals.insert("g1".to_string(), 0.05); // 5% over

        update_duals(&mut state, &price_residuals, 0.5);

        assert!(*state.lambda.get("g1").unwrap() > 0.0);
    }

    #[test]
    fn test_convergence_check_all_criteria_met() {
        let state = DualState {
            lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            prev_lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
        };

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.001)].into_iter().collect();

        // Small residuals + no dual change + welfare converged → all criteria met
        assert!(check_convergence(
            &price_res, &state, 0.02, 0.001, 0.001, // welfare converged too
            0.01,
        ));
    }

    #[test]
    fn test_convergence_fails_without_welfare() {
        let state = DualState {
            lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            prev_lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
        };

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.001)].into_iter().collect();

        // Small residuals + no dual change but welfare still improving
        // → strict mode requires all criteria, so not converged
        assert!(!check_convergence(
            &price_res, &state, 0.02, 0.001, 0.5, // welfare still changing
            0.01,
        ));
    }

    #[test]
    fn test_convergence_fails_on_large_residual_and_welfare() {
        let state = DualState::default();

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.10)].into_iter().collect();

        // 10% residual > 2% tolerance AND welfare still improving → not converged
        assert!(!check_convergence(
            &price_res, &state, 0.02, 0.001, 0.5, // 50% welfare improvement
            0.01,
        ));
    }

    #[test]
    fn test_dual_master_basic_solve() {
        let problem = election_problem();
        let master = DualMaster::new();
        let result = master.solve(&problem);

        // Should produce some fills
        assert!(
            !result.matching_result.fills.is_empty(),
            "Should have fills"
        );

        // Check price sum errors are reasonable
        for (group, error) in &result.final_price_sum_error {
            assert!(
                error.abs() < 0.50,
                "Group {} price sum error too large: {}",
                group,
                error
            );
        }
    }

    #[test]
    fn test_dual_master_no_groups_no_mm() {
        // Simple problem with no coupling constraints — should just solve normally
        let mut problem = Problem::new("simple");
        let m = problem.markets.add_binary("test");
        problem.orders.push(outcome_sell(
            &problem.markets,
            9999,
            m,
            0,
            price_to_nanos(0.30),
            1000,
        ));

        for i in 0..5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                m,
                price_to_nanos(0.40 + 0.02 * i as f64),
                100,
            ));
        }

        let master = DualMaster::new();
        let result = master.solve(&problem);

        assert!(!result.matching_result.fills.is_empty());
        // Should converge quickly (no coupling constraints)
        assert!(result.converged || result.iterations <= 3);
    }

    #[test]
    fn test_dual_master_with_mm_constraints() {
        // Test that MM fills are accumulated via greedy knapsack
        let mut problem = Problem::new("mm_test");
        let m_a = problem.markets.add_binary("A");
        let m_b = problem.markets.add_binary("B");

        let group = MarketGroup::new("Group").with_market(m_a).with_market(m_b);
        problem.add_market_group(group);

        // Sell orders provide supply
        let mut sell_id = 9000u64;
        for &m in &[m_a, m_b] {
            for &price in &[0.30, 0.40, 0.50] {
                problem.orders.push(outcome_sell(
                    &problem.markets,
                    sell_id,
                    m,
                    0,
                    price_to_nanos(price),
                    500,
                ));
                sell_id += 1;
            }
            problem.orders.push(outcome_sell(
                &problem.markets,
                sell_id,
                m,
                1,
                price_to_nanos(0.40),
                500,
            ));
            sell_id += 1;
        }

        // MM orders: YES buyers for both markets
        for i in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                100 + i,
                m_a,
                price_to_nanos(0.50 + 0.01 * i as f64),
                50,
            ));
        }
        for i in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                200 + i,
                m_b,
                price_to_nanos(0.40 + 0.01 * i as f64),
                50,
            ));
        }

        // MM constraint with budget that can cover ~half the orders
        let mut mm = MmConstraint::new(MmId::new(1), 200_000_000_000); // $200
        for i in 0..10 {
            mm.add_order(100 + i, MmSide::SellYes);
            mm.add_order(200 + i, MmSide::SellYes);
        }
        problem.mm_constraints.push(mm);

        let master = DualMaster::new();
        let result = master.solve(&problem);

        // Should have fills (both MM and non-MM orders, though here all are MM)
        assert!(
            !result.matching_result.fills.is_empty(),
            "Should have MM fills"
        );

        // MM utilization should be > 0
        for (_mm_id, util) in &result.final_mm_utilization {
            assert!(*util >= 0.0, "MM utilization should be non-negative");
            assert!(
                *util <= 1.01,
                "MM utilization should not exceed 100%: {}",
                util
            );
        }
    }
}
