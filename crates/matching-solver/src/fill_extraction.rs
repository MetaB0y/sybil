//! Shared fill extraction infrastructure.
//!
//! Converts clearing prices into valid fills with position balance and MM budget enforcement.
//! Used by SmoothedSolver and potentially other solvers that produce prices first.

use std::collections::HashMap;

use matching_engine::{Fill, MarketId, MmSide, Nanos, Order, Problem, Qty};

/// Tracks remaining MM budget across markets during fill extraction.
///
/// Encapsulates the per-constraint remaining budget and per-order MM metadata,
/// replacing the ad-hoc closure pattern in individual solvers.
#[derive(Clone)]
pub struct MmBudgetTracker {
    remaining: Vec<u128>,
    order_map: HashMap<u64, (usize, MmSide)>,
}

impl MmBudgetTracker {
    /// Build tracker from problem's MM constraints.
    pub fn new(problem: &Problem) -> Self {
        let remaining: Vec<u128> = problem
            .mm_constraints
            .iter()
            .map(|mm| mm.max_capital as u128)
            .collect();

        let mut order_map = HashMap::new();
        for (idx, mm) in problem.mm_constraints.iter().enumerate() {
            for &oid in &mm.order_ids {
                if let Some(&side) = mm.order_sides.get(&oid) {
                    order_map.insert(oid, (idx, side));
                }
            }
        }

        Self {
            remaining,
            order_map,
        }
    }

    /// Returns the MM info for an order, if it's an MM order.
    pub fn mm_info(&self, order_id: u64) -> Option<(usize, MmSide)> {
        self.order_map.get(&order_id).copied()
    }

    /// Cap quantity by remaining budget. Returns the affordable quantity.
    pub fn cap_qty(&self, order_id: u64, qty: Qty, price: Nanos) -> Qty {
        let Some(&(mm_idx, side)) = self.order_map.get(&order_id) else {
            return qty;
        };
        let budget = self.remaining[mm_idx];
        if budget == 0 {
            return 0;
        }
        let cap_per_unit = side.capital_needed(price, 1) as u128;
        if cap_per_unit == 0 {
            return qty;
        }
        let max_affordable = (budget / cap_per_unit) as Qty;
        qty.min(max_affordable)
    }

    /// Deduct capital from budget after fill. Returns actual fill qty
    /// (may be less than requested if budget runs out between cap and fill).
    pub fn fill(&mut self, order_id: u64, qty: Qty, price: Nanos) -> Qty {
        let Some(&(mm_idx, side)) = self.order_map.get(&order_id) else {
            return qty;
        };
        let budget = self.remaining[mm_idx];
        if budget == 0 {
            return 0;
        }
        let cap_per_unit = side.capital_needed(price, 1) as u128;
        let fill_qty = if cap_per_unit > 0 {
            let max_affordable = (budget / cap_per_unit) as Qty;
            qty.min(max_affordable)
        } else {
            qty
        };
        if fill_qty == 0 {
            return 0;
        }
        let capital = side.capital_needed(price, fill_qty) as u128;
        self.remaining[mm_idx] = self.remaining[mm_idx].saturating_sub(capital);
        fill_qty
    }
}

/// Extract position-balanced fills for a single binary market at clearing prices.
///
/// Handles all 4 order types (YES/NO buyers/sellers) in unified YES form.
/// MM budget is enforced inline via the tracker to maintain position balance.
///
/// Returns fills for this market. The fills are position-balanced: total YES demand
/// filled == total YES supply filled.
pub fn fill_binary_market(
    market_id: MarketId,
    orders: &[&Order],
    yes_price: Nanos,
    mut mm_tracker: Option<&mut MmBudgetTracker>,
) -> Vec<Fill> {
    let no_price = matching_engine::NANOS_PER_DOLLAR.saturating_sub(yes_price);

    // Classify orders into the 4 categories
    let mut yes_buyers: Vec<(&Order, Qty)> = Vec::new();
    let mut no_sellers: Vec<(&Order, Qty)> = Vec::new();
    let mut yes_sellers: Vec<(&Order, Qty)> = Vec::new();
    let mut no_buyers: Vec<(&Order, Qty)> = Vec::new();

    for &order in orders {
        if order.num_markets != 1 || order.markets[0] != market_id {
            continue;
        }
        let ns = order.num_states as usize;
        if ns > 0 && order.payoffs[0] > 0 {
            yes_buyers.push((order, order.max_fill));
        }
        if ns > 1 && order.payoffs[1] > 0 {
            no_buyers.push((order, order.max_fill));
        }
        if ns > 0 && order.payoffs[0] < 0 {
            yes_sellers.push((order, order.max_fill));
        }
        if ns > 1 && order.payoffs[1] < 0 {
            no_sellers.push((order, order.max_fill));
        }
    }

    yes_buyers.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));
    no_sellers.sort_by_key(|(o, _)| o.limit_price);
    yes_sellers.sort_by_key(|(o, _)| o.limit_price);
    no_buyers.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));

    // Build willing lists with MM caps (read-only pass)
    let willing_yes_buyers =
        filter_willing(&yes_buyers, yes_price, true, mm_tracker.as_deref());
    let willing_no_sellers =
        filter_willing(&no_sellers, no_price, false, mm_tracker.as_deref());
    let willing_yes_sellers =
        filter_willing(&yes_sellers, yes_price, false, mm_tracker.as_deref());
    let willing_no_buyers =
        filter_willing(&no_buyers, no_price, true, mm_tracker.as_deref());

    let demand: Qty = willing_yes_buyers.iter().map(|(_, q)| q).sum::<Qty>()
        + willing_no_sellers.iter().map(|(_, q)| q).sum::<Qty>();
    let supply: Qty = willing_yes_sellers.iter().map(|(_, q)| q).sum::<Qty>()
        + willing_no_buyers.iter().map(|(_, q)| q).sum::<Qty>();

    let matched = demand.min(supply);
    if matched == 0 {
        return Vec::new();
    }

    let mut all_fills = Vec::new();

    // Fill demand side
    let mut demand_remaining = matched;
    for (order, qty) in &willing_yes_buyers {
        if let Some(fill) =
            make_fill(order, *qty, yes_price, &mut demand_remaining, &mut mm_tracker)
        {
            all_fills.push(fill);
        }
    }
    for (order, qty) in &willing_no_sellers {
        if let Some(fill) =
            make_fill(order, *qty, no_price, &mut demand_remaining, &mut mm_tracker)
        {
            all_fills.push(fill);
        }
    }

    // Supply must match exactly how much demand was filled
    let demand_filled = matched - demand_remaining;
    let mut supply_remaining = demand_filled;

    for (order, qty) in &willing_yes_sellers {
        if let Some(fill) =
            make_fill(order, *qty, yes_price, &mut supply_remaining, &mut mm_tracker)
        {
            all_fills.push(fill);
        }
    }
    for (order, qty) in &willing_no_buyers {
        if let Some(fill) =
            make_fill(order, *qty, no_price, &mut supply_remaining, &mut mm_tracker)
        {
            all_fills.push(fill);
        }
    }

    all_fills
}

/// Filter orders that are willing to trade at the given price, applying MM budget caps.
fn filter_willing<'a>(
    list: &[(&'a Order, Qty)],
    price: Nanos,
    is_buyer: bool,
    tracker: Option<&MmBudgetTracker>,
) -> Vec<(&'a Order, Qty)> {
    let mut willing = Vec::new();
    for &(order, qty) in list {
        let satisfied = if is_buyer {
            order.limit_price >= price
        } else {
            order.limit_price <= price
        };
        if satisfied {
            let capped = if let Some(t) = tracker {
                t.cap_qty(order.id, qty, price)
            } else {
                qty
            };
            if capped > 0 {
                willing.push((order, capped));
            }
        }
    }
    willing
}

/// Create a fill and update MM budget. Returns None if no fill possible.
fn make_fill(
    order: &Order,
    qty: Qty,
    price: Nanos,
    remaining: &mut Qty,
    tracker: &mut Option<&mut MmBudgetTracker>,
) -> Option<Fill> {
    if *remaining == 0 {
        return None;
    }
    let mut fill_qty = qty.min(*remaining);

    // Re-check and cap by MM budget at fill time (multiple MM orders share budget)
    if let Some(ref mut t) = tracker {
        fill_qty = t.fill(order.id, fill_qty, price);
        if fill_qty == 0 {
            return None;
        }
    }

    if fill_qty < order.min_fill {
        return None;
    }
    *remaining -= fill_qty;
    Some(Fill::new(order.id, fill_qty, price))
}

/// Compute per-market net position delta from fills.
///
/// Uses `Order::marginal_payoffs_i64()` for stride decomposition.
/// Positive = net YES bought, Negative = net YES sold.
/// Position-balanced when all values are 0.
pub fn compute_position_delta(
    fills: &[Fill],
    order_map: &HashMap<u64, &Order>,
) -> HashMap<MarketId, i64> {
    let mut net_position: HashMap<MarketId, i64> = HashMap::new();

    for fill in fills {
        if fill.fill_qty == 0 {
            continue;
        }
        let Some(&order) = order_map.get(&fill.order_id) else {
            continue;
        };

        for (market, marginal) in order.marginal_payoffs_i64() {
            *net_position.entry(market).or_insert(0) += marginal * fill.fill_qty as i64;
        }
    }

    net_position
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_no_buy, simple_yes_buy, MmConstraint, MmId};

    fn make_test_problem() -> (Problem, MarketId) {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("m");
        (problem, market)
    }

    #[test]
    fn test_mm_budget_tracker_basic() {
        let (mut problem, market) = make_test_problem();
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100));

        let mut mm = MmConstraint::new(MmId(1), 10_000_000_000);
        mm.add_order(1, MmSide::BuyYes);
        problem.mm_constraints.push(mm);

        let tracker = MmBudgetTracker::new(&problem);
        assert!(tracker.mm_info(1).is_some());
        assert!(tracker.mm_info(999).is_none());

        // At price 600M, capital per unit = 600M. Budget = 10B. Max affordable = 16.
        let capped = tracker.cap_qty(1, 100, 600_000_000);
        assert_eq!(capped, 16);
    }

    #[test]
    fn test_mm_budget_tracker_fill_deducts() {
        let (mut problem, market) = make_test_problem();
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100));

        let mut mm = MmConstraint::new(MmId(1), 10_000_000_000);
        mm.add_order(1, MmSide::BuyYes);
        problem.mm_constraints.push(mm);

        let mut tracker = MmBudgetTracker::new(&problem);
        let filled = tracker.fill(1, 10, 600_000_000);
        assert_eq!(filled, 10);

        // Budget should be reduced: 10B - 10*600M = 4B
        let capped = tracker.cap_qty(1, 100, 600_000_000);
        assert_eq!(capped, 6); // 4B / 600M = 6
    }

    #[test]
    fn test_fill_binary_market_basic() {
        let (mut problem, market) = make_test_problem();

        // YES buyer at 60c
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100));
        // NO buyer at 50c (= YES supply at 50c)
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 500_000_000, 100));

        let orders: Vec<&Order> = problem.orders.iter().collect();
        let fills = fill_binary_market(market, &orders, 500_000_000, None);

        assert!(!fills.is_empty());
        // Should be position balanced: sum of YES marginals * qty == 0
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        let delta = compute_position_delta(&fills, &order_map);
        for (_, net) in &delta {
            assert_eq!(*net, 0, "Position should be balanced");
        }
    }

    #[test]
    fn test_compute_position_delta() {
        let (mut problem, market) = make_test_problem();

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100));
        problem
            .orders
            .push(simple_no_buy(&problem.markets, 2, market, 500_000_000, 50));

        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        let fills = vec![
            Fill::new(1, 100, 500_000_000),
            Fill::new(2, 50, 500_000_000),
        ];

        let delta = compute_position_delta(&fills, &order_map);
        // YES buy 100 + NO buy 50 (= -50 YES) → net = 100 - 50 = 50
        assert_eq!(*delta.get(&market).unwrap(), 50);
    }
}
