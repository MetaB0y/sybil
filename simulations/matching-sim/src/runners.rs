//! Specialized test runners.
//!
//! Contains functions for running specific tests like quick tests,
//! platform stress tests, and MILP killer tests.

use std::time::Instant;

use matching_scenarios::{
    generate_mega_scenario, generate_milp_killer_scenario, generate_presidential_scenario,
    generate_realistic_scenario, MegaScenarioConfig, MilpKillerConfig, PresidentialConfig,
    RealisticConfig,
};
use matching_solver::{
    CompositeSolver, GreedySolver, MilpSolver, PlatformConfig, RandomizedGreedySolver, Solver,
    SolverPlatform,
};

/// Run a quick test to verify the system works.
pub fn run_quick_test() {
    println!("Running quick matching test...\n");

    let problem = generate_presidential_scenario(PresidentialConfig::default());
    println!("{}", problem.summary());

    let solvers: Vec<Box<dyn Solver>> = vec![
        Box::new(GreedySolver::new()),
        Box::new(RandomizedGreedySolver::new()),
        Box::new(MilpSolver::new()),
        Box::new(CompositeSolver::new()),
        Box::new(SolverPlatform::new()),
    ];

    for solver in &solvers {
        let start = Instant::now();
        let result = solver.solve(&problem);
        let elapsed = start.elapsed().as_secs_f64();

        println!("\n{} solver results:", solver.name());
        println!(
            "  Orders filled: {} / {}",
            result.orders_filled,
            problem.num_orders()
        );
        println!("  Total welfare: {}", result.total_welfare);
        println!(
            "  Unfilled (liquidity): {}",
            result.orders_unfilled_liquidity
        );
        println!("  Unfilled (AON): {}", result.orders_unfilled_aon);
        println!("  Time: {:.3}s", elapsed);
    }

    println!("\nQuick test completed successfully!");
}

/// Run platform stress test.
pub fn run_platform_stress_test(timeout_secs: f64) {
    println!("Running platform stress test...\n");
    println!("MILP timeout: {}s", timeout_secs);

    let problem = generate_mega_scenario(MegaScenarioConfig::medium());
    println!("\n{}", problem.summary());

    println!("\n--- Running individual solvers ---\n");

    // Run greedy
    let start = Instant::now();
    let greedy = GreedySolver::new();
    let greedy_result = greedy.solve(&problem);
    println!(
        "Greedy: welfare={}, fills={}, time={:.3}s",
        greedy_result.total_welfare,
        greedy_result.orders_filled,
        start.elapsed().as_secs_f64()
    );

    // Run MILP with timeout
    let start = Instant::now();
    let milp = MilpSolver::with_timeout(timeout_secs);
    let milp_result = milp.solve_with_status(&problem);
    println!(
        "MILP: welfare={}, fills={}, status={:?}, time={:.3}s",
        milp_result.result.total_welfare,
        milp_result.result.orders_filled,
        milp_result.status,
        start.elapsed().as_secs_f64()
    );

    // Run platform
    println!("\n--- Running platform ---\n");
    let platform_config = PlatformConfig {
        total_time_budget_ms: (timeout_secs * 1000.0 / 0.6) as u64,
        milp_time_fraction: 0.6,
        ..Default::default()
    };
    let platform = SolverPlatform::with_config(platform_config);
    let platform_result = platform.solve(&problem);

    platform_result.print_summary();
}

/// Run realistic scenario test - demonstrates cross-market matching value.
pub fn run_realistic_test(timeout_secs: f64, config_name: &str) {
    println!("Running realistic scenario test...\n");
    println!("Config: {}", config_name);
    println!("MILP timeout: {}s", timeout_secs);

    let config = match config_name {
        "extreme" => RealisticConfig::extreme(),
        "standard" => RealisticConfig::standard(),
        "cross-market" => RealisticConfig::cross_market_demo(),
        "small" => RealisticConfig::small(),
        _ => RealisticConfig::test(),
    };

    let problem = generate_realistic_scenario(config);
    println!("\n{}", problem.summary());

    println!("\n--- Running MILP with timeout ---\n");

    let start = Instant::now();
    let milp = MilpSolver::with_timeout(timeout_secs);
    let (milp_result, dual_analysis) = milp.solve_with_duals(&problem);
    let milp_time = start.elapsed().as_secs_f64();

    println!(
        "MILP: welfare={}, fills={}, status={:?}, time={:.3}s",
        milp_result.result.total_welfare,
        milp_result.result.orders_filled,
        milp_result.status,
        milp_time
    );
    println!("\n{}", dual_analysis.value_summary());

    println!("\n--- Running greedy ---\n");

    let start = Instant::now();
    let greedy = GreedySolver::new();
    let greedy_result = greedy.solve(&problem);
    println!(
        "Greedy: welfare={}, fills={}, time={:.3}s",
        greedy_result.total_welfare,
        greedy_result.orders_filled,
        start.elapsed().as_secs_f64()
    );

    println!("\n--- Running platform with all solvers ---\n");

    let platform_config = PlatformConfig {
        total_time_budget_ms: (timeout_secs * 1000.0 / 0.6) as u64,
        milp_time_fraction: 0.6,
        include_arbitrage: true,
        include_bundle_decomposer: true,
        include_chain_finder: true,
        ..Default::default()
    };
    let platform = SolverPlatform::with_config(platform_config);
    let platform_result = platform.solve(&problem);

    platform_result.print_summary();

    // Print comparison
    print_comparison(
        milp_result.result.total_welfare,
        greedy_result.total_welfare,
        platform_result.result.total_welfare,
    );
}

/// Run MILP killer test - designed to force MILP timeout.
pub fn run_milp_killer_test(timeout_secs: f64, config_name: &str) {
    println!("Running MILP killer test...\n");
    println!("Config: {}", config_name);
    println!("MILP timeout: {}s", timeout_secs);

    let config = match config_name {
        "extreme" => MilpKillerConfig::extreme(),
        "full" => MilpKillerConfig::timeout_guaranteed(),
        _ => MilpKillerConfig::test(),
    };

    let problem = generate_milp_killer_scenario(config);
    println!("\n{}", problem.summary());

    println!("\n--- Running MILP with timeout ---\n");

    let start = Instant::now();
    let milp = MilpSolver::with_timeout(timeout_secs);
    let milp_result = milp.solve_with_status(&problem);
    let milp_time = start.elapsed().as_secs_f64();

    println!(
        "MILP: welfare={}, fills={}, status={:?}, time={:.3}s",
        milp_result.result.total_welfare,
        milp_result.result.orders_filled,
        milp_result.status,
        milp_time
    );

    println!("\n--- Running greedy ---\n");

    let start = Instant::now();
    let greedy = GreedySolver::new();
    let greedy_result = greedy.solve(&problem);
    println!(
        "Greedy: welfare={}, fills={}, time={:.3}s",
        greedy_result.total_welfare,
        greedy_result.orders_filled,
        start.elapsed().as_secs_f64()
    );

    println!("\n--- Running platform with all solvers ---\n");

    let platform_config = PlatformConfig {
        total_time_budget_ms: (timeout_secs * 1000.0 / 0.6) as u64,
        milp_time_fraction: 0.6,
        include_arbitrage: true,
        include_bundle_decomposer: true,
        include_chain_finder: true,
        ..Default::default()
    };
    let platform = SolverPlatform::with_config(platform_config);
    let platform_result = platform.solve(&problem);

    platform_result.print_summary();

    // Print comparison (MILP vs Platform only)
    let milp_welfare = milp_result.result.total_welfare;
    let platform_welfare = platform_result.result.total_welfare;
    let improvement = if milp_welfare > 0 {
        ((platform_welfare as f64 - milp_welfare as f64) / milp_welfare as f64) * 100.0
    } else {
        0.0
    };

    println!("\n========================================");
    println!("         COMPARISON SUMMARY             ");
    println!("========================================\n");

    println!("MILP welfare:     {}", milp_welfare);
    println!("Platform welfare: {}", platform_welfare);
    println!("Improvement:      {:.2}%", improvement);

    if platform_welfare > milp_welfare {
        println!("\n Platform BEATS MILP-with-timeout!");
    } else if platform_welfare == milp_welfare {
        println!("\n= Platform EQUALS MILP-with-timeout");
    } else {
        println!("\n MILP-with-timeout beats platform");
    }
}

/// Print comparison summary for solver results.
fn print_comparison(milp_welfare: i64, greedy_welfare: i64, platform_welfare: i64) {
    println!("\n========================================");
    println!("         COMPARISON SUMMARY             ");
    println!("========================================\n");

    println!("Greedy welfare:   {:>15}", greedy_welfare);
    println!("MILP welfare:     {:>15}", milp_welfare);
    println!("Platform welfare: {:>15}", platform_welfare);

    let milp_vs_greedy = if greedy_welfare > 0 {
        ((milp_welfare as f64 - greedy_welfare as f64) / greedy_welfare as f64) * 100.0
    } else {
        0.0
    };
    let platform_vs_greedy = if greedy_welfare > 0 {
        ((platform_welfare as f64 - greedy_welfare as f64) / greedy_welfare as f64) * 100.0
    } else {
        0.0
    };
    let platform_vs_milp = if milp_welfare > 0 {
        ((platform_welfare as f64 - milp_welfare as f64) / milp_welfare as f64) * 100.0
    } else {
        0.0
    };

    println!("\nMILP vs Greedy:     {:+.1}%", milp_vs_greedy);
    println!("Platform vs Greedy: {:+.1}%", platform_vs_greedy);
    println!("Platform vs MILP:   {:+.1}%", platform_vs_milp);

    if platform_welfare > milp_welfare {
        println!("\n Platform BEATS MILP-with-timeout!");
    } else if platform_welfare == milp_welfare {
        println!("\n= Platform EQUALS MILP-with-timeout");
    } else {
        println!("\n MILP-with-timeout beats platform");
    }
}
