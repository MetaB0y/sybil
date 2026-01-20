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
use matching_engine::{simple_yes_buy, MmConstraint, MmId, MmSide, Problem, NANOS_PER_DOLLAR};
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};
use matching_solver::{
    local_solver::{LocalSolver, MarketSolution},
    mm_allocator::MmAllocator,
    Pipeline, BenchmarkHarness,
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

    // Add liquidity
    problem.liquidity.add_ask(market_id, 0, 500_000_000, 100_000);
    problem.liquidity.add_ask(market_id, 1, 500_000_000, 100_000);

    // Add orders: varying prices
    for i in 0..order_count {
        let price = ((400 + (i % 200)) as u64) * 1_000_000; // 0.40 to 0.60
        let qty = 10 + (i % 100) as u64;

        problem
            .orders
            .push(simple_yes_buy(&problem.markets, i as u64, market_id, price, qty));
    }

    let solver = LocalSolver::new();
    let book = problem
        .liquidity
        .books
        .get(&(market_id, 0))
        .cloned()
        .unwrap();

    bencher.bench_local(|| {
        solver.solve_market(market_id, &problem.markets, &problem.orders, &book)
    });
}

/// Benchmark solving all markets with realistic order counts.
/// Uses 100-200 orders per market (matching plan's orders_per_market range).
#[divan::bench(args = [10, 50, 100, 200])]
fn bench_solve_all_markets(bencher: Bencher, market_count: usize) {
    let mut problem = Problem::new("bench");
    let orders_per_market = 150; // Middle of plan's 100-200 range

    for m in 0..market_count {
        let market_id = problem.markets.add_binary(&format!("market_{}", m));

        // Add liquidity
        problem.liquidity.add_ask(market_id, 0, 500_000_000, 50_000);
        problem.liquidity.add_ask(market_id, 1, 500_000_000, 50_000);

        // Add orders
        for i in 0..orders_per_market {
            let order_id = (m * orders_per_market + i) as u64;
            let price = ((400 + (i % 200)) as u64) * 1_000_000;
            let qty = 10 + (i % 100) as u64;

            problem
                .orders
                .push(simple_yes_buy(&problem.markets, order_id, market_id, price, qty));
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
            let book = problem
                .liquidity
                .books
                .get(&(market.id, 0))
                .cloned()
                .unwrap();
            let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);
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

    // Compute welfare for each order (simplified: just use 1 for each)
    let welfare: HashMap<u64, i64> = problem.orders.iter().map(|o| (o.id, 1i64)).collect();

    let allocator = MmAllocator::new();

    bencher.bench_local(|| {
        allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &welfare)
    });
}

// ============================================================================
// Mega Scenario Benchmarks (Generation Only)
// ============================================================================

#[divan::bench]
fn bench_mega_scenario_generation_small() {
    // ~200-500 total orders
    let config = MegaScenarioConfigV2::small();
    let _ = generate_mega_scenario_v2(config);
}

#[divan::bench]
fn bench_mega_scenario_generation_medium() {
    // ~1,500-4,500 total orders (plan target)
    let config = MegaScenarioConfigV2::medium();
    let _ = generate_mega_scenario_v2(config);
}

#[divan::bench]
fn bench_mega_scenario_generation_large() {
    // ~10,000-30,000 total orders (stress test)
    let config = MegaScenarioConfigV2::large();
    let _ = generate_mega_scenario_v2(config);
}

// ============================================================================
// Full Pipeline Benchmarks (Generation + Clearing + MM Allocation)
// ============================================================================

fn run_full_pipeline(problem: &Problem) {
    // Phase 1: Per-market clearing (heuristic)
    let solver = LocalSolver::new();
    let mut market_solutions: HashMap<_, _> = HashMap::new();

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solver.solve_market(market.id, &problem.markets, &problem.orders, &book);
        market_solutions.insert(market.id, solution);
    }

    // Phase 2: MM allocation
    let mut prices = HashMap::new();
    for (market_id, solution) in &market_solutions {
        prices.insert(*market_id, solution.prices.clone());
    }

    let welfare: HashMap<u64, i64> = problem.orders.iter().map(|o| (o.id, 1i64)).collect();

    let allocator = MmAllocator::new();
    let _ = allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &welfare);
}

fn run_full_pipeline_lp(problem: &Problem) {
    use matching_solver::solve_market_lp;

    // Phase 1: Per-market clearing (LP-based with unified liquidity)
    let mut market_solutions: HashMap<_, _> = HashMap::new();

    for market in problem.markets.iter() {
        let book = problem
            .liquidity
            .books
            .get(&(market.id, 0))
            .cloned()
            .unwrap_or_else(|| matching_engine::LiquidityBook::new(market.id, 0));
        let solution = solve_market_lp(market.id, &problem.markets, &problem.orders, &book);
        market_solutions.insert(market.id, solution);
    }

    // Phase 2: MM allocation
    let mut prices = HashMap::new();
    for (market_id, solution) in &market_solutions {
        prices.insert(*market_id, solution.prices.clone());
    }

    let welfare: HashMap<u64, i64> = problem.orders.iter().map(|o| (o.id, 1i64)).collect();

    let allocator = MmAllocator::new();
    let _ = allocator.allocate(&problem.mm_constraints, &prices, &problem.orders, &welfare);
}

#[divan::bench]
fn bench_full_pipeline_small() {
    // ~200-500 total orders
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_medium() {
    // ~1,500-4,500 total orders (plan target)
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_large() {
    // ~10,000-30,000 total orders (stress test)
    let config = MegaScenarioConfigV2::large();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline(&problem);
}

#[divan::bench]
fn bench_full_pipeline_extreme() {
    // ~40,000-100,000 total orders (extreme stress)
    let config = MegaScenarioConfigV2::extreme();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline(&problem);
}

// ============================================================================
// LP Pipeline Benchmarks (using solve_market_lp with unified liquidity)
// ============================================================================

#[divan::bench]
fn bench_lp_pipeline_small() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline_lp(&problem);
}

#[divan::bench]
fn bench_lp_pipeline_medium() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);
    run_full_pipeline_lp(&problem);
}

// ============================================================================
// Pipeline Architecture Benchmarks
// ============================================================================

#[divan::bench]
fn bench_pipeline_current_small() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);
    let pipeline = Pipeline::current();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_current_medium() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);
    let pipeline = Pipeline::current();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_full_platform_small() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);
    let pipeline = Pipeline::full_platform();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_full_platform_medium() {
    let config = MegaScenarioConfigV2::medium();
    let problem = generate_mega_scenario_v2(config);
    let pipeline = Pipeline::full_platform();
    let _ = pipeline.solve(&problem);
}

#[divan::bench]
fn bench_pipeline_iterative_small() {
    let config = MegaScenarioConfigV2::small();
    let problem = generate_mega_scenario_v2(config);
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
    harness.add_scenario("small", generate_mega_scenario_v2(MegaScenarioConfigV2::small()));

    // Add pipelines to compare
    harness.add_pipeline("current", Pipeline::current());
    harness.add_pipeline("full_platform", Pipeline::full_platform());

    // Run comparison
    let _results = harness.run();
    // In real usage, you'd call harness.report(&results) to print the comparison
}

// Validation tests moved to tests/validation.rs
// Run with: cargo test -p matching-solver --test validation
