//! Group-level minting for mutually exclusive market groups.
//!
//! In a group of N mutually exclusive markets, minting 1 YES share on every
//! market costs $1 total (guaranteed $1 payoff since exactly one resolves YES).
//! This is N times cheaper than N separate per-market mints ($1 each).
//!
//! MILP captures this via a `group_mint_g` continuous variable. This module
//! provides the equivalent for heuristic solvers.
//!
//! # Algorithms
//!
//! ## Water-filling (`find_group_mints`)
//!
//! For each group, sort each market's unfilled buy-YES orders by limit descending.
//! Find Q* = max Q such that Σ_m L_{m,Q} ≥ $1 (the sum of marginal limits across
//! all markets exceeds the minting cost). Fill Q* orders on each market.
//!
//! ## Simplex price search (`simplex_search`)
//!
//! Re-solves each group from scratch by binary-searching for the fill quantity Q*
//! where the clearing prices land on the simplex Σ p_m = $1. Unlike water-filling,
//! this considers ALL orders (not just unfilled) and includes natural sellers in
//! the clearing, finding welfare-maximizing prices on the simplex.
//!
//! **Note**: Not currently called from the pipeline. Changing clearing prices
//! causes `enforce_ucp` to reprice ALL fills globally, which can drop MM fills
//! that were valid at DualMaster prices, creating more UCP damage than the
//! simplex welfare improvement. Needs a price-change-aware UCP integration.
//!
//! # Position Balance
//!
//! Creates synthetic sell-YES arb orders (one per market). When used via the pipeline,
//! arb order limits are set proportionally (limit_m = $1 × p_m / Σp), so the minting
//! cost is automatically captured in arb welfare = Q × (Σp - $1).

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketGroup, MarketId, MarketSet, Nanos, Order, NANOS_PER_DOLLAR};

/// Result of group minting for one or more groups.
#[derive(Clone, Debug, Default)]
pub struct GroupMintResult {
    /// Fills for real buyer orders (filled via group minting supply).
    pub buyer_fills: Vec<Fill>,
    /// Synthetic arb sell-YES orders for position balance.
    pub arb_orders: Vec<Order>,
    /// Fills for the arb orders.
    pub arb_fills: Vec<Fill>,
    /// Per-market clearing prices from group minting.
    pub clearing_prices: HashMap<MarketId, Nanos>,
    /// Total minting cost across all groups (Σ Q_g × $1).
    pub minting_cost: i64,
}

/// Find group minting opportunities across all market groups.
///
/// For each group, applies the water-filling algorithm to unfilled buy-YES demand.
/// Returns fills, arb orders, and the total minting cost.
pub fn find_group_mints(
    groups: &[MarketGroup],
    order_map: &HashMap<u64, &Order>,
    filled_order_ids: &HashSet<u64>,
    this_iter_filled: &HashSet<u64>,
    next_arb_id: &mut u64,
) -> GroupMintResult {
    let mut result = GroupMintResult::default();

    for group in groups {
        if group.markets.len() < 2 {
            continue;
        }
        if let Some(group_result) =
            find_group_mint_single(group, order_map, filled_order_ids, this_iter_filled, next_arb_id)
        {
            result.buyer_fills.extend(group_result.buyer_fills);
            result.arb_orders.extend(group_result.arb_orders);
            result.arb_fills.extend(group_result.arb_fills);
            result.clearing_prices.extend(group_result.clearing_prices);
            result.minting_cost += group_result.minting_cost;
        }
    }

    result
}

/// Demand entry: an unfilled buy-YES order on a single market.
struct DemandEntry {
    order_id: u64,
    limit_price: Nanos,
    max_fill: u64,
}

/// Find group minting opportunity for a single market group.
fn find_group_mint_single(
    group: &MarketGroup,
    order_map: &HashMap<u64, &Order>,
    filled_order_ids: &HashSet<u64>,
    this_iter_filled: &HashSet<u64>,
    next_arb_id: &mut u64,
) -> Option<GroupMintResult> {
    let group_markets: HashSet<MarketId> = group.markets.iter().copied().collect();

    // Collect unfilled buy-YES orders per market, sorted by limit descending.
    let mut demands: HashMap<MarketId, Vec<DemandEntry>> = HashMap::new();
    for &market in &group.markets {
        demands.insert(market, Vec::new());
    }

    for (&order_id, &order) in order_map {
        if filled_order_ids.contains(&order_id) || this_iter_filled.contains(&order_id) {
            continue;
        }
        // Single-market buy-YES only
        if order.num_markets != 1 || order.num_states != 2 {
            continue;
        }
        let market = order.markets[0];
        if !group_markets.contains(&market) {
            continue;
        }
        // Buy YES: payoffs[0] > 0 (YES state pays), payoffs[1] == 0
        let is_buy_yes = order.payoffs[0] > 0 && order.payoffs[1] == 0;
        if !is_buy_yes {
            continue;
        }
        demands.entry(market).or_default().push(DemandEntry {
            order_id,
            limit_price: order.limit_price,
            max_fill: order.max_fill,
        });
    }

    // Sort each market's demand descending by limit price
    for entries in demands.values_mut() {
        entries.sort_by(|a, b| b.limit_price.cmp(&a.limit_price));
    }

    // Water-filling: find Q* where Σ_m L_{m,Q} ≥ $1.
    //
    // We expand orders by max_fill (each unit of an order is a separate "slot").
    // For efficiency, we flatten the sorted demand into cumulative slot arrays.
    let mut slot_limits: Vec<Vec<(u64, Nanos)>> = Vec::new(); // per-market: (order_id, limit)
    for &market in &group.markets {
        let entries = demands.get(&market).unwrap();
        let mut slots = Vec::new();
        for entry in entries {
            for _ in 0..entry.max_fill {
                slots.push((entry.order_id, entry.limit_price));
            }
        }
        slot_limits.push(slots);
    }

    // max_q = min number of slots across all markets (linking constraint)
    let max_q = slot_limits.iter().map(|s| s.len()).min().unwrap_or(0);
    if max_q == 0 {
        return None;
    }

    // Find Q*: largest Q where sum of marginal limits ≥ $1
    let mut q_star = 0usize;
    for q in 0..max_q {
        let marginal_sum: u128 = slot_limits.iter().map(|s| s[q].1 as u128).sum();
        if marginal_sum < NANOS_PER_DOLLAR as u128 {
            break;
        }
        q_star = q + 1;
    }

    if q_star == 0 {
        return None;
    }

    // Compute per-market clearing price = the marginal limit (Q*-th slot's limit)
    let clearing_prices: HashMap<MarketId, Nanos> = group
        .markets
        .iter()
        .enumerate()
        .map(|(i, &market)| {
            let price = slot_limits[i][q_star - 1].1;
            (market, price)
        })
        .collect();

    // Create buyer fills: fill the first Q* slots on each market
    let mut buyer_fills = Vec::new();
    // Track per-order fill quantities (an order may span multiple slots)
    let mut order_fill_qtys: HashMap<u64, u64> = HashMap::new();

    for (i, &market) in group.markets.iter().enumerate() {
        let price = clearing_prices[&market];
        for q in 0..q_star {
            let (order_id, _limit) = slot_limits[i][q];
            *order_fill_qtys.entry(order_id).or_insert(0) += 1;
            // We'll consolidate fills below
            let _ = (order_id, price, market);
        }
    }

    // Consolidate: one fill per order with total quantity
    // Group by (order_id, market) to get the right fill_price
    let mut fill_map: HashMap<u64, (Nanos, u64)> = HashMap::new(); // order_id -> (price, qty)
    for (i, &market) in group.markets.iter().enumerate() {
        let price = clearing_prices[&market];
        let mut per_order: HashMap<u64, u64> = HashMap::new();
        for q in 0..q_star {
            let (order_id, _) = slot_limits[i][q];
            *per_order.entry(order_id).or_insert(0) += 1;
        }
        for (order_id, qty) in per_order {
            fill_map.insert(order_id, (price, qty));
        }
    }

    for (order_id, (price, qty)) in &fill_map {
        buyer_fills.push(Fill {
            order_id: *order_id,
            fill_price: *price,
            fill_qty: *qty,
        });
    }

    // Create arb sell-YES orders for position balance.
    // One per market, selling Q* units at the clearing price.
    // limit_price = fill_price → zero welfare contribution.
    let mut arb_orders = Vec::new();
    let mut arb_fills = Vec::new();

    for &market in &group.markets {
        let price = clearing_prices[&market];
        let arb_id = *next_arb_id;
        *next_arb_id += 1;

        let mut order = Order::new(arb_id);
        order.markets[0] = market;
        order.num_markets = 1;
        order.num_states = 2;
        order.payoffs[0] = -1; // Sell YES: owe $1 if YES
        order.payoffs[1] = 0;
        order.limit_price = price; // limit = fill_price → zero welfare
        order.max_fill = q_star as u64;

        arb_fills.push(Fill {
            order_id: arb_id,
            fill_price: price,
            fill_qty: q_star as u64,
        });

        arb_orders.push(order);
    }

    // Minting cost: Q* × $1
    let minting_cost = q_star as i64 * NANOS_PER_DOLLAR as i64;

    Some(GroupMintResult {
        buyer_fills,
        arb_orders,
        arb_fills,
        clearing_prices,
        minting_cost,
    })
}

// ============================================================================
// Simplex Price Search
// ============================================================================

/// Result of simplex search for a single group.
#[derive(Clone, Debug)]
pub struct SimplexResult {
    /// Fills for real orders (buyers + natural sellers) at simplex prices.
    pub fills: Vec<Fill>,
    /// Synthetic arb sell-YES orders for position balance.
    pub arb_orders: Vec<Order>,
    /// Fills for the arb orders.
    pub arb_fills: Vec<Fill>,
    /// Clearing prices at the simplex solution.
    pub clearing_prices: HashMap<MarketId, Nanos>,
    /// Total welfare from new fills (real + arb).
    pub welfare: i64,
    /// Welfare of DualMaster fills being replaced.
    pub existing_welfare: i64,
    /// Order IDs of existing fills that are replaced.
    pub replaced_order_ids: Vec<u64>,
}

/// Run simplex price search on all market groups.
///
/// For each group, tries to find better fills by re-solving with group minting
/// supply on the simplex Σp = $1. Returns results only for groups where the
/// simplex search improves over existing fills.
pub fn simplex_search(
    groups: &[MarketGroup],
    all_orders: &[Order],
    markets: &MarketSet,
    existing_fills: &[Fill],
    order_map: &HashMap<u64, &Order>,
    mm_order_ids: &HashSet<u64>,
    next_arb_id: &mut u64,
) -> Vec<SimplexResult> {
    let mut results = Vec::new();

    for group in groups {
        if let Some(result) = simplex_search_for_group(
            group,
            all_orders,
            markets,
            existing_fills,
            order_map,
            mm_order_ids,
            next_arb_id,
        ) {
            results.push(result);
        }
    }

    results
}

/// Simplex price search for a single market group.
///
/// Binary-searches on Q (extra supply per market from group minting) to find
/// Q* where Σ p_m(Q*) ≈ $1. At Q*, re-clears all markets using LocalSolver
/// with Q* extra supply. Compares welfare with existing fills.
///
/// Returns `Some(SimplexResult)` only if the simplex fills have strictly
/// higher welfare than the existing fills on the group's markets.
fn simplex_search_for_group(
    group: &MarketGroup,
    all_orders: &[Order],
    markets: &MarketSet,
    existing_fills: &[Fill],
    order_map: &HashMap<u64, &Order>,
    mm_order_ids: &HashSet<u64>,
    next_arb_id: &mut u64,
) -> Option<SimplexResult> {
    use crate::fill_extraction::compute_position_delta;
    use crate::local_solver::{LocalSolver, PrecomputedMarket};

    if group.markets.len() < 2 {
        return None;
    }

    let group_market_set: HashSet<MarketId> = group.markets.iter().copied().collect();

    // Collect non-MM single-market orders on group markets
    let non_mm_orders: Vec<Order> = all_orders
        .iter()
        .filter(|o| !mm_order_ids.contains(&o.id))
        .filter(|o| o.num_markets == 1 && group_market_set.contains(&o.markets[0]))
        .cloned()
        .collect();

    // Build PrecomputedMarket per market for fast binary search
    let mut precomputed: Vec<(MarketId, PrecomputedMarket)> = Vec::new();
    for &market in &group.markets {
        let market_orders: Vec<Order> = non_mm_orders
            .iter()
            .filter(|o| o.markets[0] == market)
            .cloned()
            .collect();
        if market_orders.is_empty() {
            return None; // Can't optimize market with no orders
        }
        precomputed.push((market, PrecomputedMarket::from_orders(&market_orders)));
    }

    // Binary search upper bound: min total demand across markets
    let q_max = precomputed
        .iter()
        .map(|(_, pm)| pm.total_demand())
        .min()
        .unwrap_or(0);

    if q_max == 0 {
        return None;
    }

    // Check Q=0: natural clearing prices without group minting
    let sum_p_0: u64 = precomputed
        .iter()
        .map(|(_, pm)| pm.crossing_with_extras(0, 0).0)
        .sum();

    // If Σp(0) < $1, adding supply only makes it worse — can't reach simplex
    if sum_p_0 < NANOS_PER_DOLLAR {
        return None;
    }

    // Binary search for max Q where Σp(Q) ≥ $1
    let mut lo = 0u64;
    let mut hi = q_max;

    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        let sum_p: u64 = precomputed
            .iter()
            .map(|(_, pm)| pm.crossing_with_extras(0, mid).0)
            .sum();

        if sum_p >= NANOS_PER_DOLLAR {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    let q_star = lo;
    if q_star == 0 {
        return None; // No group minting beneficial
    }

    // Generate fills at Q* using LocalSolver
    let solver = LocalSolver::new();
    let mut all_fills = Vec::new();
    let mut arb_orders = Vec::new();
    let mut arb_fills = Vec::new();
    let mut clearing_prices = HashMap::new();
    let mut total_welfare: i64 = 0;

    // Build order map for non-MM orders (for position delta computation)
    let non_mm_order_map: HashMap<u64, &Order> =
        non_mm_orders.iter().map(|o| (o.id, o)).collect();

    for &market in &group.markets {
        let solution = solver.solve_market_with_extra_demand(
            market,
            markets,
            &non_mm_orders,
            0,      // no extra demand
            q_star, // Q* extra supply from group minting
        )?;

        let yes_price = solution.prices[0];
        clearing_prices.insert(market, yes_price);
        total_welfare += solution.welfare;

        for fill in &solution.fills {
            all_fills.push(fill.clone());
        }

        // Compute position imbalance → arb order quantity
        let delta = compute_position_delta(&solution.fills, &non_mm_order_map);
        let imbalance = delta.get(&market).copied().unwrap_or(0);

        if imbalance > 0 {
            // Excess YES demand → arb sells YES (from group minting supply)
            let arb_qty = imbalance as u64;
            let arb_id = *next_arb_id;
            *next_arb_id += 1;

            // Arb limit = proportional minting cost: limit_m = $1 × p_m / Σp
            // At Σp ≈ $1, this ≈ p_m, so arb welfare ≈ 0 (zero-cost minting)
            let price_sum: u64 = precomputed
                .iter()
                .map(|(_, pm)| pm.crossing_with_extras(0, q_star).0)
                .sum();
            let arb_limit = if price_sum > 0 {
                ((NANOS_PER_DOLLAR as u128 * yes_price as u128) / price_sum as u128) as Nanos
            } else {
                yes_price
            };

            let mut arb_order = Order::new(arb_id);
            arb_order.markets[0] = market;
            arb_order.num_markets = 1;
            arb_order.num_states = 2;
            arb_order.payoffs[0] = -1; // Sell YES
            arb_order.payoffs[1] = 0;
            arb_order.limit_price = arb_limit;
            arb_order.max_fill = arb_qty;

            let arb_welfare = arb_order.welfare_contribution(yes_price, arb_qty);
            total_welfare += arb_welfare;

            arb_fills.push(Fill {
                order_id: arb_id,
                fill_price: yes_price,
                fill_qty: arb_qty,
            });
            arb_orders.push(arb_order);
        }
        // imbalance < 0 means excess natural supply — no arb needed
        // imbalance = 0 means naturally balanced — no arb needed
    }

    // Compute existing welfare on group markets (ALL single-market fills, incl MM)
    let existing_welfare: i64 = existing_fills
        .iter()
        .filter_map(|f| {
            let order = order_map.get(&f.order_id)?;
            if order.num_markets == 1 && group_market_set.contains(&order.markets[0]) {
                Some(order.welfare_contribution(f.fill_price, f.fill_qty))
            } else {
                None
            }
        })
        .sum();

    // MM fills are NOT replaced by the simplex search, but enforce_ucp will reprice
    // them at the new clearing prices. Compute which MM fills survive and their welfare.
    let mut mm_survival_welfare: i64 = 0;
    for f in existing_fills {
        if !mm_order_ids.contains(&f.order_id) {
            continue;
        }
        let Some(&order) = order_map.get(&f.order_id) else {
            continue;
        };
        if order.num_markets != 1 || !group_market_set.contains(&order.markets[0]) {
            continue;
        }
        let market = order.markets[0];
        if let Some(&new_price) = clearing_prices.get(&market) {
            if order.is_satisfied_at_price(new_price) {
                mm_survival_welfare += order.welfare_contribution(new_price, f.fill_qty);
            }
        }
    }

    // Total new welfare = simplex fills (non-MM) + surviving MM at new prices
    let total_new_welfare = total_welfare + mm_survival_welfare;

    // Only return if strictly better than existing (including MM at current prices)
    if total_new_welfare <= existing_welfare {
        return None;
    }

    // Collect order IDs of existing fills being replaced (non-MM on group markets)
    let replaced_order_ids: Vec<u64> = existing_fills
        .iter()
        .filter(|f| !mm_order_ids.contains(&f.order_id))
        .filter(|f| {
            order_map
                .get(&f.order_id)
                .map(|o| o.num_markets == 1 && group_market_set.contains(&o.markets[0]))
                .unwrap_or(false)
        })
        .map(|f| f.order_id)
        .collect();

    Some(SimplexResult {
        fills: all_fills,
        arb_orders,
        arb_fills,
        clearing_prices,
        welfare: total_new_welfare,
        existing_welfare,
        replaced_order_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, MarketSet};

    #[test]
    fn test_basic_group_minting() {
        // 3-candidate election, only buy-YES orders
        // A@40c, B@35c, C@30c. Sum = $1.05, so group minting is profitable.
        let mut markets = MarketSet::new();
        let m_a = markets.add_binary("A");
        let m_b = markets.add_binary("B");
        let m_c = markets.add_binary("C");

        let group = MarketGroup::new("Election")
            .with_market(m_a)
            .with_market(m_b)
            .with_market(m_c);

        let orders = vec![
            simple_yes_buy(&markets, 1, m_a, 400_000_000, 100),
            simple_yes_buy(&markets, 2, m_b, 350_000_000, 100),
            simple_yes_buy(&markets, 3, m_c, 300_000_000, 100),
        ];

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let filled = HashSet::new();
        let iter_filled = HashSet::new();
        let mut next_arb = 1_000_000_000u64;

        let result = find_group_mints(&[group], &order_map, &filled, &iter_filled, &mut next_arb);

        assert_eq!(result.buyer_fills.len(), 3, "should fill all 3 buyers");
        assert_eq!(result.arb_orders.len(), 3, "should create 3 arb sells");
        assert_eq!(result.arb_fills.len(), 3);

        // Minting cost = 100 × $1 = $100 in nanos
        assert_eq!(result.minting_cost, 100 * NANOS_PER_DOLLAR as i64);

        // Clearing prices should be the limit prices (only 1 order per market)
        assert_eq!(result.clearing_prices[&m_a], 400_000_000);
        assert_eq!(result.clearing_prices[&m_b], 350_000_000);
        assert_eq!(result.clearing_prices[&m_c], 300_000_000);
    }

    #[test]
    fn test_no_minting_when_sum_below_one() {
        // A@30c, B@30c, C@30c. Sum = 90c < $1. Not profitable.
        let mut markets = MarketSet::new();
        let m_a = markets.add_binary("A");
        let m_b = markets.add_binary("B");
        let m_c = markets.add_binary("C");

        let group = MarketGroup::new("Election")
            .with_market(m_a)
            .with_market(m_b)
            .with_market(m_c);

        let orders = vec![
            simple_yes_buy(&markets, 1, m_a, 300_000_000, 100),
            simple_yes_buy(&markets, 2, m_b, 300_000_000, 100),
            simple_yes_buy(&markets, 3, m_c, 300_000_000, 100),
        ];

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let filled = HashSet::new();
        let iter_filled = HashSet::new();
        let mut next_arb = 1_000_000_000u64;

        let result = find_group_mints(&[group], &order_map, &filled, &iter_filled, &mut next_arb);

        assert!(result.buyer_fills.is_empty(), "should not mint when sum < $1");
        assert_eq!(result.minting_cost, 0);
    }

    #[test]
    fn test_partial_fill_water_filling() {
        // Multiple orders at different limits. Only the top Q* get filled.
        // A: 50c, 40c. B: 60c, 20c. Sum at Q=1: 50+60=110c ≥ $1 ✓
        // Sum at Q=2: 40+20=60c < $1 ✗. So Q*=1.
        let mut markets = MarketSet::new();
        let m_a = markets.add_binary("A");
        let m_b = markets.add_binary("B");

        let group = MarketGroup::new("Group").with_market(m_a).with_market(m_b);

        let orders = vec![
            simple_yes_buy(&markets, 1, m_a, 500_000_000, 1),
            simple_yes_buy(&markets, 2, m_a, 400_000_000, 1),
            simple_yes_buy(&markets, 3, m_b, 600_000_000, 1),
            simple_yes_buy(&markets, 4, m_b, 200_000_000, 1),
        ];

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let filled = HashSet::new();
        let iter_filled = HashSet::new();
        let mut next_arb = 1_000_000_000u64;

        let result = find_group_mints(&[group], &order_map, &filled, &iter_filled, &mut next_arb);

        // Q* = 1: only the top orders get filled
        assert_eq!(result.buyer_fills.len(), 2); // one per market
        assert_eq!(result.minting_cost, 1 * NANOS_PER_DOLLAR as i64);

        // Clearing price on A = 50c (marginal at Q=1), B = 60c
        assert_eq!(result.clearing_prices[&m_a], 500_000_000);
        assert_eq!(result.clearing_prices[&m_b], 600_000_000);
    }

    #[test]
    fn test_respects_filled_orders() {
        // Order 1 already filled → should be excluded from group minting
        let mut markets = MarketSet::new();
        let m_a = markets.add_binary("A");
        let m_b = markets.add_binary("B");

        let group = MarketGroup::new("Group").with_market(m_a).with_market(m_b);

        let orders = vec![
            simple_yes_buy(&markets, 1, m_a, 600_000_000, 100),
            simple_yes_buy(&markets, 2, m_b, 600_000_000, 100),
        ];

        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let mut filled = HashSet::new();
        filled.insert(1); // Order 1 already filled
        let iter_filled = HashSet::new();
        let mut next_arb = 1_000_000_000u64;

        let result = find_group_mints(&[group], &order_map, &filled, &iter_filled, &mut next_arb);

        // Market A has no unfilled demand → Q* = 0
        assert!(result.buyer_fills.is_empty());
    }

    #[test]
    fn test_no_minting_for_non_group_markets() {
        // Orders on markets not in any group → no group minting
        let mut markets = MarketSet::new();
        let m_a = markets.add_binary("A");

        // No groups
        let orders = vec![simple_yes_buy(&markets, 1, m_a, 600_000_000, 100)];
        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();
        let filled = HashSet::new();
        let iter_filled = HashSet::new();
        let mut next_arb = 1_000_000_000u64;

        let result = find_group_mints(&[], &order_map, &filled, &iter_filled, &mut next_arb);
        assert!(result.buyer_fills.is_empty());
    }
}
