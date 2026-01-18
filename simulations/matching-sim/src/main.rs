//! CLI simulation harness for running matching simulations.

use std::time::Instant;

use matching_solver::{CompositeSolver, GreedySolver, MilpSolver, RandomizedGreedySolver, Solver};
use matching_scenarios::{
    generate_adversarial_scenario, generate_conditional_chain_scenario,
    generate_deep_implication_scenario, generate_large_interconnected_scenario,
    generate_liquidity_cliff_scenario, generate_nested_bundle_scenario,
    generate_presidential_scenario, generate_random_scenario, generate_tournament_scenario,
    AdversarialConfig, ConditionalChainConfig, DeepImplicationConfig, LargeInterconnectedConfig,
    LiquidityCliffConfig, NestedBundleConfig, PresidentialConfig, Problem, RandomConfig,
    TournamentConfig,
};

mod metrics;
use metrics::{print_comparison_table, OptimalityMetrics, ScenarioComparison};

/// Which solver(s) to use
#[derive(Clone, Debug, PartialEq)]
pub enum SolverChoice {
    Greedy,
    Milp,
    Randomized,
    Composite,
    All,
}

impl SolverChoice {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "greedy" => Some(Self::Greedy),
            "milp" => Some(Self::Milp),
            "randomized" | "random" => Some(Self::Randomized),
            "composite" => Some(Self::Composite),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// Configuration for the hard matching simulation.
#[derive(Clone, Debug)]
pub struct SimulationConfig {
    pub num_batches: usize,
    pub seed: u64,
    pub scenarios: Vec<String>,
    pub verbose: bool,
    pub solver: SolverChoice,
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
            solver: SolverChoice::Greedy,
        }
    }
}

/// Results from running a solver on a batch.
#[derive(Clone, Debug)]
pub struct SolverResult {
    pub solver_name: String,
    pub welfare: i64,
    pub orders_filled: usize,
    pub total_orders: usize,
}

/// Results from a simulation run.
#[derive(Clone, Debug)]
pub struct SimulationResults {
    pub config: SimulationConfig,
    pub scenarios: Vec<ScenarioComparison>,
    pub solver_comparisons: Vec<SolverComparisonResult>,
    pub elapsed_secs: f64,
}

/// Comparison results across solvers for a scenario.
#[derive(Clone, Debug)]
pub struct SolverComparisonResult {
    pub scenario_name: String,
    pub results: Vec<SolverAggregateResult>,
}

/// Aggregate results for a single solver across batches.
#[derive(Clone, Debug, Default)]
pub struct SolverAggregateResult {
    pub solver_name: String,
    pub total_welfare: i64,
    pub total_filled: usize,
    pub total_orders: usize,
    pub batch_count: usize,
}

impl SolverAggregateResult {
    pub fn mean_welfare(&self) -> f64 {
        if self.batch_count > 0 {
            self.total_welfare as f64 / self.batch_count as f64
        } else {
            0.0
        }
    }

    pub fn fill_rate(&self) -> f64 {
        if self.total_orders > 0 {
            self.total_filled as f64 / self.total_orders as f64
        } else {
            0.0
        }
    }

    pub fn add(&mut self, result: &SolverResult) {
        self.total_welfare += result.welfare;
        self.total_filled += result.orders_filled;
        self.total_orders += result.total_orders;
        self.batch_count += 1;
    }
}

impl SimulationResults {
    pub fn print(&self) {
        println!("\n========================================");
        println!("      MATCHING SIMULATION RESULTS       ");
        println!("========================================\n");

        if self.config.solver == SolverChoice::All && !self.solver_comparisons.is_empty() {
            self.print_solver_comparisons();
        } else {
            print_comparison_table(&self.scenarios);
        }

        println!("\nTotal time: {:.2}s", self.elapsed_secs);
        println!("Batches per scenario: {}", self.config.num_batches);
    }

    fn print_solver_comparisons(&self) {
        for comparison in &self.solver_comparisons {
            println!("Scenario: {}", comparison.scenario_name);
            println!("+------------+------------+----------+----------+");
            println!("| Solver     | Welfare    | Gap      | Fill %   |");
            println!("+------------+------------+----------+----------+");

            // Find the best (MILP) welfare for gap calculation
            let milp_welfare = comparison
                .results
                .iter()
                .find(|r| r.solver_name == "MILP")
                .map(|r| r.mean_welfare())
                .unwrap_or(0.0);

            for result in &comparison.results {
                let mean_welfare = result.mean_welfare();
                let gap = if milp_welfare > 0.0 && result.solver_name != "MILP" {
                    format!("{:.1}%", (milp_welfare - mean_welfare) / milp_welfare * 100.0)
                } else if result.solver_name == "MILP" {
                    "0.0%".to_string()
                } else {
                    "-".to_string()
                };

                println!(
                    "| {:<10} | {:>10.0} | {:>8} | {:>7.1}% |",
                    result.solver_name,
                    mean_welfare,
                    gap,
                    result.fill_rate() * 100.0,
                );
            }

            println!("+------------+------------+----------+----------+\n");
        }
    }
}

fn create_solvers(choice: &SolverChoice, seed: u64) -> Vec<Box<dyn Solver>> {
    match choice {
        SolverChoice::Greedy => vec![Box::new(GreedySolver::new())],
        SolverChoice::Milp => vec![Box::new(MilpSolver::new())],
        SolverChoice::Randomized => vec![Box::new(RandomizedGreedySolver::new(100, seed))],
        SolverChoice::Composite => vec![Box::new(CompositeSolver::new())],
        SolverChoice::All => vec![
            Box::new(MilpSolver::new()),
            Box::new(GreedySolver::new()),
            Box::new(RandomizedGreedySolver::new(100, seed)),
            Box::new(CompositeSolver::new()),
        ],
    }
}

fn calculate_optimality(problem: &Problem, solver: &dyn Solver) -> OptimalityMetrics {
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
        solver_comparisons: Vec::new(),
        elapsed_secs: 0.0,
    };

    for scenario_name in &config.scenarios {
        if config.solver == SolverChoice::All {
            let comparison = run_scenario_all_solvers(&config, scenario_name);
            results.solver_comparisons.push(comparison);
        } else {
            let comparison = run_scenario(&config, scenario_name);
            results.scenarios.push(comparison);
        }
    }

    results.elapsed_secs = start.elapsed().as_secs_f64();
    results
}

fn run_scenario_all_solvers(config: &SimulationConfig, scenario_name: &str) -> SolverComparisonResult {
    let solvers = create_solvers(&SolverChoice::All, config.seed);

    let mut aggregates: Vec<SolverAggregateResult> = solvers
        .iter()
        .map(|s| SolverAggregateResult {
            solver_name: s.name().to_string(),
            ..Default::default()
        })
        .collect();

    for batch in 0..config.num_batches {
        let seed = config.seed + batch as u64;
        let problem = create_problem(scenario_name, seed);

        if config.verbose {
            println!("Running {} batch {} (seed {})", scenario_name, batch, seed);
            println!("{}", problem.summary());
        }

        for (i, solver) in solvers.iter().enumerate() {
            let result = solver.solve(&problem);

            let solver_result = SolverResult {
                solver_name: solver.name().to_string(),
                welfare: result.total_welfare,
                orders_filled: result.orders_filled,
                total_orders: problem.num_orders(),
            };

            aggregates[i].add(&solver_result);

            if config.verbose {
                println!(
                    "  {}: welfare={}, filled={}/{}",
                    solver.name(),
                    result.total_welfare,
                    result.orders_filled,
                    problem.num_orders()
                );
            }
        }
    }

    SolverComparisonResult {
        scenario_name: scenario_name.to_string(),
        results: aggregates,
    }
}

fn run_scenario(config: &SimulationConfig, scenario_name: &str) -> ScenarioComparison {
    let solvers = create_solvers(&config.solver, config.seed);
    let solver = &solvers[0];

    let mut comparison = ScenarioComparison::new(scenario_name);

    for batch in 0..config.num_batches {
        let seed = config.seed + batch as u64;
        let problem = create_problem(scenario_name, seed);

        if config.verbose {
            println!("Running {} batch {} (seed {})", scenario_name, batch, seed);
            println!("{}", problem.summary());
        }

        let metrics = calculate_optimality(&problem, solver.as_ref());
        comparison.add(&metrics);

        if config.verbose {
            println!("{}", metrics);
        }
    }

    comparison
}

fn create_problem(scenario_name: &str, seed: u64) -> Problem {
    match scenario_name {
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
        // Complex scenarios
        "nested-bundles" => generate_nested_bundle_scenario(NestedBundleConfig {
            seed,
            ..Default::default()
        }),
        "conditional-chains" => generate_conditional_chain_scenario(ConditionalChainConfig {
            seed,
            ..Default::default()
        }),
        "deep-implications" => generate_deep_implication_scenario(DeepImplicationConfig {
            seed,
            ..Default::default()
        }),
        "liquidity-cliffs" => generate_liquidity_cliff_scenario(LiquidityCliffConfig {
            seed,
            ..Default::default()
        }),
        "adversarial" => generate_adversarial_scenario(AdversarialConfig {
            seed,
            ..Default::default()
        }),
        "large-interconnected" => generate_large_interconnected_scenario(LargeInterconnectedConfig {
            seed,
            ..Default::default()
        }),
        _ => {
            eprintln!("Unknown scenario: {}, using random-easy", scenario_name);
            generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::easy()
            })
        }
    }
}

/// Run a quick test to verify the system works.
pub fn run_quick_test() {
    println!("Running quick matching test...\n");

    let problem = generate_presidential_scenario(PresidentialConfig::default());
    println!("{}", problem.summary());

    let solvers: Vec<Box<dyn Solver>> = vec![
        Box::new(GreedySolver::new()),
        Box::new(RandomizedGreedySolver::new(50, 42)),
        Box::new(MilpSolver::new()),
        Box::new(CompositeSolver::new()),
    ];

    for solver in &solvers {
        let result = solver.solve(&problem);
        println!("\n{} solver results:", solver.name());
        println!(
            "  Orders filled: {} / {}",
            result.orders_filled,
            problem.num_orders()
        );
        println!("  Total welfare: {}", result.total_welfare);
        println!("  Unfilled (liquidity): {}", result.orders_unfilled_liquidity);
        println!("  Unfilled (AON): {}", result.orders_unfilled_aon);
    }

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
            "--solver" => {
                if i + 1 < args.len() {
                    if let Some(choice) = SolverChoice::from_str(&args[i + 1]) {
                        config.solver = choice;
                    } else {
                        eprintln!("Unknown solver: {}. Using greedy.", args[i + 1]);
                    }
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
                println!("                     Standard scenarios:");
                println!("                       presidential, presidential-hard");
                println!("                       tournament, tournament-large");
                println!("                       random-easy, random-medium, random-hard");
                println!("                     Complex scenarios:");
                println!("                       nested-bundles");
                println!("                       conditional-chains");
                println!("                       deep-implications");
                println!("                       liquidity-cliffs");
                println!("                       adversarial");
                println!("                       large-interconnected");
                println!("  --solver <S>     Solver to use:");
                println!("                     greedy (default)");
                println!("                     milp (optimal via MILP)");
                println!("                     randomized (random order shuffling)");
                println!("                     composite (decomposition + specialized)");
                println!("                     all (compare all solvers)");
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
    println!("  Solver: {:?}", config.solver);
    println!();

    let results = run_simulation(config);
    results.print();
}
