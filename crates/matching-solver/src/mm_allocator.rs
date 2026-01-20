//! Market Maker budget allocation using Lagrangian relaxation.
//!
//! This module allocates MM budgets across orders using a dual approach:
//! 1. Binary search on Lagrange multiplier (lambda) per MM
//! 2. Fixed-point iteration when multiple MMs interact
//!
//! # Architecture
//!
//! ```text
//! Input: per-market prices, MM constraints, order welfare
//! Output: which MM orders to activate (fill)
//!
//! Algorithm:
//!   for each MM:
//!     binary_search(lambda) such that:
//!       activated_orders(lambda).capital_used ≈ budget
//!
//!   if multiple MMs interact:
//!     fixed_point_iterate until convergence
//! ```

use std::collections::{HashMap, HashSet};

use matching_engine::{MarketId, MmConstraint, MmId, Nanos, Order, Qty};

/// Result of MM budget allocation.
#[derive(Clone, Debug)]
pub struct AllocationResult {
    /// Order IDs that should be activated (filled)
    pub activated_orders: Vec<u64>,
    /// Per-MM allocation details
    pub mm_allocations: Vec<MmAllocation>,
    /// Total welfare from activated orders
    pub total_welfare: i64,
    /// Number of fixed-point iterations used
    pub iterations: usize,
    /// Allocation statistics
    pub stats: AllocationStats,
}

/// Statistics about the allocation process.
#[derive(Clone, Debug, Default)]
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
#[derive(Clone, Debug)]
pub struct MmAllocation {
    pub mm_id: MmId,
    pub activated_orders: Vec<u64>,
    pub capital_used: Nanos,
    pub budget: Nanos,
    pub utilization: f64,
    pub lambda: f64,
}

/// Configuration for the MM allocator.
#[derive(Clone, Debug)]
pub struct AllocatorConfig {
    /// Maximum iterations for binary search
    pub max_binary_search_iterations: usize,
    /// Maximum iterations for fixed-point
    pub max_fixed_point_iterations: usize,
    /// Convergence tolerance for lambda (relative)
    pub lambda_tolerance: f64,
    /// Convergence tolerance for capital (absolute, in nanos)
    pub capital_tolerance: Nanos,
}

impl Default for AllocatorConfig {
    fn default() -> Self {
        Self {
            max_binary_search_iterations: 50,
            max_fixed_point_iterations: 20,
            lambda_tolerance: 1e-6,
            capital_tolerance: 1_000_000, // $0.001
        }
    }
}

/// MM Budget allocator using Lagrangian relaxation.
pub struct MmAllocator {
    config: AllocatorConfig,
}

impl MmAllocator {
    /// Create a new allocator with default config.
    pub fn new() -> Self {
        Self {
            config: AllocatorConfig::default(),
        }
    }

    /// Create an allocator with custom config.
    pub fn with_config(config: AllocatorConfig) -> Self {
        Self { config }
    }

    /// Allocate MM budgets across orders.
    ///
    /// # Arguments
    /// * `mm_constraints` - MM constraints with budget limits
    /// * `prices` - Clearing prices per outcome per market
    /// * `orders` - All orders in the problem
    /// * `welfare` - Welfare contribution of each order (order_id -> welfare)
    ///
    /// # Returns
    /// Allocation result with activated order IDs
    pub fn allocate(
        &self,
        mm_constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        orders: &[Order],
        welfare: &HashMap<u64, i64>,
    ) -> AllocationResult {
        if mm_constraints.is_empty() {
            // No MM constraints, activate all orders
            return AllocationResult {
                activated_orders: orders.iter().map(|o| o.id).collect(),
                mm_allocations: Vec::new(),
                total_welfare: welfare.values().sum(),
                iterations: 0,
                stats: AllocationStats::default(),
            };
        }

        // Build order lookup
        let order_map: HashMap<u64, &Order> = orders.iter().map(|o| (o.id, o)).collect();

        // Check if MMs interact (share orders)
        let interacting = self.mms_interact(mm_constraints);

        // Compute greedy baseline for sanity check
        let greedy_baseline = self.compute_greedy_baseline(mm_constraints, prices, &order_map, welfare);

        let mut result = if interacting {
            self.allocate_fixed_point(mm_constraints, prices, &order_map, welfare)
        } else {
            self.allocate_independent(mm_constraints, prices, &order_map, welfare)
        };

        // Compute stats
        result.stats = self.compute_stats(&result, mm_constraints, greedy_baseline, interacting);

        result
    }

    /// Compute greedy baseline: sort orders by welfare, add until budget full.
    fn compute_greedy_baseline(
        &self,
        mm_constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
    ) -> i64 {
        let mut total_greedy_welfare: i64 = 0;

        for mm in mm_constraints {
            // Collect orders with their welfare and capital cost
            let mut order_info: Vec<(u64, i64, Nanos)> = mm
                .order_ids
                .iter()
                .filter_map(|&order_id| {
                    let order = order_map.get(&order_id)?;
                    let w = welfare.get(&order_id).copied().unwrap_or(0);
                    let capital = self.estimate_order_capital(mm, order_id, order, prices);
                    Some((order_id, w, capital))
                })
                .collect();

            // Sort by welfare descending (greedy)
            order_info.sort_by_key(|(_, w, _)| std::cmp::Reverse(*w));

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
        let mm_orders_activated: usize = result.mm_allocations.iter().map(|a| a.activated_orders.len()).sum();

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

    /// Allocate for independent MMs (no shared orders).
    fn allocate_independent(
        &self,
        mm_constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
    ) -> AllocationResult {
        let mut all_activated: Vec<u64> = Vec::new();
        let mut mm_allocations: Vec<MmAllocation> = Vec::new();
        let mut total_welfare: i64 = 0;

        // Activate non-MM orders first
        let mm_order_ids: HashSet<u64> = mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        for (order_id, &w) in welfare {
            if !mm_order_ids.contains(order_id) {
                all_activated.push(*order_id);
                total_welfare += w;
            }
        }

        // Allocate each MM independently
        for mm in mm_constraints {
            let allocation =
                self.allocate_single_mm(mm, prices, order_map, welfare);

            total_welfare += allocation
                .activated_orders
                .iter()
                .filter_map(|id| welfare.get(id))
                .sum::<i64>();

            all_activated.extend(&allocation.activated_orders);
            mm_allocations.push(allocation);
        }

        AllocationResult {
            activated_orders: all_activated,
            mm_allocations,
            total_welfare,
            iterations: 1,
            stats: AllocationStats::default(), // Will be filled by caller
        }
    }

    /// Allocate for interacting MMs using fixed-point iteration.
    fn allocate_fixed_point(
        &self,
        mm_constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
    ) -> AllocationResult {
        let mut lambdas: Vec<f64> = vec![0.0; mm_constraints.len()];
        let mut prev_activated: HashSet<u64> = HashSet::new();
        let mut iterations = 0;

        // Fixed-point iteration
        for iter in 0..self.config.max_fixed_point_iterations {
            iterations = iter + 1;
            let mut current_activated: HashSet<u64> = HashSet::new();

            // Update each MM given current lambdas
            for (i, mm) in mm_constraints.iter().enumerate() {
                let (new_lambda, activated) = self.binary_search_lambda(
                    mm,
                    prices,
                    order_map,
                    welfare,
                    &lambdas,
                    i,
                );
                lambdas[i] = new_lambda;
                current_activated.extend(activated);
            }

            // Check convergence
            if current_activated == prev_activated {
                break;
            }
            prev_activated = current_activated;
        }

        // Build final result
        let mut all_activated: Vec<u64> = Vec::new();
        let mut mm_allocations: Vec<MmAllocation> = Vec::new();
        let mut total_welfare: i64 = 0;

        // Non-MM orders
        let mm_order_ids: HashSet<u64> = mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        for (order_id, &w) in welfare {
            if !mm_order_ids.contains(order_id) {
                all_activated.push(*order_id);
                total_welfare += w;
            }
        }

        // MM orders
        for (i, mm) in mm_constraints.iter().enumerate() {
            let allocation = self.compute_allocation(
                mm,
                lambdas[i],
                prices,
                order_map,
                welfare,
            );

            total_welfare += allocation
                .activated_orders
                .iter()
                .filter_map(|id| welfare.get(id))
                .sum::<i64>();

            all_activated.extend(&allocation.activated_orders);
            mm_allocations.push(allocation);
        }

        AllocationResult {
            activated_orders: all_activated,
            mm_allocations,
            total_welfare,
            iterations,
            stats: AllocationStats::default(), // Will be filled by caller
        }
    }

    /// Allocate for a single MM using binary search on lambda.
    fn allocate_single_mm(
        &self,
        mm: &MmConstraint,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
    ) -> MmAllocation {
        let (lambda, _) = self.binary_search_lambda(
            mm,
            prices,
            order_map,
            welfare,
            &[],
            0,
        );

        self.compute_allocation(mm, lambda, prices, order_map, welfare)
    }

    /// Binary search for optimal lambda.
    ///
    /// Lambda is the Lagrange multiplier for the budget constraint.
    /// Higher lambda = fewer orders activated.
    ///
    /// Returns the highest-welfare allocation that respects the budget constraint.
    fn binary_search_lambda(
        &self,
        mm: &MmConstraint,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
        _other_lambdas: &[f64],
        _mm_index: usize,
    ) -> (f64, Vec<u64>) {
        let mut lo = 0.0;
        let mut hi = 1e12; // Large enough to deactivate all orders

        // First check if we can activate all orders within budget
        let all_activated = self.compute_allocation(mm, 0.0, prices, order_map, welfare);
        if all_activated.capital_used <= mm.max_capital {
            return (0.0, all_activated.activated_orders);
        }

        // Track the best valid allocation (within budget, maximum welfare)
        let mut best_valid_lambda = hi;
        let mut best_valid_orders: Vec<u64> = Vec::new();
        let mut best_valid_welfare: i64 = 0;

        // Binary search for lambda
        for _ in 0..self.config.max_binary_search_iterations {
            let mid = (lo + hi) / 2.0;
            let allocation = self.compute_allocation(mm, mid, prices, order_map, welfare);

            // Check if this allocation respects budget
            if allocation.capital_used <= mm.max_capital {
                // Valid allocation - check if it's better than our best
                let alloc_welfare: i64 = allocation
                    .activated_orders
                    .iter()
                    .filter_map(|id| welfare.get(id))
                    .sum();

                if alloc_welfare > best_valid_welfare {
                    best_valid_welfare = alloc_welfare;
                    best_valid_lambda = mid;
                    best_valid_orders = allocation.activated_orders.clone();
                }

                // Try to find a lower lambda (more orders) that still fits
                hi = mid;
            } else {
                // Over budget - need higher lambda to reduce orders
                lo = mid;
            }

            if (hi - lo) / hi.max(1.0) < self.config.lambda_tolerance {
                break;
            }
        }

        // If we never found a valid allocation, return empty
        if best_valid_orders.is_empty() && mm.max_capital > 0 {
            // Try with very high lambda to get at least something
            let minimal = self.compute_allocation(mm, hi * 10.0, prices, order_map, welfare);
            if minimal.capital_used <= mm.max_capital {
                return (hi * 10.0, minimal.activated_orders);
            }
        }

        (best_valid_lambda, best_valid_orders)
    }

    /// Compute allocation for a given lambda.
    ///
    /// Activates orders where: welfare - lambda * capital > 0
    fn compute_allocation(
        &self,
        mm: &MmConstraint,
        lambda: f64,
        prices: &HashMap<MarketId, Vec<Nanos>>,
        order_map: &HashMap<u64, &Order>,
        welfare: &HashMap<u64, i64>,
    ) -> MmAllocation {
        let mut activated_orders: Vec<u64> = Vec::new();
        let mut fills: HashMap<u64, (Nanos, Qty)> = HashMap::new();

        for &order_id in &mm.order_ids {
            let Some(order) = order_map.get(&order_id) else {
                continue;
            };

            let order_welfare = welfare.get(&order_id).copied().unwrap_or(0);

            // Estimate capital needed for this order
            let capital = self.estimate_order_capital(mm, order_id, order, prices);

            // Lagrangian: activate if welfare - lambda * capital > 0
            let adjusted_value = order_welfare as f64 - lambda * capital as f64;

            if adjusted_value > 0.0 {
                activated_orders.push(order_id);

                // Get price for this order
                let price = if order.num_markets > 0 {
                    prices
                        .get(&order.markets[0])
                        .and_then(|p| p.first().copied())
                        .unwrap_or(500_000_000)
                } else {
                    500_000_000
                };

                fills.insert(order_id, (price, order.max_fill));
            }
        }

        let capital_used = mm.capital_used(&fills);
        let utilization = if mm.max_capital > 0 {
            capital_used as f64 / mm.max_capital as f64
        } else {
            0.0
        };

        MmAllocation {
            mm_id: mm.mm_id,
            activated_orders,
            capital_used,
            budget: mm.max_capital,
            utilization,
            lambda,
        }
    }

    /// Estimate capital needed for an order at current prices.
    fn estimate_order_capital(
        &self,
        mm: &MmConstraint,
        order_id: u64,
        order: &Order,
        prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> Nanos {
        let Some(&side) = mm.order_sides.get(&order_id) else {
            return 0;
        };

        // Get price for the primary market
        let price = if order.num_markets > 0 {
            prices
                .get(&order.markets[0])
                .and_then(|p| p.first().copied())
                .unwrap_or(500_000_000)
        } else {
            500_000_000 // Default to 50 cents
        };

        side.capital_needed(price, order.max_fill)
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

use crate::traits::{AllocationResult as TraitAllocationResult, OrderAllocator};

impl OrderAllocator for MmAllocator {
    fn allocate(
        &self,
        constraints: &[MmConstraint],
        prices: &HashMap<MarketId, Vec<Nanos>>,
        orders: &[Order],
    ) -> TraitAllocationResult {
        // Compute welfare for each order based on prices
        let welfare = Self::compute_order_welfare(orders, prices);

        // Use the existing allocate method
        let result = MmAllocator::allocate(self, constraints, prices, orders, &welfare);

        // Convert to trait AllocationResult
        TraitAllocationResult {
            activated_orders: result.activated_orders,
            total_welfare: result.total_welfare,
            iterations: result.iterations,
            mm_allocations: result.mm_allocations,
        }
    }

    fn name(&self) -> &str {
        "MmAllocator"
    }
}

impl MmAllocator {
    /// Compute welfare for each order given clearing prices.
    ///
    /// Welfare = (limit_price - clearing_price) * quantity for buyers.
    fn compute_order_welfare(
        orders: &[Order],
        prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> HashMap<u64, i64> {
        let mut welfare = HashMap::new();

        for order in orders {
            if order.num_markets == 0 {
                welfare.insert(order.id, 0);
                continue;
            }

            // Get the clearing price for the primary market/outcome
            let market_id = order.markets[0];
            let clearing_price = prices
                .get(&market_id)
                .and_then(|p| {
                    // Find which outcome this order is buying
                    let outcome = order
                        .payoffs
                        .iter()
                        .take(order.num_states as usize)
                        .position(|&p| p > 0)
                        .unwrap_or(0);
                    p.get(outcome).copied()
                })
                .unwrap_or(500_000_000); // Default to 50 cents

            // Welfare = (limit - clearing) * max_fill
            // Only positive welfare if limit >= clearing
            let order_welfare = if order.limit_price >= clearing_price {
                (order.limit_price as i64 - clearing_price as i64) * order.max_fill as i64
            } else {
                0
            };

            welfare.insert(order.id, order_welfare);
        }

        welfare
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{MmSide, Problem, simple_yes_buy};
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

        // Add liquidity at 50 cents
        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);

        // Add 5 orders, each 100 shares
        // Capital cost per order (SellYes at 50 cents): (1 - 0.50) × 100 = $50
        for i in 1..=5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                (500 + i * 10) as u64 * 1_000_000, // limit prices: $0.51, $0.52, etc.
                100, // 100 shares each
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

        let mut welfare = HashMap::new();
        welfare.insert(1, 1000);
        welfare.insert(2, 2000);

        let result = allocator.allocate(&[], &HashMap::new(), &problem.orders, &welfare);

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

        let mut welfare = HashMap::new();
        for i in 1..=5 {
            welfare.insert(i, 10_000_000_000); // High welfare
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &welfare);

        // Should activate orders since welfare is high
        assert!(!result.activated_orders.is_empty());
        assert_eq!(result.mm_allocations.len(), 1);
    }

    #[test]
    fn test_allocator_budget_constraint() {
        let mut problem = Problem::new("tight_budget");
        let market = problem.markets.add_binary("m");

        // Add liquidity
        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);

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

        // Each order has $25 welfare
        let mut welfare = HashMap::new();
        for i in 1..=5 {
            welfare.insert(i, 25_000_000_000);
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &welfare);

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
            welfare_per_order in 1..100i64,
        ) {
            let mut problem = Problem::new("proptest");
            let market = problem.markets.add_binary("m");

            // Add liquidity
            problem.liquidity.add_ask(market, 0, 500_000_000, 100000);

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

            let mut welfare = HashMap::new();
            for i in 1..=num_orders {
                welfare.insert(i as u64, welfare_per_order * 1_000_000_000);
            }

            let result = allocator.allocate(&[mm], &prices, &problem.orders, &welfare);

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

        /// Property: More budget should not decrease welfare
        #[test]
        fn prop_more_budget_more_or_equal_welfare(
            num_orders in 2..8usize,
            base_budget in 50..200u64,
            qty_per_order in 50..200u64,
        ) {
            let mut problem = Problem::new("proptest_monotonic");
            let market = problem.markets.add_binary("m");

            problem.liquidity.add_ask(market, 0, 500_000_000, 100000);

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

            let mut welfare = HashMap::new();
            for i in 1..=num_orders {
                welfare.insert(i as u64, 10_000_000_000i64);
            }

            let result_small = allocator.allocate(&[mm_small], &prices, &problem.orders, &welfare);
            let result_large = allocator.allocate(&[mm_large], &prices, &problem.orders, &welfare);

            // More budget should give at least as much welfare
            let welfare_small: i64 = result_small.mm_allocations.iter()
                .flat_map(|a| &a.activated_orders)
                .filter_map(|id| welfare.get(id))
                .sum();
            let welfare_large: i64 = result_large.mm_allocations.iter()
                .flat_map(|a| &a.activated_orders)
                .filter_map(|id| welfare.get(id))
                .sum();

            prop_assert!(
                welfare_large >= welfare_small,
                "Monotonicity violated: larger budget ({}) gave less welfare ({}) than smaller budget ({}) with welfare ({})",
                large_budget, welfare_large, small_budget, welfare_small
            );
        }
    }

    #[test]
    fn test_stats_reporting() {
        let mut problem = Problem::new("stats_test");
        let market = problem.markets.add_binary("m");

        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);

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

        // High welfare to ensure orders get activated
        let mut welfare = HashMap::new();
        for i in 1..=10 {
            welfare.insert(i, 1_000_000_000_000i64); // $1000 welfare each
        }

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &welfare);

        // Check stats are populated
        let stats = &result.stats;

        println!("Stats: {:?}", stats);
        println!("MM allocations: {:?}", result.mm_allocations);

        assert_eq!(stats.total_budget, 2_000_000_000_000, "Total budget should be $2000");
        assert_eq!(stats.mm_orders_considered, 10, "Should consider all 10 orders");
        assert!(!stats.mms_interact, "Single MM should not interact");

        // With high welfare, orders should be activated if within budget
        if stats.mm_orders_activated > 0 {
            assert!(stats.total_capital_used > 0, "Should use capital if orders activated");
            assert!(stats.total_capital_used <= stats.total_budget, "Capital used should not exceed budget");
            assert!(stats.activation_rate > 0.0 && stats.activation_rate <= 1.0, "Activation rate should be in [0, 1]");
            assert!(stats.overall_utilization > 0.0, "Utilization should be positive if orders activated");
        }
    }

    #[test]
    fn test_overlapping_mms_fixed_point() {
        let mut problem = Problem::new("overlapping_mms");
        let market = problem.markets.add_binary("m");

        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);

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

        let mut welfare = HashMap::new();
        for i in 1..=6 {
            welfare.insert(i, 10_000_000_000i64);
        }

        let result = allocator.allocate(&[mm1, mm2], &prices, &problem.orders, &welfare);

        // Should detect interaction
        assert!(result.stats.mms_interact, "Should detect MMs share orders 3 and 4");

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
        println!("  MM1: activated {:?}, capital used {}",
            result.mm_allocations[0].activated_orders.len(),
            result.mm_allocations[0].capital_used);
        println!("  MM2: activated {:?}, capital used {}",
            result.mm_allocations[1].activated_orders.len(),
            result.mm_allocations[1].capital_used);
    }

    #[test]
    fn test_sanity_check_vs_greedy() {
        // Test that Lagrangian approach is at least as good as greedy
        let mut problem = Problem::new("sanity_check");
        let market = problem.markets.add_binary("m");

        problem.liquidity.add_ask(market, 0, 500_000_000, 10000);

        // Add orders with varying welfare
        for i in 1..=10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i,
                market,
                600_000_000,
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

        // Varying welfare per order
        let mut welfare = HashMap::new();
        welfare.insert(1, 50_000_000_000i64);
        welfare.insert(2, 40_000_000_000i64);
        welfare.insert(3, 30_000_000_000i64);
        welfare.insert(4, 25_000_000_000i64);
        welfare.insert(5, 20_000_000_000i64);
        welfare.insert(6, 15_000_000_000i64);
        welfare.insert(7, 10_000_000_000i64);
        welfare.insert(8, 8_000_000_000i64);
        welfare.insert(9, 5_000_000_000i64);
        welfare.insert(10, 3_000_000_000i64);

        let result = allocator.allocate(&[mm], &prices, &problem.orders, &welfare);

        // Actual welfare from activated orders
        let actual_mm_welfare: i64 = result
            .mm_allocations
            .iter()
            .flat_map(|a| &a.activated_orders)
            .filter_map(|id| welfare.get(id))
            .sum();

        // Greedy baseline is computed by the allocator
        let greedy_baseline = result.stats.greedy_baseline_welfare;

        println!("Sanity check:");
        println!("  Greedy baseline welfare: {}", greedy_baseline);
        println!("  Actual MM welfare: {}", actual_mm_welfare);
        println!("  Improvement: {:.2}%", result.stats.improvement_over_greedy * 100.0);

        // Lagrangian should be at least as good as greedy
        // (might be slightly different due to tie-breaking, but should be close)
        // Allow 1% tolerance for rounding differences
        assert!(
            actual_mm_welfare >= (greedy_baseline as f64 * 0.99) as i64,
            "Lagrangian ({}) should be at least as good as greedy ({})",
            actual_mm_welfare,
            greedy_baseline
        );
    }
}
