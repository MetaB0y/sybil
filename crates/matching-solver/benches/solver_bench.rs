//! Divan benchmarks for matching solver components.
//!
//! # ⚠️ Benchmark Caveats
//!
//! The local_solver is a HEURISTIC (O(n log n)), not an optimal solver (O(n³)).
//! See local_solver.rs documentation for limitations.
//!
//! These benchmarks measure:
//! 1. Speed of the heuristic (which is fast but not optimal)
//! 2. MM allocation with Lagrangian relaxation
//!
//! # Scenarios
//!
//! - small: ~200-500 orders (quick validation)
//! - medium: ~1,500-4,500 orders (plan target)
//! - large: ~10,000-30,000 orders (stress testing)

use divan::Bencher;
use matching_engine::{
    outcome_sell, simple_yes_buy, MmConstraint, MmId, MmSide, Problem, NANOS_PER_DOLLAR,
};
use matching_scenarios::{generate_scenario, ScenarioConfig};
use matching_solver::{
    local_solver::{LocalSolver, MarketSolution},
    mm_allocator::MmAllocator,
    BenchmarkHarness, Pipeline,
};
use std::collections::HashMap;

fn main() {
    divan::main();
}

// ============================================================================
// Per-Market Clearing Benchmarks
// ============================================================================

#[divan::bench(args = [100, 500, 1000, 2000, 5000])]
fn bench_local_solver_single_market(bencher: Bencher, order_count: usize) {
    // Setup: Create a single market with N orders
    let mut problem = Problem::new("bench");
    let market_id = problem.markets.add_binary("test");

    // Add sell orders as supply
    problem.orders.push(outcome_sell(
        &problem.markets,
        9_000_000,
        market_id,
        0,
        500_000_000,
        100_000,
    ));
    problem.orders.push(outcome_sell(
        &problem.markets,
        9_000_001,
        market_id,
        1,
        500_000_000,
        100_000,
    ));

    // Add orders: varying prices
    for i in 0..order_count {
        let price = ((400 + (i % 200)) as u64) * 1_000_000; // 0.40 to 0.60
        let qty = 10 + (i % 100) as u64;

        problem.orders.push(simple_yes_buy(
            &problem.markets,
            i as u64,
            market_id,
            price,
            qty,
        ));
    }

    let solver = LocalSolver::new();

    bencher.bench_local(|| solver.solve_market(market_id, &problem.markets, &problem.orders));
}

/// Benchmark solving all markets with realistic order counts.
/// Uses 100-200 orders per market (matching plan's orders_per_market range).
#[divan::bench(args = [10, 50, 100, 200])]
fn bench_solve_all_markets(bencher: Bencher, market_count: usize) {
    let mut problem = Problem::new("bench");
    let orders_per_market = 150; // Middle of plan's 100-200 range
    let mut liq_id = 9_000_000u64;

    for m in 0..market_count {
        let market_id = problem.markets.add_binary(&format!("market_{}", m));

        // Add sell orders as supply
        problem.orders.push(outcome_sell(
            &problem.markets,
            liq_id,
            market_id,
            0,
            500_000_000,
            50_000,
        ));
        liq_id += 1;
        problem.orders.push(outcome_sell(
            &problem.markets,
            liq_id,
            market_id,
            1,
            500_000_000,
            50_000,
        ));
        liq_id += 1;

        // Add orders
        for i in 0..orders_per_market {
            let order_id = (m * orders_per_market + i) as u64;
            let price = ((400 + (i % 200)) as u64) * 1_000_000;
            let qty = 10 + (i % 100) as u64;

            problem.orders.push(simple_yes_buy(
                &problem.markets,
                order_id,
                market_id,
                price,
                qty,
            ));
        }
    }

    // Total orders: market_count × 150
    // 10 markets = 1,500 orders
    // 50 markets = 7,500 orders
    // 100 markets = 15,000 orders
    // 200 markets = 30,000 orders

    let solver = LocalSolver::new();

    bencher.bench_local(|| {
        let mut results: HashMap<_, MarketSolution> = HashMap::new();
        for market in problem.markets.iter() {
            let solution = solver.solve_market(market.id, &problem.markets, &problem.orders);
            results.insert(market.id, solution);
        }
        results
    });
}

// ============================================================================
// MM Allocator Benchmarks
// ============================================================================

#[divan::bench(args = [1, 3, 5, 10])]
fn bench_mm_allocation(bencher: Bencher, mm_count: usize) {
    // Setup: Create problem with multiple MMs
    let mut problem = Problem::new("bench");
    let market_count = 50;
    let orders_per_mm = 200; // More realistic: each MM has ~200 orders

    // Create markets
    let mut market_ids = Vec::new();
    for m in 0..market_count {
        market_ids.push(problem.markets.add_binary(&format!("market_{}", m)));
    }

    // Create MMs with their orders
    for mm_idx in 0..mm_count {
        let mut mm = MmConstraint::new(MmId::new(mm_idx as u64), 100 * NANOS_PER_DOLLAR);

        // Add MM orders
        for i in 0..orders_per_mm {
            let order_id = (mm_idx * orders_per_mm + i) as u64;
            let market_id = market_ids[i % market_count];
            let price = ((400 + (i % 200)) as u64) * 1_000_000;
            let qty = 10 + (i % 50) as u64;

            problem.orders.push(simple_yes_buy(
                &problem.markets,
                order_id,
                market_id,
                price,
                qty,
            ));

            mm.add_order(order_id, MmSide::BuyYes);
        }

        problem.mm_constraints.push(mm);
    }

    // Total orders: mm_count × 200
    // 1 MM = 200 orders
    // 5 MMs = 1,000 orders
    // 10 MMs = 2,000 orders

    // Setup prices (uniform)
    let mut prices = HashMap::new();
    for &market_id in &market_ids {
        prices.insert(market_id, vec![500_000_000u64, 500_000_000u64]);
    }

    // Provide fills (price, qty) for each order
    let fills: HashMap<u64, (u64, u64)> = problem
        .orders
        .iter()
        .map(|o| (o.id, (500_000_000u64, o.max_fill)))
        .collect();

    let allocator = MmAllocator::new();

    bencher.bench_local(|| {
        allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &fills)
    });
}

// ============================================================================
// Mega Scenario Benchmarks (Generation Only)
// ============================================================================

#[divan::bench]
fn bench_mega_scenario_generation_small() {
    // ~200-500 total orders
    let config = ScenarioConfig::small();
    let _ = generate_scenario(config);
}

#[divan::bench]
fn bench_mega_scenario_generation_medium() {
    // ~1,500-4,500 total orders (plan target)
    let config = ScenarioConfig::medium();
    let _ = generate_scenario(config);
}

#[divan::bench]
fn bench_mega_scenario_generation_large() {
    // ~10,000-30,000 total orders (stress test)
    let config = ScenarioConfig::large();
    let _ = generate_scenario(config);
}

// ============================================================================
// Full Pipeline Benchmarks (Generation + Clearing + MM Allocation)
// ============================================================================

fn run_full_pipeline(problem: &Problem) {
    // Phase 1: Per-market clearing (heuristic)
    let solver = LocalSolver::new();
    let mut market_solutions: HashMap<_, _> = HashMap::new();

    for market in problem.markets.iter() {
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders);
        market_solutions.insert(market.id, solution);
    }

    // Phase 2: MM allocation
    let mut prices = HashMap::new();
    for (market_id, solution) in &market_solutions {
        prices.insert(*market_id, solution.prices.clone());
    }

    let fills: HashMap<u64, (u64, u64)> = problem
        .orders
        .iter()
        .map(|o| (o.id, (500_000_000u64, o.max_fill)))
        .collect();

    let allocator = MmAllocator::new();
    let _ = allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &fills);
}

#[divan::bench]
fn bench_full_pipeline_small() {
    // ~200-500 total orders
    let config = ScenarioConfig::small();
    let problem = generate_scenario(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_medium() {
    // ~1,500-4,500 total orders (plan target)
    let config = ScenarioConfig::medium();
    let problem = generate_scenario(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_large() {
    // ~10,000-30,000 total orders (stress test)
    let config = ScenarioConfig::large();
    let problem = generate_scenario(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_extreme() {
    // ~40,000-100,000 total orders (extreme stress)
    let config = ScenarioConfig::extreme();
    let problem = generate_scenario(config);
    run_full_pipeline(&problem);
}

// ============================================================================
// Pipeline Architecture Benchmarks
// ============================================================================

#[divan::bench]
fn bench_pipeline_current_small() {
    let config = ScenarioConfig::small();
    let problem = generate_scenario(config);
    let pipeline = Pipeline::current();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_current_medium() {
    let config = ScenarioConfig::medium();
    let problem = generate_scenario(config);
    let pipeline = Pipeline::current();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_full_platform_small() {
    let config = ScenarioConfig::small();
    let problem = generate_scenario(config);
    let pipeline = Pipeline::full_platform();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_full_platform_medium() {
    let config = ScenarioConfig::medium();
    let problem = generate_scenario(config);
    let pipeline = Pipeline::full_platform();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_iterative_small() {
    let config = ScenarioConfig::small();
    let problem = generate_scenario(config);
    let pipeline = Pipeline::iterative();
    let _ = pipeline.solve(&problem);
}

// ============================================================================
// BenchmarkHarness Demonstration
// ============================================================================

/// Demonstrates using the BenchmarkHarness for pipeline comparison.
/// This is a one-shot comparison, not a repeated benchmark.
#[divan::bench]
fn bench_harness_comparison() {
    let mut harness = BenchmarkHarness::new();

    // Add scenarios
    harness.add_scenario("small", generate_scenario(ScenarioConfig::small()));

    // Add pipelines to compare
    harness.add_pipeline("current", Pipeline::current());
    harness.add_pipeline("full_platform", Pipeline::full_platform());

    // Run comparison
    let _results = harness.run();
    // In real usage, you'd call harness.report(&results) to print the comparison
}

// ============================================================================
// Comparative Benchmark: Pipeline on medium with high bundles
// ============================================================================

use std::sync::OnceLock;

#[divan::bench]
fn bench_pipeline_medium_high_bundles(bencher: Bencher) {
    static PROBLEM: OnceLock<Problem> = OnceLock::new();
    let problem = PROBLEM.get_or_init(|| {
        let mut config = ScenarioConfig::medium();
        config.bundle_fraction = 0.30;
        generate_scenario(config)
    });

    bencher.bench_local(|| {
        let pipeline = Pipeline::current();
        pipeline.solve(problem)
    });
}

// Validation tests moved to tests/validation.rs
// Run with: cargo test -p matching-solver --test validation
