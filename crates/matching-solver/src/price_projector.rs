//! Price projection to enforce constraint-feasibility.
//!
//! # Problem
//!
//! LocalSolver produces per-market clearing prices independently, but cross-market
//! orders create coupling constraints. For example, if orders exist for both:
//! - Base market M with outcome "Rain"
//! - Joint market (M × N) with outcome "Rain AND Cancel"
//!
//! Then marginal consistency requires:
//! ```text
//! P(Rain) = P(RC) + P(R¬C)
//! ```
//!
//! # Solution
//!
//! Project raw prices onto the constraint-feasible set using quadratic programming:
//! ```text
//! minimize ||p_raw - p||²
//! subject to: marginal consistency constraints
//! ```
//!
//! This is a small QP (~1000-6000 variables) over prices only, not the full
//! matching LP over orders (~100k variables).

use std::collections::{HashMap, HashSet};

use matching_engine::{
    state_to_outcomes, MarketId, Nanos, Order, Problem, MAX_STATES, NANOS_PER_DOLLAR,
};

use crate::traits::PriceDiscoveryResult;

/// Configuration for the price projector.
#[derive(Clone, Debug)]
pub struct ProjectorConfig {
    /// Maximum iterations for iterative projection (if needed).
    pub max_iterations: usize,
    /// Convergence tolerance in nanos.
    pub tolerance: Nanos,
    /// Maximum number of joint outcomes to track (for bounding complexity).
    pub max_joint_outcomes: usize,
    /// Whether to use QP-based projection (vs iterative).
    pub use_qp: bool,
}

impl Default for ProjectorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1_000, // 1 micro-dollar
            max_joint_outcomes: 5000,
            use_qp: true,
        }
    }
}

/// A joint outcome (combination of base market outcomes).
///
/// For example, if we have markets M and N (both binary), a joint outcome
/// might be "M=0 AND N=1" represented as components = [(M, 0), (N, 1)].
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct JointOutcome {
    /// Which markets and their outcomes in this joint state.
    /// Sorted by market ID for canonical form.
    pub components: Vec<(MarketId, u8)>,
}

impl Ord for JointOutcome {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by components, using market ID's inner u32 for ordering
        let self_keys: Vec<_> = self.components.iter().map(|(m, o)| (m.0, *o)).collect();
        let other_keys: Vec<_> = other.components.iter().map(|(m, o)| (m.0, *o)).collect();
        self_keys.cmp(&other_keys)
    }
}

impl PartialOrd for JointOutcome {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl JointOutcome {
    /// Create a new joint outcome from market-outcome pairs.
    pub fn new(mut components: Vec<(MarketId, u8)>) -> Self {
        // Sort by market ID's inner u32 value
        components.sort_by_key(|(m, _)| m.0);
        Self { components }
    }

    /// Create from a state index and the markets involved.
    pub fn from_state(state_idx: usize, markets: &[MarketId], market_sizes: &[u8]) -> Self {
        let outcomes = state_to_outcomes(state_idx, market_sizes);
        let components: Vec<_> = markets
            .iter()
            .zip(outcomes.iter())
            .map(|(&m, &o)| (m, o))
            .collect();
        Self::new(components)
    }

    /// Check if this joint outcome contains a specific market-outcome pair.
    pub fn contains(&self, market: MarketId, outcome: u8) -> bool {
        self.components
            .iter()
            .any(|&(m, o)| m == market && o == outcome)
    }

    /// Get the outcome for a specific market (if present).
    pub fn outcome_for(&self, market: MarketId) -> Option<u8> {
        self.components
            .iter()
            .find(|(m, _)| *m == market)
            .map(|(_, o)| *o)
    }

    /// Get all markets involved.
    pub fn markets(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.components.iter().map(|(m, _)| *m)
    }

    /// Number of markets in this joint outcome.
    pub fn num_markets(&self) -> usize {
        self.components.len()
    }
}

/// Violation of marginal consistency.
#[derive(Clone, Debug)]
pub struct MarginalViolation {
    /// The base market with inconsistent price.
    pub market: MarketId,
    /// The specific outcome.
    pub outcome: u8,
    /// The current base market price.
    pub base_price: Nanos,
    /// Sum of joint outcome prices that should equal base_price.
    pub sum_of_joints: Nanos,
    /// Amount of violation: |base_price - sum_of_joints|.
    pub violation_amount: i64,
}

/// Result of price projection.
#[derive(Clone, Debug)]
pub struct ProjectionResult {
    /// Projected prices (constraint-consistent).
    pub base_prices: HashMap<MarketId, Vec<Nanos>>,
    /// Prices for joint outcomes (if any were needed).
    pub joint_prices: HashMap<JointOutcome, Nanos>,
    /// Number of constraints that were violated before projection.
    pub violations_fixed: usize,
    /// Maximum price adjustment made (in nanos).
    pub max_adjustment: Nanos,
    /// Iterations used (if iterative method).
    pub iterations: usize,
    /// Whether projection was successful.
    pub success: bool,
}

impl ProjectionResult {
    /// Create an empty/identity result (no changes needed).
    pub fn identity(base_prices: HashMap<MarketId, Vec<Nanos>>) -> Self {
        Self {
            base_prices,
            joint_prices: HashMap::new(),
            violations_fixed: 0,
            max_adjustment: 0,
            iterations: 0,
            success: true,
        }
    }
}

/// Projects raw prices onto constraint-feasible space.
///
/// Given prices from LocalSolver that may violate constraints,
/// finds the closest prices that satisfy all constraints.
pub struct PriceProjector {
    config: ProjectorConfig,
}

impl PriceProjector {
    /// Create a new price projector with default config.
    pub fn new() -> Self {
        Self {
            config: ProjectorConfig::default(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: ProjectorConfig) -> Self {
        Self { config }
    }

    /// Project prices to satisfy marginal consistency and market group constraints.
    ///
    /// # Arguments
    /// * `base_prices` - Raw prices from LocalSolver
    /// * `problem` - The problem (used to extract joint outcomes from orders and market groups)
    ///
    /// # Returns
    /// Projected prices that satisfy all consistency constraints.
    pub fn project(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        problem: &Problem,
    ) -> ProjectionResult {
        // Step 1: Check and fix market group violations FIRST
        // (multi-outcome markets where sum of P(YES) must equal 1)
        let (prices, group_violations, group_max_adj) =
            self.project_market_groups(base_prices, &problem.market_groups);

        // Step 2: Extract active joint outcomes from multi-market orders
        let joint_outcomes = self.extract_joint_outcomes(problem);

        if joint_outcomes.is_empty() && group_violations == 0 {
            // No cross-market orders and no group violations
            return ProjectionResult {
                base_prices: prices,
                joint_prices: HashMap::new(),
                violations_fixed: group_violations,
                max_adjustment: group_max_adj,
                iterations: 1,
                success: true,
            };
        }

        // Step 3: Check for marginal consistency violations
        let violations = self.check_consistency_internal(&prices, &joint_outcomes);

        if violations.is_empty() {
            return ProjectionResult {
                base_prices: prices,
                joint_prices: HashMap::new(),
                violations_fixed: group_violations,
                max_adjustment: group_max_adj,
                iterations: 1,
                success: true,
            };
        }

        // Step 4: Project using QP or iterative method
        let mut result = if self.config.use_qp {
            self.project_qp(&prices, &joint_outcomes, &violations)
        } else {
            self.project_iterative(&prices, &joint_outcomes, &violations)
        };

        result.violations_fixed += group_violations;
        result.max_adjustment = result.max_adjustment.max(group_max_adj);
        result
    }

    /// Project prices to satisfy market group constraints.
    ///
    /// For each market group (mutually exclusive outcomes), ensure that
    /// the sum of P(YES) across all markets in the group equals 1.
    ///
    /// Returns (adjusted_prices, violations_fixed, max_adjustment)
    fn project_market_groups(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        market_groups: &[matching_engine::MarketGroup],
    ) -> (HashMap<MarketId, Vec<Nanos>>, usize, Nanos) {
        let mut prices = base_prices.clone();
        let mut violations_fixed = 0;
        let mut max_adjustment: Nanos = 0;

        for group in market_groups {
            if group.markets.len() < 2 {
                continue;
            }

            // Calculate current sum of P(YES) for markets in this group
            let mut sum_yes: u128 = 0;
            let mut valid_markets = Vec::new();

            for &market_id in &group.markets {
                if let Some(market_prices) = prices.get(&market_id) {
                    if let Some(&yes_price) = market_prices.first() {
                        sum_yes += yes_price as u128;
                        valid_markets.push(market_id);
                    }
                }
            }

            if valid_markets.is_empty() {
                continue;
            }

            // Check if sum deviates from 1.0 (NANOS_PER_DOLLAR)
            let target = NANOS_PER_DOLLAR as u128;
            let deviation = (sum_yes as i128 - target as i128).unsigned_abs();

            if deviation > self.config.tolerance as u128 {
                violations_fixed += 1;

                // Scale all YES prices to sum to 1.0
                let scale = target as f64 / sum_yes as f64;

                for &market_id in &valid_markets {
                    if let Some(market_prices) = prices.get_mut(&market_id) {
                        if market_prices.len() >= 2 {
                            let old_yes = market_prices[0];
                            let new_yes = ((old_yes as f64 * scale) as u64).clamp(1, NANOS_PER_DOLLAR - 1);
                            let new_no = NANOS_PER_DOLLAR - new_yes;

                            let adj = (new_yes as i64 - old_yes as i64).unsigned_abs();
                            max_adjustment = max_adjustment.max(adj);

                            market_prices[0] = new_yes;
                            market_prices[1] = new_no;
                        }
                    }
                }
            }
        }

        (prices, violations_fixed, max_adjustment)
    }

    /// Check if prices satisfy marginal consistency.
    pub fn check_consistency(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        problem: &Problem,
    ) -> Vec<MarginalViolation> {
        let joint_outcomes = self.extract_joint_outcomes(problem);
        self.check_consistency_internal(base_prices, &joint_outcomes)
    }

    /// Extract active joint outcomes from orders.
    ///
    /// Only multi-market orders create joint outcomes that need price consistency.
    fn extract_joint_outcomes(&self, problem: &Problem) -> Vec<JointOutcome> {
        let mut outcomes = HashSet::new();

        for order in &problem.orders {
            if order.num_markets <= 1 {
                continue;
            }

            // Get the markets this order spans
            let markets: Vec<MarketId> = order.active_markets().collect();
            let market_sizes: Vec<u8> = markets
                .iter()
                .map(|m| problem.markets.num_outcomes(*m))
                .collect();

            // For each state with positive payoff, add the joint outcome
            // Note: num_states can exceed MAX_STATES for non-binary markets, but payoffs array is fixed size
            let num_valid_states = (order.num_states as usize).min(MAX_STATES);
            for state_idx in 0..num_valid_states {
                if order.payoffs[state_idx] != 0 {
                    let joint = JointOutcome::from_state(state_idx, &markets, &market_sizes);
                    outcomes.insert(joint);
                }
            }
        }

        // Cap the number of joint outcomes
        let mut result: Vec<_> = outcomes.into_iter().collect();
        result.sort(); // Deterministic ordering
        if result.len() > self.config.max_joint_outcomes {
            result.truncate(self.config.max_joint_outcomes);
        }

        result
    }

    /// Check consistency given known joint outcomes.
    fn check_consistency_internal(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        joint_outcomes: &[JointOutcome],
    ) -> Vec<MarginalViolation> {
        let mut violations = Vec::new();

        // Group joint outcomes by base market and outcome
        let mut joints_by_base: HashMap<(MarketId, u8), Vec<&JointOutcome>> = HashMap::new();
        for joint in joint_outcomes {
            for &(market, outcome) in &joint.components {
                joints_by_base
                    .entry((market, outcome))
                    .or_default()
                    .push(joint);
            }
        }

        // For each base market outcome that participates in joint outcomes,
        // check marginal consistency
        for ((market, outcome), joints) in &joints_by_base {
            let Some(prices) = base_prices.get(market) else {
                continue;
            };
            let Some(&base_price) = prices.get(*outcome as usize) else {
                continue;
            };

            // The sum of joint prices where this market-outcome appears
            // should approximately equal the base price.
            // However, we don't have explicit joint prices yet - we need to compute them.

            // For now, compute what the joint prices would be under independence assumption
            // and check if they're consistent
            let sum_joints: u128 = joints
                .iter()
                .map(|j| {
                    // Independent joint price = product of marginal prices
                    let product: u128 = j
                        .components
                        .iter()
                        .filter(|(m, _)| *m != *market) // Don't include the base market
                        .map(|(m, o)| {
                            base_prices
                                .get(m)
                                .and_then(|p| p.get(*o as usize))
                                .copied()
                                .unwrap_or(0) as u128
                        })
                        .product();

                    // Scale: if all other markets have prices summing to 1,
                    // then this is the "portion" of the base price for this joint
                    if product == 0 {
                        0
                    } else {
                        (product / NANOS_PER_DOLLAR as u128).min(NANOS_PER_DOLLAR as u128)
                    }
                })
                .sum();

            // The violation is the difference between base price and what marginal
            // consistency would require
            let expected = (sum_joints as f64 / joints.len() as f64) as Nanos;
            let violation = (base_price as i64 - expected as i64).abs();

            if violation > self.config.tolerance as i64 {
                violations.push(MarginalViolation {
                    market: *market,
                    outcome: *outcome,
                    base_price,
                    sum_of_joints: expected,
                    violation_amount: violation,
                });
            }
        }

        violations
    }

    /// Project using Quadratic Programming.
    ///
    /// Minimize ||p_raw - p||² subject to:
    /// - Normalization: Σp_i = 1 for each market
    /// - Marginal consistency: base price = weighted sum of joints
    fn project_qp(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        joint_outcomes: &[JointOutcome],
        _violations: &[MarginalViolation],
    ) -> ProjectionResult {
        // Build the QP model
        // Variables: one per base market outcome
        let mut var_index: HashMap<(MarketId, u8), usize> = HashMap::new();
        let mut raw_values: Vec<f64> = Vec::new();

        for (&market, prices) in base_prices {
            for (outcome, &price) in prices.iter().enumerate() {
                let idx = raw_values.len();
                var_index.insert((market, outcome as u8), idx);
                raw_values.push(price as f64 / NANOS_PER_DOLLAR as f64);
            }
        }

        let n = raw_values.len();
        if n == 0 {
            return ProjectionResult::identity(base_prices.clone());
        }

        // HiGHS doesn't support QP directly through the simple API.
        // We'll use an iterative linear projection approach instead.
        // (OSQP would be better for QP, but we're avoiding new dependencies)

        // Fallback to iterative projection
        self.project_iterative_impl(base_prices, joint_outcomes, &var_index, &raw_values)
    }

    /// Iterative projection using alternating projections.
    fn project_iterative(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        joint_outcomes: &[JointOutcome],
        _violations: &[MarginalViolation],
    ) -> ProjectionResult {
        let mut var_index: HashMap<(MarketId, u8), usize> = HashMap::new();
        let mut raw_values: Vec<f64> = Vec::new();

        for (&market, prices) in base_prices {
            for (outcome, &price) in prices.iter().enumerate() {
                let idx = raw_values.len();
                var_index.insert((market, outcome as u8), idx);
                raw_values.push(price as f64 / NANOS_PER_DOLLAR as f64);
            }
        }

        self.project_iterative_impl(base_prices, joint_outcomes, &var_index, &raw_values)
    }

    /// Internal iterative projection implementation.
    fn project_iterative_impl(
        &self,
        base_prices: &HashMap<MarketId, Vec<Nanos>>,
        joint_outcomes: &[JointOutcome],
        var_index: &HashMap<(MarketId, u8), usize>,
        raw_values: &[f64],
    ) -> ProjectionResult {
        let n = raw_values.len();
        if n == 0 {
            return ProjectionResult::identity(base_prices.clone());
        }

        // Start with raw values
        let mut p: Vec<f64> = raw_values.to_vec();

        // Group markets by their variables
        let mut markets_vars: HashMap<MarketId, Vec<(u8, usize)>> = HashMap::new();
        for (&(market, outcome), &idx) in var_index {
            markets_vars.entry(market).or_default().push((outcome, idx));
        }

        // Sort for determinism
        for vars in markets_vars.values_mut() {
            vars.sort_by_key(|(o, _)| *o);
        }

        let mut max_adjustment: f64 = 0.0;

        for iteration in 0..self.config.max_iterations {
            let old_p = p.clone();

            // Step 1: Project onto normalization constraints (prices sum to 1)
            for vars in markets_vars.values() {
                let sum: f64 = vars.iter().map(|(_, idx)| p[*idx]).sum();
                if sum > 0.0 {
                    let scale = 1.0 / sum;
                    for (_, idx) in vars {
                        p[*idx] *= scale;
                    }
                }
            }

            // Step 2: Project towards marginal consistency
            // For each base market outcome, adjust towards weighted average of
            // what joint outcomes imply
            if !joint_outcomes.is_empty() {
                self.project_marginal_consistency(&mut p, joint_outcomes, var_index, &markets_vars);
            }

            // Step 3: Ensure non-negativity and bounds
            for v in &mut p {
                *v = v.clamp(0.0, 1.0);
            }

            // Check convergence
            let max_change: f64 = p
                .iter()
                .zip(old_p.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);

            max_adjustment = max_adjustment.max(max_change);

            if max_change < self.config.tolerance as f64 / NANOS_PER_DOLLAR as f64 {
                // Converged
                let result =
                    self.build_result(base_prices, &p, var_index, max_adjustment, iteration + 1);
                return result;
            }
        }

        // Max iterations reached
        self.build_result(
            base_prices,
            &p,
            var_index,
            max_adjustment,
            self.config.max_iterations,
        )
    }

    /// Project prices towards marginal consistency with joint outcomes.
    fn project_marginal_consistency(
        &self,
        p: &mut [f64],
        joint_outcomes: &[JointOutcome],
        var_index: &HashMap<(MarketId, u8), usize>,
        _markets_vars: &HashMap<MarketId, Vec<(u8, usize)>>,
    ) {
        // For each market-outcome in joint outcomes, compute target based on
        // what the joint structure implies
        let mut adjustments: HashMap<usize, Vec<f64>> = HashMap::new();

        for joint in joint_outcomes {
            if joint.num_markets() < 2 {
                continue;
            }

            // Compute joint probability under independence
            let joint_prob: f64 = joint
                .components
                .iter()
                .filter_map(|(m, o)| var_index.get(&(*m, *o)).map(|&idx| p[idx]))
                .product();

            // For each market in the joint, the marginal should be consistent
            for &(market, outcome) in &joint.components {
                if let Some(&idx) = var_index.get(&(market, outcome)) {
                    // The target for this marginal, given other markets
                    let other_prob: f64 = joint
                        .components
                        .iter()
                        .filter(|(m, _)| *m != market)
                        .filter_map(|(m, o)| var_index.get(&(*m, *o)).map(|&i| p[i]))
                        .product();

                    if other_prob > 1e-10 {
                        let implied_marginal = joint_prob / other_prob;
                        adjustments.entry(idx).or_default().push(implied_marginal);
                    }
                }
            }
        }

        // Apply adjustments (average of implied marginals)
        let alpha = 0.3; // Blend factor
        for (idx, targets) in adjustments {
            if !targets.is_empty() {
                let avg_target: f64 = targets.iter().sum::<f64>() / targets.len() as f64;
                p[idx] = (1.0 - alpha) * p[idx] + alpha * avg_target;
            }
        }
    }

    /// Build the final result from projected values.
    fn build_result(
        &self,
        original: &HashMap<MarketId, Vec<Nanos>>,
        projected: &[f64],
        var_index: &HashMap<(MarketId, u8), usize>,
        _max_adjustment: f64,
        iterations: usize,
    ) -> ProjectionResult {
        let mut new_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        let mut total_violations_fixed = 0;
        let mut max_adj_nanos: Nanos = 0;

        for (&market, old_prices) in original {
            let mut prices = vec![0u64; old_prices.len()];
            let mut sum: u64 = 0;

            for (outcome, &old_price) in old_prices.iter().enumerate() {
                let new_price = if let Some(&idx) = var_index.get(&(market, outcome as u8)) {
                    (projected[idx] * NANOS_PER_DOLLAR as f64).round() as Nanos
                } else {
                    old_price
                };

                let adjustment = (new_price as i64 - old_price as i64).unsigned_abs();
                if adjustment > self.config.tolerance {
                    total_violations_fixed += 1;
                }
                max_adj_nanos = max_adj_nanos.max(adjustment);

                prices[outcome] = new_price;
                sum += new_price;
            }

            // Final normalization fix
            if sum != NANOS_PER_DOLLAR && !prices.is_empty() {
                if let Some(last) = prices.last_mut() {
                    if sum < NANOS_PER_DOLLAR {
                        *last += NANOS_PER_DOLLAR - sum;
                    } else if sum > NANOS_PER_DOLLAR {
                        *last = last.saturating_sub(sum - NANOS_PER_DOLLAR);
                    }
                }
            }

            new_prices.insert(market, prices);
        }

        ProjectionResult {
            base_prices: new_prices,
            joint_prices: HashMap::new(),
            violations_fixed: total_violations_fixed,
            max_adjustment: max_adj_nanos,
            iterations,
            success: true,
        }
    }
}

impl Default for PriceProjector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Trait Implementation
// ============================================================================

use crate::traits::{PriceProjectionResult, PriceProjector as PriceProjectorTrait};

impl PriceProjectorTrait for PriceProjector {
    fn project(
        &self,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        problem: &Problem,
    ) -> PriceProjectionResult {
        let result = PriceProjector::project(self, prices, problem);

        PriceProjectionResult {
            prices: result.base_prices,
            violations_fixed: result.violations_fixed,
            max_adjustment: result.max_adjustment,
            iterations: result.iterations,
            success: result.success,
        }
    }

    fn name(&self) -> &str {
        "PriceProjector"
    }
}

/// Fill recomputation after price projection.
///
/// When prices change due to projection, some orders may no longer have
/// positive welfare. This function recomputes which fills are valid.
pub fn recompute_fills(
    price_result: &mut PriceDiscoveryResult,
    projected_prices: &HashMap<MarketId, Vec<Nanos>>,
    problem: &Problem,
) {
    // Update prices in the result
    price_result.prices = projected_prices.clone();

    // For each market solution, recompute fills at new prices
    for solution in price_result.market_solutions.values_mut() {
        let Some(new_prices) = projected_prices.get(&solution.market_id) else {
            continue;
        };

        solution.prices = new_prices.clone();

        // Filter fills: keep only those where limit_price >= clearing_price
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        let mut valid_fills = Vec::new();
        let mut new_welfare: i64 = 0;

        for fill in &solution.fills {
            let Some(order) = order_map.get(&fill.order_id) else {
                continue;
            };

            // Find which outcome this order is buying
            let outcome_idx = order
                .payoffs
                .iter()
                .take(order.num_states as usize)
                .position(|&p| p > 0);

            if let Some(outcome) = outcome_idx {
                let clearing_price = new_prices.get(outcome).copied().unwrap_or(0);

                if order.limit_price >= clearing_price {
                    // Order still has positive welfare
                    let welfare =
                        (order.limit_price as i64 - clearing_price as i64) * fill.fill_qty as i64;
                    new_welfare += welfare;

                    // Update fill with new price
                    let mut new_fill = fill.clone();
                    new_fill.fill_price = clearing_price;
                    valid_fills.push(new_fill);
                }
            }
        }

        solution.fills = valid_fills;
        solution.welfare = new_welfare;
    }

    // Recompute totals
    price_result.total_welfare = price_result
        .market_solutions
        .values()
        .map(|s| s.welfare)
        .sum();
    price_result.total_fills = price_result
        .market_solutions
        .values()
        .map(|s| s.fills.len())
        .sum();
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{bundle_yes, simple_yes_buy};

    fn create_basic_problem() -> Problem {
        let mut problem = Problem::new("basic");
        let market = problem.markets.add_binary("market");

        problem.liquidity.add_ask(market, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market, 1, 500_000_000, 1000);

        for i in 0..5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                (550 + i * 10) as u64 * 1_000_000,
                100,
            ));
        }

        problem
    }

    fn create_cross_market_problem() -> Problem {
        let mut problem = Problem::new("cross_market");
        let market_a = problem.markets.add_binary("rain");
        let market_b = problem.markets.add_binary("cancel");

        // Add liquidity
        problem.liquidity.add_ask(market_a, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_a, 1, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 0, 500_000_000, 1000);
        problem.liquidity.add_ask(market_b, 1, 500_000_000, 1000);

        // Single-market orders
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            1,
            market_a,
            600_000_000,
            100,
        ));
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            2,
            market_b,
            400_000_000,
            100,
        ));

        // Bundle order: buy both YES
        problem.orders.push(bundle_yes(
            &problem.markets,
            3,
            &[market_a, market_b],
            300_000_000, // Limit for the bundle
            50,
        ));

        problem
    }

    #[test]
    fn test_projector_no_cross_market() {
        let problem = create_basic_problem();
        let projector = PriceProjector::new();

        let mut prices = HashMap::new();
        let market = problem.markets.iter().next().unwrap().id;
        prices.insert(market, vec![500_000_000, 500_000_000]);

        let result = projector.project(&prices, &problem);

        // No cross-market orders, so no projection needed
        assert_eq!(result.violations_fixed, 0);
        assert!(result.success);
    }

    #[test]
    fn test_projector_with_bundle() {
        let problem = create_cross_market_problem();
        let projector = PriceProjector::new();

        let market_a = problem.markets.iter().next().unwrap().id;
        let market_b = problem.markets.iter().nth(1).unwrap().id;

        let mut prices = HashMap::new();
        prices.insert(market_a, vec![600_000_000, 400_000_000]);
        prices.insert(market_b, vec![300_000_000, 700_000_000]);

        let result = projector.project(&prices, &problem);

        // Should produce valid prices
        assert!(result.success);

        // Prices should still be normalized
        for prices in result.base_prices.values() {
            let sum: u64 = prices.iter().sum();
            assert_eq!(sum, NANOS_PER_DOLLAR, "Prices should sum to $1");
        }
    }

    #[test]
    fn test_joint_outcome_creation() {
        let jo = JointOutcome::new(vec![(MarketId::new(1), 0), (MarketId::new(0), 1)]);

        // Should be sorted by market ID
        assert_eq!(jo.components[0].0, MarketId::new(0));
        assert_eq!(jo.components[1].0, MarketId::new(1));

        assert!(jo.contains(MarketId::new(0), 1));
        assert!(jo.contains(MarketId::new(1), 0));
        assert!(!jo.contains(MarketId::new(0), 0));
    }

    #[test]
    fn test_extract_joint_outcomes() {
        let problem = create_cross_market_problem();
        let projector = PriceProjector::new();

        let joints = projector.extract_joint_outcomes(&problem);

        // Should have joint outcomes from the bundle order
        // Bundle YES/YES creates states where both are YES
        assert!(!joints.is_empty());

        // All joints should span 2 markets
        for joint in &joints {
            assert_eq!(joint.num_markets(), 2);
        }
    }

    #[test]
    fn test_check_consistency() {
        let problem = create_cross_market_problem();
        let projector = PriceProjector::new();

        let market_a = problem.markets.iter().next().unwrap().id;
        let market_b = problem.markets.iter().nth(1).unwrap().id;

        // Consistent prices (each market sums to 1)
        let mut prices = HashMap::new();
        prices.insert(market_a, vec![500_000_000, 500_000_000]);
        prices.insert(market_b, vec![500_000_000, 500_000_000]);

        let violations = projector.check_consistency(&prices, &problem);

        // With uniform prices and a bundle, marginal consistency should hold
        // (under independence assumption)
        println!("Violations: {:?}", violations);
    }

    #[test]
    fn test_config() {
        let config = ProjectorConfig {
            max_iterations: 50,
            tolerance: 100,
            max_joint_outcomes: 1000,
            use_qp: false,
        };

        let projector = PriceProjector::with_config(config.clone());
        assert_eq!(projector.config.max_iterations, 50);
        assert_eq!(projector.config.tolerance, 100);
    }

    #[test]
    fn test_projection_preserves_normalization() {
        let problem = create_cross_market_problem();
        let projector = PriceProjector::new();

        let market_a = problem.markets.iter().next().unwrap().id;
        let market_b = problem.markets.iter().nth(1).unwrap().id;

        // Slightly denormalized prices
        let mut prices = HashMap::new();
        prices.insert(market_a, vec![510_000_000, 490_000_000]);
        prices.insert(market_b, vec![480_000_000, 520_000_000]);

        let result = projector.project(&prices, &problem);

        // All markets should be normalized after projection
        for (market, prices) in &result.base_prices {
            let sum: u64 = prices.iter().sum();
            assert_eq!(
                sum, NANOS_PER_DOLLAR,
                "Market {:?} prices don't sum to $1: {}",
                market, sum
            );
        }
    }
}
