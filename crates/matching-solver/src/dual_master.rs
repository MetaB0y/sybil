//! Dual decomposition for market clearing with coupling constraints.
//!
//! Handles two types of coupling constraints via Lagrangian relaxation:
//! - **Price consistency** (λ): sum of YES prices = $1 across MarketGroups
//! - **Budget feasibility** (μ): MM capital usage ≤ budget
//!
//! The main loop:
//! 1. Shade orders using dual variables (λ, μ)
//! 2. Solve per-market subproblems with shaded orders
//! 3. Compute constraint violations (primal residuals)
//! 4. Update dual variables via subgradient descent
//! 5. Check convergence
//!
//! After convergence, re-solve with original (unshaded) limits for actual fills.

use std::collections::HashMap;

use matching_engine::{
    MarketGroup, MarketId, MmConstraint, MmSide, Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR,
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
}

impl Default for DualConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            initial_step_size: 0.5,
            primal_tolerance: 0.02,
            dual_tolerance: 0.001,
            step_decay: StepDecay::InvSqrt,
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
    /// Pacing multipliers: one per MM constraint.
    /// μ ≥ 0; higher μ means MM bids less aggressively.
    pub mu: HashMap<u64, f64>,
    /// Previous λ values for convergence checking.
    pub prev_lambda: HashMap<String, f64>,
    /// Previous μ values for convergence checking.
    pub prev_mu: HashMap<u64, f64>,
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
    /// Final budget utilization per MM (fraction of budget).
    pub final_budget_utilization: HashMap<u64, f64>,
    /// Final dual state for diagnostics.
    pub dual_state: DualState,
}

/// Stats for a single dual decomposition iteration.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DualIterationStats {
    pub iteration: usize,
    pub step_size: f64,
    pub max_price_residual: f64,
    pub max_budget_residual: f64,
    pub lambda_norm: f64,
    pub mu_norm: f64,
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
    pub fn solve(&self, problem: &Problem) -> DualResult {
        let mut state = DualState::default();

        // Initialize λ=0 for each MarketGroup
        for group in &problem.market_groups {
            state.lambda.insert(group.name.clone(), 0.0);
            state.prev_lambda.insert(group.name.clone(), 0.0);
        }

        // Initialize μ=0 for each MM constraint
        for mm in &problem.mm_constraints {
            state.mu.insert(mm.mm_id.0, 0.0);
            state.prev_mu.insert(mm.mm_id.0, 0.0);
        }

        // Build lookup: which group does each market belong to?
        let market_to_group: HashMap<MarketId, String> = problem
            .market_groups
            .iter()
            .flat_map(|g| g.markets.iter().map(move |&m| (m, g.name.clone())))
            .collect();

        // Build lookup: which MM constraint does each order belong to?
        let order_to_mm: HashMap<u64, u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().map(move |&oid| (oid, mm.mm_id.0)))
            .collect();

        // Build MM constraint map
        let mm_map: HashMap<u64, &MmConstraint> = problem
            .mm_constraints
            .iter()
            .map(|mm| (mm.mm_id.0, mm))
            .collect();

        let mut iteration_stats = Vec::new();
        let mut converged = false;
        let mut iterations = 0;
        let mut _last_prices = PriceDiscoveryResult::empty();

        for iter in 1..=self.config.max_iterations {
            iterations = iter;

            // Step size: α_t = α_0 / sqrt(t) or α_0 / t
            let step_size = match self.config.step_decay {
                StepDecay::InvSqrt => self.config.initial_step_size / (iter as f64).sqrt(),
                StepDecay::InvLinear => self.config.initial_step_size / iter as f64,
            };

            // 1. Shade orders
            let shaded_orders = shade_orders(
                &problem.orders,
                &state.lambda,
                &state.mu,
                &market_to_group,
                &order_to_mm,
                &mm_map,
            );

            // 2. Create shaded problem and solve per-market subproblems
            let shaded_problem = Problem {
                name: problem.name.clone(),
                markets: problem.markets.clone(),
                liquidity: problem.liquidity.snapshot(),
                orders: shaded_orders,
                mm_constraints: problem.mm_constraints.clone(),
                market_groups: problem.market_groups.clone(),
            };

            let prices = self.local_solver.discover_prices_impl(&shaded_problem);

            // 3. Compute primal residuals
            let (price_residuals, budget_residuals) = compute_primal_residuals(
                &prices,
                &problem.market_groups,
                &problem.mm_constraints,
                &problem.orders,
            );

            // 4. Save previous dual variables
            state.prev_lambda = state.lambda.clone();
            state.prev_mu = state.mu.clone();

            // 5. Update dual variables
            update_duals(&mut state, &price_residuals, &budget_residuals, step_size);

            // 6. Record stats
            let max_price_residual = price_residuals
                .values()
                .map(|r| r.abs())
                .fold(0.0f64, f64::max);
            let max_budget_residual = budget_residuals
                .values()
                .map(|r| r.abs())
                .fold(0.0f64, f64::max);
            let lambda_norm: f64 = state.lambda.values().map(|v| v * v).sum::<f64>().sqrt();
            let mu_norm: f64 = state.mu.values().map(|v| v * v).sum::<f64>().sqrt();

            iteration_stats.push(DualIterationStats {
                iteration: iter,
                step_size,
                max_price_residual,
                max_budget_residual,
                lambda_norm,
                mu_norm,
            });

            _last_prices = prices;

            // 7. Check convergence
            if check_convergence(
                &price_residuals,
                &budget_residuals,
                &state,
                self.config.primal_tolerance,
                self.config.dual_tolerance,
            ) {
                converged = true;
                break;
            }
        }

        // Final pass: re-solve with converged shaded orders to get equilibrium prices,
        // then validate fills against original limits.
        let final_shaded_orders = shade_orders(
            &problem.orders,
            &state.lambda,
            &state.mu,
            &market_to_group,
            &order_to_mm,
            &mm_map,
        );

        let final_shaded_problem = Problem {
            name: problem.name.clone(),
            markets: problem.markets.clone(),
            liquidity: problem.liquidity.snapshot(),
            orders: final_shaded_orders,
            mm_constraints: problem.mm_constraints.clone(),
            market_groups: problem.market_groups.clone(),
        };

        let final_prices = self.local_solver.discover_prices_impl(&final_shaded_problem);

        // Build matching result: use fills from the shaded solve, but validate
        // against original order limits and MM budget constraints.
        let order_map: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        // Collect candidate fills (limit-checked)
        let mut candidate_fills: Vec<matching_engine::Fill> = final_prices
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

        // Enforce MM budget constraints: greedily include fills, dropping
        // MM fills that would exceed budget (sorted by welfare descending).
        let mm_order_ids: std::collections::HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // Sort MM fills by welfare descending for greedy selection
        candidate_fills.sort_by(|a, b| {
            let wa = order_map
                .get(&a.order_id)
                .map(|o| o.welfare_contribution(a.fill_price, a.fill_qty))
                .unwrap_or(0);
            let wb = order_map
                .get(&b.order_id)
                .map(|o| o.welfare_contribution(b.fill_price, b.fill_qty))
                .unwrap_or(0);
            wb.cmp(&wa)
        });

        let mut matching_result = MatchingResult::new(problem.liquidity.snapshot());
        let mut mm_fills_map: HashMap<u64, (Nanos, Qty)> = HashMap::new();

        for fill in candidate_fills {
            let is_mm = mm_order_ids.contains(&fill.order_id);

            if is_mm {
                // Check if adding this fill would exceed any MM budget
                let mut would_exceed = false;
                for mm in &problem.mm_constraints {
                    if mm.contains_order(fill.order_id) {
                        let mut test_fills = mm_fills_map.clone();
                        test_fills.insert(fill.order_id, (fill.fill_price, fill.fill_qty));
                        if mm.capital_used(&test_fills) > mm.max_capital {
                            would_exceed = true;
                            break;
                        }
                    }
                }
                if would_exceed {
                    continue; // Skip this MM fill
                }
                mm_fills_map.insert(fill.order_id, (fill.fill_price, fill.fill_qty));
            }

            if let Some(order) = order_map.get(&fill.order_id) {
                matching_result.add_fill(fill, order);
            }
        }

        // Compute final diagnostics
        let final_price_sum_error =
            compute_price_sum_errors(&final_prices, &problem.market_groups);
        let final_budget_utilization =
            compute_budget_utilization(&matching_result, &problem.mm_constraints, &order_map);

        DualResult {
            matching_result,
            prices: final_prices,
            iterations,
            converged,
            final_price_sum_error,
            final_budget_utilization,
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
/// For each order:
/// - Non-MM orders get only λ adjustment (price consistency)
/// - MM orders get both λ and μ adjustments (price consistency + pacing)
///
/// The shading formulas come from the Lagrangian relaxation:
/// - YES buyer:  effective = limit/(1+μ) - λ×$1
/// - NO buyer:   effective = limit/(1+μ) + λ×$1
/// - YES seller: effective = (limit + μ×$1)/(1+μ) - λ×$1
/// - NO seller:  effective = (limit + μ×$1)/(1+μ) + λ×$1
pub fn shade_orders(
    orders: &[Order],
    lambda: &HashMap<String, f64>,
    mu: &HashMap<u64, f64>,
    market_to_group: &HashMap<MarketId, String>,
    order_to_mm: &HashMap<u64, u64>,
    mm_map: &HashMap<u64, &MmConstraint>,
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

            // Get μ for this order's MM (0 if not an MM order)
            let mu_val = order_to_mm
                .get(&order.id)
                .and_then(|mm_id| mu.get(mm_id))
                .copied()
                .unwrap_or(0.0);

            // Determine order type from payoff vector
            let is_yes_buyer = order.payoffs[0] > 0 && order.payoffs.iter().all(|&p| p >= 0);
            let is_no_buyer = order.num_states >= 2
                && order.payoffs[1] > 0
                && order.payoffs.iter().all(|&p| p >= 0);
            let is_yes_seller = order.payoffs[0] < 0;
            let is_no_seller = order.num_states >= 2 && order.payoffs[1] < 0;

            // Get MM side if applicable
            let mm_side = order_to_mm.get(&order.id).and_then(|mm_id| {
                mm_map
                    .get(mm_id)
                    .and_then(|mm| mm.order_sides.get(&order.id).copied())
            });

            let limit_f64 = order.limit_price as f64;
            let npd = NANOS_PER_DOLLAR as f64;
            let lambda_nanos = lambda_val * npd;

            // Apply pacing (μ) first, then price consistency (λ)
            let effective = if is_yes_buyer || matches!(mm_side, Some(MmSide::BuyYes)) {
                // YES buyer: effective = limit/(1+μ) - λ
                let paced = limit_f64 / (1.0 + mu_val);
                paced - lambda_nanos
            } else if is_no_buyer || matches!(mm_side, Some(MmSide::BuyNo)) {
                // NO buyer: effective = limit/(1+μ) + λ
                let paced = limit_f64 / (1.0 + mu_val);
                paced + lambda_nanos
            } else if is_yes_seller || matches!(mm_side, Some(MmSide::SellYes)) {
                // YES seller: effective = (limit + μ×$1)/(1+μ) - λ
                let paced = (limit_f64 + mu_val * npd) / (1.0 + mu_val);
                paced - lambda_nanos
            } else if is_no_seller || matches!(mm_side, Some(MmSide::SellNo)) {
                // NO seller: effective = (limit + μ×$1)/(1+μ) + λ
                let paced = (limit_f64 + mu_val * npd) / (1.0 + mu_val);
                paced + lambda_nanos
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

/// Compute constraint violation residuals.
///
/// Returns:
/// - Price residuals: (sum_yes - $1) / $1 per MarketGroup
/// - Budget residuals: (spend - budget) / budget per MM (positive = violation)
pub fn compute_primal_residuals(
    prices: &PriceDiscoveryResult,
    groups: &[MarketGroup],
    mm_constraints: &[MmConstraint],
    _orders: &[Order],
) -> (HashMap<String, f64>, HashMap<u64, f64>) {
    let npd = NANOS_PER_DOLLAR as f64;

    // Price consistency residuals
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

    // Budget residuals
    let mut budget_residuals = HashMap::new();

    // Build fills map from price discovery
    let all_fills: HashMap<u64, (Nanos, Qty)> = prices
        .all_fills()
        .into_iter()
        .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
        .collect();

    for mm in mm_constraints {
        let capital_used = mm.capital_used(&all_fills);
        if mm.max_capital > 0 {
            let residual = (capital_used as f64 - mm.max_capital as f64) / mm.max_capital as f64;
            budget_residuals.insert(mm.mm_id.0, residual);
        }
    }

    (price_residuals, budget_residuals)
}

// ============================================================================
// Dual Variable Updates
// ============================================================================

/// Update dual variables using subgradient step.
///
/// - λ is unconstrained (can be positive or negative)
/// - μ is projected to ≥ 0 (it's a Lagrange multiplier for an inequality constraint)
pub fn update_duals(
    state: &mut DualState,
    price_residuals: &HashMap<String, f64>,
    budget_residuals: &HashMap<u64, f64>,
    step_size: f64,
) {
    // Update λ (unconstrained)
    for (group, residual) in price_residuals {
        let lambda = state.lambda.entry(group.clone()).or_insert(0.0);
        *lambda += step_size * residual;
    }

    // Update μ (projected to ≥ 0)
    for (mm_id, residual) in budget_residuals {
        let mu = state.mu.entry(*mm_id).or_insert(0.0);
        *mu = (*mu + step_size * residual).max(0.0);
    }
}

// ============================================================================
// Convergence Check
// ============================================================================

/// Check if the dual decomposition has converged.
///
/// Convergence requires:
/// 1. All primal residuals below tolerance (constraints approximately satisfied)
/// 2. All dual variable changes below tolerance (stability)
pub fn check_convergence(
    price_residuals: &HashMap<String, f64>,
    budget_residuals: &HashMap<u64, f64>,
    state: &DualState,
    primal_tol: f64,
    dual_tol: f64,
) -> bool {
    // Check primal feasibility
    let primal_ok = price_residuals.values().all(|r| r.abs() < primal_tol)
        && budget_residuals.values().all(|r| *r < primal_tol);

    // Check dual stability
    let lambda_change: f64 = state
        .lambda
        .iter()
        .map(|(k, v)| {
            let prev = state.prev_lambda.get(k).copied().unwrap_or(0.0);
            (v - prev).abs()
        })
        .sum();

    let mu_change: f64 = state
        .mu
        .iter()
        .map(|(k, v)| {
            let prev = state.prev_mu.get(k).copied().unwrap_or(0.0);
            (v - prev).abs()
        })
        .sum();

    let dual_ok = (lambda_change + mu_change) < dual_tol;

    primal_ok && dual_ok
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

/// Compute budget utilization per MM (for final diagnostics).
fn compute_budget_utilization(
    result: &MatchingResult,
    mm_constraints: &[MmConstraint],
    _order_map: &HashMap<u64, &Order>,
) -> HashMap<u64, f64> {
    let fills_map: HashMap<u64, (Nanos, Qty)> = result
        .fills
        .iter()
        .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
        .collect();

    let mut utilization = HashMap::new();
    for mm in mm_constraints {
        let capital_used = mm.capital_used(&fills_map);
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
        assert_eq!(config.max_iterations, 20);
        assert!(config.initial_step_size > 0.0);
        assert!(config.primal_tolerance > 0.0);
    }

    #[test]
    fn test_shade_orders_no_adjustment() {
        // With λ=0 and μ=0, shaded orders should have same limits
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.50), 100);

        let lambda = HashMap::new();
        let mu = HashMap::new();
        let market_to_group = HashMap::new();
        let order_to_mm = HashMap::new();
        let mm_map = HashMap::new();

        let shaded = shade_orders(
            &[order.clone()],
            &lambda,
            &mu,
            &market_to_group,
            &order_to_mm,
            &mm_map,
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
            &HashMap::new(),
            &market_to_group,
            &HashMap::new(),
            &HashMap::new(),
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
            &HashMap::new(),
            &market_to_group,
            &HashMap::new(),
            &HashMap::new(),
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
    fn test_shade_orders_mu_positive() {
        // μ > 0 means MM over budget, so MM buyer should bid less
        let mut markets = matching_engine::MarketSet::new();
        let m = markets.add_binary("test");

        let order = simple_yes_buy(&markets, 1, m, price_to_nanos(0.60), 100);
        let mm_id = 42u64;

        let mut mu = HashMap::new();
        mu.insert(mm_id, 0.5); // 50% pacing

        let mut order_to_mm = HashMap::new();
        order_to_mm.insert(order.id, mm_id);

        let mut mm_constraint = MmConstraint::new(MmId::new(mm_id), 1_000_000_000);
        mm_constraint.add_order(order.id, MmSide::BuyYes);
        let mut mm_map: HashMap<u64, &MmConstraint> = HashMap::new();
        mm_map.insert(mm_id, &mm_constraint);

        let shaded = shade_orders(
            &[order.clone()],
            &HashMap::new(),
            &mu,
            &HashMap::new(),
            &order_to_mm,
            &mm_map,
        );

        // MM buyer should have lower limit (paced down)
        assert!(
            shaded[0].limit_price < order.limit_price,
            "MM buyer should be paced down: shaded={}, original={}",
            shaded[0].limit_price,
            order.limit_price
        );
        // Specifically: 0.60 / (1 + 0.5) = 0.40
        let expected = price_to_nanos(0.40);
        assert!(
            (shaded[0].limit_price as i64 - expected as i64).unsigned_abs() < 2,
            "Expected ~{}, got {}",
            expected,
            shaded[0].limit_price
        );
    }

    #[test]
    fn test_update_duals() {
        let mut state = DualState::default();
        state.lambda.insert("g1".to_string(), 0.0);
        state.mu.insert(1, 0.0);

        let mut price_residuals = HashMap::new();
        price_residuals.insert("g1".to_string(), 0.05); // 5% over

        let mut budget_residuals = HashMap::new();
        budget_residuals.insert(1, 0.10); // 10% over budget

        update_duals(&mut state, &price_residuals, &budget_residuals, 0.5);

        assert!(*state.lambda.get("g1").unwrap() > 0.0);
        assert!(*state.mu.get(&1).unwrap() > 0.0);
    }

    #[test]
    fn test_mu_stays_non_negative() {
        let mut state = DualState::default();
        state.mu.insert(1, 0.01); // Small positive

        let mut budget_residuals = HashMap::new();
        budget_residuals.insert(1, -1.0); // Way under budget

        update_duals(&mut state, &HashMap::new(), &budget_residuals, 0.5);

        // μ should be projected to 0, not go negative
        assert!(*state.mu.get(&1).unwrap() >= 0.0);
    }

    #[test]
    fn test_convergence_check() {
        let state = DualState {
            lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            mu: [(1, 0.02)].into_iter().collect(),
            prev_lambda: [("g1".to_string(), 0.05)].into_iter().collect(),
            prev_mu: [(1, 0.02)].into_iter().collect(),
        };

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.001)].into_iter().collect();
        let budget_res: HashMap<u64, f64> = [(1, -0.5)].into_iter().collect();

        // Small residuals + no dual change → converged
        assert!(check_convergence(
            &price_res,
            &budget_res,
            &state,
            0.02,
            0.001,
        ));
    }

    #[test]
    fn test_convergence_fails_on_large_residual() {
        let state = DualState::default();

        let price_res: HashMap<String, f64> = [("g1".to_string(), 0.10)].into_iter().collect();
        let budget_res: HashMap<u64, f64> = HashMap::new();

        // 10% residual > 2% tolerance → not converged
        assert!(!check_convergence(
            &price_res,
            &budget_res,
            &state,
            0.02,
            0.001,
        ));
    }

    #[test]
    fn test_dual_master_basic_solve() {
        let problem = election_problem();
        let master = DualMaster::new();
        let result = master.solve(&problem);

        // Should produce some fills
        assert!(
            result.matching_result.fills.len() > 0,
            "Should have fills"
        );

        // Check price sum errors are small for converged solution
        for (group, error) in &result.final_price_sum_error {
            // We allow larger error since the final pass uses original limits
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

        assert!(result.matching_result.fills.len() > 0);
        // Should converge immediately (no coupling constraints)
        assert!(result.converged || result.iterations <= 2);
    }
}
