//! Market Maker budget allocation using greedy allocation with actual fills.
//!
//! This module allocates MM budgets across orders by:
//! 1. Computing actual capital cost from fills
//! 2. Sorting orders by welfare/capital ratio
//! 3. Activating greedily until budget exhausted
//!
//! # Architecture
//!
//! ```text
//! Input: per-market prices, MM constraints, order welfare, actual fills
//! Output: which MM orders to activate (fill)
//!
//! Algorithm:
//!   for each MM:
//!     1. Compute actual capital from fills (not max_fill estimates)
//!     2. Sort orders by welfare/capital ratio
//!     3. Greedily activate until budget exhausted
//! ```

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use matching_engine::{MarketId, MmConstraint, MmId, Nanos, Order, Qty};

/// Statistics about the allocation process.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AllocationStats {
    /// Total budget across all MMs
    pub total_budget: Nanos,
    /// Total capital used across all MMs
    pub total_capital_used: Nanos,
    /// Overall utilization (capital_used / budget)
    pub overall_utilization: f64,
    /// Number of MM orders considered
    pub mm_orders_considered: usize,
    /// Number of MM orders activated
    pub mm_orders_activated: usize,
    /// Activation rate (activated / considered)
    pub activation_rate: f64,
    /// Whether MMs interact (share orders)
    pub mms_interact: bool,
    /// Greedy baseline welfare (for sanity check)
    pub greedy_baseline_welfare: i64,
    /// Improvement over greedy baseline
    pub improvement_over_greedy: f64,
}

/// Allocation details for a single MM.
#[derive(Clone, Debug, Serialize)]
pub struct MmAllocation {
    pub mm_id: MmId,
    pub activated_orders: Vec<u64>,
    pub capital_used: Nanos,
    pub budget: Nanos,
    pub utilization: f64,
    pub lambda: f64,
}


/// MM Budget allocator using greedy allocation with actual fills.
///
/// The allocator uses a simple greedy approach:
/// 1. Compute actual capital for each order from fills
/// 2. Sort by welfare/capital ratio
/// 3. Activate orders greedily until budget exhausted
pub struct MmAllocator;

impl MmAllocator {
    /// Create a new allocator.
    pub fn new() -> Self {
        Self
    }

    /// Allocate MM budgets across orders.
    ///
    /// # Arguments
    /// * `mm_constraints` - MM constraints with budget limits
    /// * `prices` - Clearing prices per outcome per market
    /// * `orders` - All orders in the problem
    /// * `fills` - Actual fills from price discovery (order_id -> (price, qty))
    ///
    /// # Returns
    /// Allocation result with activated order IDs
    pub fn allocate(
        &self,
        mm_constraints: &[MmConstraint],
        _prices: &HashMap<MarketId, Vec<Nanos>>,
        orders: &[Order],
        fills: &HashMap<u64, (Nanos, Qty)>,
    ) -> AllocationResult {
        if mm_constraints.is_empty() {
            // No MM constraints, activate all orders
            return AllocationResult {
                activated_orders: orders.iter().map(|o| o.id).collect(),
                mm_allocations: Vec::new(),
                total_welfare: fills.iter().map(|(&id, &(price, qty))| {
                    orders.iter().find(|o| o.id == id)
                        .map(|o| o.welfare_contribution(price, qty))
                        .unwrap_or(0)
                }).sum(),
                iterations: 0,
                stats: AllocationStats::default(),
            };
        }

        // Build order lookup
        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();

        // Compute welfare from actual fills
        let welfare: HashMap<u64, i64> = fills.iter().map(|(&id, &(price, qty))| {
            let w = order_map.get(&id)
                .map(|o| o.welfare_contribution(price, qty))
                .unwrap_or(0);
            (id, w)
        }).collect();

        // Check if MMs interact (share orders)
        let interacting = self.mms_interact(mm_constraints);

        // Compute greedy baseline for sanity check
        let greedy_baseline =
            self.compute_greedy_baseline_with_fills(mm_constraints, &order_map, fills, &welfare);

        let mut result = if interacting {
            self.allocate_fixed_point_with_fills(mm_constraints, &order_map, fills, &welfare)
        } else {
            self.allocate_independent_with_fills(mm_constraints, &order_map, fills, &welfare)
        };

        // Compute stats
        result.stats = self.compute_stats(&result, mm_constraints, greedy_baseline, interacting);

        result
    }

    /// Compute greedy baseline using actual fills.
    fn compute_greedy_baseline_with_fills(
        &self,
        mm_constraints: &[MmConstraint],
        _order_map: &HashMap<u64, &Order>,
        fills: &HashMap<u64, (Nanos, Qty)>,
        welfare: &HashMap<u64, i64>,
    ) -> i64 {
        let mut total_greedy_welfare: i64 = 0;

        for mm in mm_constraints {
            // Collect orders with their actual fill welfare and capital cost
            let mut order_info: Vec<(u64, i64, Nanos)> = mm
                .order_ids
                .iter()
                .filter_map(|&order_id| {
                    let (price, qty) = fills.get(&order_id).copied()?;
                    if qty == 0 { return None; }
                    let w = welfare.get(&order_id).copied().unwrap_or(0);
                    let side = mm.order_sides.get(&order_id)?;
                    let capital = side.capital_needed(price, qty);
                    Some((order_id, w, capital))
                })
                .collect();

            // Sort by welfare/capital ratio descending (greedy)
            order_info.sort_by(|(_, w1, c1), (_, w2, c2)| {
                let ratio1 = if *c1 > 0 { *w1 as f64 / *c1 as f64 } else { f64::MAX };
                let ratio2 = if *c2 > 0 { *w2 as f64 / *c2 as f64 } else { f64::MAX };
                ratio2.partial_cmp(&ratio1).unwrap_or(std::cmp::Ordering::Equal)
            });

            // Greedily add until budget full
            let mut budget_remaining = mm.max_capital;
            let mut greedy_welfare: i64 = 0;

            for (_, w, capital) in order_info {
                if capital <= budget_remaining {
                    greedy_welfare += w;
                    budget_remaining -= capital;
                }
            }

            total_greedy_welfare += greedy_welfare;
        }

        total_greedy_welfare
    }

    /// Allocate independent MMs using actual fills.
    fn allocate_independent_with_fills(
        &self,
        mm_constraints: &[MmConstraint],
        order_map: &HashMap<u64, &Order>,
        fills: &HashMap<u64, (Nanos, Qty)>,
        welfare: &HashMap<u64, i64>,
    ) -> AllocationResult {
        let mut all_activated = Vec::new();
        let mut allocations = Vec::new();
        let mut total_welfare: i64 = 0;

        for mm in mm_constraints {
            let (activated, capital_used, mm_welfare) =
                self.allocate_single_mm_with_fills(mm, order_map, fills, welfare);

            allocations.push(MmAllocation {
                mm_id: mm.mm_id,
                activated_orders: activated.clone(),
                capital_used,
                budget: mm.max_capital,
                utilization: if mm.max_capital > 0 {
                    capital_used as f64 / mm.max_capital as f64
                } else { 0.0 },
                lambda: 0.0,
            });

            all_activated.extend(activated);
            total_welfare += mm_welfare;
        }

        // Also activate non-MM orders
        for order in order_map.values() {
            let is_mm_order = mm_constraints.iter().any(|mm| mm.order_ids.contains(&order.id));
            if !is_mm_order {
                all_activated.push(order.id);
                total_welfare += welfare.get(&order.id).copied().unwrap_or(0);
            }
        }

        AllocationResult {
            activated_orders: all_activated,
            mm_allocations: allocations,
            total_welfare,
            iterations: 1,
            stats: AllocationStats::default(),
        }
    }

    /// Allocate a single MM's orders using actual fills, greedy by welfare/capital ratio.
    fn allocate_single_mm_with_fills(
        &self,
        mm: &MmConstraint,
        _order_map: &HashMap<u64, &Order>,
        fills: &HashMap<u64, (Nanos, Qty)>,
        welfare: &HashMap<u64, i64>,
    ) -> (Vec<u64>, Nanos, i64) {
        // Collect orders with their actual fill info
        let mut order_info: Vec<(u64, i64, Nanos)> = mm
            .order_ids
            .iter()
            .filter_map(|&order_id| {
                let (price, qty) = fills.get(&order_id).copied()?;
                if qty == 0 { return None; }
                let w = welfare.get(&order_id).copied().unwrap_or(0);
                let side = mm.order_sides.get(&order_id)?;
                let capital = side.capital_needed(price, qty);
                Some((order_id, w, capital))
            })
            .collect();

        // Sort by welfare/capital ratio descending
        order_info.sort_by(|(_, w1, c1), (_, w2, c2)| {
            let ratio1 = if *c1 > 0 { *w1 as f64 / *c1 as f64 } else { f64::MAX };
            let ratio2 = if *c2 > 0 { *w2 as f64 / *c2 as f64 } else { f64::MAX };
            ratio2.partial_cmp(&ratio1).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Greedily activate until budget full
        let mut budget_remaining = mm.max_capital;
        let mut activated = Vec::new();
        let mut capital_used: Nanos = 0;
        let mut mm_welfare: i64 = 0;

        for (order_id, w, capital) in order_info {
            if capital <= budget_remaining {
                activated.push(order_id);
                capital_used += capital;
                mm_welfare += w;
                budget_remaining -= capital;
            }
        }

        (activated, capital_used, mm_welfare)
    }

    /// Fixed-point allocation for interacting MMs using actual fills.
    fn allocate_fixed_point_with_fills(
        &self,
        mm_constraints: &[MmConstraint],
        order_map: &HashMap<u64, &Order>,
        fills: &HashMap<u64, (Nanos, Qty)>,
        welfare: &HashMap<u64, i64>,
    ) -> AllocationResult {
        // For now, use independent allocation (can be improved later)
        self.allocate_independent_with_fills(mm_constraints, order_map, fills, welfare)
    }

    /// Compute allocation statistics.
    fn compute_stats(
        &self,
        result: &AllocationResult,
        mm_constraints: &[MmConstraint],
        greedy_baseline: i64,
        mms_interact: bool,
    ) -> AllocationStats {
        let total_budget: Nanos = result.mm_allocations.iter().map(|a| a.budget).sum();
        let total_capital_used: Nanos = result.mm_allocations.iter().map(|a| a.capital_used).sum();
        let mm_orders_considered: usize = mm_constraints.iter().map(|mm| mm.order_ids.len()).sum();
        let mm_orders_activated: usize = result
            .mm_allocations
            .iter()
            .map(|a| a.activated_orders.len())
            .sum();

        let overall_utilization = if total_budget > 0 {
            total_capital_used as f64 / total_budget as f64
        } else {
            0.0
        };

        let activation_rate = if mm_orders_considered > 0 {
            mm_orders_activated as f64 / mm_orders_considered as f64
        } else {
            0.0
        };

        let improvement_over_greedy = if greedy_baseline > 0 {
            (result.total_welfare as f64 - greedy_baseline as f64) / greedy_baseline as f64
        } else if result.total_welfare > 0 {
            1.0 // Infinite improvement (greedy got 0)
        } else {
            0.0
        };

        AllocationStats {
            total_budget,
            total_capital_used,
            overall_utilization,
            mm_orders_considered,
            mm_orders_activated,
            activation_rate,
            mms_interact,
            greedy_baseline_welfare: greedy_baseline,
            improvement_over_greedy,
        }
    }

    /// Check if any MMs share orders.
    fn mms_interact(&self, mm_constraints: &[MmConstraint]) -> bool {
        let mut seen_orders: HashSet<u64> = HashSet::new();

        for mm in mm_constraints {
            for &order_id in &mm.order_ids {
                if seen_orders.contains(&order_id) {
                    return true;
                }
                seen_orders.insert(order_id);
            }
        }
        false
    }
}

impl Default for MmAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// OrderAllocator Trait Implementation
// ============================================================================

use crate::traits::{AllocationResult, OrderAllocator};

impl OrderAllocator for MmAllocator {
    fn allocate(
        &self,
        constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        orders: &[Order],
        fills: &HashMap<u64, (Nanos, Qty)>,
    ) -> AllocationResult {
        MmAllocator::allocate(self, constraints, prices, orders, fills)
    }

    fn name(&self) -> &str {
        "MmAllocator"
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{simple_yes_buy, MmSide, Problem};
    use proptest::prelude::*;

    /// Creates a test problem with GENEROUS budget for happy-path testing.
    ///
    /// - 5 orders, each 100 shares
    /// - MM sells YES at 50 cents → capital cost = $0.50 × 100 = $50 per order
    /// - Total capital needed for all 5 orders = $250
    /// - Budget = $300 (intentionally sufficient to cover all orders)
    ///
    /// Use this for tests that verify orders ARE activated when budget allows.
    /// For budget constraint testing, use a tighter budget (see test_allocator_budget_constraint).
    fn create_mm_test_problem() -> (Problem, MmConstraint) {
        let mut problem = Problem::new("mm_test");
        let market = problem.markets.add_binary("test_market");

        // Add 5 orders, each 100 shares
        // Capital cost per order (SellYes at 50 cents): (1 - 0.50) × 100 = $50
        for i in 1..=5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                (500 + i * 10) as u64 * 1_000_000, // limit prices: $0.51, $0.52, etc.
                100,                               // 100 shares each
            ));
        }

        // Budget = $300, Total needed = 5 × $50 = $250
        // Budget is intentionally generous for happy-path testing
        let mm = MmConstraint::new(MmId(1), 300_000_000_000) // $300
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes)
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes)
            .with_order(5, MmSide::SellYes);

        (problem, mm)
    }

    #[test]
    fn test_allocator_no_constraints() {
        let problem = Problem::new("empty");
        let allocator = MmAllocator::new();

        // Provide fills (price, qty) instead of welfare
        let mut fills = HashMap::new();
        fills.insert(1, (500_000_000u64, 100u64));
        fills.insert(2, (500_000_000u64, 100u64));

        let result = allocator.allocate(&[], &HashMap::new(), &problem.orders, &fills);

        assert!(result.mm_allocations.is_empty());
        assert_eq!(result.iterations, 0);
    }

    #[test]
    fn test_allocator_within_budget() {
        let (problem, mm) = create_mm_test_problem();
        let allocator = MmAllocator::new();

        let mut prices = HashMap::new();
        let market_id = problem.markets.iter().next().unwrap().id;
        prices.insert(market_id, vec![500_000_000, 500_000_000]);

        // Provide fills (price, qty) - 50 cents price, 100 shares each
        let mut fills = HashMap::new();
        for i in 1..=5 {
            fills.insert(i, (500_000_000u64, 100u64));
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &fills);

        // Should activate orders
        assert!(!result.activated_orders.is_empty());
        assert_eq!(result.mm_allocations.len(), 1);
    }

    #[test]
    fn test_allocator_budget_constraint() {
        let mut problem = Problem::new("tight_budget");
        let market = problem.markets.add_binary("m");

        // Add liquidity

        // Add 5 orders, each costing $50 to fill (selling YES at 50 cents, 100 shares)
        for i in 1..=5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000, // $0.60 limit price
                100,         // 100 shares each
            ));
        }

        // Budget of $100 - each order selling YES at 50 cents costs $0.50 * 100 = $50 per order
        // With $100 budget, can afford at most 2 orders
        let mm = MmConstraint::new(MmId(1), 100_000_000_000) // $100 budget
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes)
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes)
            .with_order(5, MmSide::SellYes);

        let allocator = MmAllocator::new();

        let mut prices = HashMap::new();
        prices.insert(market, vec![500_000_000, 500_000_000]); // 50 cents each

        // Provide fills (price, qty) - 50 cents price, 100 shares each
        let mut fills = HashMap::new();
        for i in 1..=5 {
            fills.insert(i, (500_000_000u64, 100u64));
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &fills);

        // Verify allocation exists
        assert!(!result.mm_allocations.is_empty());
        let mm_alloc = &result.mm_allocations[0];

        // CRITICAL: Budget constraint must be respected
        assert!(
            mm_alloc.capital_used <= mm_alloc.budget,
            "Budget constraint violated: capital_used={} > budget={}",
            mm_alloc.capital_used,
            mm_alloc.budget
        );

        // With $100 budget and $50 per order, at most 2 orders can be activated
        assert!(
            mm_alloc.activated_orders.len() <= 2,
            "Expected at most 2 orders activated (budget=$100, cost=$50/order), got {}",
            mm_alloc.activated_orders.len()
        );

        // Utilization should be reasonable (not exceeding 100%)
        assert!(
            mm_alloc.utilization <= 1.0,
            "Utilization {} exceeds 100%",
            mm_alloc.utilization
        );
    }

    // Property-based tests using proptest
    proptest! {
        /// Property: Budget constraint must ALWAYS be respected
        #[test]
        fn prop_budget_constraint_always_respected(
            num_orders in 1..10usize,
            budget_dollars in 10..1000u64,
            qty_per_order in 10..500u64,
        ) {
            let mut problem = Problem::new("proptest");
            let market = problem.markets.add_binary("m");

            // Add orders
            for i in 1..=num_orders {
                problem.orders.push(simple_yes_buy(
                    &problem.markets,
                    i as u64,
                    market,
                    600_000_000,
                    qty_per_order,
                ));
            }

            // Create MM constraint
            let budget_nanos = budget_dollars as Nanos * 1_000_000_000;
            let mut mm = MmConstraint::new(MmId(1), budget_nanos);
            for i in 1..=num_orders {
                mm.add_order(i as u64, MmSide::SellYes);
            }

            let allocator = MmAllocator::new();

            let mut prices = HashMap::new();
            prices.insert(market, vec![500_000_000, 500_000_000]);

            // Provide fills (price, qty)
            let mut fills = HashMap::new();
            for i in 1..=num_orders {
                fills.insert(i as u64, (500_000_000u64, qty_per_order));
            }

            let result = allocator.allocate(&[mm], &prices, &problem.orders, &fills);

            // THE KEY PROPERTY: budget must never be exceeded
            for alloc in &result.mm_allocations {
                prop_assert!(
                    alloc.capital_used <= alloc.budget,
                    "Budget violated: used {} > budget {}",
                    alloc.capital_used,
                    alloc.budget
                );
            }
        }

        /// Property: More budget should not decrease activated orders
        #[test]
        fn prop_more_budget_more_or_equal_orders(
            num_orders in 2..8usize,
            base_budget in 50..200u64,
            qty_per_order in 50..200u64,
        ) {
            let mut problem = Problem::new("proptest_monotonic");
            let market = problem.markets.add_binary("m");

            for i in 1..=num_orders {
                problem.orders.push(simple_yes_buy(
                    &problem.markets,
                    i as u64,
                    market,
                    600_000_000,
                    qty_per_order,
                ));
            }

            // Create two MM constraints: one with base budget, one with 2x budget
            let small_budget = base_budget as Nanos * 1_000_000_000;
            let large_budget = small_budget * 2;

            let mut mm_small = MmConstraint::new(MmId(1), small_budget);
            let mut mm_large = MmConstraint::new(MmId(2), large_budget);
            for i in 1..=num_orders {
                mm_small.add_order(i as u64, MmSide::SellYes);
                mm_large.add_order(i as u64, MmSide::SellYes);
            }

            let allocator = MmAllocator::new();

            let mut prices = HashMap::new();
            prices.insert(market, vec![500_000_000, 500_000_000]);

            // Provide fills (price, qty)
            let mut fills = HashMap::new();
            for i in 1..=num_orders {
                fills.insert(i as u64, (500_000_000u64, qty_per_order));
            }

            let result_small = allocator.allocate(&[mm_small], &prices, &problem.orders, &fills);
            let result_large = allocator.allocate(&[mm_large], &prices, &problem.orders, &fills);

            // More budget should give at least as many activated orders
            let orders_small = result_small.mm_allocations.iter()
                .map(|a| a.activated_orders.len())
                .sum::<usize>();
            let orders_large = result_large.mm_allocations.iter()
                .map(|a| a.activated_orders.len())
                .sum::<usize>();

            prop_assert!(
                orders_large >= orders_small,
                "Monotonicity violated: larger budget ({}) activated {} orders vs smaller budget ({}) with {} orders",
                large_budget, orders_large, small_budget, orders_small
            );
        }
    }

    #[test]
    fn test_stats_reporting() {
        let mut problem = Problem::new("stats_test");
        let market = problem.markets.add_binary("m");


        // Add 10 orders - use large quantity to ensure capital cost is significant
        for i in 1..=10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000, // $0.60 limit price
                1000,        // 1000 shares each
            ));
        }

        // Each order costs $500 capital (1000 shares × $0.50 cost per share for SellYes)
        // Budget of $2000 allows ~4 orders
        let mm = MmConstraint::new(MmId(1), 2_000_000_000_000) // $2000
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes)
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes)
            .with_order(5, MmSide::SellYes)
            .with_order(6, MmSide::SellYes)
            .with_order(7, MmSide::SellYes)
            .with_order(8, MmSide::SellYes)
            .with_order(9, MmSide::SellYes)
            .with_order(10, MmSide::SellYes);

        let allocator = MmAllocator::new();

        let mut prices = HashMap::new();
        prices.insert(market, vec![500_000_000, 500_000_000]); // 50 cents

        // Provide fills (price, qty)
        let mut fills = HashMap::new();
        for i in 1..=10 {
            fills.insert(i, (500_000_000u64, 1000u64));
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &fills);

        // Check stats are populated
        let stats = &result.stats;

        println!("Stats: {:?}", stats);
        println!("MM allocations: {:?}", result.mm_allocations);

        assert_eq!(
            stats.total_budget, 2_000_000_000_000,
            "Total budget should be $2000"
        );
        assert_eq!(
            stats.mm_orders_considered, 10,
            "Should consider all 10 orders"
        );
        assert!(!stats.mms_interact, "Single MM should not interact");

        // With high welfare, orders should be activated if within budget
        if stats.mm_orders_activated > 0 {
            assert!(
                stats.total_capital_used > 0,
                "Should use capital if orders activated"
            );
            assert!(
                stats.total_capital_used <= stats.total_budget,
                "Capital used should not exceed budget"
            );
            assert!(
                stats.activation_rate > 0.0 && stats.activation_rate <= 1.0,
                "Activation rate should be in [0, 1]"
            );
            assert!(
                stats.overall_utilization > 0.0,
                "Utilization should be positive if orders activated"
            );
        }
    }

    #[test]
    fn test_overlapping_mms_fixed_point() {
        let mut problem = Problem::new("overlapping_mms");
        let market = problem.markets.add_binary("m");


        // Add 6 orders
        for i in 1..=6 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000,
                100,
            ));
        }

        // MM1 owns orders 1, 2, 3, 4 - share orders 3, 4 with MM2
        let mm1 = MmConstraint::new(MmId(1), 150_000_000_000) // $150
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes)
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes);

        // MM2 owns orders 3, 4, 5, 6 - share orders 3, 4 with MM1
        let mm2 = MmConstraint::new(MmId(2), 150_000_000_000) // $150
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes)
            .with_order(5, MmSide::SellYes)
            .with_order(6, MmSide::SellYes);

        let allocator = MmAllocator::new();

        let mut prices = HashMap::new();
        prices.insert(market, vec![500_000_000, 500_000_000]);

        // Provide fills (price, qty)
        let mut fills = HashMap::new();
        for i in 1..=6 {
            fills.insert(i, (500_000_000u64, 100u64));
        }

        let result = allocator.allocate(&[mm1, mm2], &prices, &problem.orders, &fills);

        // Should detect interaction
        assert!(
            result.stats.mms_interact,
            "Should detect MMs share orders 3 and 4"
        );

        // Both MMs should be within budget
        for alloc in &result.mm_allocations {
            assert!(
                alloc.capital_used <= alloc.budget,
                "MM {:?} budget violated: {} > {}",
                alloc.mm_id,
                alloc.capital_used,
                alloc.budget
            );
        }

        println!("Overlapping MMs result:");
        println!("  Iterations: {}", result.iterations);
        println!(
            "  MM1: activated {:?}, capital used {}",
            result.mm_allocations[0].activated_orders.len(),
            result.mm_allocations[0].capital_used
        );
        println!(
            "  MM2: activated {:?}, capital used {}",
            result.mm_allocations[1].activated_orders.len(),
            result.mm_allocations[1].capital_used
        );
    }

    #[test]
    fn test_budget_constraint_with_varied_welfare() {
        // Test that allocator respects budget with varying welfare per fill
        let mut problem = Problem::new("varied_welfare");
        let market = problem.markets.add_binary("m");


        // Add orders with varying limit prices (affects welfare)
        for i in 1..=10 {
            let limit_price = (500 + i * 10) as u64 * 1_000_000; // $0.51, $0.52, etc.
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                limit_price,
                100,
            ));
        }

        let mm = MmConstraint::new(MmId(1), 200_000_000_000) // $200 (can afford ~4 orders)
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes)
            .with_order(3, MmSide::SellYes)
            .with_order(4, MmSide::SellYes)
            .with_order(5, MmSide::SellYes)
            .with_order(6, MmSide::SellYes)
            .with_order(7, MmSide::SellYes)
            .with_order(8, MmSide::SellYes)
            .with_order(9, MmSide::SellYes)
            .with_order(10, MmSide::SellYes);

        let allocator = MmAllocator::new();

        let mut prices = HashMap::new();
        prices.insert(market, vec![500_000_000, 500_000_000]);

        // Provide fills (price, qty) - all at 50 cents
        let mut fills = HashMap::new();
        for i in 1..=10 {
            fills.insert(i, (500_000_000u64, 100u64));
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &fills);

        // Check greedy baseline is computed
        println!("Budget constraint with varied welfare:");
        println!("  Greedy baseline welfare: {}", result.stats.greedy_baseline_welfare);
        println!("  Total welfare: {}", result.total_welfare);
        println!(
            "  Improvement: {:.2}%",
            result.stats.improvement_over_greedy * 100.0
        );

        // Most important: budget is respected
        for alloc in &result.mm_allocations {
            assert!(
                alloc.capital_used <= alloc.budget,
                "Budget violated: {} > {}",
                alloc.capital_used,
                alloc.budget
            );
        }
    }
}
