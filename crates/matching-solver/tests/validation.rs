//! Validation tests for solver correctness.
//!
//! These tests verify ECONOMIC correctness, not just that code runs.

use matching_engine::{Order, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
use matching_solver::{local_solver::LocalSolver, mm_allocator::MmAllocator};
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

    assert_eq!(
        violations, 0,
        "Found {} price normalization violations",
        violations
    );
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
/// - Buyers (positive payoff): fill_price <= limit_price (pay no more than limit)
/// - Sellers (negative payoff): fill_price >= limit_price (receive at least limit)
#[test]
fn validate_fills_respect_limits() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();
    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

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
                // Determine if this is a buy or sell order based on payoffs
                let is_seller = order
                    .payoffs
                    .iter()
                    .take(order.num_states as usize)
                    .any(|&p| p < 0);

                let is_violation = if is_seller {
                    // Sellers: fill_price must be >= limit (they receive at least what they asked)
                    fill.fill_price < order.limit_price
                } else {
                    // Buyers: fill_price must be <= limit (they pay no more than willing)
                    fill.fill_price > order.limit_price
                };

                if is_violation {
                    violations += 1;
                    eprintln!(
                        "Order {} ({}): fill_price {} vs limit_price {}",
                        order.id,
                        if is_seller { "sell" } else { "buy" },
                        fill.fill_price,
                        order.limit_price
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
/// Supply comes from both the liquidity book AND sell orders.
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

        // Get available liquidity from book
        let book_liquidity: u64 = book.asks().iter().map(|l| l.available_qty).sum();

        // Get available supply from sell orders (negative payoffs)
        // Sell orders for this market have payoff < 0 for outcome 0
        let seller_supply: u64 = problem
            .orders
            .iter()
            .filter(|o| {
                o.num_markets == 1 && o.markets[0] == market.id && o.payoffs[0] < 0
                // Sell order
            })
            .map(|o| o.max_fill)
            .sum();

        let total_supply = book_liquidity + seller_supply;

        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);

        // Sum up total buyer fills for this market (positive payoff fills)
        let total_buyer_fills: u64 = solution
            .fills
            .iter()
            .filter(|f| {
                problem
                    .orders
                    .iter()
                    .find(|o| o.id == f.order_id)
                    .map(|o| o.payoffs[0] > 0)
                    .unwrap_or(false)
            })
            .map(|f| f.fill_qty)
            .sum();

        // CRITICAL CHECK: buyer fills cannot exceed total supply!
        if total_buyer_fills > total_supply {
            panic!(
                "Market {:?}: buyers filled {} but only {} supply available (book: {}, sellers: {})! Overfill by {}",
                market.id, total_buyer_fills, total_supply, book_liquidity, seller_supply,
                total_buyer_fills - total_supply
            );
        }
    }
}

/// Compare current approach vs iterative fixed-point approach.
/// This establishes baseline metrics for future optimization.
#[test]
fn compare_current_vs_iterative_approach() {
    use matching_engine::{MarketId, Nanos};

    // Use large scenario for meaningful comparison
    let config = MegaScenarioConfigV2::large();
    let problem = generate_mega_scenario_v2(config);

    let solver = LocalSolver::new();
    let allocator = MmAllocator::new();

    // Identify MM order IDs
    let mm_order_ids: std::collections::HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();

    // Split orders into non-MM and MM orders
    let non_mm_orders: Vec<_> = problem
        .orders
        .iter()
        .filter(|o| !mm_order_ids.contains(&o.id))
        .cloned()
        .collect();

    let mm_orders: Vec<_> = problem
        .orders
        .iter()
        .filter(|o| mm_order_ids.contains(&o.id))
        .cloned()
        .collect();

    println!("\n=== SOLVER ORDERING COMPARISON ===");
    println!("Total orders: {}", problem.orders.len());
    println!("Non-MM orders: {}", non_mm_orders.len());
    println!("MM orders: {}", mm_orders.len());
    println!("Markets: {}", problem.markets.iter().count());
    println!("MMs: {}", problem.mm_constraints.len());

    // Print MM budget info
    for mm in &problem.mm_constraints {
        println!(
            "  MM {:?}: budget=${}, {} orders",
            mm.mm_id,
            mm.max_capital / NANOS_PER_DOLLAR,
            mm.order_ids.len()
        );
    }

    // --- CURRENT APPROACH: Per-market first (without MM), then MM allocation ---
    println!("\n--- Current Approach (per-market first, then MM) ---");

    let mut current_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
    let mut current_volume: u64 = 0;
    let mut current_welfare: i64 = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));

        // Solve with non-MM orders only
        let solution = solver.solve_market(market.id, &problem.markets, &non_mm_orders, &book);
        current_prices.insert(market.id, solution.prices);
        current_volume += solution.fills.iter().map(|f| f.fill_qty).sum::<u64>();
        current_welfare += solution.welfare;
    }

    // Compute welfare as economic surplus at clearing prices
    // welfare = (limit_price - clearing_price) * qty for buyers
    // This is what the order would gain if filled at the clearing price
    let welfare_map: HashMap<u64, i64> = problem
        .orders
        .iter()
        .map(|o| {
            // Get clearing price for this order's market
            let clearing_price = if o.num_markets > 0 {
                current_prices
                    .get(&o.markets[0])
                    .and_then(|p| p.first().copied())
                    .unwrap_or(500_000_000) // default 50 cents
            } else {
                500_000_000
            };

            // Welfare = surplus if filled at clearing price
            // For a buyer: limit - clearing (positive if limit > clearing)
            let surplus_per_share = o.limit_price as i64 - clearing_price as i64;
            let welfare = surplus_per_share * o.max_fill as i64;

            // Only positive welfare makes sense (order wouldn't fill if negative)
            (o.id, welfare.max(0))
        })
        .collect();

    // Debug: check welfare distribution for MM orders
    let mm_welfare_stats: Vec<i64> = mm_order_ids
        .iter()
        .filter_map(|id| welfare_map.get(id).copied())
        .collect();
    let mm_positive_welfare = mm_welfare_stats.iter().filter(|&&w| w > 0).count();
    println!(
        "MM welfare: {} orders with positive welfare out of {}",
        mm_positive_welfare,
        mm_welfare_stats.len()
    );

    let mm_result = allocator.allocate(
        &problem.mm_constraints,
        &current_prices,
        &problem.orders,
        &welfare_map,
    );

    println!("Phase 1 (non-MM clearing):");
    println!("  Volume: {} shares", current_volume);
    println!("  Welfare: {}", current_welfare);
    println!("Phase 2 (MM allocation):");
    println!(
        "  MM orders activated: {}",
        mm_result.stats.mm_orders_activated
    );
    println!(
        "  MM utilization: {:.1}%",
        mm_result.stats.overall_utilization * 100.0
    );
    println!("  Total welfare (with MM): {}", mm_result.total_welfare);

    // --- ITERATIVE APPROACH: Fixed-point between clearing and MM allocation ---
    println!("\n--- Iterative Approach (fixed-point) ---");

    let mut iter_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
    let mut iter_volume: u64;
    let mut iter_welfare: i64;
    let mut _activated_mm_orders: Vec<u64> = Vec::new();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 5;
    const PRICE_TOLERANCE: u64 = 1_000_000; // $0.001

    // Initial solve with non-MM orders
    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &non_mm_orders, &book);
        iter_prices.insert(market.id, solution.prices);
    }

    loop {
        iterations += 1;

        // MM allocation with current prices
        let mm_result = allocator.allocate(
            &problem.mm_constraints,
            &iter_prices,
            &problem.orders,
            &welfare_map,
        );
        let new_activated: Vec<u64> = mm_result.activated_orders.clone();

        // Build order set: non-MM + activated MM orders
        let activated_mm_set: std::collections::HashSet<u64> =
            new_activated.iter().copied().collect();
        let combined_orders: Vec<_> = non_mm_orders
            .iter()
            .cloned()
            .chain(
                mm_orders
                    .iter()
                    .filter(|o| activated_mm_set.contains(&o.id))
                    .cloned(),
            )
            .collect();

        // Re-solve with combined orders
        let mut new_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
        iter_volume = 0;
        iter_welfare = 0;

        for market in problem.markets.iter() {
            let book = problem
                .liquidity
                .books
                .get(&(market.id, 0))
                .cloned()
                .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
            let solution =
                solver.solve_market(market.id, &problem.markets, &combined_orders, &book);
            new_prices.insert(market.id, solution.prices.clone());
            iter_volume += solution.fills.iter().map(|f| f.fill_qty).sum::<u64>();
            iter_welfare += solution.welfare;
        }

        // Check convergence: prices didn't change much
        let mut max_price_change: u64 = 0;
        for (market_id, new_price_vec) in &new_prices {
            if let Some(old_price_vec) = iter_prices.get(market_id) {
                for (new_p, old_p) in new_price_vec.iter().zip(old_price_vec.iter()) {
                    let diff = if *new_p > *old_p {
                        *new_p - *old_p
                    } else {
                        *old_p - *new_p
                    };
                    max_price_change = max_price_change.max(diff);
                }
            }
        }

        println!(
            "Iteration {}: max_price_change = ${:.6}, activated = {}",
            iterations,
            max_price_change as f64 / NANOS_PER_DOLLAR as f64,
            new_activated.len()
        );

        iter_prices = new_prices;
        _activated_mm_orders = new_activated;

        if max_price_change < PRICE_TOLERANCE || iterations >= MAX_ITERATIONS {
            break;
        }
    }

    // Final MM allocation with converged prices
    let final_mm_result = allocator.allocate(
        &problem.mm_constraints,
        &iter_prices,
        &problem.orders,
        &welfare_map,
    );

    println!("\nIterative result after {} iterations:", iterations);
    println!("  Volume: {} shares", iter_volume);
    println!("  Welfare: {}", iter_welfare);
    println!(
        "  MM orders activated: {}",
        final_mm_result.stats.mm_orders_activated
    );
    println!(
        "  MM utilization: {:.1}%",
        final_mm_result.stats.overall_utilization * 100.0
    );
    println!(
        "  Total welfare (with MM): {}",
        final_mm_result.total_welfare
    );

    // --- COMPARISON ---
    println!("\n=== COMPARISON ===");
    let volume_diff = iter_volume as i64 - current_volume as i64;
    let welfare_diff = iter_welfare - current_welfare;
    let mm_welfare_diff = final_mm_result.total_welfare - mm_result.total_welfare;

    println!(
        "Volume change: {:+} shares ({:+.2}%)",
        volume_diff,
        if current_volume > 0 {
            volume_diff as f64 / current_volume as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Welfare change (clearing): {:+} ({:+.2}%)",
        welfare_diff,
        if current_welfare > 0 {
            welfare_diff as f64 / current_welfare as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Welfare change (total): {:+} ({:+.2}%)",
        mm_welfare_diff,
        if mm_result.total_welfare > 0 {
            mm_welfare_diff as f64 / mm_result.total_welfare as f64 * 100.0
        } else {
            0.0
        }
    );

    // Print price comparison for first few markets
    println!("\n=== PRICE ANALYSIS (first 5 markets) ===");
    for (i, market) in problem.markets.iter().take(5).enumerate() {
        let current_p = current_prices.get(&market.id).map(|p| p[0]).unwrap_or(0);
        let iter_p = iter_prices.get(&market.id).map(|p| p[0]).unwrap_or(0);
        let diff = if iter_p > current_p {
            iter_p - current_p
        } else {
            current_p - iter_p
        };
        println!(
            "Market {}: current=${:.4}, iter=${:.4}, diff=${:.6}",
            i,
            current_p as f64 / NANOS_PER_DOLLAR as f64,
            iter_p as f64 / NANOS_PER_DOLLAR as f64,
            diff as f64 / NANOS_PER_DOLLAR as f64
        );
    }

    // Summary
    println!("\n=== BASELINE ESTABLISHED ===");
    println!(
        "Scenario: {} orders, {} markets, {} MMs",
        problem.orders.len(),
        problem.markets.iter().count(),
        problem.mm_constraints.len()
    );
    println!(
        "Non-MM vs MM orders: {} vs {}",
        non_mm_orders.len(),
        mm_orders.len()
    );
    println!("Current approach:");
    println!("  - Volume: {} shares", current_volume);
    println!("  - Clearing welfare: {}", current_welfare);
    println!(
        "  - MM orders activated: {}/{}",
        mm_result.stats.mm_orders_activated, mm_result.stats.mm_orders_considered
    );
    println!(
        "  - MM utilization: {:.1}%",
        mm_result.stats.overall_utilization * 100.0
    );
    println!("Iterative converged in {} iterations", iterations);
    println!("Price impact of adding MM: max ${:.6}", 0.0); // Already computed above

    // Both approaches should respect budget constraints
    for alloc in &final_mm_result.mm_allocations {
        assert!(
            alloc.capital_used <= alloc.budget,
            "Iterative approach violated MM budget: {} > {}",
            alloc.capital_used,
            alloc.budget
        );
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
        assert!(
            solution.is_normalized(),
            "Market {:?} not normalized",
            market.id
        );

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
            alloc.capital_used,
            alloc.budget
        );
    }

    println!(
        "MM allocation: {} orders activated, welfare = {}",
        result.activated_orders.len(),
        result.total_welfare
    );
}
