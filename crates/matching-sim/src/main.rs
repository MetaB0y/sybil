//! CLI simulation harness for matching engine testing.
//!
//! # Usage
//!
//! ```bash
//! # Run quick test
//! matching-sim --preset quick
//!
//! # Run with specific solver
//! matching-sim --preset medium --solver pipeline
//!
//! # Compare all solvers
//! matching-sim --preset small --solver all
//!
//! # Custom configuration
//! matching-sim --markets 50 --orders 5000 --bundles 0.2 --aon 0.3
//!
//! # MILP with timeout
//! matching-sim --preset milp-killer --solver milp --milp-timeout 1.0
//! ```

use std::time::Instant;

use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

use matching_scenarios::{generate_scenario, ScenarioConfig};
use matching_solver::{GreedySolver, MilpSolver, Pipeline, Solver};


fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let config = parse_scenario_config(&args);
    let solver_choice = parse_solver_choice(&args);
    let milp_timeout = parse_milp_timeout(&args);
    let num_batches = parse_batches(&args);
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    println!("========================================");
    println!("       MATCHING SIMULATION              ");
    println!("========================================\n");

    println!("Configuration:");
    println!("  Markets: {}", config.num_markets);
    println!("  Orders: {}", config.num_orders);
    println!("  Bundles: {:.0}%", config.bundle_fraction * 100.0);
    println!("  AON: {:.0}%", config.aon_fraction * 100.0);
    println!("  Solver: {:?}", solver_choice);
    println!("  Batches: {}", num_batches);
    if let Some(timeout) = milp_timeout {
        println!("  MILP timeout: {}s", timeout);
    }
    println!();

    let start = Instant::now();
    let results = run_simulation(&config, &solver_choice, milp_timeout, num_batches, verbose);
    let elapsed = start.elapsed().as_secs_f64();

    print_results(&results, &solver_choice);
    println!("\nTotal time: {:.2}s", elapsed);
}

fn print_help() {
    println!("Matching Simulation\n");
    println!("Usage: matching-sim [OPTIONS]\n");
    println!("Presets:");
    println!("  --preset <NAME>      Use a preset configuration:");
    println!("                         quick      ~50 orders, fast");
    println!("                         small      ~300 orders");
    println!("                         medium     ~3000 orders");
    println!("                         large      ~10000 orders");
    println!("                         extreme    ~50000 orders");
    println!("                         milp-killer Forces MILP timeout");
    println!();
    println!("Custom configuration:");
    println!("  --markets <N>        Number of markets");
    println!("  --orders <N>         Number of orders");
    println!("  --bundles <F>        Bundle fraction (0.0-1.0)");
    println!("  --spreads <F>        Spread fraction (0.0-1.0)");
    println!("  --aon <F>            All-or-none fraction (0.0-1.0)");
    println!("  --scarcity <F>       Liquidity scarcity (0.0-1.0, lower=scarcer)");
    println!("  --mms <N>            Number of market makers");
    println!();
    println!("Solver options:");
    println!("  --solver <S>         Solver to use:");
    println!("                         greedy (default)");
    println!("                         milp");
    println!("                         pipeline");
    println!("                         all (compare all)");
    println!("  --milp-timeout <S>   MILP time limit in seconds");
    println!();
    println!("Other options:");
    println!("  --batches <N>        Number of batches to run (default: 5)");
    println!("  --seed <N>           Random seed (default: 42)");
    println!("  --verbose, -v        Show detailed output");
    println!("  --help, -h           Show this help message");
}

fn parse_scenario_config(args: &[String]) -> ScenarioConfig {
    // Check for preset first
    if let Some(preset) = get_arg_value(args, "--preset") {
        let mut config = match preset.as_str() {
            "quick" => ScenarioConfig::quick(),
            "small" => ScenarioConfig::small(),
            "medium" => ScenarioConfig::medium(),
            "large" => ScenarioConfig::large(),
            "extreme" => ScenarioConfig::extreme(),
            "milp-killer" | "milp_killer" => ScenarioConfig::milp_killer(),
            _ => {
                eprintln!("Unknown preset: {}, using medium", preset);
                ScenarioConfig::medium()
            }
        };

        // Allow overriding preset values
        if let Some(seed) = get_arg_value(args, "--seed") {
            config.seed = seed.parse().unwrap_or(42);
        }

        return config;
    }

    // Build custom config
    let mut config = ScenarioConfig::default();

    if let Some(v) = get_arg_value(args, "--seed") {
        config.seed = v.parse().unwrap_or(42);
    }
    if let Some(v) = get_arg_value(args, "--markets") {
        config.num_markets = v.parse().unwrap_or(30);
    }
    if let Some(v) = get_arg_value(args, "--orders") {
        config.num_orders = v.parse().unwrap_or(1000);
    }
    if let Some(v) = get_arg_value(args, "--bundles") {
        config.bundle_fraction = v.parse().unwrap_or(0.15);
    }
    if let Some(v) = get_arg_value(args, "--spreads") {
        config.spread_fraction = v.parse().unwrap_or(0.05);
    }
    if let Some(v) = get_arg_value(args, "--aon") {
        config.aon_fraction = v.parse().unwrap_or(0.1);
    }
    if let Some(v) = get_arg_value(args, "--scarcity") {
        config.liquidity_scarcity = v.parse().unwrap_or(0.5);
    }
    if let Some(v) = get_arg_value(args, "--mms") {
        config.num_mms = v.parse().unwrap_or(2);
    }

    config
}

#[derive(Clone, Debug, PartialEq)]
enum SolverChoice {
    Greedy,
    Milp,
    Pipeline,
    All,
}

fn parse_solver_choice(args: &[String]) -> SolverChoice {
    match get_arg_value(args, "--solver").as_deref() {
        Some("greedy") => SolverChoice::Greedy,
        Some("milp") => SolverChoice::Milp,
        Some("pipeline") => SolverChoice::Pipeline,
        Some("all") => SolverChoice::All,
        _ => SolverChoice::Greedy,
    }
}

fn parse_milp_timeout(args: &[String]) -> Option<f64> {
    get_arg_value(args, "--milp-timeout").and_then(|v| v.parse().ok())
}

fn parse_batches(args: &[String]) -> usize {
    get_arg_value(args, "--batches")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

fn get_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn create_solvers(choice: &SolverChoice, milp_timeout: Option<f64>) -> Vec<Box<dyn Solver>> {
    match choice {
        SolverChoice::Greedy => vec![Box::new(GreedySolver::new())],
        SolverChoice::Milp => {
            if let Some(timeout) = milp_timeout {
                vec![Box::new(MilpSolver::with_timeout(timeout))]
            } else {
                vec![Box::new(MilpSolver::new())]
            }
        }
        SolverChoice::Pipeline => vec![Box::new(Pipeline::current())],
        SolverChoice::All => {
            let milp: Box<dyn Solver> = if let Some(timeout) = milp_timeout {
                Box::new(MilpSolver::with_timeout(timeout))
            } else {
                Box::new(MilpSolver::with_timeout(5.0)) // Default 5s for comparison
            };
            vec![
                Box::new(GreedySolver::new()),
                milp,
                Box::new(Pipeline::current()),
            ]
        }
    }
}

#[derive(Default)]
struct SolverResults {
    name: String,
    total_welfare: i64,
    total_filled: usize,
    total_orders: usize,
    total_time_secs: f64,
    batches: usize,
}

impl SolverResults {
    fn mean_welfare(&self) -> f64 {
        if self.batches > 0 {
            self.total_welfare as f64 / self.batches as f64
        } else {
            0.0
        }
    }

    fn fill_rate(&self) -> f64 {
        if self.total_orders > 0 {
            self.total_filled as f64 / self.total_orders as f64 * 100.0
        } else {
            0.0
        }
    }

    fn mean_time(&self) -> f64 {
        if self.batches > 0 {
            self.total_time_secs / self.batches as f64
        } else {
            0.0
        }
    }
}

fn run_simulation(
    base_config: &ScenarioConfig,
    solver_choice: &SolverChoice,
    milp_timeout: Option<f64>,
    num_batches: usize,
    verbose: bool,
) -> Vec<SolverResults> {
    let solvers = create_solvers(solver_choice, milp_timeout);

    let mut results: Vec<SolverResults> = solvers
        .iter()
        .map(|s| SolverResults {
            name: s.name().to_string(),
            ..Default::default()
        })
        .collect();

    for batch in 0..num_batches {
        let config = ScenarioConfig {
            seed: base_config.seed + batch as u64,
            ..base_config.clone()
        };

        let problem = generate_scenario(config);

        if verbose {
            println!("Batch {} (seed {})", batch, base_config.seed + batch as u64);
            println!("{}", problem.summary());
        }

        for (i, solver) in solvers.iter().enumerate() {
            let start = Instant::now();
            let result = solver.solve(&problem);
            let elapsed = start.elapsed().as_secs_f64();

            results[i].total_welfare += result.total_welfare;
            results[i].total_filled += result.orders_filled;
            results[i].total_orders += problem.num_orders();
            results[i].total_time_secs += elapsed;
            results[i].batches += 1;

            if verbose {
                println!(
                    "  {}: welfare={}, filled={}/{}, time={:.3}s",
                    solver.name(),
                    result.total_welfare,
                    result.orders_filled,
                    problem.num_orders(),
                    elapsed
                );
            }
        }

        if verbose {
            println!();
        }
    }

    results
}

fn print_results(results: &[SolverResults], choice: &SolverChoice) {
    println!("\n========================================");
    println!("              RESULTS                   ");
    println!("========================================\n");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Solver", "Welfare", "Fill %", "Time (avg)"]);

    // Find best welfare for gap calculation
    let best_welfare = results.iter().map(|r| r.mean_welfare()).fold(0.0, f64::max);

    for result in results {
        let welfare = result.mean_welfare();
        let gap = if best_welfare > 0.0 {
            (best_welfare - welfare) / best_welfare * 100.0
        } else {
            0.0
        };

        let welfare_str = if gap < 0.1 {
            format!("{:.0}", welfare)
        } else {
            format!("{:.0} (-{:.1}%)", welfare, gap)
        };

        let welfare_cell = if gap < 0.1 {
            Cell::new(&welfare_str).fg(Color::Green)
        } else if gap < 5.0 {
            Cell::new(&welfare_str).fg(Color::Yellow)
        } else {
            Cell::new(&welfare_str).fg(Color::Red)
        };

        let fill_rate = result.fill_rate();
        let fill_cell = if fill_rate >= 90.0 {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Green)
        } else if fill_rate >= 70.0 {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Yellow)
        } else {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Red)
        };

        table.add_row(vec![
            Cell::new(&result.name),
            welfare_cell,
            fill_cell,
            Cell::new(format!("{:.3}s", result.mean_time())),
        ]);
    }

    println!("{table}");

    if *choice == SolverChoice::All && results.len() >= 2 {
        println!();
        let greedy = results.iter().find(|r| r.name == "Greedy");
        let pipeline = results.iter().find(|r| r.name.contains("Pipeline") || r.name.contains("Current"));

        if let (Some(g), Some(p)) = (greedy, pipeline) {
            let improvement = if g.mean_welfare() > 0.0 {
                (p.mean_welfare() - g.mean_welfare()) / g.mean_welfare() * 100.0
            } else {
                0.0
            };
            println!(
                "Pipeline vs Greedy: {:+.1}% welfare improvement",
                improvement
            );
        }
    }
}
