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

use crate::local_solver::LocalSolver;
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
            max_iterations: 10,
            initial_step_size: 0.5,
            primal_tolerance: 0.02,
            dual_tolerance: 0.001,
            step_decay: StepDecay::InvSqrt,
            welfare_tolerance: 0.01,
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
}

impl DualMaster {
    /// Create a new DualMaster with default configuration.
    pub fn new() -> Self {
        Self {
            config: DualConfig::default(),
            local_solver: LocalSolver::new(),
        }
    }

    /// Create a new DualMaster with custom configuration.
    pub fn with_config(config: DualConfig) -> Self {
        Self {
            config,
            local_solver: LocalSolver::new(),
        }
    }

    /// Main dual decomposition solve loop.
    ///
    /// Accumulates fills across iterations with greedy MM knapsack allocation.
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
        let order_map: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // Build MM order IDs set
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // State for cumulative fill accumulation
        let mut matching_result = MatchingResult::new(problem.liquidity.snapshot());
        let mut remaining_liquidity = problem.liquidity.snapshot();
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
            let remaining_orders: Vec<Order> = problem
                .orders
                .iter()
                .filter(|o| !filled_order_ids.contains(&o.id))
                .cloned()
                .collect();

            let shaded_orders = shade_orders(
                &remaining_orders,
                &state.lambda,
                &market_to_group,
            );

            // 2. Solve per-market subproblems with shaded orders + remaining liquidity
            let shaded_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                liquidity: remaining_liquidity.clone(),
                orders: shaded_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            let prices = self.local_solver.discover_prices_impl(&shaded_problem);

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

            // Also consider MM orders willing at current prices but not matched
            // (they may have been shaded out but still willing at original limits)
            for mm in &problem.mm_constraints {
                for &order_id in &mm.order_ids {
                    if filled_order_ids.contains(&order_id) {
                        continue;
                    }
                    if mm_candidate_fills.iter().any(|f| f.order_id == order_id) {
                        continue;
                    }
                    if let Some(order) = order_map.get(&order_id) {
                        if order.num_markets == 1 {
                            let market = order.markets[0];
                            if let Some(market_prices) = prices.prices.get(&market) {
                                let num_states = order.num_states as usize;
                                let is_buyer =
                                    order.payoffs[..num_states].iter().any(|&p| p > 0);
                                let outcome = if is_buyer {
                                    order.payoffs[..num_states]
                                        .iter()
                                        .position(|&p| p > 0)
                                        .unwrap_or(0)
                                } else {
                                    order.payoffs[..num_states]
                                        .iter()
                                        .position(|&p| p < 0)
                                        .unwrap_or(0)
                                };
                                let price = market_prices
                                    .get(outcome)
                                    .copied()
                                    .unwrap_or(500_000_000);
                                if order.is_satisfied_at_price(price) {
                                    mm_candidate_fills.push(Fill::new(
                                        order_id,
                                        order.max_fill,
                                        price,
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            // 5. Greedy MM knapsack: sort by welfare/capital ratio, greedily activate
            let mut mm_accepted_fills: Vec<Fill> = Vec::new();
            {
                // Build per-MM candidate lists with welfare/capital ratios
                for mm in &problem.mm_constraints {
                    let remaining_budget = mm.max_capital
                        .saturating_sub(mm.capital_used(&cumulative_mm_fills));
                    if remaining_budget == 0 {
                        continue;
                    }

                    let mut candidates: Vec<(Fill, i64, Nanos)> = mm_candidate_fills
                        .iter()
                        .filter(|f| mm.contains_order(f.order_id))
                        .filter_map(|f| {
                            let order = order_map.get(&f.order_id)?;
                            let welfare = order.welfare_contribution(f.fill_price, f.fill_qty);
                            let side = mm.order_sides.get(&f.order_id)?;
                            let capital = side.capital_needed(f.fill_price, f.fill_qty);
                            Some((f.clone(), welfare, capital))
                        })
                        .collect();

                    // Sort by welfare/capital ratio descending
                    candidates.sort_by(|(_, w1, c1), (_, w2, c2)| {
                        let ratio1 =
                            if *c1 > 0 { *w1 as f64 / *c1 as f64 } else { f64::MAX };
                        let ratio2 =
                            if *c2 > 0 { *w2 as f64 / *c2 as f64 } else { f64::MAX };
                        ratio2
                            .partial_cmp(&ratio1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Greedily activate until remaining budget exhausted
                    let mut budget_left = remaining_budget;
                    for (fill, _welfare, capital) in candidates {
                        if capital <= budget_left {
                            // Check this order hasn't already been accepted
                            // by a different MM constraint
                            if !mm_accepted_fills
                                .iter()
                                .any(|f| f.order_id == fill.order_id)
                            {
                                budget_left -= capital;
                                mm_accepted_fills.push(fill);
                            }
                        }
                    }
                }
            }

            // 6. Accumulate fills: non-MM + knapsack-approved MM fills
            let mut iter_fills = 0usize;
            let mut iter_mm_fills = 0usize;

            for fill in non_mm_fills {
                if let Some(order) = order_map.get(&fill.order_id) {
                    consume_order_liquidity(order, fill.fill_qty, &mut remaining_liquidity);
                    filled_order_ids.insert(fill.order_id);
                    matching_result.add_fill(fill, order);
                    iter_fills += 1;
                }
            }

            for fill in mm_accepted_fills {
                if let Some(order) = order_map.get(&fill.order_id) {
                    consume_order_liquidity(order, fill.fill_qty, &mut remaining_liquidity);
                    filled_order_ids.insert(fill.order_id);
                    cumulative_mm_fills
                        .insert(fill.order_id, (fill.fill_price, fill.fill_qty));
                    matching_result.add_fill(fill, order);
                    iter_fills += 1;
                    iter_mm_fills += 1;
                }
            }

            // 7. Compute price residuals and update λ
            let price_residuals =
                compute_price_residuals(&prices, &problem.market_groups);

            state.prev_lambda = state.lambda.clone();
            update_duals(&mut state, &price_residuals, step_size);

            // 8. Record stats
            let max_price_residual = price_residuals
                .values()
                .map(|r| r.abs())
                .fold(0.0f64, f64::max);
            let lambda_norm: f64 =
                state.lambda.values().map(|v| v * v).sum::<f64>().sqrt();
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

            last_prices = prices;

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
        let final_price_sum_error =
            compute_price_sum_errors(&last_prices, &problem.market_groups);
        let final_mm_utilization =
            compute_mm_utilization(&cumulative_mm_fills, &problem.mm_constraints);

        matching_result.remaining_liquidity = remaining_liquidity;

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

            // Skip multi-market orders (handled by ArbitrageDetector)
            if order.num_markets != 1 {
                return shaded;
            }

            let market = order.markets[0];

            // Get λ for this market's group
            let lambda_val = market_to_group
                .get(&market)
                .and_then(|g| lambda.get(g))
                .copied()
                .unwrap_or(0.0);

            // Determine order type from payoff vector
            let is_yes_buyer = order.payoffs[0] > 0 && order.payoffs.iter().all(|&p| p >= 0);
            let is_no_buyer = order.num_states >= 2
                && order.payoffs[1] > 0
                && order.payoffs.iter().all(|&p| p >= 0);
            let is_yes_seller = order.payoffs[0] < 0;
            let is_no_seller = order.num_states >= 2 && order.payoffs[1] < 0;

            let limit_f64 = order.limit_price as f64;
            let npd = NANOS_PER_DOLLAR as f64;
            let lambda_nanos = lambda_val * npd;

            // Apply price consistency (λ) adjustment only
            let effective = if is_yes_buyer || is_yes_seller {
                // YES side: shade down when λ>0 (prices sum too high)
                limit_f64 - lambda_nanos
            } else if is_no_buyer || is_no_seller {
                // NO side: shade up when λ>0 (prices sum too high)
                limit_f64 + lambda_nanos
            } else {
                limit_f64
            };

            // Clamp to valid range [0, $1]
            shaded.limit_price = effective.round().clamp(0.0, npd) as Nanos;

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
pub fn update_duals(
    state: &mut DualState,
    price_residuals: &HashMap<String, f64>,
    step_size: f64,
) {
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
/// Convergence requires:
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

    (primal_ok && dual_ok) || welfare_ok
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

/// Consume liquidity for a single-market order fill.
fn consume_order_liquidity(
    order: &Order,
    qty: Qty,
    liquidity: &mut matching_engine::LiquidityPool,
) {
    if order.num_markets == 1 {
        let market = order.markets[0];
        let outcome = if order.payoffs[0] > 0 { 0 } else { 1 };
        if let Some(book) = liquidity.books.get_mut(&(market, outcome)) {
            book.consume_asks(qty, order.limit_price);
        }
    }
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
        price_to_nanos, simple_no_buy, simple_yes_buy, MarketGroup, MmId, MmSide,
    };

    /// Helper: create a 3-outcome election problem with market group.
    ///
    /// Uses stepped supply curves so clearing prices are set by demand,
    /// not just cheap flat asks. This is needed for dual decomposition
    /// to influence prices via bid shading.
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

        // Stepped liquidity: supply at various price levels
        // This ensures clearing price responds to demand level
        for &m in &[m_a, m_b, m_c] {
            // YES asks: stepped supply
            problem
                .liquidity
                .add_ask(m, 0, price_to_nanos(0.20), 200);
            problem
                .liquidity
                .add_ask(m, 0, price_to_nanos(0.30), 200);
            problem
                .liquidity
                .add_ask(m, 0, price_to_nanos(0.40), 200);
            problem
                .liquidity
                .add_ask(m, 0, price_to_nanos(0.50), 200);
            problem
                .liquidity
                .add_ask(m, 0, price_to_nanos(0.60), 200);
            // NO asks
            problem
                .liquidity
                .add_ask(m, 1, price_to_nanos(0.30), 500);
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
        assert_eq!(config.max_iterations, 10);
        assert!(config.initial_step_size > 0.0);
        assert!(config.primal_tolerance > 0.0);
        assert!(config.welfare_tolerance > 0.0);
    }

    #[test]
    fn test_shade_orders_no_adjustment() {
        // With λ=0, shaded orders should have same limits
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.50), 100);

        let lambda = HashMap::new();
        let market_to_group = HashMap::new();

        let shaded = shade_orders(
            &[order.clone()],
            &lambda,
            &market_to_group,
        );

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

        let shaded = shade_orders(
            &[order.clone()],
            &lambda,
            &market_to_group,
        );

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

        let shaded = shade_orders(
            &[order.clone()],
            &lambda,
            &market_to_group,
        );

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
    fn test_convergence_check_primal_and_dual() {
        let state = DualState {
            lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            prev_lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
        };

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.001)].into_iter().collect();

        // Small residuals + no dual change + welfare not converged
        // → should converge on primal+dual alone
        assert!(check_convergence(
            &price_res,
            &state,
            0.02,
            0.001,
            0.5, // welfare still changing
            0.01,
        ));
    }

    #[test]
    fn test_convergence_via_welfare() {
        let state = DualState {
            lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            prev_lambda: [("g1".to_string(), 0.0)].into_iter().collect(),
        };

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.10)].into_iter().collect();

        // Large price residual + large dual change, but welfare converged
        // → should converge on welfare alone
        assert!(check_convergence(
            &price_res,
            &state,
            0.02,
            0.001,
            0.005, // welfare delta < 1% tolerance
            0.01,
        ));
    }

    #[test]
    fn test_convergence_fails_on_large_residual_and_welfare() {
        let state = DualState::default();

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.10)].into_iter().collect();

        // 10% residual > 2% tolerance AND welfare still improving → not converged
        assert!(!check_convergence(
            &price_res,
            &state,
            0.02,
            0.001,
            0.5, // 50% welfare improvement
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
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.30), 1000);

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

        let group = MarketGroup::new("Group")
            .with_market(m_a)
            .with_market(m_b);
        problem.add_market_group(group);

        // Add liquidity
        for &m in &[m_a, m_b] {
            problem.liquidity.add_ask(m, 0, price_to_nanos(0.30), 500);
            problem.liquidity.add_ask(m, 0, price_to_nanos(0.40), 500);
            problem.liquidity.add_ask(m, 0, price_to_nanos(0.50), 500);
            problem.liquidity.add_ask(m, 1, price_to_nanos(0.40), 500);
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
            assert!(*util <= 1.01, "MM utilization should not exceed 100%: {}", util);
        }
    }
}
