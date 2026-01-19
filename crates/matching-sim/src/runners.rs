//! Specialized test runners.
//!
//! Contains functions for running specific tests like quick tests,
//! platform stress tests, and MILP killer tests.

use std::time::Instant;

use matching_scenarios::{
    generate_mega_scenario, generate_milp_killer_scenario, generate_random_scenario,
    MegaScenarioConfig, MilpKillerConfig, RandomConfig,
};
use matching_solver::{
    GreedySolver, MilpSolver, PlatformConfig, RandomizedGreedySolver, Solver,
    SolverPlatform,
};

/// Run a quick test to verify the system works.
pub fn run_quick_test() {
    println!("Running quick matching test...\n");

    let problem = generate_random_scenario(RandomConfig::easy());
    println!("{}", problem.summary());

    let solvers: Vec<Box<dyn Solver>> = vec![
        Box::new(GreedySolver::new()),
        Box::new(RandomizedGreedySolver::new()),
        Box::new(MilpSolver::new()),
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
