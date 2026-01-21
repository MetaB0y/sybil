//! Detailed welfare analysis to understand solver behavior.

use matching_engine::{MarketId, MmSide, Nanos, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
use matching_solver::{local_solver::LocalSolver, mm_allocator::MmAllocator};
use std::collections::{HashMap, HashSet};

/// Analyze welfare at each stage and understand why convergence is immediate.
#[test]
fn analyze_welfare_stages() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);

    println!("\n{:=^80}", " WELFARE ANALYSIS ");

    // Identify MM orders
    let mm_order_ids: HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();

    // Build order side lookup
    let mut order_sides: HashMap<u64, MmSide> = HashMap::new();
    for mm in &problem.mm_constraints {
        for (&order_id, &side) in &mm.order_sides {
            order_sides.insert(order_id, side);
        }
    }

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

    println!("\n--- Problem Statistics ---");
    println!("Total orders: {}", problem.orders.len());
    println!("Non-MM orders: {}", non_mm_orders.len());
    println!("MM orders: {}", mm_orders.len());
    println!("Markets: {}", problem.markets.iter().count());
    println!("MMs: {}", problem.mm_constraints.len());

    // Count MM order types
    let mut buy_yes_count = 0;
    let mut sell_yes_count = 0;
    for mm in &problem.mm_constraints {
        for &side in mm.order_sides.values() {
            match side {
                MmSide::BuyYes => buy_yes_count += 1,
                MmSide::SellYes => sell_yes_count += 1,
                _ => {}
            }
        }
    }
    println!("MM BuyYes orders: {}", buy_yes_count);
    println!("MM SellYes orders: {}", sell_yes_count);

    // Stage 1: Solve with non-MM orders only
    println!("\n--- Stage 1: Non-MM Clearing ---");
    let solver = LocalSolver::new();
    let mut stage1_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
    let mut stage1_volume: u64 = 0;
    let mut stage1_fills: usize = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &non_mm_orders, &book);
        stage1_prices.insert(market.id, solution.prices);
        stage1_volume += solution.fills.iter().map(|f| f.fill_qty).sum::<u64>();
        stage1_fills += solution.fills.len();
    }

    println!("Volume: {} shares", stage1_volume);
    println!("Fills: {}", stage1_fills);

    // Analyze price distribution
    let prices_vec: Vec<f64> = stage1_prices
        .values()
        .filter_map(|p| p.first().copied())
        .map(|p| p as f64 / NANOS_PER_DOLLAR as f64)
        .collect();
    if !prices_vec.is_empty() {
        let min_price = prices_vec.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_price = prices_vec.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let avg_price: f64 = prices_vec.iter().sum::<f64>() / prices_vec.len() as f64;
        println!(
            "Clearing prices: min=${:.4}, max=${:.4}, avg=${:.4}",
            min_price, max_price, avg_price
        );
    }

    // Stage 2: Analyze MM order welfare
    println!("\n--- Stage 2: MM Order Welfare Analysis ---");

    // Proper welfare calculation for each order type
    let mut welfare_map: HashMap<u64, i64> = HashMap::new();
    let mut positive_welfare_count = 0;
    let mut negative_welfare_count = 0;
    let mut zero_welfare_count = 0;

    for order in &problem.orders {
        let clearing_price = stage1_prices
            .get(&order.markets[0])
            .and_then(|p| p.first().copied())
            .unwrap_or(500_000_000) as i64;

        let side = order_sides.get(&order.id);
        let limit_price = order.limit_price as i64;
        let qty = order.max_fill as i64;

        // Calculate welfare based on order type
        let welfare = if let Some(&s) = side {
            match s {
                MmSide::BuyYes => {
                    // Buy YES: profit if limit > clearing price
                    // Welfare = (limit - clearing) * qty (buyer's surplus)
                    (limit_price - clearing_price) * qty
                }
                MmSide::SellYes => {
                    // Sell YES: profit if clearing > limit price (receives premium)
                    // Welfare = (clearing - limit) * qty (seller's surplus)
                    // But wait - for a SELL order, limit_price is the MINIMUM they'll accept
                    // So welfare = (clearing - limit) * qty
                    (clearing_price - limit_price) * qty
                }
                _ => (limit_price - clearing_price) * qty,
            }
        } else {
            // Non-MM order (assume buy)
            (limit_price - clearing_price) * qty
        };

        welfare_map.insert(order.id, welfare);

        if mm_order_ids.contains(&order.id) {
            if welfare > 0 {
                positive_welfare_count += 1;
            } else if welfare < 0 {
                negative_welfare_count += 1;
            } else {
                zero_welfare_count += 1;
            }
        }
    }

    println!(
        "MM orders with positive welfare: {}",
        positive_welfare_count
    );
    println!(
        "MM orders with negative welfare: {}",
        negative_welfare_count
    );
    println!("MM orders with zero welfare: {}", zero_welfare_count);

    // Detailed breakdown by side
    let mut buy_yes_positive = 0;
    let mut buy_yes_negative = 0;
    let mut sell_yes_positive = 0;
    let mut sell_yes_negative = 0;

    for &order_id in &mm_order_ids {
        let welfare = welfare_map.get(&order_id).copied().unwrap_or(0);
        let side = order_sides.get(&order_id);
        match side {
            Some(MmSide::BuyYes) => {
                if welfare > 0 {
                    buy_yes_positive += 1;
                } else {
                    buy_yes_negative += 1;
                }
            }
            Some(MmSide::SellYes) => {
                if welfare > 0 {
                    sell_yes_positive += 1;
                } else {
                    sell_yes_negative += 1;
                }
            }
            _ => {}
        }
    }

    println!("\nBreakdown by order type:");
    println!(
        "  BuyYes:  {} positive, {} negative",
        buy_yes_positive, buy_yes_negative
    );
    println!(
        "  SellYes: {} positive, {} negative",
        sell_yes_positive, sell_yes_negative
    );

    // Stage 3: MM Allocation
    println!("\n--- Stage 3: MM Allocation ---");
    let allocator = MmAllocator::new();
    let result = allocator.allocate(
        &problem.mm_constraints,
        &stage1_prices,
        &problem.orders,
        &welfare_map,
    );

    println!("MM orders activated: {}", result.stats.mm_orders_activated);
    println!(
        "MM orders considered: {}",
        result.stats.mm_orders_considered
    );
    println!(
        "Activation rate: {:.1}%",
        result.stats.activation_rate * 100.0
    );
    println!(
        "MM utilization: {:.1}%",
        result.stats.overall_utilization * 100.0
    );

    for alloc in &result.mm_allocations {
        println!(
            "  MM {:?}: activated {}/{} orders, capital ${:.2}/${:.2} ({:.1}% util)",
            alloc.mm_id,
            alloc.activated_orders.len(),
            problem
                .mm_constraints
                .iter()
                .find(|m| m.mm_id == alloc.mm_id)
                .map(|m| m.order_ids.len())
                .unwrap_or(0),
            alloc.capital_used as f64 / NANOS_PER_DOLLAR as f64,
            alloc.budget as f64 / NANOS_PER_DOLLAR as f64,
            alloc.utilization * 100.0
        );
    }

    // Stage 4: Re-solve with activated MM orders
    println!("\n--- Stage 4: Re-clearing with MM Orders ---");
    let activated_set: HashSet<u64> = result.activated_orders.iter().copied().collect();
    let combined_orders: Vec<_> = non_mm_orders
        .iter()
        .cloned()
        .chain(
            mm_orders
                .iter()
                .filter(|o| activated_set.contains(&o.id))
                .cloned(),
        )
        .collect();

    let mut stage4_prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
    let mut stage4_volume: u64 = 0;
    let mut stage4_fills: usize = 0;

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &combined_orders, &book);
        stage4_prices.insert(market.id, solution.prices);
        stage4_volume += solution.fills.iter().map(|f| f.fill_qty).sum::<u64>();
        stage4_fills += solution.fills.len();
    }

    println!("Volume: {} shares", stage4_volume);
    println!("Fills: {}", stage4_fills);

    // Compare prices
    let mut max_price_diff: f64 = 0.0;
    let mut price_changes = 0;
    for market in problem.markets.iter() {
        let p1 = stage1_prices
            .get(&market.id)
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        let p4 = stage4_prices
            .get(&market.id)
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        let diff = (p4 as f64 - p1 as f64).abs() / NANOS_PER_DOLLAR as f64;
        if diff > 0.0001 {
            price_changes += 1;
        }
        max_price_diff = max_price_diff.max(diff);
    }

    println!(
        "Price changes: {} markets changed by > $0.0001",
        price_changes
    );
    println!("Max price change: ${:.6}", max_price_diff);
    println!(
        "Volume change: {:+} shares",
        stage4_volume as i64 - stage1_volume as i64
    );

    // Analyze why MM orders might not affect prices
    println!("\n--- Why Immediate Convergence? ---");

    // Check 1: Are MM orders getting filled?
    let mut mm_fills = 0;
    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &combined_orders, &book);
        for fill in &solution.fills {
            if activated_set.contains(&fill.order_id) {
                mm_fills += 1;
            }
        }
    }
    println!(
        "1. MM orders filled: {} (out of {} activated)",
        mm_fills, result.stats.mm_orders_activated
    );

    // Check 2: How much liquidity is available?
    let total_liquidity: u64 = problem
        .liquidity
        .books
        .values()
        .flat_map(|b| b.asks())
        .map(|l| l.available_qty)
        .sum();
    println!("2. Total liquidity available: {} shares", total_liquidity);

    // Check 3: Are MM order prices competitive?
    let mut mm_competitive = 0;
    let mut mm_not_competitive = 0;
    for order in &mm_orders {
        if !activated_set.contains(&order.id) {
            continue;
        }
        let clearing = stage1_prices
            .get(&order.markets[0])
            .and_then(|p| p.first().copied())
            .unwrap_or(500_000_000);
        let side = order_sides.get(&order.id);

        let is_competitive = match side {
            Some(MmSide::BuyYes) => order.limit_price >= clearing, // Buy: willing to pay at or above clearing
            Some(MmSide::SellYes) => order.limit_price <= clearing, // Sell: willing to accept at or below clearing
            _ => false,
        };

        if is_competitive {
            mm_competitive += 1;
        } else {
            mm_not_competitive += 1;
        }
    }
    println!(
        "3. MM orders at competitive prices: {}, not competitive: {}",
        mm_competitive, mm_not_competitive
    );

    // Check 4: Sample some MM order details
    println!("\n--- Sample MM Orders (first 5 of each type) ---");
    let mut buy_samples = 0;
    let mut sell_samples = 0;

    for order in &mm_orders {
        let side = order_sides.get(&order.id);
        let clearing = stage1_prices
            .get(&order.markets[0])
            .and_then(|p| p.first().copied())
            .unwrap_or(500_000_000);

        let limit_f = order.limit_price as f64 / NANOS_PER_DOLLAR as f64;
        let clearing_f = clearing as f64 / NANOS_PER_DOLLAR as f64;

        match side {
            Some(MmSide::BuyYes) if buy_samples < 5 => {
                println!(
                    "  BuyYes #{}: limit=${:.4}, clearing=${:.4}, diff=${:.4}, qty={}",
                    order.id,
                    limit_f,
                    clearing_f,
                    limit_f - clearing_f,
                    order.max_fill
                );
                buy_samples += 1;
            }
            Some(MmSide::SellYes) if sell_samples < 5 => {
                println!(
                    "  SellYes #{}: limit=${:.4}, clearing=${:.4}, diff=${:.4}, qty={}",
                    order.id,
                    limit_f,
                    clearing_f,
                    clearing_f - limit_f,
                    order.max_fill
                );
                sell_samples += 1;
            }
            _ => {}
        }
    }

    println!("\n{:=^80}", " END ANALYSIS ");
}

/// Test with a carefully constructed scenario where MM orders SHOULD affect prices.
#[test]
fn test_mm_price_impact() {
    use matching_engine::{outcome_buy, outcome_sell, MmConstraint, MmId, Problem};

    println!("\n{:=^80}", " MM PRICE IMPACT TEST ");

    let mut problem = Problem::new("price_impact_test");

    // Create a single market
    let market = problem.markets.add_binary("test_market");

    // Add limited liquidity at 50 cents
    // Only 1000 shares available - this is key!
    problem.liquidity.add_ask(market, 0, 500_000_000, 1000);

    // Add non-MM orders that want to buy at high prices
    // These should drive price up
    for i in 1..=10 {
        problem.orders.push(outcome_buy(
            &problem.markets,
            i,
            market,
            0,
            600_000_000, // willing to pay $0.60
            200,         // 200 shares each = 2000 total demand
        ));
    }

    // Add MM buy orders at competitive prices
    // These should compete for the limited liquidity
    for i in 11..=15 {
        problem.orders.push(outcome_buy(
            &problem.markets,
            i,
            market,
            0,
            550_000_000, // willing to pay $0.55
            500,         // 500 shares each = 2500 total MM demand
        ));
    }

    // Add MM sell orders
    for i in 16..=20 {
        problem.orders.push(outcome_sell(
            &problem.markets,
            i,
            market,
            0,
            520_000_000, // willing to accept $0.52
            500,
        ));
    }

    // Create MM constraint
    let mut mm = MmConstraint::new(MmId::new(1), 100_000 * NANOS_PER_DOLLAR); // $100k budget
    for i in 11..=15 {
        mm.add_order(i, MmSide::BuyYes);
    }
    for i in 16..=20 {
        mm.add_order(i, MmSide::SellYes);
    }
    problem.mm_constraints.push(mm);

    let mm_order_ids: HashSet<u64> = (11..=20).collect();

    let non_mm_orders: Vec<_> = problem
        .orders
        .iter()
        .filter(|o| !mm_order_ids.contains(&o.id))
        .cloned()
        .collect();

    let all_orders = problem.orders.clone();

    let solver = LocalSolver::new();
    let book = problem.liquidity.books.get(&(market, 0)).cloned().unwrap();

    // Solve without MM
    let sol_without_mm = solver.solve_market(market, &problem.markets, &non_mm_orders, &book);
    println!("\nWithout MM orders:");
    println!(
        "  Clearing price: ${:.4}",
        sol_without_mm.prices[0] as f64 / NANOS_PER_DOLLAR as f64
    );
    println!(
        "  Volume: {} shares",
        sol_without_mm.fills.iter().map(|f| f.fill_qty).sum::<u64>()
    );
    println!("  Fills: {}", sol_without_mm.fills.len());

    // Solve with MM
    let sol_with_mm = solver.solve_market(market, &problem.markets, &all_orders, &book);
    println!("\nWith MM orders:");
    println!(
        "  Clearing price: ${:.4}",
        sol_with_mm.prices[0] as f64 / NANOS_PER_DOLLAR as f64
    );
    println!(
        "  Volume: {} shares",
        sol_with_mm.fills.iter().map(|f| f.fill_qty).sum::<u64>()
    );
    println!("  Fills: {}", sol_with_mm.fills.len());

    let price_diff = (sol_with_mm.prices[0] as i64 - sol_without_mm.prices[0] as i64).abs();
    println!(
        "\nPrice impact: ${:.6}",
        price_diff as f64 / NANOS_PER_DOLLAR as f64
    );

    // Check which MM orders got filled
    let mm_fills: Vec<_> = sol_with_mm
        .fills
        .iter()
        .filter(|f| mm_order_ids.contains(&f.order_id))
        .collect();
    println!("MM orders filled: {}", mm_fills.len());
    for fill in &mm_fills {
        println!(
            "  Order {}: {} shares at ${:.4}",
            fill.order_id,
            fill.fill_qty,
            fill.fill_price as f64 / NANOS_PER_DOLLAR as f64
        );
    }

    println!("\n{:=^80}", " END TEST ");
}
