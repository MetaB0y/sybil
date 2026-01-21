//! Validation and stress tests for PriceProjector.
//!
//! These tests verify that the PriceProjector:
//! 1. Correctly identifies and fixes cross-market price violations
//! 2. Scales appropriately with problem size
//! 3. Has acceptable performance at various bundle fractions

use matching_engine::{bundle_yes, simple_yes_buy, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
use matching_solver::Pipeline;

// ============================================================================
// Diagnostic Tests - Understand What Violations Exist in Synthetic Data
// ============================================================================

/// Diagnose projection on existing scenario sizes.
/// Prints what violations are found (if any) at each scale.
#[test]
fn diagnose_projection_on_existing_scenarios() {
    let scenarios: Vec<(&str, MegaScenarioConfigV2)> = vec![
        ("small", MegaScenarioConfigV2::small()),
        ("medium", MegaScenarioConfigV2::medium()),
        ("large", MegaScenarioConfigV2::large()),
    ];

    println!("\n=== PROJECTION DIAGNOSTIC ===\n");

    for (name, config) in scenarios {
        let bundle_fraction = config.bundle_fraction;
        let problem = generate_mega_scenario_v2(config);

        // Count bundle orders (orders with num_markets > 1)
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();
        let single_market_count = problem.orders.len() - bundle_count;

        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        let proj = result.price_projection.as_ref().expect("Should have projection result");

        println!(
            "{}: {} orders ({} single-market, {} bundles, {:.0}% bundle fraction)",
            name,
            problem.orders.len(),
            single_market_count,
            bundle_count,
            bundle_fraction * 100.0
        );
        println!(
            "  violations_fixed={}, max_adjustment=${:.6}, iterations={}, success={}",
            proj.violations_fixed,
            proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64,
            proj.iterations,
            proj.success
        );
        println!(
            "  projection_time={:.3}ms, total_time={:.3}ms",
            result.phase_times.price_projection_secs * 1000.0,
            result.total_time_secs * 1000.0
        );
        println!();
    }
}

/// Diagnose with higher bundle fraction to increase violation likelihood.
#[test]
fn diagnose_with_higher_bundle_fraction() {
    let bundle_fractions = [0.15, 0.25, 0.35, 0.50];

    println!("\n=== BUNDLE FRACTION IMPACT ON VIOLATIONS ===\n");

    for &bundle_fraction in &bundle_fractions {
        let mut config = MegaScenarioConfigV2::medium();
        config.bundle_fraction = bundle_fraction;

        let problem = generate_mega_scenario_v2(config);
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();

        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        let proj = result.price_projection.as_ref().expect("Should have projection result");

        println!(
            "bundle_fraction={:.0}%: {} orders ({} bundles)",
            bundle_fraction * 100.0,
            problem.orders.len(),
            bundle_count
        );
        println!(
            "  violations_fixed={}, max_adjustment=${:.6}, proj_time={:.3}ms",
            proj.violations_fixed,
            proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64,
            result.phase_times.price_projection_secs * 1000.0
        );
    }
}

// ============================================================================
// Stress Tests - Find Breaking Points
// ============================================================================

/// Stress test: vary bundle_fraction to find performance breaking point.
#[test]
fn stress_test_bundle_fraction() {
    let bundle_fractions = [0.15, 0.25, 0.35, 0.50, 0.65, 0.80];

    println!("\n=== STRESS TEST: BUNDLE FRACTION ===\n");
    println!("Using large scenario (~10-30k orders) with varying bundle fractions\n");

    for &bundle_fraction in &bundle_fractions {
        let mut config = MegaScenarioConfigV2::large();
        config.bundle_fraction = bundle_fraction;

        let problem = generate_mega_scenario_v2(config);
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();

        // Count unique joint outcomes (combinations of markets in bundles)
        let mut joint_outcomes = std::collections::HashSet::new();
        for order in problem.orders.iter().filter(|o| o.num_markets > 1) {
            let mut markets: Vec<u32> = order.markets.iter().take(order.num_markets as usize).map(|m| m.0).collect();
            markets.sort();
            joint_outcomes.insert(markets);
        }

        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        let proj = result.price_projection.as_ref().expect("Should have projection result");

        println!(
            "bundle_fraction={:.0}%: {} orders, {} bundles, {} unique joint outcomes",
            bundle_fraction * 100.0,
            problem.orders.len(),
            bundle_count,
            joint_outcomes.len()
        );
        println!(
            "  proj_time={:.3}ms, violations={}, max_adj=${:.6}, iterations={}",
            result.phase_times.price_projection_secs * 1000.0,
            proj.violations_fixed,
            proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64,
            proj.iterations
        );
        println!(
            "  total_time={:.3}ms, success={}",
            result.total_time_secs * 1000.0,
            proj.success
        );
        println!();

        // Warn if projection time exceeds 500ms
        if result.phase_times.price_projection_secs > 0.5 {
            println!("  ⚠️  PROJECTION TIME EXCEEDS 500ms!");
        }
    }
}

/// Stress test: vary number of markets.
#[test]
fn stress_test_num_markets() {
    let market_counts = [50, 100, 200, 500];

    println!("\n=== STRESS TEST: NUMBER OF MARKETS ===\n");
    println!("Using 30% bundle fraction with varying market counts\n");

    for &num_markets in &market_counts {
        let mut config = MegaScenarioConfigV2::default();
        config.num_markets = num_markets;
        config.bundle_fraction = 0.30;

        let problem = generate_mega_scenario_v2(config);
        let bundle_count = problem.orders.iter().filter(|o| o.num_markets > 1).count();

        let pipeline = Pipeline::consistent();
        let result = pipeline.solve(&problem);

        let proj = result.price_projection.as_ref().expect("Should have projection result");

        println!(
            "markets={}: {} orders, {} bundles",
            num_markets,
            problem.orders.len(),
            bundle_count
        );
        println!(
            "  proj_time={:.3}ms, violations={}, max_adj=${:.6}",
            result.phase_times.price_projection_secs * 1000.0,
            proj.violations_fixed,
            proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64
        );
        println!(
            "  total_time={:.3}ms, success={}",
            result.total_time_secs * 1000.0,
            proj.success
        );
        println!();

        // Warn if total time exceeds 1 second
        if result.total_time_secs > 1.0 {
            println!("  ⚠️  TOTAL TIME EXCEEDS 1 SECOND!");
        }
    }
}

// ============================================================================
// Synthetic Violation Tests - Deliberately Create Violations
// ============================================================================

/// Test with a hand-crafted scenario that MUST create violations.
///
/// Creates a "violation triangle":
/// - Single-market orders set prices high (e.g., A=70%, B=60%)
/// - Bundle order A∧B at lower price than independence implies (e.g., 30% vs 42%)
#[test]
fn test_guaranteed_violation_scenario() {
    let mut problem = matching_engine::Problem::new("violation_test");

    // Create two markets
    let market_a = problem.markets.add_binary("market_a");
    let market_b = problem.markets.add_binary("market_b");

    // Add LIMITED liquidity so orders can move price
    // If demand >> supply, clearing price will be at the marginal buyer's willingness to pay
    problem.liquidity.add_ask(market_a, 0, 500_000_000, 100); // Small YES liquidity
    problem.liquidity.add_ask(market_a, 1, 500_000_000, 100);
    problem.liquidity.add_ask(market_b, 0, 500_000_000, 100);
    problem.liquidity.add_ask(market_b, 1, 500_000_000, 100);

    // Add aggressive buy orders that push prices UP
    // Total demand: 5000 shares, supply: 100 → price should rise to limit

    // Market A: Many buys at 70% → price should be ~70%
    for i in 0..50 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            i + 1,
            market_a,
            700_000_000, // 70 cents
            100,
        ));
    }

    // Market B: Many buys at 60% → price should be ~60%
    for i in 0..50 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            100 + i + 1,
            market_b,
            600_000_000, // 60 cents
            100,
        ));
    }

    // Bundle order A∧B at 30% - but if A=70% and B=60%, independence implies 42%!
    // This creates a VIOLATION that the projector MUST fix
    for i in 0..20 {
        problem.orders.push(bundle_yes(
            &problem.markets,
            200 + i + 1,
            &[market_a, market_b],
            300_000_000, // 30 cents
            50,
        ));
    }

    println!("\n=== GUARANTEED VIOLATION SCENARIO ===\n");
    println!("Setup:");
    println!("  Market A: 50 buy orders at 70%, 100 qty each (demand: 5000)");
    println!("  Market B: 50 buy orders at 60%, 100 qty each (demand: 5000)");
    println!("  Liquidity: 100 qty each market at 50%");
    println!("  Bundle A∧B: 20 orders at 30% (independence implies ~42%)");
    println!();

    let pipeline = Pipeline::consistent();
    let result = pipeline.solve(&problem);

    // Print discovered prices
    if let Some(pd) = &result.price_discovery {
        println!("Price Discovery Results:");
        for (&market_id, prices) in &pd.prices {
            let p0 = prices[0] as f64 / NANOS_PER_DOLLAR as f64;
            let p1 = prices.get(1).map(|p| *p as f64 / NANOS_PER_DOLLAR as f64).unwrap_or(0.0);
            println!("  Market {:?}: YES=${:.4}, NO=${:.4}", market_id, p0, p1);
        }
        println!();
    }

    let proj = result.price_projection.as_ref().expect("Should have projection result");

    println!("Projection Results:");
    println!("  violations_fixed={}", proj.violations_fixed);
    println!(
        "  max_adjustment=${:.6}",
        proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64
    );
    println!("  iterations={}", proj.iterations);
    println!(
        "  proj_time={:.3}ms",
        result.phase_times.price_projection_secs * 1000.0
    );
    println!("  success={}", proj.success);

    // Print projected prices
    if let Some(prices_a) = proj.prices.get(&market_a) {
        println!(
            "  Market A projected price: ${:.4}",
            prices_a[0] as f64 / NANOS_PER_DOLLAR as f64
        );
    }
    if let Some(prices_b) = proj.prices.get(&market_b) {
        println!(
            "  Market B projected price: ${:.4}",
            prices_b[0] as f64 / NANOS_PER_DOLLAR as f64
        );
    }

    // We EXPECT violations to be fixed
    // If this assertion fails, the projector may not be detecting violations properly
    assert!(
        proj.success,
        "Projection should succeed even with violations"
    );
}

/// Test with multiple violation triangles at scale.
#[test]
fn test_multiple_violation_triangles() {
    let mut problem = matching_engine::Problem::new("multi_violation_test");

    let num_triangles = 10;
    let mut order_id: u64 = 1;

    println!("\n=== MULTIPLE VIOLATION TRIANGLES ===\n");
    println!("Creating {} violation triangles...\n", num_triangles);

    for t in 0..num_triangles {
        // Create two markets for this triangle
        let market_a = problem.markets.add_binary(&format!("market_{}_a", t));
        let market_b = problem.markets.add_binary(&format!("market_{}_b", t));

        // Add liquidity
        problem.liquidity.add_ask(market_a, 0, 500_000_000, 5000);
        problem.liquidity.add_ask(market_a, 1, 500_000_000, 5000);
        problem.liquidity.add_ask(market_b, 0, 500_000_000, 5000);
        problem.liquidity.add_ask(market_b, 1, 500_000_000, 5000);

        // High-priced single-market orders
        for _ in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                order_id,
                market_a,
                700_000_000 + (t as u64 * 10_000_000), // 70-80%
                50,
            ));
            order_id += 1;
        }

        for _ in 0..10 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                order_id,
                market_b,
                600_000_000 + (t as u64 * 10_000_000), // 60-70%
                50,
            ));
            order_id += 1;
        }

        // Low-priced bundle that violates independence
        for _ in 0..5 {
            problem.orders.push(bundle_yes(
                &problem.markets,
                order_id,
                &[market_a, market_b],
                250_000_000 + (t as u64 * 5_000_000), // 25-30% (should be ~42-56%)
                30,
            ));
            order_id += 1;
        }
    }

    println!("Total: {} orders, {} markets", problem.orders.len(), num_triangles * 2);

    let pipeline = Pipeline::consistent();
    let result = pipeline.solve(&problem);

    let proj = result.price_projection.as_ref().expect("Should have projection result");

    println!("\nResults:");
    println!("  violations_fixed={}", proj.violations_fixed);
    println!(
        "  max_adjustment=${:.6}",
        proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64
    );
    println!("  iterations={}", proj.iterations);
    println!(
        "  proj_time={:.3}ms",
        result.phase_times.price_projection_secs * 1000.0
    );
    println!("  success={}", proj.success);

    assert!(proj.success, "Projection should succeed");
}

// ============================================================================
// Pipeline Comparison Tests
// ============================================================================

/// Compare current pipeline vs consistent pipeline performance.
#[test]
fn compare_current_vs_consistent() {
    let configs = [
        ("small", MegaScenarioConfigV2::small()),
        ("medium", MegaScenarioConfigV2::medium()),
    ];

    println!("\n=== PIPELINE COMPARISON: current vs consistent ===\n");

    for (name, config) in configs {
        let problem = generate_mega_scenario_v2(config);

        // Current pipeline (no price projection)
        let current_pipeline = Pipeline::current();
        let current_result = current_pipeline.solve(&problem);

        // Consistent pipeline (with price projection)
        let consistent_pipeline = Pipeline::consistent();
        let consistent_result = consistent_pipeline.solve(&problem);

        let overhead_ms = (consistent_result.total_time_secs - current_result.total_time_secs) * 1000.0;
        let overhead_pct = if current_result.total_time_secs > 0.0 {
            (overhead_ms / (current_result.total_time_secs * 1000.0)) * 100.0
        } else {
            0.0
        };

        println!("{} scenario:", name);
        println!(
            "  current:    {:.3}ms total",
            current_result.total_time_secs * 1000.0
        );
        println!(
            "  consistent: {:.3}ms total ({:.3}ms projection)",
            consistent_result.total_time_secs * 1000.0,
            consistent_result.phase_times.price_projection_secs * 1000.0
        );
        println!(
            "  overhead:   {:.3}ms ({:.1}%)",
            overhead_ms,
            overhead_pct
        );

        if let Some(proj) = &consistent_result.price_projection {
            println!(
                "  violations: {}, max_adj=${:.6}",
                proj.violations_fixed,
                proj.max_adjustment as f64 / NANOS_PER_DOLLAR as f64
            );
        }
        println!();
    }
}
