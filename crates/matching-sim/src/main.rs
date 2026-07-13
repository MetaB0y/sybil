//! CLI simulation harness for matching engine testing.
//!
//! # Usage
//!
//! ```bash
//! # Run quick test with LP solver (default)
//! matching-sim --preset quick
//!
//! # Run with verbose step-by-step output
//! matching-sim --preset small -v
//!
//! # Compare all solvers
//! matching-sim --preset small --solver all
//!
//! # Custom configuration
//! matching-sim --markets 50 --orders 5000
//! ```
//!
//! The harness is organised into modules:
//! - [`cli`] — argument parsing and solver selection
//! - [`witness`] — block-witness construction for verification
//! - [`report`] — terminal statistics, formatting, and comparison tables
//! - [`json_export`] — JSON comparison export
//!
//! `main.rs` itself only wires these together and drives the batch loops.

mod cli;
mod json_export;
mod report;
mod witness;

#[cfg(any(feature = "lp", feature = "conic"))]
use std::collections::HashMap;
use std::time::Instant;

use clap::Parser;
use matching_engine::{Nanos, Problem};
use matching_scenarios::{ScenarioConfig, generate_scenario};
use matching_solver::MmBudgetMode;
#[cfg(any(feature = "lp", feature = "conic"))]
use matching_solver::VizSnapshot;
use sybil_verifier::{BlockWitness, verify_match};

use cli::*;
use json_export::build_comparison_json;
use report::*;
use witness::*;

#[cfg(feature = "conic")]
type CliConicConfig = matching_solver::ConicConfig;
#[cfg(not(feature = "conic"))]
type CliConicConfig = ();

fn main() {
    let cli = Cli::parse();
    let config = cli.scenario_config();
    let solver_choice = cli.solver.clone();
    let milp_timeout = cli.milp_timeout;
    let mm_mode = cli.mm_budget_mode();
    let num_batches = cli.batches;
    let verbose = cli.verbose;
    let export_json = cli.export_json.as_deref();
    let export_comparison = cli.export_comparison.as_deref();
    let show_charts = cli.show_charts;
    let mm_budget_scale = cli.mm_budget_scale;

    #[cfg(feature = "conic")]
    let conic_config = cli.conic_config();
    #[cfg(not(feature = "conic"))]
    let conic_config = ();

    println!("========================================");
    println!("       MATCHING SIMULATION              ");
    println!("========================================\n");

    println!("Configuration:");
    println!("  Markets: {}", config.num_markets);
    println!("  Orders: {}", config.num_orders);
    println!("  MMs: {}", config.num_mms);
    println!("  Solver: {:?}", solver_choice);
    println!("  Batches: {}", num_batches);
    if let Some(timeout) = milp_timeout {
        println!("  MILP timeout: {}s", timeout);
    }
    #[cfg(feature = "conic")]
    if matches!(
        solver_choice,
        SolverChoice::Conic | SolverChoice::DecomposedConic
    ) {
        println!("  Mode: {:?}", conic_config.mode);
        if conic_config.temperature > 0.0 {
            println!("  Temperature: {}", conic_config.temperature);
        }
    }
    println!();

    let start = Instant::now();

    if supports_detailed_pipeline(&solver_choice)
        && (verbose || export_json.is_some() || show_charts)
    {
        // Detailed pipeline run with step-by-step output
        run_detailed_pipeline(
            &config,
            num_batches,
            export_json,
            show_charts,
            verbose,
            &solver_choice,
            mm_budget_scale,
            &conic_config,
        );
    } else {
        // Standard comparison run
        let (results, gap_data) = run_simulation(
            &config,
            &solver_choice,
            milp_timeout,
            mm_mode,
            num_batches,
            verbose,
            export_comparison,
            mm_budget_scale,
            &conic_config,
        );
        print_results(&results, &solver_choice);
        if let Some(ref data) = gap_data {
            print_gap_analysis(data);
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("\nTotal time: {:.2}s", elapsed);
}

// ============================================================================
// Detailed Pipeline Runner
// ============================================================================

#[allow(unused_variables, clippy::too_many_arguments)]
#[cfg(any(feature = "lp", feature = "conic"))]
fn run_detailed_pipeline(
    base_config: &ScenarioConfig,
    num_batches: usize,
    export_json: Option<&str>,
    show_charts: bool,
    verbose: bool,
    solver_choice: &SolverChoice,
    mm_budget_scale: Option<f64>,
    conic_config: &CliConicConfig,
) {
    for batch in 0..num_batches {
        let config = ScenarioConfig {
            seed: base_config.seed + batch as u64,
            ..base_config.clone()
        };

        let mut problem = generate_scenario(config.clone());

        if let Some(scale) = mm_budget_scale {
            for mm in &mut problem.mm_constraints {
                mm.max_capital = Nanos((mm.max_capital.0 as f64 * scale) as u64);
            }
        }
        let order_stats = OrderStats::compute(&problem);

        if verbose {
            println!("========================================");
            println!(
                "  BATCH {} (seed {})",
                batch + 1,
                base_config.seed + batch as u64
            );
            println!("========================================\n");

            // Print problem summary
            print_problem_summary(&problem, &order_stats);
        }

        // Select sample markets for detailed output
        let sample_markets = select_sample_markets(&problem, 10);

        // No initial liquidity capture needed (liquidity pool removed)

        // Run solver and get detailed results
        let result = match solver_choice {
            #[cfg(feature = "lp")]
            SolverChoice::Lp => {
                let solver = matching_solver::LpSolver::new();
                solver.solve(&problem)
            }
            #[cfg(feature = "lp")]
            SolverChoice::Eg => {
                let solver = matching_solver::EgSolver::new();
                solver.solve(&problem)
            }
            #[cfg(feature = "conic")]
            SolverChoice::Conic => {
                let solver = matching_solver::ConicSolver::with_config(conic_config.clone());
                solver.solve(&problem)
            }
            #[cfg(feature = "lp")]
            SolverChoice::DecomposedLp => {
                let solver =
                    matching_solver::DecomposedSolver::new(matching_solver::LpSolver::new());
                solver.solve(&problem)
            }
            #[cfg(feature = "lp")]
            SolverChoice::DecomposedEg => {
                let solver =
                    matching_solver::DecomposedSolver::new(matching_solver::EgSolver::new());
                solver.solve(&problem)
            }
            #[cfg(feature = "conic")]
            SolverChoice::DecomposedConic => {
                let solver = matching_solver::DecomposedSolver::new(
                    matching_solver::ConicSolver::with_config(conic_config.clone()),
                );
                solver.solve(&problem)
            }
            #[cfg(feature = "lp")]
            SolverChoice::IterLp => {
                let solver = matching_solver::IterLpSolver::new();
                solver.solve(&problem)
            }
            #[cfg(feature = "lp")]
            SolverChoice::DecomposedIterLp => {
                let solver =
                    matching_solver::DecomposedSolver::new(matching_solver::IterLpSolver::new());
                solver.solve(&problem)
            }
            _ => unreachable!("only LP/EG/Conic/IterLP/Decomposed reach run_detailed_pipeline"),
        };

        if verbose {
            {
                let solver_label = match solver_choice {
                    #[cfg(feature = "lp")]
                    SolverChoice::Eg => "EG (Fisher) Solver",
                    #[cfg(feature = "lp")]
                    SolverChoice::DecomposedLp => "Decomposed(LP) Solver",
                    #[cfg(feature = "lp")]
                    SolverChoice::DecomposedEg => "Decomposed(EG) Solver",
                    #[cfg(feature = "conic")]
                    SolverChoice::DecomposedConic => "Decomposed(Conic) Solver",
                    #[cfg(feature = "conic")]
                    SolverChoice::Conic => "Conic (EG) Solver",
                    #[cfg(feature = "lp")]
                    SolverChoice::IterLp => "IterLP Solver",
                    #[cfg(feature = "lp")]
                    SolverChoice::DecomposedIterLp => "Decomposed(IterLP) Solver",
                    _ => "LP Solver",
                };
                println!("{}:", solver_label);
                println!("─────────────────────────────────────────");
                println!("  Solve time:     {:.3}s", result.total_time_secs);
                println!("  Fills:          {}", result.result.fills.len());
                println!(
                    "  Welfare:        {}",
                    format_welfare(result.result.total_welfare())
                );
                println!(
                    "  Volume:         {}",
                    format_qty(result.result.total_quantity_filled)
                );

                if !problem.mm_constraints.is_empty() {
                    let mm_fills: HashMap<u64, (matching_engine::Nanos, matching_engine::Qty)> =
                        result
                            .result
                            .fills
                            .iter()
                            .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
                            .collect();
                    let mm_filled: usize = problem
                        .mm_constraints
                        .iter()
                        .flat_map(|mm| &mm.order_ids)
                        .filter(|id| mm_fills.contains_key(id))
                        .count();
                    let mm_total: usize = problem
                        .mm_constraints
                        .iter()
                        .map(|mm| mm.order_ids.len())
                        .sum();
                    println!("  MM orders:      {}/{} filled", mm_filled, mm_total);
                    for mm in &problem.mm_constraints {
                        let cap = mm.capital_used(&mm_fills);
                        let budget = mm.max_capital;
                        let util = if budget.0 > 0 {
                            cap.0 as f64 / budget.0 as f64 * 100.0
                        } else {
                            0.0
                        };
                        println!(
                            "    MM{}: {}/{} ({:.0}% util)",
                            mm.mm_id.0,
                            format_price(cap.0),
                            format_price(budget.0),
                            util,
                        );
                    }
                }
                println!();
            }

            // Print sample market details
            print_market_details(&problem, &result, &sample_markets);

            // Print fill statistics
            let fill_stats = FillStats::compute(&problem, &result, &order_stats);
            print_fill_stats(&fill_stats, &order_stats, problem.markets.len());

            // Verify the result using the new comprehensive verifier
            let witness = witness_from_pipeline(&problem, &result);
            let verification = verify_match(&witness, false);
            print_verification_result(&verification);
        }

        // Export JSON if requested
        if let Some(path) = export_json {
            let scenario_name = format!(
                "batch_{}_seed_{}",
                batch + 1,
                base_config.seed + batch as u64
            );

            let snapshot = VizSnapshot::from_pipeline_result(&result, &problem, scenario_name);

            let json = snapshot.to_json();

            // If multiple batches, append batch number to path
            let output_path = if num_batches > 1 {
                let path = std::path::Path::new(path);
                let stem = path.file_stem().unwrap_or_default().to_str().unwrap_or("");
                let ext = path
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("json");
                let parent = path.parent().unwrap_or(std::path::Path::new("."));
                parent
                    .join(format!("{}_{}.{}", stem, batch + 1, ext))
                    .to_string_lossy()
                    .to_string()
            } else {
                path.to_string()
            };

            match std::fs::write(&output_path, &json) {
                Ok(_) => println!("Exported JSON snapshot to: {}", output_path),
                Err(e) => eprintln!("Failed to export JSON to {}: {}", output_path, e),
            }
        }

        // Show ASCII charts if requested
        if show_charts {
            println!("No iteration data available.");
        }

        if verbose {
            println!();
        }
    }
}

#[allow(unused_variables, clippy::too_many_arguments)]
#[cfg(not(any(feature = "lp", feature = "conic")))]
fn run_detailed_pipeline(
    base_config: &ScenarioConfig,
    num_batches: usize,
    export_json: Option<&str>,
    show_charts: bool,
    verbose: bool,
    solver_choice: &SolverChoice,
    mm_budget_scale: Option<f64>,
    _conic_config: &CliConicConfig,
) {
    unreachable!("detailed pipeline requires the lp or conic feature")
}

// ============================================================================
// Standard Simulation Runner
// ============================================================================

/// Run a single solver choice on a problem and return (MatchingResult, witness for verification).
#[allow(unused_variables)]
fn run_solver_with_witness(
    choice: &SolverChoice,
    problem: &Problem,
    milp_timeout: Option<f64>,
    mm_mode: MmBudgetMode,
    conic_config: &CliConicConfig,
) -> (matching_solver::MatchingResult, BlockWitness) {
    match choice {
        SolverChoice::Milp => {
            let milp = create_milp_solver(milp_timeout, mm_mode);
            let milp_result = milp.solve_with_status(problem);
            let witness = witness_from_milp(problem, &milp_result);
            (milp_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::Lp => {
            let solver = matching_solver::LpSolver::new();
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::Eg => {
            let solver = matching_solver::EgSolver::new();
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "conic")]
        SolverChoice::Conic => {
            let solver = matching_solver::ConicSolver::with_config(conic_config.clone());
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedLp => {
            let solver = matching_solver::DecomposedSolver::new(matching_solver::LpSolver::new());
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedEg => {
            let solver = matching_solver::DecomposedSolver::new(matching_solver::EgSolver::new());
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "conic")]
        SolverChoice::DecomposedConic => {
            let solver = matching_solver::DecomposedSolver::new(
                matching_solver::ConicSolver::with_config(conic_config.clone()),
            );
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::IterLp => {
            let solver = matching_solver::IterLpSolver::new();
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedIterLp => {
            let solver =
                matching_solver::DecomposedSolver::new(matching_solver::IterLpSolver::new());
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        SolverChoice::All => unreachable!("expand_solver_choices should be called first"),
    }
}

#[allow(unused_variables, clippy::too_many_arguments)]
fn run_simulation(
    base_config: &ScenarioConfig,
    solver_choice: &SolverChoice,
    milp_timeout: Option<f64>,
    mm_mode: MmBudgetMode,
    num_batches: usize,
    verbose: bool,
    export_comparison: Option<&str>,
    mm_budget_scale: Option<f64>,
    conic_config: &CliConicConfig,
) -> (Vec<SolverResults>, Option<GapAnalysisData>) {
    let choices = expand_solver_choices(solver_choice);
    let collect_gap = *solver_choice == SolverChoice::All && verbose;

    let mut results: Vec<SolverResults> = choices
        .iter()
        .map(|c| SolverResults {
            name: solver_display_name(c, milp_timeout),
            ..Default::default()
        })
        .collect();

    let mut gap_data: Option<GapAnalysisData> = None;

    for batch in 0..num_batches {
        let config = ScenarioConfig {
            seed: base_config.seed + batch as u64,
            ..base_config.clone()
        };

        let mut problem = generate_scenario(config);

        if let Some(scale) = mm_budget_scale {
            for mm in &mut problem.mm_constraints {
                mm.max_capital = Nanos((mm.max_capital.0 as f64 * scale) as u64);
            }
        }

        if verbose {
            println!(
                "Batch {} (seed {})",
                batch + 1,
                base_config.seed + batch as u64
            );
            println!("{}", problem.summary());
        }

        // Collect per-solver matching results for comparison export and gap analysis
        let mut batch_matching_results = Vec::new();
        let mut gap_batch_data: Vec<SolverDetail> = Vec::new();

        for (i, choice) in choices.iter().enumerate() {
            let start = Instant::now();

            let (matching_result, witness) =
                run_solver_with_witness(choice, &problem, milp_timeout, mm_mode, conic_config);

            let elapsed = start.elapsed().as_secs_f64();

            results[i].total_filled += matching_result.orders_filled;
            results[i].total_orders += problem.num_orders();
            results[i].total_volume += matching_result.total_quantity_filled;
            results[i].total_time_secs += elapsed;
            results[i].batches += 1;

            let verification = verify_match(&witness, false);
            let computed_welfare = verification.stats.computed_welfare;
            results[i].total_welfare += computed_welfare;
            let is_valid = verification.valid;

            if verbose {
                println!(
                    "  {}: welfare={}, filled={}/{}, time={:.3}s  {}",
                    results[i].name,
                    format_welfare(computed_welfare),
                    matching_result.orders_filled,
                    problem.num_orders(),
                    elapsed,
                    if is_valid {
                        "\u{2713} VALID".to_string()
                    } else {
                        format!("\u{2717} {} violations", verification.violations.len())
                    }
                );
            }

            if collect_gap {
                gap_batch_data.push(SolverDetail {
                    name: solver_display_name(choice, milp_timeout),
                    result: matching_result.clone(),
                    clearing_prices: witness.clearing_prices.clone(),
                    is_valid,
                });
            }

            if export_comparison.is_some() {
                batch_matching_results.push((
                    solver_display_name(choice, milp_timeout),
                    matching_result,
                    witness,
                ));
            }

            match results[i].verification.as_mut() {
                Some(existing) => existing.merge(verification),
                None => results[i].verification = Some(verification),
            }
        }

        // Collect gap analysis data (last batch wins)
        if collect_gap {
            gap_data = Some(GapAnalysisData {
                problem: problem.clone(),
                solver_details: std::mem::take(&mut gap_batch_data),
            });
        }

        // Export detailed comparison JSON
        if let Some(path) = export_comparison {
            let output_path = if num_batches > 1 {
                let p = std::path::Path::new(path);
                let stem = p.file_stem().unwrap_or_default().to_str().unwrap_or("");
                let ext = p.extension().unwrap_or_default().to_str().unwrap_or("json");
                let parent = p.parent().unwrap_or(std::path::Path::new("."));
                parent
                    .join(format!("{}_{}.{}", stem, batch + 1, ext))
                    .to_string_lossy()
                    .to_string()
            } else {
                path.to_string()
            };
            let json = build_comparison_json(&problem, &batch_matching_results);
            match std::fs::write(&output_path, &json) {
                Ok(_) => println!("Exported comparison to: {}", output_path),
                Err(e) => eprintln!("Failed to export comparison: {}", e),
            }
        }

        if verbose {
            println!();
        }
    }

    (results, gap_data)
}
