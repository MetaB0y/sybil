//! CLI simulation harness for matching engine testing.
//!
//! # Usage
//!
//! ```bash
//! # Run quick test with pipeline (default)
//! matching-sim --preset quick
//!
//! # Run with verbose step-by-step output
//! matching-sim --preset small -v
//!
//! # Compare all solvers
//! matching-sim --preset small --solver all
//!
//! # Custom configuration
//! matching-sim --markets 50 --orders 5000 --bundles 0.2 --aon 0.3
//! ```

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

use matching_engine::{MarketId, Order, Problem};
use matching_scenarios::{generate_scenario, ScenarioConfig};
use matching_solver::{
    verify, GreedySolver, IterationStats, MilpSolver, Pipeline, PipelineResult, Solver,
    VerificationResult, VizSnapshot,
};

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
    let export_json = get_arg_value(&args, "--export-json");
    let show_charts = args.iter().any(|a| a == "--show-charts");

    println!("========================================");
    println!("       MATCHING SIMULATION              ");
    println!("========================================\n");

    println!("Configuration:");
    println!("  Markets: {}", config.num_markets);
    println!("  Orders: {}", config.num_orders);
    println!("  Bundles: {:.0}%", config.bundle_fraction * 100.0);
    println!("  AON: {:.0}%", config.aon_fraction * 100.0);
    println!("  MMs: {}", config.num_mms);
    println!("  Solver: {:?}", solver_choice);
    println!("  Batches: {}", num_batches);
    if let Some(timeout) = milp_timeout {
        println!("  MILP timeout: {}s", timeout);
    }
    println!();

    let start = Instant::now();

    if (solver_choice == SolverChoice::Pipeline || solver_choice == SolverChoice::Negrisk)
        && (verbose || export_json.is_some() || show_charts)
    {
        // Detailed pipeline run with step-by-step output
        let use_negrisk = solver_choice == SolverChoice::Negrisk;
        run_detailed_pipeline(&config, num_batches, export_json.as_deref(), show_charts, verbose, use_negrisk);
    } else {
        // Standard comparison run
        let results = run_simulation(&config, &solver_choice, milp_timeout, num_batches, verbose);
        print_results(&results, &solver_choice);
    }

    let elapsed = start.elapsed().as_secs_f64();
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
    println!("                         pipeline (default)");
    println!("                         greedy");
    println!("                         milp");
    println!("                         all (compare all)");
    println!("  --milp-timeout <S>   MILP time limit in seconds");
    println!();
    println!("Other options:");
    println!("  --batches <N>        Number of batches to run (default: 1)");
    println!("  --seed <N>           Random seed (default: 42)");
    println!("  --verbose, -v        Show detailed step-by-step output");
    println!("  --help, -h           Show this help message");
    println!();
    println!("Visualization options:");
    println!("  --export-json <PATH> Export pipeline snapshot as JSON");
    println!("  --show-charts        Show ASCII convergence charts after run");
}

// ============================================================================
// Detailed Pipeline Runner
// ============================================================================

/// Compute statistics about orders in the problem
struct OrderStats {
    total_orders: usize,
    single_market_orders: usize,
    bundle_orders: usize,
    aon_orders: usize,
    mm_order_ids: HashSet<u64>,
    user_order_count: usize,
    mm_order_count: usize,
}

impl OrderStats {
    fn compute(problem: &Problem) -> Self {
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|c| c.order_ids.iter().copied())
            .collect();

        let mut single_market = 0;
        let mut bundle = 0;
        let mut aon = 0;

        for order in &problem.orders {
            if order.num_markets > 1 {
                bundle += 1;
            } else {
                single_market += 1;
            }
            if order.is_all_or_none() {
                aon += 1;
            }
        }

        let mm_count = problem
            .orders
            .iter()
            .filter(|o| mm_order_ids.contains(&o.id))
            .count();

        Self {
            total_orders: problem.orders.len(),
            single_market_orders: single_market,
            bundle_orders: bundle,
            aon_orders: aon,
            mm_order_ids,
            user_order_count: problem.orders.len() - mm_count,
            mm_order_count: mm_count,
        }
    }

    fn is_mm_order(&self, order_id: u64) -> bool {
        self.mm_order_ids.contains(&order_id)
    }
}

/// Compute fill statistics from a result
struct FillStats {
    // By fill status
    fully_filled: usize,
    partially_filled: usize,
    unfilled: usize,

    // By order type
    user_filled: usize,
    user_welfare: i64,
    user_volume: u64,
    mm_filled: usize,
    mm_welfare: i64,
    mm_volume: u64,

    // By market type
    bundle_filled: usize,

    // Markets with activity
    markets_with_volume: usize,
}

impl FillStats {
    fn compute(
        problem: &Problem,
        result: &PipelineResult,
        order_stats: &OrderStats,
    ) -> Self {
        let order_map: HashMap<u64, &Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        let fill_map: HashMap<u64, u64> = result
            .result
            .fills
            .iter()
            .map(|f| (f.order_id, f.fill_qty))
            .collect();

        let mut fully_filled = 0;
        let mut partially_filled = 0;
        let mut unfilled = 0;

        let mut user_filled = 0;
        let mut user_welfare: i64 = 0;
        let mut user_volume: u64 = 0;
        let mut mm_filled = 0;
        let mut mm_welfare: i64 = 0;
        let mut mm_volume: u64 = 0;

        let mut bundle_filled = 0;

        for order in &problem.orders {
            let fill_qty = fill_map.get(&order.id).copied().unwrap_or(0);
            let is_mm = order_stats.is_mm_order(order.id);

            if fill_qty == 0 {
                unfilled += 1;
            } else if fill_qty >= order.max_fill {
                fully_filled += 1;
            } else {
                partially_filled += 1;
            }

            if fill_qty > 0 {
                // Find the fill to get welfare
                if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order.id) {
                    let welfare = fill.welfare(order);
                    if is_mm {
                        mm_filled += 1;
                        mm_welfare += welfare;
                        mm_volume += fill_qty;
                    } else {
                        user_filled += 1;
                        user_welfare += welfare;
                        user_volume += fill_qty;
                    }

                    if order.num_markets > 1 {
                        bundle_filled += 1;
                    }
                }
            }
        }

        // Count markets with volume
        let mut market_volumes: HashMap<_, u64> = HashMap::new();
        for fill in &result.result.fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                for market_id in order.active_markets() {
                    *market_volumes.entry(market_id).or_default() += fill.fill_qty;
                }
            }
        }
        let markets_with_volume = market_volumes.values().filter(|&&v| v > 0).count();

        Self {
            fully_filled,
            partially_filled,
            unfilled,
            user_filled,
            user_welfare,
            user_volume,
            mm_filled,
            mm_welfare,
            mm_volume,
            bundle_filled,
            markets_with_volume,
        }
    }
}

/// Select a representative subset of markets for detailed output.
/// Picks markets from different groups + some standalone markets.
fn select_sample_markets(problem: &Problem, max_markets: usize) -> Vec<MarketId> {
    let mut selected = Vec::new();

    // First, pick one market from each group (up to half of max)
    let group_quota = max_markets / 2;
    for (i, group) in problem.market_groups.iter().enumerate() {
        if i >= group_quota {
            break;
        }
        if let Some(&market) = group.markets.first() {
            selected.push(market);
        }
    }

    // Then add standalone markets (first few not in any group)
    let markets_in_groups: HashSet<MarketId> = problem
        .market_groups
        .iter()
        .flat_map(|g| g.markets.iter().copied())
        .collect();

    for market in problem.markets.iter() {
        if selected.len() >= max_markets {
            break;
        }
        if !markets_in_groups.contains(&market.id) && !selected.contains(&market.id) {
            selected.push(market.id);
        }
    }

    // If still need more, add from groups
    for market in problem.markets.iter() {
        if selected.len() >= max_markets {
            break;
        }
        if !selected.contains(&market.id) {
            selected.push(market.id);
        }
    }

    selected
}

/// Print detailed market stats table
fn print_market_details(
    problem: &Problem,
    result: &PipelineResult,
    sample_markets: &[MarketId],
) {
    if sample_markets.is_empty() {
        return;
    }

    println!("\nSample Market Details ({} of {}):", sample_markets.len(), problem.markets.len());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Market", "Group", "P(YES)", "P(NO)", "Volume", "Welfare", "Liq Rem"]);

    // Build order map for volume/welfare calculation
    let _order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
    let fill_map: HashMap<u64, u64> = result
        .result
        .fills
        .iter()
        .map(|f| (f.order_id, f.fill_qty))
        .collect();

    // Find which group each market belongs to
    let market_to_group: HashMap<MarketId, &str> = problem
        .market_groups
        .iter()
        .flat_map(|g| g.markets.iter().map(|&m| (m, g.name.as_str())))
        .collect();

    for &market_id in sample_markets {
        // Get prices
        let (yes_price, no_price) = if let Some(ref pd) = result.price_discovery {
            if let Some(prices) = pd.prices.get(&market_id) {
                (
                    prices.get(0).copied().unwrap_or(0),
                    prices.get(1).copied().unwrap_or(0),
                )
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        // Calculate volume and welfare for this market
        let mut volume: u64 = 0;
        let mut welfare: i64 = 0;
        for order in &problem.orders {
            if order.num_markets == 1 && order.markets[0] == market_id {
                if let Some(&fill_qty) = fill_map.get(&order.id) {
                    volume += fill_qty;
                    if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order.id) {
                        welfare += fill.welfare(order);
                    }
                }
            }
        }

        // Get remaining liquidity
        let liq_yes = result
            .result
            .remaining_liquidity
            .book(market_id, 0)
            .map(|b| b.total_ask_qty())
            .unwrap_or(0);
        let liq_no = result
            .result
            .remaining_liquidity
            .book(market_id, 1)
            .map(|b| b.total_ask_qty())
            .unwrap_or(0);

        let group_name = market_to_group.get(&market_id).copied().unwrap_or("-");

        // Format market name
        let market_name = problem
            .markets
            .iter()
            .find(|m| m.id == market_id)
            .map(|m| m.name.as_str())
            .unwrap_or("?");

        table.add_row(vec![
            Cell::new(market_name),
            Cell::new(group_name),
            Cell::new(format!("{:.1}%", yes_price as f64 / 1e7)),
            Cell::new(format!("{:.1}%", no_price as f64 / 1e7)),
            Cell::new(format_qty(volume)),
            Cell::new(format_welfare(welfare)),
            Cell::new(format!("{}/{}", format_qty(liq_yes), format_qty(liq_no))),
        ]);
    }

    println!("{table}");
}

fn run_detailed_pipeline(
    base_config: &ScenarioConfig,
    num_batches: usize,
    export_json: Option<&str>,
    show_charts: bool,
    verbose: bool,
    use_negrisk: bool,
) {
    for batch in 0..num_batches {
        let config = ScenarioConfig {
            seed: base_config.seed + batch as u64,
            ..base_config.clone()
        };

        let problem = generate_scenario(config.clone());
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

        // Capture initial liquidity for viz feature
        #[cfg(feature = "viz")]
        let initial_liquidity = problem.liquidity.snapshot();

        // Run pipeline and get detailed results
        // Use full() which includes ArbitrageDetector for bundle matching
        // Or use with_negrisk() for negrisk arbitrage instead of price projection
        let pipeline = if use_negrisk {
            Pipeline::with_negrisk()
        } else {
            Pipeline::with_negrisk()
        };
        let result = pipeline.solve(&problem);

        if verbose {
            // Print step-by-step results
            print_pipeline_steps(&result, &problem);

            // Print sample market details
            print_market_details(&problem, &result, &sample_markets);

            // Add arbitrage orders to problem for stats and verification
            let mut problem_with_arb = problem.clone();
            if let Some(ref negrisk) = result.negrisk {
                for order in &negrisk.arbitrage_orders {
                    problem_with_arb.orders.push(order.clone());
                }
            }

            // Print fill statistics
            let fill_stats = FillStats::compute(&problem_with_arb, &result, &order_stats);
            print_fill_stats(&fill_stats, &order_stats, problem.markets.len());

            // Verify the result
            let verification = verify(&problem_with_arb, &result.result);
            print_verification_result(&verification);
        }

        // Export JSON if requested
        if let Some(path) = export_json {
            let scenario_name = format!(
                "batch_{}_seed_{}",
                batch + 1,
                base_config.seed + batch as u64
            );

            #[cfg(feature = "viz")]
            let snapshot = VizSnapshot::from_pipeline_result_with_liquidity(
                &result,
                &problem,
                scenario_name,
                &initial_liquidity,
                result.phase_snapshots.clone(),
            );

            #[cfg(not(feature = "viz"))]
            let snapshot = VizSnapshot::from_pipeline_result(&result, &problem, scenario_name);

            let json = snapshot.to_json();

            // If multiple batches, append batch number to path
            let output_path = if num_batches > 1 {
                let path = std::path::Path::new(path);
                let stem = path.file_stem().unwrap_or_default().to_str().unwrap_or("");
                let ext = path.extension().unwrap_or_default().to_str().unwrap_or("json");
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
            println!("{}", matching_solver::viz::ascii::convergence_summary(&result.iteration_stats));
        }

        if verbose {
            println!();
        }
    }
}

fn print_verification_result(result: &VerificationResult) {
    println!();
    println!("Result Verification (ZK-ready):");
    println!("─────────────────────────────────────────");

    if result.valid {
        println!("  Status: {} VALID", "✓");
        println!(
            "  Fills verified: {}",
            result.stats.fills_checked
        );
        println!(
            "  MM constraints verified: {}",
            result.stats.mm_constraints_checked
        );
        println!(
            "  Welfare: computed={} reported={}",
            format_welfare(result.stats.computed_welfare),
            format_welfare(result.stats.reported_welfare)
        );
    } else {
        println!("  Status: {} INVALID ({} violations)", "✗", result.violations.len());
        println!();
        println!("  Violations:");
        for (i, violation) in result.violations.iter().enumerate().take(10) {
            println!("    {}. {:?}: {}", i + 1, violation.kind, violation.details);
        }
        if result.violations.len() > 10 {
            println!("    ... and {} more", result.violations.len() - 10);
        }
    }
}

fn print_problem_summary(problem: &Problem, stats: &OrderStats) {
    println!("Problem Summary:");
    println!("  Markets: {}", problem.markets.len());
    if !problem.market_groups.is_empty() {
        let markets_in_groups: usize = problem.market_groups.iter().map(|g| g.markets.len()).sum();
        println!(
            "    In {} multi-outcome groups: {} markets",
            problem.market_groups.len(),
            markets_in_groups
        );
    }
    println!("  Total orders: {}", stats.total_orders);
    println!("    User orders: {}", stats.user_order_count);
    println!("    MM orders: {}", stats.mm_order_count);
    println!("    Single-market: {}", stats.single_market_orders);
    println!("    Bundles: {}", stats.bundle_orders);
    println!("    AON: {}", stats.aon_orders);
    println!("  MM constraints: {}", problem.mm_constraints.len());
    println!();
}

fn print_pipeline_steps(result: &PipelineResult, _problem: &Problem) {
    println!("Pipeline Steps (fixed-point, {} iterations):", result.iterations);
    println!("─────────────────────────────────────────");

    // Phase 1: Price Discovery (LocalSolver for single-market orders)
    if let Some(ref pd) = result.price_discovery {
        println!(
            "  1. Price Discovery    {:>7.3}s",
            result.phase_times.price_discovery_secs
        );
        println!(
            "     └─ {} markets priced (last iter: {} fills)",
            pd.prices.len(),
            pd.total_fills,
        );
    }

    // Phase 2: Negrisk Arbitrage
    if let Some(ref negrisk) = result.negrisk {
        println!(
            "  2. Negrisk Arbitrage  {:>7.3}s",
            result.phase_times.negrisk_secs
        );
        if negrisk.opportunities_found > 0 {
            println!(
                "     └─ {} opportunities, {} shares, ${:.2} welfare",
                negrisk.opportunities_found,
                negrisk.total_shares,
                negrisk.total_welfare as f64 / 1e9
            );
            for fill in &negrisk.fills {
                println!(
                    "        {}: {} shares @ ${:.4} profit/share = ${:.2}",
                    fill.group_name,
                    fill.shares,
                    fill.profit_per_share as f64 / 1e9,
                    fill.welfare as f64 / 1e9
                );
            }
        } else {
            println!("     └─ no arbitrage opportunities found");
        }
    }

    // Phase 3: MM Allocation
    if let Some(ref alloc) = result.allocation {
        println!(
            "  3. MM Allocation      {:>7.3}s",
            result.phase_times.allocation_secs
        );
        println!(
            "     └─ {} orders activated, {} iters",
            alloc.activated_orders.len(),
            alloc.iterations
        );
        if !alloc.mm_allocations.is_empty() {
            for mm_alloc in &alloc.mm_allocations {
                let util = if mm_alloc.budget > 0 {
                    mm_alloc.capital_used as f64 / mm_alloc.budget as f64 * 100.0
                } else {
                    0.0
                };
                println!(
                    "        MM{}: {}/{} capital ({:.0}% util), {} orders",
                    mm_alloc.mm_id.0,
                    format_price(mm_alloc.capital_used),
                    format_price(mm_alloc.budget),
                    util,
                    mm_alloc.activated_orders.len()
                );
            }
        }
    }

    // Show bundle matching (ArbitrageDetector) results
    if result.phase_times.partial_solving_secs > 0.0 || !result.contributions.is_empty() {
        println!(
            "  4. Bundle Matching    {:>7.3}s",
            result.phase_times.partial_solving_secs
        );

        // Aggregate contributions by solver
        let mut solver_stats: HashMap<String, (usize, i64)> = HashMap::new();
        for contrib in &result.contributions {
            let entry = solver_stats.entry(contrib.solver_name.clone()).or_insert((0, 0));
            entry.0 += contrib.fills_contributed;
            entry.1 += contrib.welfare_contributed;
        }

        if solver_stats.is_empty() {
            println!("     └─ no bundle fills");
        } else {
            for (name, (fills, welfare)) in &solver_stats {
                println!(
                    "     └─ {}: {} fills, welfare {}",
                    name,
                    fills,
                    format_welfare(*welfare)
                );
            }
        }
    }

    println!("─────────────────────────────────────────");
    println!(
        "  Total                 {:>7.3}s",
        result.total_time_secs
    );
    println!();

    // Print iteration convergence stats
    if !result.iteration_stats.is_empty() {
        print_iteration_convergence(&result.iteration_stats);
    }
}

fn print_iteration_convergence(stats: &[IterationStats]) {
    println!("Fixed-Point Convergence:");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Iter", "Welfare", "Δ Welfare", "Volume", "Δ Volume", "Fills", "PD Fills", "Bundle",
        ]);

    for stat in stats {
        let welfare_delta_str = if stat.welfare_delta > 0 {
            format!("+{}", format_welfare(stat.welfare_delta))
        } else if stat.welfare_delta < 0 {
            format_welfare(stat.welfare_delta)
        } else {
            "—".to_string()
        };

        let volume_delta_str = if stat.volume_delta > 0 {
            format!("+{}", format_qty(stat.volume_delta))
        } else {
            "—".to_string()
        };

        // Color the delta based on whether we're converging
        let welfare_cell = if stat.iteration == 1 {
            Cell::new(&welfare_delta_str)
        } else if stat.welfare_delta == 0 {
            Cell::new(&welfare_delta_str).fg(Color::Green)
        } else if stat.welfare_delta > 0 {
            Cell::new(&welfare_delta_str).fg(Color::Yellow)
        } else {
            Cell::new(&welfare_delta_str).fg(Color::Red)
        };

        table.add_row(vec![
            Cell::new(stat.iteration),
            Cell::new(format_welfare(stat.welfare)),
            welfare_cell,
            Cell::new(format_qty(stat.volume)),
            Cell::new(&volume_delta_str),
            Cell::new(stat.fills),
            Cell::new(stat.price_discovery_fills),
            Cell::new(stat.bundle_fills),
        ]);
    }

    println!("{table}");
    println!();
}

fn print_fill_stats(stats: &FillStats, order_stats: &OrderStats, num_markets: usize) {
    println!("Fill Statistics:");
    println!("─────────────────────────────────────────");

    // Fill status breakdown
    let total = order_stats.total_orders;
    println!(
        "  Fill Status:    Full {:>4} ({:>5.1}%)  Partial {:>4} ({:>5.1}%)  Unfilled {:>4} ({:>5.1}%)",
        stats.fully_filled,
        pct(stats.fully_filled, total),
        stats.partially_filled,
        pct(stats.partially_filled, total),
        stats.unfilled,
        pct(stats.unfilled, total)
    );
    println!(
        "  Markets active: {}/{} ({:.1}%)",
        stats.markets_with_volume,
        num_markets,
        pct(stats.markets_with_volume, num_markets)
    );

    // User vs MM breakdown
    println!();
    println!("  By Order Type:");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Type", "Filled", "Volume", "Welfare"]);

    table.add_row(vec![
        Cell::new("User"),
        Cell::new(format!(
            "{}/{} ({:.1}%)",
            stats.user_filled,
            order_stats.user_order_count,
            pct(stats.user_filled, order_stats.user_order_count)
        )),
        Cell::new(format_qty(stats.user_volume)),
        Cell::new(format_welfare(stats.user_welfare)).fg(Color::Green),
    ]);

    table.add_row(vec![
        Cell::new("MM"),
        Cell::new(format!(
            "{}/{} ({:.1}%)",
            stats.mm_filled,
            order_stats.mm_order_count,
            pct(stats.mm_filled, order_stats.mm_order_count)
        )),
        Cell::new(format_qty(stats.mm_volume)),
        Cell::new(format_welfare(stats.mm_welfare)).fg(Color::Yellow),
    ]);

    table.add_row(vec![
        Cell::new("Bundle"),
        Cell::new(format!(
            "{}/{} ({:.1}%)",
            stats.bundle_filled,
            order_stats.bundle_orders,
            pct(stats.bundle_filled, order_stats.bundle_orders)
        )),
        Cell::new("-"),
        Cell::new("-"),
    ]);

    println!("{table}");

    // Market activity
    println!();
    let total_welfare = stats.user_welfare + stats.mm_welfare;
    println!("  Total welfare: {}", format_welfare(total_welfare));
    println!(
        "  Total volume: {}",
        format_qty(stats.user_volume + stats.mm_volume)
    );
}

fn pct(num: usize, denom: usize) -> f64 {
    if denom > 0 {
        num as f64 / denom as f64 * 100.0
    } else {
        0.0
    }
}

fn format_welfare(w: i64) -> String {
    // Welfare is in nanos (1e9 = $1)
    let dollars = w as f64 / 1_000_000_000.0;
    if dollars.abs() >= 1_000_000.0 {
        format!("${:.2}M", dollars / 1_000_000.0)
    } else if dollars.abs() >= 1_000.0 {
        format!("${:.2}K", dollars / 1_000.0)
    } else if dollars.abs() >= 1.0 {
        format!("${:.2}", dollars)
    } else {
        format!("{:.0}¢", dollars * 100.0)
    }
}

fn format_price(p: u64) -> String {
    // Prices are in nanos (1e9 = $1)
    let dollars = p as f64 / 1_000_000_000.0;
    if dollars >= 1_000_000.0 {
        format!("${:.2}M", dollars / 1_000_000.0)
    } else if dollars >= 1_000.0 {
        format!("${:.2}K", dollars / 1_000.0)
    } else if dollars >= 1.0 {
        format!("${:.2}", dollars)
    } else {
        format!("{:.0}¢", dollars * 100.0)
    }
}

fn format_qty(q: u64) -> String {
    if q >= 1_000_000 {
        format!("{:.2}M", q as f64 / 1_000_000.0)
    } else if q >= 1_000 {
        format!("{:.2}K", q as f64 / 1_000.0)
    } else {
        format!("{}", q)
    }
}

// ============================================================================
// Standard Simulation Runner
// ============================================================================

fn parse_scenario_config(args: &[String]) -> ScenarioConfig {
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

        if let Some(seed) = get_arg_value(args, "--seed") {
            config.seed = seed.parse().unwrap_or(42);
        }

        return config;
    }

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
    Negrisk,
    All,
}

fn parse_solver_choice(args: &[String]) -> SolverChoice {
    match get_arg_value(args, "--solver").as_deref() {
        Some("greedy") => SolverChoice::Greedy,
        Some("milp") => SolverChoice::Milp,
        Some("pipeline") => SolverChoice::Pipeline,
        Some("negrisk") => SolverChoice::Negrisk,
        Some("all") => SolverChoice::All,
        _ => SolverChoice::Pipeline, // Default to pipeline
    }
}

fn parse_milp_timeout(args: &[String]) -> Option<f64> {
    get_arg_value(args, "--milp-timeout").and_then(|v| v.parse().ok())
}

fn parse_batches(args: &[String]) -> usize {
    get_arg_value(args, "--batches")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1) // Default to 1 batch
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
        SolverChoice::Negrisk => vec![Box::new(Pipeline::with_negrisk())],
        SolverChoice::All => {
            let milp: Box<dyn Solver> = if let Some(timeout) = milp_timeout {
                Box::new(MilpSolver::with_timeout(timeout))
            } else {
                Box::new(MilpSolver::with_timeout(5.0))
            };
            vec![
                Box::new(GreedySolver::new()),
                milp,
                Box::new(Pipeline::current()),
                Box::new(Pipeline::with_negrisk()),
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
            println!(
                "Batch {} (seed {})",
                batch + 1,
                base_config.seed + batch as u64
            );
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
                    format_welfare(result.total_welfare),
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

    let best_welfare = results.iter().map(|r| r.mean_welfare()).fold(0.0, f64::max);

    for result in results {
        let welfare = result.mean_welfare();
        let gap = if best_welfare > 0.0 {
            (best_welfare - welfare) / best_welfare * 100.0
        } else {
            0.0
        };

        let welfare_str = if gap < 0.1 {
            format_welfare(welfare as i64)
        } else {
            format!("{} (-{:.1}%)", format_welfare(welfare as i64), gap)
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
        let pipeline = results
            .iter()
            .find(|r| r.name.contains("Pipeline") || r.name.contains("Current"));

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
