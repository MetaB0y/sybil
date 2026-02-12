//! Joint Group Solver — primary price discovery on the Σp=$1 simplex.
//!
//! For each market group, finds prices by binary-searching for the extra supply Q*
//! where the sum of clearing prices equals $1. This jointly optimizes prices, fills,
//! and minting within each group, replacing the post-hoc patching approach.
//!
//! Standalone markets (not in any group) are cleared via LocalSolver as usual.
//!
//! Multi-pass: fill → remove filled orders → rebuild PrecomputedMarkets → re-search.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use matching_engine::{
    Fill, MarketGroup, MarketId, Nanos, Order, Problem, NANOS_PER_DOLLAR,
};

use crate::fill_extraction::{compute_position_delta, fill_binary_market, MmBudgetTracker};
use crate::local_solver::{LocalSolver, PrecomputedMarket};
use crate::pipeline::PipelineResult;
use crate::traits::PriceDiscoveryResult;
use crate::Pipeline;
use crate::MatchingResult;

const MAX_PASSES: usize = 8;
/// Arb order ID offset to avoid collisions.
const ARB_ID_OFFSET: u64 = 2_000_000_000;

pub struct JointGroupSolver;

impl JointGroupSolver {
    pub fn new() -> Self {
        Self
    }

    pub fn solve(&self, problem: &Problem) -> PipelineResult {
        let start = Instant::now();

        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        // Identify which markets belong to groups
        let mut market_to_group: HashMap<MarketId, usize> = HashMap::new();
        for (gi, group) in problem.market_groups.iter().enumerate() {
            for &m in &group.markets {
                market_to_group.insert(m, gi);
            }
        }

        // Collect all market IDs
        let all_markets: Vec<MarketId> = problem.markets.iter().map(|m| m.id).collect();

        // MM order IDs for filtering
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        let max_existing_id = problem.orders.iter().map(|o| o.id).max().unwrap_or(0);
        let mut next_arb_id = max_existing_id + ARB_ID_OFFSET;

        let mut filled_ids: HashSet<u64> = HashSet::new();
        let mut all_fills: Vec<Fill> = Vec::new();
        let mut all_arb_orders: Vec<Order> = Vec::new();
        let mut clearing_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        let mut mm_tracker = MmBudgetTracker::new(problem);

        let solver = LocalSolver::new();

        for _pass in 0..MAX_PASSES {
            let mut pass_fills: Vec<Fill> = Vec::new();

            // Remaining non-MM orders (owned, for LocalSolver and PrecomputedMarket)
            let remaining_non_mm: Vec<Order> = problem
                .orders
                .iter()
                .filter(|o| !filled_ids.contains(&o.id) && !mm_order_ids.contains(&o.id))
                .cloned()
                .collect();

            // All remaining orders (refs, for fill_binary_market)
            let remaining_all: Vec<&Order> = problem
                .orders
                .iter()
                .filter(|o| !filled_ids.contains(&o.id))
                .collect();

            // Phase A: Grouped markets — parametric search on Σp=$1 simplex
            for group in &problem.market_groups {
                if group.markets.len() < 2 {
                    continue;
                }
                let group_fills = self.solve_group(
                    group,
                    &remaining_non_mm,
                    &remaining_all,
                    &solver,
                    &problem.markets,
                    &mm_order_ids,
                    &mut mm_tracker,
                    &mut clearing_prices,
                    &mut all_arb_orders,
                    &mut next_arb_id,
                );
                pass_fills.extend(group_fills);
            }

            // Phase B: Standalone markets — price discovery from all orders,
            // fill extraction with MM budget tracking.
            // Remaining all orders as owned vec for solve_market
            let remaining_all_owned: Vec<Order> = remaining_all.iter().map(|o| (*o).clone()).collect();

            for &market_id in &all_markets {
                if market_to_group.contains_key(&market_id) {
                    continue; // Handled in Phase A
                }

                let has_orders = remaining_all_owned
                    .iter()
                    .any(|o| o.num_markets == 1 && o.markets[0] == market_id);

                if !has_orders {
                    continue;
                }

                // Price discovery uses ALL orders (including MM) to find crossing
                let solution =
                    solver.solve_market(market_id, &problem.markets, &remaining_all_owned);

                if !solution.has_activity {
                    continue;
                }

                let yes_price = solution.prices[0];
                let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);
                clearing_prices.insert(market_id, vec![yes_price, no_price]);

                // Extract fills with MM budget tracking
                let market_orders: Vec<&Order> = remaining_all
                    .iter()
                    .filter(|o| o.num_markets == 1 && o.markets[0] == market_id)
                    .copied()
                    .collect();

                let fills =
                    fill_binary_market(market_id, &market_orders, yes_price, Some(&mut mm_tracker));
                pass_fills.extend(fills);
            }

            if pass_fills.is_empty() {
                break;
            }

            // Accumulate: mark filled, store fills
            for fill in &pass_fills {
                filled_ids.insert(fill.order_id);
            }
            all_fills.extend(pass_fills);
        }

        // Phase C: Bundle fills (multi-market orders)
        let bundle_fills = Self::extract_bundle_fills(
            problem,
            &clearing_prices,
            &filled_ids,
            &order_map,
        );
        for fill in &bundle_fills {
            filled_ids.insert(fill.order_id);
        }
        all_fills.extend(bundle_fills);

        // Build PipelineResult with price discovery for enforce_ucp
        let mut pd = PriceDiscoveryResult::empty();
        for (&market, prices) in &clearing_prices {
            pd.prices.insert(market, prices.clone());
        }

        let mut result = PipelineResult::empty();
        result.price_discovery = Some(pd);

        // Compute result stats
        let mut matching_result = MatchingResult::new();
        let mut combined_order_map = order_map.clone();
        for arb in &all_arb_orders {
            combined_order_map.insert(arb.id, arb);
        }

        for fill in &all_fills {
            if let Some(&order) = combined_order_map.get(&fill.order_id) {
                matching_result.add_fill(fill.clone(), order);
            }
        }
        result.result = matching_result;

        // Enforce UCP
        Pipeline::enforce_ucp(&mut result, &combined_order_map);

        // Store arb orders for witness
        result.group_minting_arb_orders = all_arb_orders;

        // Gate: negative welfare → empty
        if result.result.total_welfare < 0 {
            result.result = MatchingResult::new();
        }

        result.total_time_secs = start.elapsed().as_secs_f64();
        result
    }

    /// Solve a single market group on the Σp=$1 simplex.
    ///
    /// Binary-searches for Q* (extra supply from group minting) where Σp(Q*) ≥ $1.
    /// For non-MM orders, uses LocalSolver with extra supply to get fills.
    /// For MM orders, extracts fills separately with budget tracking.
    /// Creates arb sell-YES orders for position balance when Q* > 0.
    fn solve_group(
        &self,
        group: &MarketGroup,
        non_mm_orders: &[Order],
        all_remaining: &[&Order],
        solver: &LocalSolver,
        markets: &matching_engine::MarketSet,
        mm_order_ids: &HashSet<u64>,
        mm_tracker: &mut MmBudgetTracker,
        clearing_prices: &mut HashMap<MarketId, Vec<Nanos>>,
        arb_orders: &mut Vec<Order>,
        next_arb_id: &mut u64,
    ) -> Vec<Fill> {
        // Build PrecomputedMarket per market from non-MM single-market orders
        let mut precomputed: Vec<(MarketId, PrecomputedMarket)> = Vec::new();
        for &market in &group.markets {
            let market_orders: Vec<Order> = non_mm_orders
                .iter()
                .filter(|o| o.num_markets == 1 && o.markets[0] == market)
                .cloned()
                .collect();

            precomputed.push((market, PrecomputedMarket::from_orders(&market_orders)));
        }

        // Check natural Σp (Q=0)
        let sum_p_0: u64 = precomputed
            .iter()
            .map(|(_, pm)| pm.crossing_with_extras(0, 0).0)
            .sum();

        let q_star = if sum_p_0 <= NANOS_PER_DOLLAR {
            // Σp ≤ $1: use natural prices, no group minting needed
            0u64
        } else {
            // Σp > $1: binary search for max Q where Σp(Q) ≥ $1
            let q_max = precomputed
                .iter()
                .map(|(_, pm)| pm.total_demand())
                .min()
                .unwrap_or(0);

            if q_max == 0 {
                0
            } else {
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
                lo
            }
        };

        // Use LocalSolver with extra supply to get fills for non-MM orders.
        // When Q* > 0, the extra supply represents group minting shares.
        // When Q* = 0, this is just normal clearing.
        let mut group_fills: Vec<Fill> = Vec::new();
        let mut group_prices: HashMap<MarketId, Nanos> = HashMap::new();

        for &market in &group.markets {
            let solution = solver.solve_market_with_extra_demand(
                market,
                markets,
                non_mm_orders,
                0,      // no extra demand
                q_star, // Q* extra supply from group minting
            );

            let Some(solution) = solution else {
                // Non-binary market or other issue — skip
                continue;
            };

            // Only update clearing prices when we have actual market activity.
            // Otherwise the midpoint default (500M) would overwrite correct prices
            // from earlier passes.
            if !solution.has_activity {
                continue;
            }

            let yes_price = solution.prices[0];
            let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);
            group_prices.insert(market, yes_price);
            clearing_prices.insert(market, vec![yes_price, no_price]);

            // Accumulate non-MM fills from LocalSolver
            group_fills.extend(solution.fills);
        }

        // Extract MM fills separately at clearing prices (with budget tracking)
        for &market in &group.markets {
            let Some(&yes_price) = group_prices.get(&market) else {
                continue;
            };

            let mm_orders: Vec<&Order> = all_remaining
                .iter()
                .filter(|o| {
                    mm_order_ids.contains(&o.id)
                        && o.num_markets == 1
                        && o.markets[0] == market
                })
                .copied()
                .collect();

            if mm_orders.is_empty() {
                continue;
            }

            // Use fill_binary_market with only MM orders + enough "virtual supply"
            // by creating temporary arb supply. But simpler: just fill MM orders
            // directly at the clearing price.
            let no_price = NANOS_PER_DOLLAR.saturating_sub(yes_price);
            for &order in &mm_orders {
                let (price, is_willing) = if order.payoffs[0] > 0 {
                    // YES buyer
                    (yes_price, order.limit_price >= yes_price)
                } else if order.payoffs[1] > 0 {
                    // NO buyer
                    (no_price, order.limit_price >= no_price)
                } else if order.payoffs[0] < 0 {
                    // YES seller
                    (yes_price, order.limit_price <= yes_price)
                } else if order.payoffs[1] < 0 {
                    // NO seller
                    (no_price, order.limit_price <= no_price)
                } else {
                    continue;
                };

                if !is_willing {
                    continue;
                }

                let fill_qty = mm_tracker.cap_qty(order.id, order.max_fill, price);
                if fill_qty == 0 || fill_qty < order.min_fill {
                    continue;
                }

                let actual_qty = mm_tracker.fill(order.id, fill_qty, price);
                if actual_qty == 0 || actual_qty < order.min_fill {
                    continue;
                }

                group_fills.push(Fill::new(order.id, actual_qty, price));
            }
        }

        // Water-filling fallback: when parametric search produces Q*=0
        // (no natural sellers → crossing_with_extras returns 0 prices),
        // use water-filling on unfilled buy-YES orders to capture group minting.
        if q_star == 0 {
            let filled_set: HashSet<u64> = group_fills.iter().map(|f| f.order_id).collect();
            let wf_result = crate::group_minting::find_group_mints(
                std::slice::from_ref(group),
                &all_remaining.iter().map(|o| (o.id, *o)).collect(),
                &filled_set,
                &HashSet::new(),
                next_arb_id,
            );

            if !wf_result.buyer_fills.is_empty() {
                // Update clearing prices from water-filling
                for (&market, &price) in &wf_result.clearing_prices {
                    let no_price = NANOS_PER_DOLLAR.saturating_sub(price);
                    group_prices.insert(market, price);
                    clearing_prices.insert(market, vec![price, no_price]);
                }

                // Use buyer fills from water-filling
                group_fills.extend(wf_result.buyer_fills);

                // Create arb orders with proportional limits (not zero-welfare limits).
                // Arb limit = $1 × p_m / Σp → welfare = Q × (Σp - $1).
                let price_sum: u64 = wf_result.clearing_prices.values().sum();
                for wf_arb in &wf_result.arb_orders {
                    let market = wf_arb.markets[0];
                    let cp = wf_result.clearing_prices[&market];
                    let arb_limit = if price_sum > 0 {
                        ((NANOS_PER_DOLLAR as u128 * cp as u128) / price_sum as u128) as Nanos
                    } else {
                        cp
                    };

                    let mut arb_order = wf_arb.clone();
                    arb_order.limit_price = arb_limit;
                    let arb_qty = wf_arb.max_fill;

                    group_fills.push(Fill {
                        order_id: arb_order.id,
                        fill_price: cp,
                        fill_qty: arb_qty,
                    });
                    arb_orders.push(arb_order);
                }
            }
        }

        // Create arb orders from position delta (for position balance)
        // Only needed when parametric search was used (Q* > 0).
        // Water-filling creates its own arb orders.
        if q_star > 0 && !group_fills.is_empty() {
            // Build order map for position delta computation
            let mut fill_order_map: HashMap<u64, &Order> = all_remaining
                .iter()
                .map(|o| (o.id, *o))
                .collect();
            for o in non_mm_orders {
                fill_order_map.entry(o.id).or_insert(o);
            }

            let delta = compute_position_delta(&group_fills, &fill_order_map);
            let price_sum: u64 = group_prices.values().sum();

            for &market in &group.markets {
                let imbalance = delta.get(&market).copied().unwrap_or(0);
                if imbalance <= 0 {
                    continue;
                }

                let arb_qty = imbalance as u64;
                let yes_price = group_prices[&market];
                let arb_id = *next_arb_id;
                *next_arb_id += 1;

                // Arb limit = proportional minting cost: $1 × p_m / Σp
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
                arb_order.min_fill = 0;
                arb_order.max_fill = arb_qty;

                group_fills.push(Fill {
                    order_id: arb_id,
                    fill_price: yes_price,
                    fill_qty: arb_qty,
                });
                arb_orders.push(arb_order);
            }
        }

        group_fills
    }

    /// Phase C: Extract fills for multi-market (bundle) orders.
    ///
    /// Greedy by surplus descending, with position balance tracking to avoid
    /// creating imbalances. Uses independent-market probability model for
    /// expected payoff (sufficient for bundles spanning standalone markets;
    /// slightly approximate for bundles spanning grouped markets).
    fn extract_bundle_fills(
        problem: &Problem,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        filled_ids: &HashSet<u64>,
        _order_map: &HashMap<u64, &Order>,
    ) -> Vec<Fill> {

        // Collect unfilled bundle orders with positive surplus
        let mut candidates: Vec<(&Order, f64, Nanos)> = Vec::new();
        for order in &problem.orders {
            if filled_ids.contains(&order.id) || order.num_markets <= 1 {
                continue;
            }

            let fill_price = Self::order_expected_payoff(order, clearing_prices);
            if !order.is_satisfied_at_price(fill_price) {
                continue;
            }

            let surplus = if order.is_seller() {
                fill_price as f64 - order.limit_price as f64
            } else {
                order.limit_price as f64 - fill_price as f64
            };
            if surplus > 0.0 {
                candidates.push((order, surplus, fill_price));
            }
        }

        // Sort by surplus descending (most valuable bundles first)
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Greedy fill with position balance tracking
        let mut net_position: HashMap<MarketId, i64> = HashMap::new();
        let mut fills = Vec::new();

        for (order, _surplus, fill_price) in &candidates {
            let fill_qty = order.max_fill;
            if fill_qty == 0 || fill_qty < order.min_fill {
                continue;
            }

            let marginals = order.marginal_payoffs_i64();

            // Only fill if it doesn't increase position imbalance
            let mut would_imbalance = false;
            for &(market, marginal) in &marginals {
                let current = *net_position.get(&market).unwrap_or(&0);
                let new_val = current + marginal * fill_qty as i64;
                if new_val.abs() > current.abs() && new_val != 0 {
                    would_imbalance = true;
                    break;
                }
            }

            if would_imbalance {
                continue;
            }

            // Update position tracking
            for &(market, marginal) in &marginals {
                *net_position.entry(market).or_insert(0) += marginal * fill_qty as i64;
            }

            fills.push(Fill::new(order.id, fill_qty, *fill_price));
        }

        fills
    }

    /// Compute risk-neutral expected payoff for a multi-market order.
    ///
    /// Uses independent-market probability model:
    /// P(state s) = Π_m P(outcome_m(s))
    /// Expected payoff = Σ_s payoff_s × P(s) × $1
    fn order_expected_payoff(
        order: &Order,
        prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> Nanos {
        let num_states = order.num_states as usize;
        let num_markets = order.num_markets as usize;
        let npd = NANOS_PER_DOLLAR as f64;

        let mut expected = 0.0;
        for s in 0..num_states {
            let payoff = order.payoffs[s] as f64;
            if payoff == 0.0 {
                continue;
            }

            let mut state_prob = 1.0;
            for m_idx in 0..num_markets {
                let market = order.markets[m_idx];
                let p_yes = prices
                    .get(&market)
                    .map(|p| p[0] as f64 / npd)
                    .unwrap_or(0.5);
                let outcome = (s >> m_idx) % 2;
                if outcome == 0 {
                    state_prob *= p_yes;
                } else {
                    state_prob *= 1.0 - p_yes;
                }
            }

            expected += payoff * state_prob;
        }

        (expected.abs() * npd).round().max(0.0).min(npd) as Nanos
    }
}

impl Default for JointGroupSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{outcome_sell, simple_yes_buy, MmConstraint, MmId, MmSide};

    #[test]
    fn test_standalone_market_clearing() {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("m");

        // YES buyer at 60c, YES seller at 40c → should trade
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, market, 600_000_000, 100));
        problem
            .orders
            .push(outcome_sell(&problem.markets, 2, market, 0, 400_000_000, 100));

        let solver = JointGroupSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.total_welfare > 0, "Should have positive welfare");
        assert!(!result.result.fills.is_empty(), "Should have fills");
    }

    #[test]
    fn test_group_minting_basic() {
        let mut problem = Problem::new("test");
        let m_a = problem.markets.add_binary("A");
        let m_b = problem.markets.add_binary("B");
        let m_c = problem.markets.add_binary("C");

        problem.market_groups.push(
            MarketGroup::new("Election")
                .with_market(m_a)
                .with_market(m_b)
                .with_market(m_c),
        );

        // Buy YES on each market: 40c + 35c + 30c = $1.05 > $1 → group minting profitable
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m_a, 400_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m_b, 350_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m_c, 300_000_000, 100));

        let solver = JointGroupSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.total_welfare > 0, "Group minting should create welfare");
        assert!(!result.result.fills.is_empty(), "Should have fills from group minting");
    }

    #[test]
    fn test_no_minting_when_sum_below_one() {
        let mut problem = Problem::new("test");
        let m_a = problem.markets.add_binary("A");
        let m_b = problem.markets.add_binary("B");

        problem.market_groups.push(
            MarketGroup::new("Group")
                .with_market(m_a)
                .with_market(m_b),
        );

        // 30c + 30c = 60c < $1 → no group minting
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m_a, 300_000_000, 100));
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 2, m_b, 300_000_000, 100));

        // Without sellers, there's no natural crossing → no fills
        let solver = JointGroupSolver::new();
        let result = solver.solve(&problem);

        // No fills expected (Σp < $1 and no natural sellers)
        assert_eq!(result.result.total_welfare, 0);
    }

    #[test]
    fn test_group_with_natural_sellers() {
        let mut problem = Problem::new("test");
        let m_a = problem.markets.add_binary("A");
        let m_b = problem.markets.add_binary("B");

        problem.market_groups.push(
            MarketGroup::new("Group")
                .with_market(m_a)
                .with_market(m_b),
        );

        // Market A: buyer at 60c, seller at 40c → natural trade
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 1, m_a, 600_000_000, 50));
        problem
            .orders
            .push(outcome_sell(&problem.markets, 2, m_a, 0, 400_000_000, 50));
        // Market B: buyer at 50c, seller at 30c → natural trade
        problem
            .orders
            .push(simple_yes_buy(&problem.markets, 3, m_b, 500_000_000, 50));
        problem
            .orders
            .push(outcome_sell(&problem.markets, 4, m_b, 0, 300_000_000, 50));

        let solver = JointGroupSolver::new();
        let result = solver.solve(&problem);

        assert!(result.result.total_welfare > 0);
        assert!(!result.result.fills.is_empty());
    }

    #[test]
    fn test_mm_budget_respected() {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("m");

        // MM buyer at 60c with tight budget
        let mm_order = simple_yes_buy(&problem.markets, 1, market, 600_000_000, 1000);
        problem.orders.push(mm_order);

        // Non-MM seller at 40c
        problem
            .orders
            .push(outcome_sell(&problem.markets, 2, market, 0, 400_000_000, 1000));

        // MM budget = 10 units * 600M = 6B
        let mut mm = MmConstraint::new(MmId(1), 6_000_000_000);
        mm.add_order(1, MmSide::BuyYes);
        problem.mm_constraints.push(mm);

        let solver = JointGroupSolver::new();
        let result = solver.solve(&problem);

        // Should fill — non-MM seller provides supply, MM buyer has budget
        assert!(result.result.total_welfare > 0);
        // MM budget = 6B, clearing price = 400M (supply price).
        // At 400M, max affordable = 6B / 400M = 15. Must be < 1000 (max_fill).
        let mm_fill = result.result.fills.iter().find(|f| f.order_id == 1);
        assert!(mm_fill.is_some(), "MM should have a fill");
        let fill = mm_fill.unwrap();
        assert!(fill.fill_qty < 1000, "MM budget should cap fill below max_fill");
        assert!(fill.fill_qty <= 15, "MM budget should cap fill to 15 at clearing price");
    }
}
