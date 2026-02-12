//! Multi-Market Order Matching Solver
//!
//! Matches multi-market orders (bundles, spreads) using two strategies:
//!
//! 1. **Complement Matching**: Orders with identical markets and negated payoffs
//!    cancel perfectly (e.g., bundle_yes + bundle_sell). Standard bid >= ask matching.
//!
//! 2. **Direct Price-Shifting (Repricing)**: Injects bundle leg demand into
//!    per-market supply/demand curves and re-clears. No synthetic orders —
//!    bundle demand is passed as numbers. Maintains UCP.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Nanos, Order, Problem, Qty, NANOS_PER_DOLLAR};

use crate::local_solver::{LocalSolver, MarketSolution, PrecomputedMarket};
use crate::traits::{PartialSolution, PartialSolver, PriceDiscoveryResult, SolutionConfidence};

/// Key for grouping orders with compatible payoff structures.
/// Orders with the same PayoffKey but opposite signs can be complement-matched.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PayoffKey {
    /// Sorted market IDs
    markets: Vec<MarketId>,
    /// Absolute values of payoffs (for grouping compatible orders)
    abs_payoffs: Vec<u8>,
}

/// A per-market position from decomposing a multi-market order.
#[derive(Clone, Debug)]
struct Leg {
    market: MarketId,
    outcome: u8,
    /// Positive = need to buy this outcome, Negative = need to sell this outcome.
    /// Expressed as numerator; denominator is `denom` (number of states per market outcome).
    shares_numer: i64,
    shares_denom: u64,
}

/// Tracking state for a market during repricing iterations.
struct MarketClearingState {
    current_solution: MarketSolution,
    base_orders: Vec<Order>,
    cumulative_extra_demand: Qty,
    cumulative_extra_supply: Qty,
    /// Precomputed curves for fast trial crossings (avoids full re-solve).
    precomputed: PrecomputedMarket,
}

/// Result of repricing multi-market orders.
pub struct RepricingResult {
    /// Fills for bundle/spread orders that were matched via repricing.
    pub bundle_fills: Vec<Fill>,
    /// Updated market solutions after repricing (only for affected markets).
    pub repriced_solutions: HashMap<MarketId, MarketSolution>,
    /// Total welfare from repriced solutions + bundle fills.
    pub welfare: i64,
    /// Number of bundles matched.
    pub bundles_matched: usize,
}

#[derive(Default)]
pub struct MultiMarketSolver;

impl MultiMarketSolver {
    pub fn new() -> Self {
        Self
    }

    /// Solve multi-market orders via direct price-shifting (repricing).
    ///
    /// Takes the base price discovery result and injects bundle leg demand
    /// into per-market curves. Returns bundle fills and repriced market solutions.
    pub fn solve_with_repricing(
        &self,
        problem: &Problem,
        base_price_result: &PriceDiscoveryResult,
    ) -> RepricingResult {
        let solver = LocalSolver::new();

        // Build set of MM order IDs to exclude
        let mm_order_ids: std::collections::HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // Collect multi-market orders
        let multi_market_orders: Vec<&Order> = problem
            .orders
            .iter()
            .filter(|o| o.num_markets > 1)
            .collect();

        if multi_market_orders.is_empty() {
            return RepricingResult {
                bundle_fills: Vec::new(),
                repriced_solutions: HashMap::new(),
                welfare: 0,
                bundles_matched: 0,
            };
        }

        // Track filled multi-market orders (complement matching handled elsewhere)
        let mut filled: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // Build MarketClearingState for each market
        let mut market_states: HashMap<MarketId, MarketClearingState> = HashMap::new();
        for market in problem.markets.iter() {
            let base_orders: Vec<Order> = problem
                .orders
                .iter()
                .filter(|o| {
                    o.num_markets == 1 && o.markets[0] == market.id && !mm_order_ids.contains(&o.id)
                })
                .cloned()
                .collect();

            let solution = base_price_result
                .market_solutions
                .get(&market.id)
                .cloned()
                .unwrap_or_else(|| MarketSolution::empty(market.id, 2));

            let precomputed = PrecomputedMarket::from_orders(&base_orders);
            market_states.insert(
                market.id,
                MarketClearingState {
                    current_solution: solution,
                    base_orders,
                    cumulative_extra_demand: 0,
                    cumulative_extra_supply: 0,
                    precomputed,
                },
            );
        }

        // Sort multi-market orders by welfare potential descending
        let mut sorted_bundles: Vec<&Order> = multi_market_orders
            .iter()
            .filter(|o| !filled.contains(&o.id))
            .copied()
            .collect();
        sorted_bundles.sort_by(|a, b| {
            let wa = a.limit_price as i128 * a.max_fill as i128;
            let wb = b.limit_price as i128 * b.max_fill as i128;
            wb.cmp(&wa)
        });

        let mut bundle_fills: Vec<Fill> = Vec::new();
        let mut bundles_matched = 0usize;

        for bundle in &sorted_bundles {
            if filled.contains(&bundle.id) {
                continue;
            }

            let market_sizes: Vec<u8> = bundle
                .markets
                .iter()
                .take(bundle.num_markets as usize)
                .map(|id| problem.markets.num_outcomes(*id))
                .collect();

            let legs = compute_legs(bundle, &market_sizes);
            if legs.is_empty() {
                continue;
            }

            let fill_qty = bundle.max_fill;
            if fill_qty == 0 {
                continue;
            }

            // Convert each leg to per-market (extra_demand, extra_supply) in unified YES space.
            //
            // Outcome-to-unified mapping:
            // - Buy outcome 0 (YES)  -> extra_unified_demand
            // - Buy outcome 1 (NO)   -> extra_unified_supply (buying NO = supplying YES)
            // - Sell outcome 0 (YES) -> extra_unified_supply
            // - Sell outcome 1 (NO)  -> extra_unified_demand (selling NO = demanding YES)
            let mut per_market_extras: HashMap<MarketId, (Qty, Qty)> = HashMap::new();

            for leg in &legs {
                let shares = (leg.shares_numer.unsigned_abs() * fill_qty) / leg.shares_denom;
                if shares == 0 {
                    continue;
                }

                let entry = per_market_extras.entry(leg.market).or_insert((0, 0));
                if leg.shares_numer > 0 {
                    // Buying this outcome
                    if leg.outcome == 0 {
                        entry.0 += shares; // demand
                    } else {
                        entry.1 += shares; // supply (buying NO = supplying YES)
                    }
                } else {
                    // Selling this outcome
                    if leg.outcome == 0 {
                        entry.1 += shares; // supply (selling YES)
                    } else {
                        entry.0 += shares; // demand (selling NO = demanding YES)
                    }
                }
            }

            // Collect affected markets
            let affected_markets: Vec<MarketId> = per_market_extras.keys().copied().collect();

            // === FAST TRIAL PHASE (O(S) per market using precomputed curves) ===
            // Compute new clearing prices and welfare without full re-solve.

            // Per-market trial results: (clearing_price_yes, matched_qty, estimated_welfare)
            let mut trial_results: Vec<(MarketId, Nanos, Qty, i64)> = Vec::new();
            let mut all_markets_feasible = true;

            for &mid in &affected_markets {
                let (leg_demand, leg_supply) = per_market_extras[&mid];
                let state = match market_states.get(&mid) {
                    Some(s) => s,
                    None => {
                        all_markets_feasible = false;
                        break;
                    }
                };

                let new_demand = state.cumulative_extra_demand + leg_demand;
                let new_supply = state.cumulative_extra_supply + leg_supply;

                let (clearing_price, matched_qty) = state
                    .precomputed
                    .crossing_with_extras(new_demand, new_supply);

                if matched_qty == 0 {
                    all_markets_feasible = false;
                    break;
                }

                let welfare = state.precomputed.estimate_welfare(
                    clearing_price,
                    matched_qty,
                    new_demand,
                    new_supply,
                );

                trial_results.push((mid, clearing_price, matched_qty, welfare));
            }

            if !all_markets_feasible {
                continue;
            }

            // Compute bundle cost at trial clearing prices
            let mut bundle_cost_per_unit: i128 = 0;
            for leg in &legs {
                let shares_per_unit =
                    leg.shares_numer.unsigned_abs() as i128 * 1000 / leg.shares_denom as i128;

                // Find clearing price for this market from trial results
                let clearing_price_yes = trial_results
                    .iter()
                    .find(|(mid, _, _, _)| *mid == leg.market)
                    .map(|(_, p, _, _)| *p)
                    .or_else(|| {
                        market_states
                            .get(&leg.market)
                            .map(|s| s.current_solution.prices[0])
                    });

                let price = match clearing_price_yes {
                    Some(p_yes) => {
                        if leg.outcome == 0 {
                            p_yes
                        } else {
                            NANOS_PER_DOLLAR.saturating_sub(p_yes)
                        }
                    }
                    None => continue,
                };

                if leg.shares_numer > 0 {
                    bundle_cost_per_unit += price as i128 * shares_per_unit;
                } else {
                    bundle_cost_per_unit -= price as i128 * shares_per_unit;
                }
            }
            bundle_cost_per_unit /= 1000;

            // Check limit price constraint
            let limit_ok = if bundle.is_seller() {
                -bundle_cost_per_unit >= bundle.limit_price as i128
            } else {
                bundle_cost_per_unit <= bundle.limit_price as i128
            };

            if !limit_ok {
                continue;
            }

            // Net welfare check using estimated welfare
            let old_welfare: i64 = affected_markets
                .iter()
                .filter_map(|mid| market_states.get(mid).map(|s| s.current_solution.welfare))
                .sum();
            let new_welfare: i64 = trial_results.iter().map(|(_, _, _, w)| w).sum();

            let bundle_welfare = if bundle.is_seller() {
                (-bundle_cost_per_unit - bundle.limit_price as i128) * fill_qty as i128
            } else {
                (bundle.limit_price as i128 - bundle_cost_per_unit) * fill_qty as i128
            };

            if new_welfare + bundle_welfare as i64 <= old_welfare {
                continue;
            }

            // === COMMIT PHASE (full re-solve only for accepted bundles) ===
            // Run exact solve to get real MarketSolution with fills.

            let mut new_solutions: HashMap<MarketId, MarketSolution> = HashMap::new();
            let mut commit_feasible = true;

            for &mid in &affected_markets {
                let (leg_demand, leg_supply) = per_market_extras[&mid];
                let state = &market_states[&mid];
                let new_demand = state.cumulative_extra_demand + leg_demand;
                let new_supply = state.cumulative_extra_supply + leg_supply;

                match solver.solve_market_with_extra_demand(
                    mid,
                    &problem.markets,
                    &state.base_orders,
                    new_demand,
                    new_supply,
                ) {
                    Some(sol) => {
                        new_solutions.insert(mid, sol);
                    }
                    None => {
                        commit_feasible = false;
                        break;
                    }
                }
            }

            if !commit_feasible {
                continue;
            }

            // Final welfare check with exact values
            let exact_new_welfare: i64 = affected_markets
                .iter()
                .filter_map(|mid| new_solutions.get(mid).map(|s| s.welfare))
                .sum();

            if exact_new_welfare + bundle_welfare as i64 <= old_welfare {
                continue;
            }

            // Update market states with new solutions and cumulative extras
            for &mid in &affected_markets {
                if let Some(sol) = new_solutions.remove(&mid) {
                    let (leg_demand, leg_supply) = per_market_extras[&mid];
                    if let Some(state) = market_states.get_mut(&mid) {
                        state.current_solution = sol;
                        state.cumulative_extra_demand += leg_demand;
                        state.cumulative_extra_supply += leg_supply;
                    }
                }
            }

            // Record fill for the bundle
            let fill_price = if bundle.is_seller() {
                (-bundle_cost_per_unit).max(0) as u64
            } else {
                bundle_cost_per_unit.max(0) as u64
            };

            bundle_fills.push(Fill::new(bundle.id, fill_qty, fill_price));
            filled.insert(bundle.id);
            bundles_matched += 1;
        }

        // Collect repriced solutions (only markets that were actually modified)
        let mut repriced_solutions: HashMap<MarketId, MarketSolution> = HashMap::new();
        for (mid, state) in market_states {
            if state.cumulative_extra_demand > 0 || state.cumulative_extra_supply > 0 {
                repriced_solutions.insert(mid, state.current_solution);
            }
        }

        let welfare: i64 = repriced_solutions.values().map(|s| s.welfare).sum::<i64>()
            + bundle_fills
                .iter()
                .zip(sorted_bundles.iter())
                .filter_map(|(f, b)| {
                    if f.order_id == b.id {
                        Some(b.welfare_contribution(f.fill_price, f.fill_qty))
                    } else {
                        None
                    }
                })
                .sum::<i64>();

        RepricingResult {
            bundle_fills,
            repriced_solutions,
            welfare,
            bundles_matched,
        }
    }
}

impl PartialSolver for MultiMarketSolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let mut solution = PartialSolution::new("MultiMarket");
        solution.confidence = SolutionConfidence::Heuristic;

        // Separate multi-market orders
        let multi_market_orders: Vec<(usize, &Order)> = problem
            .orders
            .iter()
            .enumerate()
            .filter(|(_, o)| o.num_markets > 1)
            .collect();

        if multi_market_orders.is_empty() {
            return solution;
        }

        // Track which orders have been filled
        let mut filled: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // Complement Matching (only strategy in solve_partial; repricing runs in pipeline)
        complement_match(&multi_market_orders, &mut filled, &mut solution, problem);

        // Calculate total welfare
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        solution.welfare = solution
            .fills
            .iter()
            .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
            .sum();

        solution
    }

    fn name(&self) -> &str {
        "MultiMarket"
    }

    fn confidence(&self) -> SolutionConfidence {
        SolutionConfidence::Heuristic
    }
}

/// Compute the payoff key for complement matching.
fn payoff_key(order: &Order) -> PayoffKey {
    let mut markets: Vec<MarketId> = order
        .markets
        .iter()
        .take(order.num_markets as usize)
        .copied()
        .collect();
    markets.sort();

    let abs_payoffs: Vec<u8> = order.payoffs[..order.num_states as usize]
        .iter()
        .map(|&p| p.unsigned_abs())
        .collect();

    PayoffKey {
        markets,
        abs_payoffs,
    }
}

/// Check if two orders have negated payoffs (complement pair).
fn is_complement_pair(a: &Order, b: &Order) -> bool {
    if a.num_states != b.num_states || a.num_markets != b.num_markets {
        return false;
    }
    // Check that markets match (same set)
    let mut a_markets: Vec<MarketId> = a.active_markets().collect();
    let mut b_markets: Vec<MarketId> = b.active_markets().collect();
    a_markets.sort();
    b_markets.sort();
    if a_markets != b_markets {
        return false;
    }
    // If markets are in same order, payoffs should negate directly
    if a.markets[..a.num_markets as usize] == b.markets[..b.num_markets as usize] {
        for i in 0..a.num_states as usize {
            if a.payoffs[i] != -b.payoffs[i] {
                return false;
            }
        }
        return true;
    }
    // Markets in different order — would need state remapping.
    // For simplicity, skip this case (rare in practice).
    false
}

/// Strategy 1: Match multi-market orders with complementary payoffs.
///
/// Within each payoff group, pairs orders whose payoffs exactly negate.
/// Uses a two-pointer approach: sort by limit price descending, then match
/// the highest-limit order with the lowest-limit complement.
fn complement_match(
    multi_orders: &[(usize, &Order)],
    filled: &mut std::collections::HashSet<u64>,
    solution: &mut PartialSolution,
    _problem: &Problem,
) {
    // Group by PayoffKey (same markets, same |payoffs|)
    let mut groups: HashMap<PayoffKey, Vec<(usize, &Order)>> = HashMap::new();
    for &(idx, order) in multi_orders {
        let key = payoff_key(order);
        groups.entry(key).or_default().push((idx, order));
    }

    for orders_in_group in groups.values() {
        // Split into two sides by the sign of the first non-zero payoff.
        // For bundles: side_a = bundle_yes [+1,...], side_b = bundle_sell [-1,...]
        // For spreads: side_a = spread [0,-1,+1,0], side_b = spread_sell [0,+1,-1,0]
        let mut side_a: Vec<&Order> = Vec::new();
        let mut side_b: Vec<&Order> = Vec::new();

        for &(_idx, order) in orders_in_group {
            let first_nonzero = order.payoffs[..order.num_states as usize]
                .iter()
                .find(|&&p| p != 0)
                .copied()
                .unwrap_or(0);
            if first_nonzero > 0 {
                side_a.push(order);
            } else if first_nonzero < 0 {
                side_b.push(order);
            }
        }

        if side_a.is_empty() || side_b.is_empty() {
            continue;
        }

        // Sort side_a by limit desc, side_b by limit desc
        side_a.sort_by(|a, b| b.limit_price.cmp(&a.limit_price));
        side_b.sort_by(|a, b| b.limit_price.cmp(&a.limit_price));

        // Match condition depends on order type:
        // - Buyer (is_seller=false): fill_price <= limit (max willing to pay)
        // - Seller (is_seller=true): fill_price >= limit (min acceptable)
        //
        // A valid complement match requires a fill_price F satisfying both constraints.
        // Pair by aggressiveness: highest-limit from each side.
        let mut bi = 0;
        for a_order in &side_a {
            if filled.contains(&a_order.id) {
                continue;
            }
            while bi < side_b.len() && filled.contains(&side_b[bi].id) {
                bi += 1;
            }
            if bi >= side_b.len() {
                break;
            }
            let b_order = side_b[bi];

            // Verify they're actually complements
            if !is_complement_pair(a_order, b_order) {
                continue;
            }

            // Compute the valid fill_price range for each order:
            // Buyer: F <= limit (upper bound)
            // Seller: F >= limit (lower bound)
            let (a_lo, a_hi) = if a_order.is_seller() {
                (a_order.limit_price as i128, i128::MAX)
            } else {
                (0, a_order.limit_price as i128)
            };
            let (b_lo, b_hi) = if b_order.is_seller() {
                (b_order.limit_price as i128, i128::MAX)
            } else {
                (0, b_order.limit_price as i128)
            };

            let lo = a_lo.max(b_lo);
            let hi = a_hi.min(b_hi);
            if lo > hi {
                // No valid fill_price exists — check if we should stop or just skip
                // For the sorted-desc pairing, if this doesn't match, later pairs won't either
                // only if both sides have the same direction. Be conservative: just skip.
                bi += 1;
                continue;
            }

            // Fill quantity = min of both sides
            let max_qty = a_order.max_fill.min(b_order.max_fill);
            if max_qty == 0 {
                bi += 1;
                continue;
            }

            // Fill price = midpoint of valid range (capped to reasonable bounds)
            let effective_hi = hi.min(2 * 1_000_000_000); // cap at $2 for midpoint calc
            let fill_price = ((lo + effective_hi) / 2) as u64;

            solution
                .fills
                .push(Fill::new(a_order.id, max_qty, fill_price));
            solution
                .fills
                .push(Fill::new(b_order.id, max_qty, fill_price));

            filled.insert(a_order.id);
            filled.insert(b_order.id);
            bi += 1;
        }
    }
}

/// Compute per-market legs from a multi-market order's payoff vector.
///
/// For each market in the order, computes the marginal position by averaging
/// the payoff across states where that market has each outcome.
fn compute_legs(order: &Order, market_sizes: &[u8]) -> Vec<Leg> {
    let num_states = order.num_states as usize;
    let num_markets = order.num_markets as usize;
    let mut legs = Vec::new();

    for m_idx in 0..num_markets {
        let market = order.markets[m_idx];
        let n_outcomes = market_sizes[m_idx] as usize;

        for outcome in 0..n_outcomes {
            // Sum payoffs over states where this market has this outcome
            let mut payoff_sum: i64 = 0;
            let mut state_count: u64 = 0;

            for s in 0..num_states {
                // Decode state to get outcome for market m_idx
                let mut remaining = s;
                let mut this_outcome = 0usize;
                for (k, &sz) in market_sizes.iter().enumerate().take(num_markets) {
                    let sz = sz as usize;
                    if k == m_idx {
                        this_outcome = remaining % sz;
                    }
                    remaining /= sz;
                }

                if this_outcome == outcome {
                    payoff_sum += order.payoffs[s] as i64;
                    state_count += 1;
                }
            }

            if payoff_sum != 0 && state_count > 0 {
                legs.push(Leg {
                    market,
                    outcome: outcome as u8,
                    shares_numer: payoff_sum,
                    shares_denom: state_count,
                });
            }
        }
    }

    legs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::PriceDiscoverer;
    use matching_engine::{
        bundle_sell, bundle_yes, outcome_buy, outcome_sell, price_to_nanos, spread, spread_sell,
        Problem,
    };

    fn setup_markets() -> (Problem, MarketId, MarketId) {
        let mut problem = Problem::new("multi_market_test");
        let m0 = problem.markets.add_binary("Market A");
        let m1 = problem.markets.add_binary("Market B");
        (problem, m0, m1)
    }

    #[test]
    fn test_complement_match_bundles() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer at 20 cents
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.20),
            100,
        ));

        // Bundle seller at 15 cents
        problem.orders.push(bundle_sell(
            &problem.markets,
            2,
            &[m0, m1],
            price_to_nanos(0.15),
            100,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        assert!(
            result.fills.len() >= 2,
            "Expected fills for buyer and seller, got {}",
            result.fills.len()
        );

        // Check fill prices are midpoint
        let buyer_fill = result.fills.iter().find(|f| f.order_id == 1).unwrap();
        let seller_fill = result.fills.iter().find(|f| f.order_id == 2).unwrap();
        assert_eq!(buyer_fill.fill_qty, 100);
        assert_eq!(seller_fill.fill_qty, 100);

        let expected_price = (price_to_nanos(0.20) + price_to_nanos(0.15)) / 2;
        assert_eq!(buyer_fill.fill_price, expected_price);
    }

    #[test]
    fn test_complement_match_spreads() {
        let (mut problem, m0, m1) = setup_markets();

        // Spread buyer at 10 cents
        problem.orders.push(spread(
            &problem.markets,
            1,
            m0,
            m1,
            price_to_nanos(0.10),
            50,
        ));

        // Spread seller at 5 cents
        problem.orders.push(spread_sell(
            &problem.markets,
            2,
            m0,
            m1,
            price_to_nanos(0.05),
            50,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        assert!(
            result.fills.len() >= 2,
            "Expected fills for spread buyer and seller"
        );
    }

    #[test]
    fn test_no_match_when_buyer_below_seller() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer at 10 cents
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.10),
            100,
        ));

        // Bundle seller at 20 cents (too expensive)
        problem.orders.push(bundle_sell(
            &problem.markets,
            2,
            &[m0, m1],
            price_to_nanos(0.20),
            100,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        assert!(
            result.fills.is_empty(),
            "Should not match when buyer limit < seller limit"
        );
    }

    #[test]
    fn test_repricing_basic() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer willing to pay 40 cents for both YES
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.40),
            100,
        ));

        // Individual sellers providing liquidity
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.15),
            200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.15),
            200,
        ));

        // Run price discovery first
        let solver = LocalSolver::new();
        let pd_result = solver.discover_prices(&problem);

        // Run repricing
        let mm_solver = MultiMarketSolver::new();
        let result = mm_solver.solve_with_repricing(&problem, &pd_result);

        let bundle_fill = result.bundle_fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_some(),
            "Bundle order should be filled via repricing"
        );
        assert_eq!(bundle_fill.unwrap().fill_qty, 100);
    }

    #[test]
    fn test_repricing_too_expensive() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer willing to pay only 20 cents
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.20),
            100,
        ));

        // Individual sellers too expensive (30 cents each = 60 cents total for bundle)
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.30),
            200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.30),
            200,
        ));

        let solver = LocalSolver::new();
        let pd_result = solver.discover_prices(&problem);

        let mm_solver = MultiMarketSolver::new();
        let result = mm_solver.solve_with_repricing(&problem, &pd_result);

        let bundle_fill = result.bundle_fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_none(),
            "Bundle order should NOT be filled when legs are too expensive"
        );
    }

    #[test]
    fn test_repricing_net_welfare_check() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer at a marginal price — should only fill if net welfare positive
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.30),
            100,
        ));

        // Sellers
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.10),
            200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.10),
            200,
        ));

        let solver = LocalSolver::new();
        let pd_result = solver.discover_prices(&problem);

        let mm_solver = MultiMarketSolver::new();
        let result = mm_solver.solve_with_repricing(&problem, &pd_result);

        // If filled, welfare must be non-negative
        if !result.bundle_fills.is_empty() {
            assert!(
                result.welfare >= 0,
                "Welfare must be non-negative after repricing"
            );
        }
    }

    #[test]
    fn test_repricing_multiple_bundles() {
        let (mut problem, m0, m1) = setup_markets();

        // Two bundle buyers
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.40),
            50,
        ));
        problem.orders.push(bundle_yes(
            &problem.markets,
            2,
            &[m0, m1],
            price_to_nanos(0.35),
            50,
        ));

        // Sellers with enough capacity for both
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.10),
            200,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.10),
            200,
        ));

        let solver = LocalSolver::new();
        let pd_result = solver.discover_prices(&problem);

        let mm_solver = MultiMarketSolver::new();
        let result = mm_solver.solve_with_repricing(&problem, &pd_result);

        // At least one bundle should fill
        assert!(
            !result.bundle_fills.is_empty(),
            "At least one bundle should fill"
        );
    }

    #[test]
    fn test_repricing_spread() {
        let (mut problem, m0, m1) = setup_markets();

        // Spread: long A, short B
        problem.orders.push(spread(
            &problem.markets,
            1,
            m0,
            m1,
            price_to_nanos(0.20),
            50,
        ));

        // Sellers for market A (spread needs to buy A-YES)
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.10),
            200,
        ));

        // Buyers for market B (spread needs to sell B-YES, i.e., find B-YES buyers)
        problem.orders.push(outcome_buy(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.80),
            200,
        ));

        let solver = LocalSolver::new();
        let pd_result = solver.discover_prices(&problem);

        let mm_solver = MultiMarketSolver::new();
        let result = mm_solver.solve_with_repricing(&problem, &pd_result);

        // Spread should match if cost < limit
        // This test primarily verifies no panics with spread orders
        let _ = result;
    }

    #[test]
    fn test_welfare_positive() {
        let (mut problem, m0, m1) = setup_markets();

        // Profitable complement match
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.25),
            100,
        ));
        problem.orders.push(bundle_sell(
            &problem.markets,
            2,
            &[m0, m1],
            price_to_nanos(0.10),
            100,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        assert!(result.welfare >= 0, "Total welfare should be non-negative");
    }

    #[test]
    fn test_empty_problem() {
        let problem = Problem::new("empty");
        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);
        assert!(result.fills.is_empty());
    }

    #[test]
    fn test_no_multi_market_orders() {
        let mut problem = Problem::new("single_only");
        let m0 = problem.markets.add_binary("A");

        problem.orders.push(outcome_buy(
            &problem.markets,
            1,
            m0,
            0,
            price_to_nanos(0.50),
            100,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);
        assert!(result.fills.is_empty(), "No multi-market orders to match");
    }
}
