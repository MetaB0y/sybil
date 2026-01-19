//! Validation tests for solver correctness.
//!
//! These tests verify ECONOMIC correctness, not just that code runs.

use matching_engine::{Order, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
use matching_solver::{
    local_solver::LocalSolver,
    mm_allocator::MmAllocator,
};
use std::collections::HashMap;

/// Validate that all market solutions have normalized prices (sum to $1).
#[test]
fn validate_price_normalization() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();
    let mut violations = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        // Check: prices sum to $1 (within tolerance)
        let sum: u64 = solution.prices.iter().sum();
        let diff = if sum > NANOS_PER_DOLLAR {
            sum - NANOS_PER_DOLLAR
        } else {
            NANOS_PER_DOLLAR - sum
        };

        if diff > 1 {
            violations += 1;
            eprintln!(
                "Market {:?}: prices sum to {} (off by {})",
                market.id, sum, diff
            );
        }
    }

    assert_eq!(violations, 0, "Found {} price normalization violations", violations);
}

/// Validate that MM allocations respect budget constraints.
/// This is the CRITICAL property that must always hold.
#[test]
fn validate_mm_budget_constraints() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);

    // Run clearing to get prices
    let solver = LocalSolver::new();
    let mut prices = HashMap::new();
    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);
        prices.insert(market.id, solution.prices);
    }

    // Compute welfare (simplified: 1 per order)
    let welfare: HashMap<u64, i64> = problem.orders.iter().map(|o| (o.id, 1i64)).collect();

    // Run MM allocation
    let allocator = MmAllocator::new();
    let result = allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &welfare);

    // Validate: each MM's capital_used <= budget
    let mut violations = 0;
    for alloc in &result.mm_allocations {
        if alloc.capital_used > alloc.budget {
            violations += 1;
            eprintln!(
                "MM {:?}: capital_used {} exceeds budget {}",
                alloc.mm_id, alloc.capital_used, alloc.budget
            );
        }
    }

    assert_eq!(violations, 0, "Found {} MM budget violations", violations);

    println!(
        "Validated {} MMs, all within budget. Total welfare: {}",
        result.mm_allocations.len(),
        result.total_welfare
    );
}

/// Validate fills respect limit prices.
#[test]
fn validate_fills_respect_limits() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();
    let order_map: HashMap<u64, &Order> =
        problem.orders.iter().map(|o| (o.id, o)).collect();

    let mut violations = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        for fill in &solution.fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                if fill.fill_price > order.limit_price {
                    violations += 1;
                    eprintln!(
                        "Order {}: fill_price {} > limit_price {}",
                        order.id, fill.fill_price, order.limit_price
                    );
                }
            }
        }
    }

    assert_eq!(violations, 0, "Found {} limit price violations", violations);
}

/// Fill prices MUST match the reported clearing prices for their respective outcomes.
/// Each order buys a specific outcome, so its fill price must match that outcome's clearing price.
#[test]
fn validate_fill_prices_match_clearing_prices() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();
    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
    let mut mismatches = 0;
    let mut total_fills = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        for fill in &solution.fills {
            total_fills += 1;

            // Find the order to determine which outcome it's buying
            let Some(order) = order_map.get(&fill.order_id) else {
                continue;
            };

            // Determine which outcome this order is buying (positive payoff)
            let outcome_idx = order
                .payoffs
                .iter()
                .take(order.num_states as usize)
                .position(|&p| p > 0);

            let Some(outcome) = outcome_idx else {
                // Order doesn't buy any outcome - skip check
                continue;
            };

            // Get the clearing price for THIS outcome
            let clearing_price = solution.prices.get(outcome).copied().unwrap_or(0);

            // Fill price MUST match the clearing price for this outcome
            if fill.fill_price != clearing_price && fill.fill_price != 0 {
                mismatches += 1;
                eprintln!(
                    "Fill {} (outcome {}) at price {} != clearing price {}",
                    fill.order_id, outcome, fill.fill_price, clearing_price
                );
            }
        }
    }

    assert_eq!(
        mismatches, 0,
        "Fill price mismatches: {}/{} - fills must be at clearing prices!",
        mismatches, total_fills
    );
}

/// CRITICAL: Fills must not exceed available liquidity.
/// This test checks that we don't create impossible fills.
#[test]
fn validate_fills_respect_liquidity() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));

        // Get available liquidity for this outcome
        let available_liquidity: u64 = book.asks().iter().map(|l| l.available_qty).sum();

        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        // Sum up total fills for this market
        let total_filled: u64 = solution.fills.iter().map(|f| f.fill_qty).sum();

        // CRITICAL CHECK: fills cannot exceed liquidity!
        if total_filled > available_liquidity {
            panic!(
                "Market {:?}: filled {} but only {} liquidity available! Overfill by {}",
                market.id, total_filled, available_liquidity,
                total_filled - available_liquidity
            );
        }
    }
}

/// Test with large scenario to stress test validation.
#[test]
fn validate_large_scenario() {
    let config = MegaScenarioConfigV2::large();
    let problem = generate_mega_scenario_v2(config);

    println!(
        "Large scenario: {} markets, {} orders, {} MMs",
        problem.markets.iter().count(),
        problem.orders.len(),
        problem.mm_constraints.len()
    );

    let solver = LocalSolver::new();
    let mut prices = HashMap::new();
    let mut total_fills = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        // Verify normalization
        assert!(solution.is_normalized(), "Market {:?} not normalized", market.id);

        prices.insert(market.id, solution.prices);
        total_fills += solution.fills.len();
    }

    println!("Total fills generated: {}", total_fills);

    // Verify MM allocation
    let welfare: HashMap<u64, i64> = problem.orders.iter().map(|o| (o.id, 1i64)).collect();
    let allocator = MmAllocator::new();
    let result = allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &welfare);

    for alloc in &result.mm_allocations {
        assert!(
            alloc.capital_used <= alloc.budget,
            "MM budget violated: {} > {}",
            alloc.capital_used, alloc.budget
        );
    }

    println!(
        "MM allocation: {} orders activated, welfare = {}",
        result.activated_orders.len(),
        result.total_welfare
    );
}
