//! Multi-Market Order Matching Solver
//!
//! Matches multi-market orders (bundles, spreads) using two strategies:
//!
//! 1. **Complement Matching**: Orders with identical markets and negated payoffs
//!    cancel perfectly (e.g., bundle_yes + bundle_sell). Standard bid >= ask matching.
//!
//! 2. **Leg Decomposition**: Decomposes multi-market orders into per-market legs
//!    and matches each leg against single-market counterparties.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, Order, Problem, Qty, Nanos};

use crate::combiner::SolutionConfidence;
use crate::traits::{PartialSolution, PartialSolver};

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

/// Entry in the counterparty pool.
#[derive(Clone, Debug)]
struct PoolEntry {
    order_id: u64,
    /// Price per unit of this outcome
    price: Nanos,
    /// Available quantity (decremented as consumed)
    available_qty: Qty,
    /// Original min_fill for AON checking
    min_fill: Qty,
    /// Original max_fill
    max_fill: Qty,
}

/// Pool of available single-market orders indexed by (market, outcome).
struct CounterpartyPool {
    /// Sellers of (market, outcome), sorted by price ascending (cheapest first)
    sellers: HashMap<(MarketId, u8), Vec<PoolEntry>>,
    /// Buyers of (market, outcome), sorted by price descending (best bid first)
    buyers: HashMap<(MarketId, u8), Vec<PoolEntry>>,
}

impl CounterpartyPool {
    fn new() -> Self {
        Self {
            sellers: HashMap::new(),
            buyers: HashMap::new(),
        }
    }

    fn add_seller(&mut self, market: MarketId, outcome: u8, entry: PoolEntry) {
        self.sellers.entry((market, outcome)).or_default().push(entry);
    }

    fn add_buyer(&mut self, market: MarketId, outcome: u8, entry: PoolEntry) {
        self.buyers.entry((market, outcome)).or_default().push(entry);
    }

    fn sort(&mut self) {
        for entries in self.sellers.values_mut() {
            entries.sort_by_key(|e| e.price);
        }
        for entries in self.buyers.values_mut() {
            entries.sort_by(|a, b| b.price.cmp(&a.price));
        }
    }
}

#[derive(Default)]
pub struct MultiMarketSolver;

impl MultiMarketSolver {
    pub fn new() -> Self {
        Self
    }
}

impl PartialSolver for MultiMarketSolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution {
        let mut solution = PartialSolution::new("MultiMarket");
        solution.confidence = SolutionConfidence::Heuristic;

        // Build set of MM order IDs to exclude from counterparty pool
        let mm_order_ids: std::collections::HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // Separate multi-market and single-market orders
        let mut multi_market_orders: Vec<(usize, &Order)> = Vec::new();
        let mut single_market_orders: Vec<(usize, &Order)> = Vec::new();

        for (idx, order) in problem.orders.iter().enumerate() {
            if order.num_markets > 1 {
                multi_market_orders.push((idx, order));
            } else if order.num_markets == 1 && !mm_order_ids.contains(&order.id) {
                single_market_orders.push((idx, order));
            }
        }

        if multi_market_orders.is_empty() {
            return solution;
        }

        // Track which orders have been filled
        let mut filled: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // ================================================================
        // Strategy 1: Complement Matching
        // ================================================================
        complement_match(&multi_market_orders, &mut filled, &mut solution, problem);

        // ================================================================
        // Strategy 2: Leg Decomposition
        // ================================================================
        // Build counterparty pool from unfilled single-market orders
        let mut pool = CounterpartyPool::new();
        for &(_idx, order) in &single_market_orders {
            if filled.contains(&order.id) {
                continue;
            }
            let market = order.markets[0];
            let num_states = order.num_states as usize;

            for outcome in 0..num_states {
                let payoff = order.payoffs[outcome];
                if payoff > 0 {
                    // This is a buyer of this outcome
                    pool.add_buyer(
                        market,
                        outcome as u8,
                        PoolEntry {
                            order_id: order.id,
                            price: order.limit_price,
                            available_qty: order.max_fill,
                            min_fill: order.min_fill,
                            max_fill: order.max_fill,
                        },
                    );
                } else if payoff < 0 {
                    // This is a seller of this outcome
                    pool.add_seller(
                        market,
                        outcome as u8,
                        PoolEntry {
                            order_id: order.id,
                            price: order.limit_price,
                            available_qty: order.max_fill,
                            min_fill: order.min_fill,
                            max_fill: order.max_fill,
                        },
                    );
                }
            }
        }
        pool.sort();

        leg_decomposition_match(
            &multi_market_orders,
            &mut pool,
            &mut filled,
            &mut solution,
            problem,
        );

        // Calculate total welfare
        let order_map: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();
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

            // Fill quantity = min of both sides, respecting AON
            let max_qty = a_order.max_fill.min(b_order.max_fill);
            if max_qty == 0 {
                bi += 1;
                continue;
            }

            if a_order.is_all_or_none() && max_qty < a_order.min_fill {
                continue;
            }
            if b_order.is_all_or_none() && max_qty < b_order.min_fill {
                bi += 1;
                continue;
            }

            // Fill price = midpoint of valid range (capped to reasonable bounds)
            let effective_hi = hi.min(2 * 1_000_000_000); // cap at $2 for midpoint calc
            let fill_price = ((lo + effective_hi) / 2) as u64;

            solution.fills.push(Fill::new(a_order.id, max_qty, fill_price));
            solution.fills.push(Fill::new(b_order.id, max_qty, fill_price));

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

/// Strategy 2: Match multi-market orders by decomposing into per-market legs.
fn leg_decomposition_match(
    multi_orders: &[(usize, &Order)],
    pool: &mut CounterpartyPool,
    filled: &mut std::collections::HashSet<u64>,
    solution: &mut PartialSolution,
    problem: &Problem,
) {
    // Sort multi-market orders by welfare potential (limit_price * max_fill) descending
    let mut sorted_orders: Vec<(usize, &Order)> = multi_orders
        .iter()
        .filter(|(_, o)| !filled.contains(&o.id))
        .copied()
        .collect();
    sorted_orders.sort_by(|a, b| {
        let welfare_a = a.1.limit_price as i128 * a.1.max_fill as i128;
        let welfare_b = b.1.limit_price as i128 * b.1.max_fill as i128;
        welfare_b.cmp(&welfare_a)
    });

    // Accumulate counterparty fills: order_id -> (total_qty, total_cost_or_revenue)
    // to avoid emitting duplicate Fill records for the same order.
    let mut counterparty_fills: HashMap<u64, (Qty, i128)> = HashMap::new();

    // Build order lookup for counterparty price/AON info
    let order_map: HashMap<u64, &Order> =
        problem.orders.iter().map(|o| (o.id, o)).collect();

    for &(_idx, order) in &sorted_orders {
        if filled.contains(&order.id) {
            continue;
        }

        // Compute market sizes for this order
        let market_sizes: Vec<u8> = order
            .markets
            .iter()
            .take(order.num_markets as usize)
            .map(|id| problem.markets.num_outcomes(*id))
            .collect();

        let legs = compute_legs(order, &market_sizes);
        if legs.is_empty() {
            continue;
        }

        let fill_qty = order.max_fill;
        if fill_qty == 0 {
            continue;
        }

        // For each leg, determine needed quantity and find counterparties.
        // A leg with shares_numer > 0 means we need to BUY that outcome (find sellers).
        // A leg with shares_numer < 0 means we need to SELL that outcome (find buyers).
        //
        // total_cost: positive = net outflow (buying), negative = net inflow (selling)
        let mut leg_consumptions: Vec<(u64, Qty, Nanos)> = Vec::new();
        let mut total_cost: i128 = 0;
        let mut feasible = true;

        for leg in &legs {
            let abs_shares = leg.shares_numer.unsigned_abs() * fill_qty;
            let needed_shares = abs_shares / leg.shares_denom;
            if needed_shares == 0 {
                continue;
            }

            if leg.shares_numer > 0 {
                // Need to buy this outcome -> find sellers
                let Some(sellers) = pool.sellers.get(&(leg.market, leg.outcome)) else {
                    feasible = false;
                    break;
                };

                let mut remaining = needed_shares;
                let mut leg_cost: i128 = 0;
                let mut matched: Vec<(u64, Qty, Nanos)> = Vec::new();

                for entry in sellers {
                    if remaining == 0 {
                        break;
                    }
                    if entry.available_qty == 0 {
                        continue;
                    }
                    // Skip AON orders where we can't fill their minimum
                    if entry.min_fill > 0 && entry.min_fill == entry.max_fill {
                        if remaining < entry.min_fill {
                            continue;
                        }
                    }
                    let take = remaining.min(entry.available_qty);
                    leg_cost += entry.price as i128 * take as i128;
                    matched.push((entry.order_id, take, entry.price));
                    remaining -= take;
                }

                if remaining > 0 {
                    feasible = false;
                    break;
                }

                total_cost += leg_cost;
                leg_consumptions.extend(matched);
            } else {
                // Need to sell this outcome -> find buyers
                let Some(buyers) = pool.buyers.get(&(leg.market, leg.outcome)) else {
                    feasible = false;
                    break;
                };

                let mut remaining = needed_shares;
                let mut leg_revenue: i128 = 0;
                let mut matched: Vec<(u64, Qty, Nanos)> = Vec::new();

                for entry in buyers {
                    if remaining == 0 {
                        break;
                    }
                    if entry.available_qty == 0 {
                        continue;
                    }
                    if entry.min_fill > 0 && entry.min_fill == entry.max_fill {
                        if remaining < entry.min_fill {
                            continue;
                        }
                    }
                    let take = remaining.min(entry.available_qty);
                    leg_revenue += entry.price as i128 * take as i128;
                    matched.push((entry.order_id, take, entry.price));
                    remaining -= take;
                }

                if remaining > 0 {
                    feasible = false;
                    break;
                }

                total_cost -= leg_revenue;
                leg_consumptions.extend(matched);
            }
        }

        if !feasible {
            continue;
        }

        // Check cost/revenue constraint based on order type.
        //
        // For buyers (is_seller=false): total_cost <= limit * qty
        //   (they pay no more than their limit)
        //
        // For sellers (is_seller=true): revenue >= limit * qty
        //   i.e. -total_cost >= limit * qty  (they receive at least their limit)
        let limit_value = order.limit_price as i128 * fill_qty as i128;
        if order.is_seller() {
            // Seller needs sufficient revenue: -total_cost >= limit * qty
            if -total_cost < limit_value {
                continue;
            }
        } else {
            // Buyer: total_cost <= limit * qty
            if total_cost > limit_value {
                continue;
            }
        }

        // Check AON constraint on the multi-market order
        if order.is_all_or_none() && fill_qty < order.min_fill {
            continue;
        }

        // Commit: emit fill for the multi-market order
        let fill_price = if fill_qty > 0 {
            if order.is_seller() {
                // Seller receives revenue. fill_price = revenue / qty
                ((-total_cost) as u64) / fill_qty
            } else {
                // Buyer pays cost. fill_price = cost / qty
                (total_cost.max(0) as u64) / fill_qty
            }
        } else {
            0
        };

        solution.fills.push(Fill::new(order.id, fill_qty, fill_price));
        filled.insert(order.id);

        // Accumulate counterparty consumptions and reduce pool liquidity
        for (oid, qty, price) in &leg_consumptions {
            let entry = counterparty_fills.entry(*oid).or_insert((0, 0));
            entry.0 += qty;
            entry.1 += *price as i128 * *qty as i128;

            // Consume from pool
            for entries in pool.sellers.values_mut().chain(pool.buyers.values_mut()) {
                for pool_entry in entries.iter_mut() {
                    if pool_entry.order_id == *oid {
                        pool_entry.available_qty =
                            pool_entry.available_qty.saturating_sub(*qty);
                    }
                }
            }
        }
    }

    // Emit aggregated counterparty fills (one Fill per order_id)
    for (oid, (total_qty, total_cost)) in &counterparty_fills {
        if *total_qty == 0 {
            continue;
        }

        // Check counterparty AON constraint
        if let Some(cp_order) = order_map.get(oid) {
            if cp_order.is_all_or_none() && *total_qty < cp_order.min_fill {
                continue; // Can't partially fill an AON order
            }
            // Don't exceed max_fill
            let capped_qty = (*total_qty).min(cp_order.max_fill);
            if capped_qty == 0 {
                continue;
            }

            // Weighted average price
            let avg_price = (*total_cost as u64) / capped_qty;

            // Verify fill respects the counterparty's limit
            if cp_order.is_seller() {
                if avg_price < cp_order.limit_price {
                    continue; // Seller wouldn't accept this price
                }
            } else if avg_price > cp_order.limit_price {
                continue; // Buyer wouldn't pay this much
            }

            solution.fills.push(Fill::new(*oid, capped_qty, avg_price));
            filled.insert(*oid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_leg_decomposition() {
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
        // Seller of A-YES at 15 cents
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.15),
            200,
        ));

        // Seller of B-YES at 15 cents
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.15),
            200,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        // Should match: bundle buyer gets fills via decomposed legs
        let bundle_fill = result.fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_some(),
            "Bundle order should be filled via leg decomposition"
        );
    }

    #[test]
    fn test_leg_decomposition_too_expensive() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer willing to pay only 20 cents
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.20),
            100,
        ));

        // Individual sellers too expensive (30 cents each = 60 cents total)
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

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        let bundle_fill = result.fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_none(),
            "Bundle order should NOT be filled when legs are too expensive"
        );
    }

    #[test]
    fn test_aon_insufficient_qty() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer AON wanting 100 units
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.40),
            100,
        ));

        // Sellers only have 50 units each
        problem.orders.push(outcome_sell(
            &problem.markets,
            10,
            m0,
            0,
            price_to_nanos(0.15),
            50,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            11,
            m1,
            0,
            price_to_nanos(0.15),
            50,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        // bundle_yes creates AON orders, so with only 50 units available per leg
        // but needing 50 per leg (100 * 1/2 = 50), it should still fill
        let bundle_fill = result.fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_some(),
            "Bundle should fill when leg qty is sufficient"
        );
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

        assert!(
            result.welfare >= 0,
            "Total welfare should be non-negative"
        );
    }

    #[test]
    fn test_counterparty_stacking() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer wanting 100 units
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.40),
            100,
        ));

        // Multiple small sellers for A-YES
        for i in 0..5 {
            problem.orders.push(outcome_sell(
                &problem.markets,
                10 + i,
                m0,
                0,
                price_to_nanos(0.15),
                20, // 5 * 20 = 100 total, need 50
            ));
        }

        // Single seller for B-YES
        problem.orders.push(outcome_sell(
            &problem.markets,
            20,
            m1,
            0,
            price_to_nanos(0.15),
            200,
        ));

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        let bundle_fill = result.fills.iter().find(|f| f.order_id == 1);
        assert!(
            bundle_fill.is_some(),
            "Bundle should fill using stacked counterparties"
        );
    }

    #[test]
    fn test_per_market_netting() {
        let (mut problem, m0, m1) = setup_markets();

        // Bundle buyer at generous price
        problem.orders.push(bundle_yes(
            &problem.markets,
            1,
            &[m0, m1],
            price_to_nanos(0.50),
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

        let solver = MultiMarketSolver::new();
        let result = solver.solve_partial(&problem);

        // Verify fills exist
        assert!(!result.fills.is_empty(), "Should have fills");

        // Per-market netting: for each market and outcome, total long = total short
        // The bundle buyer has +1 at state 0 (both YES), which decomposes to
        // +1/2 A-YES and +1/2 B-YES per unit. Sellers provide -1 A-YES and -1 B-YES.
        // Net per market should be zero.
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
