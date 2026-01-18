//! CLI simulation harness for running matching simulations.

use std::time::Instant;

use matching_solver::{GreedySolver, Solver};
use matching_scenarios::{
    Problem, generate_presidential_scenario, generate_tournament_scenario,
    generate_random_scenario, PresidentialConfig, TournamentConfig, RandomConfig,
};

mod metrics;
use metrics::{OptimalityMetrics, ScenarioComparison, print_comparison_table};

/// Configuration for the hard matching simulation.
#[derive(Clone, Debug)]
pub struct SimulationConfig {
    pub num_batches: usize,
    pub seed: u64,
    pub scenarios: Vec<String>,
    pub verbose: bool,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            num_batches: 20,
            seed: 42,
            scenarios: vec![
                "presidential".to_string(),
                "tournament".to_string(),
                "random-easy".to_string(),
                "random-hard".to_string(),
            ],
            verbose: false,
        }
    }
}

/// Results from a simulation run.
#[derive(Clone, Debug)]
pub struct SimulationResults {
    pub config: SimulationConfig,
    pub scenarios: Vec<ScenarioComparison>,
    pub elapsed_secs: f64,
}

impl SimulationResults {
    pub fn print(&self) {
        println!("\n========================================");
        println!("      MATCHING SIMULATION RESULTS       ");
        println!("========================================\n");

        print_comparison_table(&self.scenarios);

        println!("\nTotal time: {:.2}s", self.elapsed_secs);
        println!("Batches per scenario: {}", self.config.num_batches);
    }
}

fn calculate_optimality(problem: &Problem) -> OptimalityMetrics {
    let solver = GreedySolver::new();
    let result = solver.solve(problem);

    OptimalityMetrics::from_greedy_only(
        result.total_welfare,
        result.orders_filled,
        result.orders_unfilled_liquidity,
        result.orders_unfilled_aon,
        problem.num_orders(),
    )
}

pub fn run_simulation(config: SimulationConfig) -> SimulationResults {
    let start = Instant::now();
    let mut results = SimulationResults {
        config: config.clone(),
        scenarios: Vec::new(),
        elapsed_secs: 0.0,
    };

    for scenario_name in &config.scenarios {
        let comparison = run_scenario(&config, scenario_name);
        results.scenarios.push(comparison);
    }

    results.elapsed_secs = start.elapsed().as_secs_f64();
    results
}

fn run_scenario(config: &SimulationConfig, scenario_name: &str) -> ScenarioComparison {
    let mut comparison = ScenarioComparison::new(scenario_name);

    for batch in 0..config.num_batches {
        let seed = config.seed + batch as u64;

        let problem = match scenario_name {
            "presidential" => generate_presidential_scenario(PresidentialConfig {
                seed,
                ..Default::default()
            }),
            "presidential-hard" => generate_presidential_scenario(PresidentialConfig {
                seed,
                num_simple_orders: 50,
                num_bundle_orders: 20,
                num_conditional_orders: 10,
                liquidity_multiplier: 0.3,
                ..Default::default()
            }),
            "tournament" => generate_tournament_scenario(TournamentConfig {
                seed,
                ..Default::default()
            }),
            "tournament-large" => generate_tournament_scenario(TournamentConfig {
                seed,
                num_teams: 16,
                orders_per_team: 8,
                liquidity_multiplier: 0.3,
            }),
            "random-easy" => generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::easy()
            }),
            "random-medium" => generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::medium()
            }),
            "random-hard" => generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::hard()
            }),
            _ => {
                println!("Unknown scenario: {}", scenario_name);
                continue;
            }
        };

        if config.verbose {
            println!("Running {} batch {} (seed {})", scenario_name, batch, seed);
            println!("{}", problem.summary());
        }

        let metrics = calculate_optimality(&problem);
        comparison.add(&metrics);

        if config.verbose {
            println!("{}", metrics);
        }
    }

    comparison
}

/// Run a quick test to verify the system works.
pub fn run_quick_test() {
    println!("Running quick matching test...\n");

    let problem = generate_presidential_scenario(PresidentialConfig::default());
    println!("{}", problem.summary());

    let solver = GreedySolver::new();
    let result = solver.solve(&problem);

    println!("\nGreedy solver results:");
    println!("  Orders filled: {} / {}", result.orders_filled, problem.num_orders());
    println!("  Total welfare: {}", result.total_welfare);
    println!("  Unfilled (liquidity): {}", result.orders_unfilled_liquidity);
    println!("  Unfilled (AON): {}", result.orders_unfilled_aon);

    println!("\nQuick test completed successfully!");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "--quick" {
        run_quick_test();
        return;
    }

    let mut config = SimulationConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--batches" => {
                if i + 1 < args.len() {
                    config.num_batches = args[i + 1].parse().unwrap_or(20);
                    i += 1;
                }
            }
            "--seed" => {
                if i + 1 < args.len() {
                    config.seed = args[i + 1].parse().unwrap_or(42);
                    i += 1;
                }
            }
            "--scenario" => {
                if i + 1 < args.len() {
                    config.scenarios = vec![args[i + 1].clone()];
                    i += 1;
                }
            }
            "--verbose" | "-v" => {
                config.verbose = true;
            }
            "--help" | "-h" => {
                println!("Matching Simulation\n");
                println!("Usage: matching-sim [OPTIONS]\n");
                println!("Options:");
                println!("  --batches <N>    Number of batches per scenario (default: 20)");
                println!("  --seed <N>       Random seed (default: 42)");
                println!("  --scenario <S>   Run specific scenario:");
                println!("                     presidential, presidential-hard");
                println!("                     tournament, tournament-large");
                println!("                     random-easy, random-medium, random-hard");
                println!("  --verbose, -v    Show detailed output");
                println!("  --quick          Run a quick test");
                println!("  --help, -h       Show this help message");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    println!("========================================");
    println!("       MATCHING SIMULATION              ");
    println!("========================================\n");

    println!("Configuration:");
    println!("  Batches per scenario: {}", config.num_batches);
    println!("  Seed: {}", config.seed);
    println!("  Scenarios: {:?}", config.scenarios);
    println!();

    let results = run_simulation(config);
    results.print();
}
