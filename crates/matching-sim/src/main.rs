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

use matching_engine::{Fill, MarketId, Order, Problem};
use matching_scenarios::{generate_scenario, ScenarioConfig};
use matching_solver::{
    IterationStats, MilpConfig, MilpSolver, MmBudgetMode, Pipeline, PipelineResult, VizSnapshot,
};
use sybil_verifier::{
    verify_match, BlockWitness, VerificationResult, WitnessBlockHeader, WitnessOrder,
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
    let mm_mode = parse_mm_mode(&args);
    let num_batches = parse_batches(&args);
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let export_json = get_arg_value(&args, "--export-json");
    let export_comparison = get_arg_value(&args, "--export-comparison");
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

    if matches!(
        solver_choice,
        SolverChoice::Pipeline | SolverChoice::Negrisk | SolverChoice::Dual
    ) && (verbose || export_json.is_some() || show_charts)
    {
        // Detailed pipeline run with step-by-step output
        run_detailed_pipeline(
            &config,
            num_batches,
            export_json.as_deref(),
            show_charts,
            verbose,
            &solver_choice,
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
            export_comparison.as_deref(),
        );
        print_results(&results, &solver_choice);
        if let Some(ref data) = gap_data {
            print_gap_analysis(data);
        }
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
    println!("                         negrisk");
    println!("                         dual");
    println!("                         milp");
    println!("                         all (compare all)");
    println!("  --milp-timeout <S>   MILP time limit in seconds");
    println!("  --mm-mode <M>        MM budget constraint mode:");
    println!("                         exact (default) - bilinear MIQCQP");
    println!("                         mccormick       - linear relaxation");
    println!("                         ignore          - skip MM constraints");
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
    bundle_welfare: i64,
    bundle_volume: u64,

    // Markets with activity
    markets_with_volume: usize,
}

impl FillStats {
    fn compute(problem: &Problem, result: &PipelineResult, order_stats: &OrderStats) -> Self {
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

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
        let mut bundle_welfare: i64 = 0;
        let mut bundle_volume: u64 = 0;

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
                        bundle_welfare += welfare;
                        bundle_volume += fill_qty;
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
            bundle_welfare,
            bundle_volume,
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
fn print_market_details(problem: &Problem, result: &PipelineResult, sample_markets: &[MarketId]) {
    if sample_markets.is_empty() {
        return;
    }

    println!(
        "\nSample Market Details ({} of {}):",
        sample_markets.len(),
        problem.markets.len()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Market", "Group", "P(YES)", "P(NO)", "Volume", "Welfare", "Liq Rem",
        ]);

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
                    prices.first().copied().unwrap_or(0),
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
                    if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order.id)
                    {
                        welfare += fill.welfare(order);
                    }
                }
            }
        }

        // Liquidity pool has been removed; show N/A
        let liq_yes: u64 = 0;
        let liq_no: u64 = 0;

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
    solver_choice: &SolverChoice,
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

        // No initial liquidity capture needed (liquidity pool removed)

        // Run pipeline and get detailed results
        let pipeline = match solver_choice {
            SolverChoice::Dual => Pipeline::with_dual_decomposition(),
            SolverChoice::Negrisk => Pipeline::with_negrisk(),
            _ => Pipeline::with_negrisk(),
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

            // Verify the result using the new comprehensive verifier
            let witness = witness_from_problem(&problem_with_arb, &result);
            let verification = verify_match(&witness, false);
            print_verification_result(&verification);

            // Also run matching-solver's verifier which checks position balance
            let solver_verification =
                matching_solver::verify(&problem_with_arb, &result.result);
            print_solver_verification(&solver_verification);
        }

        // Export JSON if requested
        if let Some(path) = export_json {
            let scenario_name = format!(
                "batch_{}_seed_{}",
                batch + 1,
                base_config.seed + batch as u64
            );

            #[cfg(feature = "viz")]
            let snapshot = VizSnapshot::from_pipeline_result_with_phases(
                &result,
                &problem,
                scenario_name,
                result.phase_snapshots.clone(),
            );

            #[cfg(not(feature = "viz"))]
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
            println!(
                "{}",
                matching_solver::viz::ascii::convergence_summary(&result.iteration_stats)
            );
        }

        if verbose {
            println!();
        }
    }
}

/// Build a BlockWitness from a Problem + PipelineResult for standalone verification.
///
/// The sim doesn't have accounts or settlement, so only Layer 1 (match verification)
/// is meaningful. Layers 2–4 (settlement, block, orders) require a full sequencer.
fn witness_from_problem(problem: &Problem, result: &PipelineResult) -> BlockWitness {
    let clearing_prices = result
        .price_discovery
        .as_ref()
        .map(|pd| pd.prices.clone())
        .unwrap_or_default();

    let witness_orders: Vec<WitnessOrder> = problem
        .orders
        .iter()
        .map(|o| WitnessOrder {
            order: o.clone(),
            account_id: 0, // not meaningful in sim
            is_mm: problem
                .mm_constraints
                .iter()
                .any(|mm| mm.order_ids.contains(&o.id)),
        })
        .collect();

    BlockWitness {
        header: WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            order_count: problem.orders.len() as u32,
            fill_count: result.result.fills.len() as u32,
            timestamp_ms: 0,
        },
        previous_header: None,
        orders: witness_orders,
        rejections: vec![],
        fills: result.result.fills.clone(),
        clearing_prices,
        total_welfare: result.result.total_welfare,
        mm_constraints: problem.mm_constraints.clone(),
        market_groups: problem.market_groups.clone(),
        pre_state: vec![],
        post_state: vec![],
        resolved_markets: vec![],
    }
}

fn print_verification_result(result: &VerificationResult) {
    println!();
    println!("Result Verification (ZK-ready):");
    println!("─────────────────────────────────────────");

    if result.valid {
        println!("  Status: \u{2713} VALID");
        println!("  Fills verified: {}", result.stats.fills_checked);
        println!(
            "  MM constraints verified: {}",
            result.stats.mm_constraints_checked
        );
        println!(
            "  Welfare: computed={} reported={}",
            format_welfare(result.stats.computed_welfare),
            format_welfare(result.stats.reported_welfare)
        );
        if let Some(delta) = result.stats.market_group_avg_delta {
            let pct = delta as f64 / 1e7; // nanos to percentage points
            println!("  Market group avg |sum-1|: {:.2}pp", pct);
        }
    } else {
        println!(
            "  Status: \u{2717} INVALID ({} violations)",
            result.violations.len()
        );
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

fn print_solver_verification(result: &matching_solver::VerificationResult) {
    if result.valid {
        println!(
            "  Position Balance: \u{2713} VALID ({} markets checked)",
            result.stats.markets_checked
        );
    } else {
        let pos_violations: Vec<_> = result
            .violations
            .iter()
            .filter(|v| v.kind == matching_solver::ViolationKind::PositionImbalance)
            .collect();
        let other_violations: Vec<_> = result
            .violations
            .iter()
            .filter(|v| v.kind != matching_solver::ViolationKind::PositionImbalance)
            .collect();

        if !pos_violations.is_empty() {
            println!(
                "  Position Balance: \u{2717} {} markets imbalanced",
                pos_violations.len()
            );
            for v in pos_violations.iter().take(5) {
                println!("    {}", v.details);
            }
            if pos_violations.len() > 5 {
                println!("    ... and {} more", pos_violations.len() - 5);
            }
        }
        if !other_violations.is_empty() {
            println!(
                "  Other violations: {} found",
                other_violations.len()
            );
            for v in other_violations.iter().take(5) {
                println!("    {:?}: {}", v.kind, v.details);
            }
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
    println!(
        "Pipeline Steps (fixed-point, {} iterations):",
        result.iterations
    );
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

    // Show partial solver results
    if result.phase_times.partial_solving_secs > 0.0 || !result.contributions.is_empty() {
        println!(
            "  4. Bundle Matching    {:>7.3}s",
            result.phase_times.partial_solving_secs
        );

        // Aggregate contributions by solver
        let mut solver_stats: HashMap<String, (usize, i64)> = HashMap::new();
        for contrib in &result.contributions {
            let entry = solver_stats
                .entry(contrib.solver_name.clone())
                .or_insert((0, 0));
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
    println!("  Total                 {:>7.3}s", result.total_time_secs);
    println!();

    // Print UCP enforcement diagnostics
    if let Some(ref ucp) = result.ucp_stats {
        print_ucp_stats(ucp);
    }

    // Print iteration convergence stats
    if !result.iteration_stats.is_empty() {
        print_iteration_convergence(&result.iteration_stats);
    }
}

fn print_ucp_stats(ucp: &matching_solver::UcpStats) {
    println!("UCP Enforcement:");
    println!("─────────────────────────────────────────");
    println!(
        "  Input:    {:>6} fills, {} welfare",
        ucp.input_fills,
        format_welfare(ucp.input_welfare)
    );

    let reprice_drop_pct = if ucp.input_fills > 0 {
        ucp.dropped_by_reprice as f64 / ucp.input_fills as f64 * 100.0
    } else {
        0.0
    };
    println!(
        "  Reprice:  {:>6} survived, {} dropped ({:.1}%)",
        ucp.after_reprice_fills, ucp.dropped_by_reprice, reprice_drop_pct
    );
    println!(
        "  Trim:     {:>6} survived, {} trimmed",
        ucp.after_trim_fills, ucp.dropped_by_trim
    );
    println!(
        "  Final:    {:>6} fills, {} welfare",
        ucp.final_fills,
        format_welfare(ucp.final_welfare)
    );
    println!("  Retention: {:.1}%", ucp.welfare_retention_pct);

    if !ucp.market_imbalances.is_empty() {
        let top_n = ucp.market_imbalances.len().min(5);
        let top: Vec<String> = ucp.market_imbalances[..top_n]
            .iter()
            .map(|(mid, yes, no, excess)| {
                format!(
                    "M{} (YES={}, NO={}, excess={})",
                    mid.0, yes, no, excess
                )
            })
            .collect();
        println!("  Top imbalanced: {}", top.join(", "));
    }

    println!();
}

fn print_iteration_convergence(stats: &[IterationStats]) {
    println!("Fixed-Point Convergence:");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Iter",
            "Welfare",
            "Δ Welfare",
            "Volume",
            "Δ Volume",
            "Fills",
            "PD Fills",
            "Bundle",
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
        Cell::new(format_qty(stats.bundle_volume)),
        Cell::new(format_welfare(stats.bundle_welfare)).fg(Color::Cyan),
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
    Milp,
    Pipeline,
    Negrisk,
    Dual,
    Smoothed,
    All,
}

fn parse_solver_choice(args: &[String]) -> SolverChoice {
    match get_arg_value(args, "--solver").as_deref() {
        Some("milp") => SolverChoice::Milp,
        Some("pipeline") => SolverChoice::Pipeline,
        Some("negrisk") => SolverChoice::Negrisk,
        Some("dual") => SolverChoice::Dual,
        Some("smoothed") => SolverChoice::Smoothed,
        Some("all") => SolverChoice::All,
        _ => SolverChoice::Pipeline, // Default to pipeline
    }
}

fn parse_milp_timeout(args: &[String]) -> Option<f64> {
    get_arg_value(args, "--milp-timeout").and_then(|v| v.parse().ok())
}

fn parse_mm_mode(args: &[String]) -> MmBudgetMode {
    match get_arg_value(args, "--mm-mode").as_deref() {
        Some("exact") => MmBudgetMode::Exact,
        Some("mccormick") => MmBudgetMode::McCormick,
        Some("ignore") => MmBudgetMode::Ignore,
        _ => MmBudgetMode::Exact, // Default: exact bilinear via SCIP MIQCQP
    }
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

fn create_milp_solver(milp_timeout: Option<f64>, mm_mode: MmBudgetMode) -> MilpSolver {
    let timeout = milp_timeout.unwrap_or(5.0);
    MilpSolver::with_config(MilpConfig {
        timeout_secs: Some(timeout),
        gap_tolerance: 0.0,
        mm_budget_mode: mm_mode,
    })
}


#[derive(Default)]
struct SolverResults {
    name: String,
    total_welfare: i64,
    total_filled: usize,
    total_orders: usize,
    total_volume: u64,
    total_time_secs: f64,
    batches: usize,
    verification: Option<VerificationResult>,
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

/// Per-solver data for gap analysis.
struct SolverDetail {
    name: String,
    result: matching_solver::MatchingResult,
    clearing_prices: HashMap<MarketId, Vec<u64>>,
    is_valid: bool,
}

/// Data for gap analysis between solvers (collected when --solver all -v).
struct GapAnalysisData {
    problem: Problem,
    solver_details: Vec<SolverDetail>,
}

/// Expand a solver choice into individual choices for comparison.
fn expand_solver_choices(choice: &SolverChoice) -> Vec<SolverChoice> {
    match choice {
        SolverChoice::All => vec![
            SolverChoice::Milp,
            SolverChoice::Negrisk,
            SolverChoice::Dual,
            SolverChoice::Smoothed,
        ],
        other => vec![other.clone()],
    }
}

/// Get the display name for a solver choice.
fn solver_display_name(choice: &SolverChoice, milp_timeout: Option<f64>) -> String {
    match choice {
        SolverChoice::Milp => {
            if milp_timeout.is_some() {
                "MILP (time-limited)".to_string()
            } else {
                "MILP".to_string()
            }
        }
        SolverChoice::Pipeline => "Pipeline".to_string(),
        SolverChoice::Negrisk => "Negrisk".to_string(),
        SolverChoice::Dual => "Dual Decomposition".to_string(),
        SolverChoice::Smoothed => "Smoothed Gradient".to_string(),
        SolverChoice::All => "All".to_string(),
    }
}

/// Run a single solver choice on a problem and return (MatchingResult, witness for verification).
fn run_solver_with_witness(
    choice: &SolverChoice,
    problem: &Problem,
    milp_timeout: Option<f64>,
    mm_mode: MmBudgetMode,
) -> (matching_solver::MatchingResult, BlockWitness) {
    match choice {
        SolverChoice::Milp => {
            let milp = create_milp_solver(milp_timeout, mm_mode);
            let milp_result = milp.solve_with_status(problem);
            let witness = witness_from_milp(problem, &milp_result);
            (milp_result.result, witness)
        }
        SolverChoice::Smoothed => {
            let solver = matching_solver::SmoothedSolver::new();
            let pipeline_result = solver.solve(problem);
            let witness = witness_from_problem(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        SolverChoice::Pipeline | SolverChoice::Negrisk | SolverChoice::Dual => {
            let pipeline = match choice {
                SolverChoice::Negrisk => Pipeline::with_negrisk(),
                SolverChoice::Dual => Pipeline::with_dual_decomposition(),
                _ => Pipeline::current(),
            };
            let pipeline_result = pipeline.solve(problem);
            let witness = witness_from_pipeline(problem, &pipeline_result);
            (pipeline_result.result, witness)
        }
        SolverChoice::All => unreachable!("expand_solver_choices should be called first"),
    }
}

/// Build a BlockWitness from a PipelineResult (includes arb orders for position balance).
fn witness_from_pipeline(problem: &Problem, result: &PipelineResult) -> BlockWitness {
    let mut problem_with_arb = problem.clone();
    if let Some(ref negrisk) = result.negrisk {
        for order in &negrisk.arbitrage_orders {
            problem_with_arb.orders.push(order.clone());
        }
    }
    witness_from_problem(&problem_with_arb, result)
}

/// Build a BlockWitness from a MilpResult.
/// Includes synthetic arb orders so verifier can validate position balance.
fn witness_from_milp(
    problem: &Problem,
    result: &matching_solver::MilpResult,
) -> BlockWitness {
    let mut all_orders: Vec<&Order> = problem.orders.iter().collect();
    all_orders.extend(result.arbitrage_orders.iter());

    let witness_orders: Vec<WitnessOrder> = all_orders
        .iter()
        .map(|o| WitnessOrder {
            order: (*o).clone(),
            account_id: 0,
            is_mm: problem
                .mm_constraints
                .iter()
                .any(|mm| mm.order_ids.contains(&o.id)),
        })
        .collect();

    BlockWitness {
        header: WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            order_count: all_orders.len() as u32,
            fill_count: result.result.fills.len() as u32,
            timestamp_ms: 0,
        },
        previous_header: None,
        orders: witness_orders,
        rejections: vec![],
        fills: result.result.fills.clone(),
        clearing_prices: result.clearing_prices.clone(),
        total_welfare: result.result.total_welfare,
        mm_constraints: problem.mm_constraints.clone(),
        market_groups: problem.market_groups.clone(),
        pre_state: vec![],
        post_state: vec![],
        resolved_markets: vec![],
    }
}

fn run_simulation(
    base_config: &ScenarioConfig,
    solver_choice: &SolverChoice,
    milp_timeout: Option<f64>,
    mm_mode: MmBudgetMode,
    num_batches: usize,
    verbose: bool,
    export_comparison: Option<&str>,
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

        let problem = generate_scenario(config);

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
                run_solver_with_witness(choice, &problem, milp_timeout, mm_mode);

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

fn print_results(results: &[SolverResults], choice: &SolverChoice) {
    println!("\n========================================");
    println!("              RESULTS                   ");
    println!("========================================\n");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Solver",
            "Welfare",
            "Fill %",
            "Volume",
            "Time (avg)",
            "Verified",
        ]);

    // Best welfare among VALID solvers only (invalid results may have inflated welfare)
    let best_valid_welfare = results
        .iter()
        .filter(|r| r.verification.as_ref().is_some_and(|v| v.valid))
        .map(|r| r.mean_welfare())
        .fold(0.0, f64::max);
    let best_welfare = if best_valid_welfare > 0.0 {
        best_valid_welfare
    } else {
        results.iter().map(|r| r.mean_welfare()).fold(0.0, f64::max)
    };

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

        let is_valid = result.verification.as_ref().is_some_and(|v| v.valid);
        let welfare_cell = if !is_valid {
            Cell::new(&welfare_str).fg(Color::DarkRed)
        } else if gap < 0.1 {
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

        let volume = if result.batches > 0 {
            result.total_volume / result.batches as u64
        } else {
            0
        };

        let verified_cell = match &result.verification {
            Some(v) if v.valid => Cell::new("\u{2713} VALID").fg(Color::Green),
            Some(v) => {
                let n = v.violations.len();
                Cell::new(format!("\u{2717} {} violations", n)).fg(Color::Red)
            }
            None => Cell::new("-"),
        };

        table.add_row(vec![
            Cell::new(&result.name),
            welfare_cell,
            fill_cell,
            Cell::new(format_qty(volume)),
            Cell::new(format!("{:.3}s", result.mean_time())),
            verified_cell,
        ]);
    }

    println!("{table}");

    // Print violation details for invalid solvers
    for result in results {
        if let Some(ref v) = result.verification {
            if !v.valid {
                println!();
                println!(
                    "{}: {} violations",
                    result.name,
                    v.violations.len()
                );
                for (i, violation) in v.violations.iter().enumerate().take(10) {
                    println!("  {}. {:?}: {}", i + 1, violation.kind, violation.details);
                }
                if v.violations.len() > 10 {
                    println!("  ... and {} more", v.violations.len() - 10);
                }
            }
        }
    }

    if *choice == SolverChoice::All && results.len() >= 2 {
        // Find best VALID solver by welfare
        let valid_results: Vec<_> = results
            .iter()
            .filter(|r| r.verification.as_ref().is_some_and(|v| v.valid))
            .collect();

        if let Some(best) = valid_results.iter().max_by(|a, b| {
            a.mean_welfare()
                .partial_cmp(&b.mean_welfare())
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            println!();
            println!(
                "Best valid solver: {} (${:.2}K welfare)",
                best.name,
                best.mean_welfare() / 1e9 / 1e3
            );
        }
    }
}

// ============================================================================
// Gap Analysis
// ============================================================================

/// Print gap analysis comparing the best valid solver against each other valid solver.
fn print_gap_analysis(data: &GapAnalysisData) {
    // Find best valid solver by welfare
    let best_idx = data
        .solver_details
        .iter()
        .enumerate()
        .filter(|(_, d)| d.is_valid)
        .max_by_key(|(_, d)| d.result.total_welfare)
        .map(|(i, _)| i);

    let Some(best_idx) = best_idx else {
        println!("\nNo valid solver results for gap analysis.");
        return;
    };

    for (i, other) in data.solver_details.iter().enumerate() {
        if i == best_idx {
            continue;
        }
        if !other.is_valid {
            continue; // skip invalid solvers
        }
        print_solver_diff(&data.problem, &data.solver_details[best_idx], other);
    }
}

/// Print a detailed comparison between two solver results.
fn print_solver_diff(problem: &Problem, best: &SolverDetail, other: &SolverDetail) {
    let best_name = &best.name;
    let other_name = &other.name;
    let best_result = &best.result;
    let other_result = &other.result;

    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
    let mm_order_ids: HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|c| c.order_ids.iter().copied())
        .collect();
    let market_names: HashMap<MarketId, &str> = problem
        .markets
        .iter()
        .map(|m| (m.id, m.name.as_str()))
        .collect();

    // Compute welfare from fills (consistent with verifier)
    let compute_welfare = |fills: &[Fill]| -> i64 {
        fills
            .iter()
            .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
            .sum()
    };

    let best_welfare = compute_welfare(&best_result.fills);
    let other_welfare = compute_welfare(&other_result.fills);
    let gap = best_welfare - other_welfare;
    let gap_pct = if best_welfare > 0 {
        gap as f64 / best_welfare as f64 * 100.0
    } else {
        0.0
    };

    // ── Header ──
    println!();
    println!(
        "══════ Gap Analysis: {} ({}) vs {} ({}) ══════",
        best_name,
        format_welfare(best_welfare),
        other_name,
        format_welfare(other_welfare)
    );
    println!("Total gap: {} ({:.1}%)", format_welfare(gap), gap_pct);
    println!();

    // ── Welfare Breakdown ──
    let breakdown = |fills: &[Fill]| -> (i64, i64, i64) {
        let mut user_w: i64 = 0;
        let mut mm_w: i64 = 0;
        let mut bundle_w: i64 = 0;
        for f in fills {
            if let Some(order) = order_map.get(&f.order_id) {
                let w = f.welfare(order);
                if mm_order_ids.contains(&f.order_id) {
                    mm_w += w;
                } else {
                    user_w += w;
                }
                if order.num_markets > 1 {
                    bundle_w += w;
                }
            }
        }
        (user_w, mm_w, bundle_w)
    };

    let (best_user, best_mm, best_bundle) = breakdown(&best_result.fills);
    let (other_user, other_mm, other_bundle) = breakdown(&other_result.fills);

    println!("Welfare Breakdown:");
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "", best_name, other_name, "Gap"
    );
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "User orders",
        format_welfare(best_user),
        format_welfare(other_user),
        format_welfare(best_user - other_user)
    );
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "MM orders",
        format_welfare(best_mm),
        format_welfare(other_mm),
        format_welfare(best_mm - other_mm)
    );
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "Bundles",
        format_welfare(best_bundle),
        format_welfare(other_bundle),
        format_welfare(best_bundle - other_bundle)
    );
    println!();

    // ── Per-Market Comparison ──
    // Build per-market welfare maps (split bundle welfare evenly across markets)
    let market_welfare = |fills: &[Fill]| -> HashMap<MarketId, i64> {
        let mut map: HashMap<MarketId, i64> = HashMap::new();
        for f in fills {
            if let Some(order) = order_map.get(&f.order_id) {
                let w = f.welfare(order);
                if order.num_markets == 1 {
                    *map.entry(order.markets[0]).or_default() += w;
                } else {
                    let n = order.num_markets as i64;
                    for mid in order.active_markets() {
                        *map.entry(mid).or_default() += w / n;
                    }
                }
            }
        }
        map
    };

    let best_market_w = market_welfare(&best_result.fills);
    let other_market_w = market_welfare(&other_result.fills);

    let all_markets: HashSet<MarketId> = best_market_w
        .keys()
        .chain(other_market_w.keys())
        .copied()
        .collect();

    let mut market_gaps: Vec<_> = all_markets
        .iter()
        .map(|&mid| {
            let bw = best_market_w.get(&mid).copied().unwrap_or(0);
            let ow = other_market_w.get(&mid).copied().unwrap_or(0);
            (mid, bw, ow, bw - ow)
        })
        .collect();
    market_gaps.sort_by(|a, b| b.3.abs().cmp(&a.3.abs()));

    let best_price_yes = |mid: &MarketId| -> u64 {
        best.clearing_prices
            .get(mid)
            .and_then(|p| p.first().copied())
            .unwrap_or(0)
    };
    let other_price_yes = |mid: &MarketId| -> u64 {
        other
            .clearing_prices
            .get(mid)
            .and_then(|p| p.first().copied())
            .unwrap_or(0)
    };

    println!("Per-Market Comparison (top 10 by gap):");
    println!(
        "  {:<8} │ {:>11} │ {:>12} │ {:>8} │ {:>9} │ {:>10} │ {:>8}",
        "Market", "Best P(YES)", "Other P(YES)", "ΔPrice", "Best W$", "Other W$", "Gap$"
    );
    println!(
        "  {:<8}─┼─{:─>11}─┼─{:─>12}─┼─{:─>8}─┼─{:─>9}─┼─{:─>10}─┼─{:─>8}",
        "────────", "", "", "", "", "", ""
    );

    for (mid, bw, ow, gap_w) in market_gaps.iter().take(10) {
        let name = market_names.get(mid).copied().unwrap_or("?");
        let bp = best_price_yes(mid);
        let op = other_price_yes(mid);
        let dp_pp = (bp as f64 - op as f64) / 1e7;

        println!(
            "  {:<8} │ {:>10.1}% │ {:>11.1}% │ {:>+7.1}pp │ {:>9} │ {:>10} │ {:>8}",
            name,
            bp as f64 / 1e7,
            op as f64 / 1e7,
            dp_pp,
            format_welfare(*bw),
            format_welfare(*ow),
            format_welfare(*gap_w)
        );
    }
    println!();

    // ── MM Budget ──
    if !problem.mm_constraints.is_empty() {
        println!("MM Budget:");

        let mm_fills_map = |fills: &[Fill]| -> HashMap<u64, (u64, u64)> {
            fills
                .iter()
                .filter(|f| mm_order_ids.contains(&f.order_id))
                .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
                .collect()
        };

        let best_mm_fills = mm_fills_map(&best_result.fills);
        let other_mm_fills = mm_fills_map(&other_result.fills);

        for mm in &problem.mm_constraints {
            let best_cap = mm.capital_used(&best_mm_fills);
            let other_cap = mm.capital_used(&other_mm_fills);
            let budget = mm.max_capital;

            let best_active: usize = mm
                .order_ids
                .iter()
                .filter(|id| best_mm_fills.contains_key(id))
                .count();
            let other_active: usize = mm
                .order_ids
                .iter()
                .filter(|id| other_mm_fills.contains_key(id))
                .count();

            let best_util = if budget > 0 {
                best_cap as f64 / budget as f64 * 100.0
            } else {
                0.0
            };
            let other_util = if budget > 0 {
                other_cap as f64 / budget as f64 * 100.0
            } else {
                0.0
            };

            println!(
                "  {}: {} of {} ({:.1}%), {} orders filled",
                best_name,
                format_price(best_cap),
                format_price(budget),
                best_util,
                best_active
            );
            println!(
                "  {}: {} of {} ({:.1}%), {} orders filled",
                other_name,
                format_price(other_cap),
                format_price(budget),
                other_util,
                other_active
            );
        }
        println!();
    }

    // ── Differential Fills ──
    let best_fill_ids: HashSet<u64> = best_result.fills.iter().map(|f| f.order_id).collect();
    let other_fill_ids: HashSet<u64> = other_result.fills.iter().map(|f| f.order_id).collect();

    // Fills in best but not in other, sorted by welfare
    let mut best_only: Vec<(&Fill, &Order, i64, String, &str)> = best_result
        .fills
        .iter()
        .filter(|f| !other_fill_ids.contains(&f.order_id))
        .filter_map(|f| {
            order_map.get(&f.order_id).map(|&order| {
                let w = f.welfare(order);
                let market_name = if order.num_markets == 1 {
                    market_names
                        .get(&order.markets[0])
                        .unwrap_or(&"?")
                        .to_string()
                } else {
                    format!("bundle({})", order.num_markets)
                };
                let order_type = if order.is_seller() { "sell" } else { "buy" };
                (f, order, w, market_name, order_type)
            })
        })
        .collect();
    best_only.sort_by(|a, b| b.2.abs().cmp(&a.2.abs()));

    if !best_only.is_empty() {
        println!(
            "Top Differential Fills (in {} only, by welfare):",
            best_name
        );
        println!(
            "  {:>3} │ {:>7} │ {:>8} │ {:>8} │ {:>7} │ {:>7} │ {:>5} │ {:>8}",
            "#", "Order", "Type", "Market", "Limit", "Price", "Qty", "W$"
        );
        println!(
            "  {:─>3}─┼─{:─>7}─┼─{:─>8}─┼─{:─>8}─┼─{:─>7}─┼─{:─>7}─┼─{:─>5}─┼─{:─>8}",
            "", "", "", "", "", "", "", ""
        );

        for (i, (fill, order, welfare, market, order_type)) in
            best_only.iter().take(15).enumerate()
        {
            println!(
                "  {:>3} │ {:>7} │ {:>8} │ {:>8} │ {:>6.1}c │ {:>6.1}c │ {:>5} │ {:>8}",
                i + 1,
                fill.order_id,
                order_type,
                market,
                order.limit_price as f64 / 1e7,
                fill.fill_price as f64 / 1e7,
                fill.fill_qty,
                format_welfare(*welfare)
            );
        }
        println!();
    }

    // Summary of fills unique to other
    let other_only_welfare: i64 = other_result
        .fills
        .iter()
        .filter(|f| !best_fill_ids.contains(&f.order_id))
        .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
        .sum();
    let other_only_count = other_result
        .fills
        .iter()
        .filter(|f| !best_fill_ids.contains(&f.order_id))
        .count();

    if other_only_count > 0 {
        println!(
            "Fills unique to {}: {} orders, {} welfare",
            other_name,
            other_only_count,
            format_welfare(other_only_welfare)
        );
        println!();
    }
}

// ============================================================================
// Comparison JSON export
// ============================================================================

/// Serialize an order into a JSON value with all relevant fields.
fn order_to_json(order: &Order) -> serde_json::Value {
    let markets: Vec<_> = (0..order.num_markets as usize)
        .map(|i| order.markets[i].0)
        .collect();
    let payoffs: Vec<_> = (0..order.num_states as usize)
        .map(|i| order.payoffs[i] as i64)
        .collect();

    serde_json::json!({
        "id": order.id,
        "markets": markets,
        "payoffs": payoffs,
        "limit_price": order.limit_price,
        "limit_price_cents": order.limit_price as f64 / 1e7,
        "min_fill": order.min_fill,
        "max_fill": order.max_fill,
        "is_seller": order.is_seller(),
        "is_aon": order.is_all_or_none(),
        "num_markets": order.num_markets,
    })
}

/// Build a detailed JSON comparison of solver results for offline analysis.
fn build_comparison_json(
    problem: &Problem,
    solver_results: &[(String, matching_solver::MatchingResult, BlockWitness)],
) -> String {
    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

    // MM order IDs
    let mm_order_ids: HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|c| c.order_ids.iter().copied())
        .collect();

    // Serialize orders
    let orders_json: Vec<_> = problem
        .orders
        .iter()
        .map(|o| {
            let mut j = order_to_json(o);
            j["is_mm"] = serde_json::json!(mm_order_ids.contains(&o.id));
            j
        })
        .collect();

    // Serialize market groups
    let groups_json: Vec<_> = problem
        .market_groups
        .iter()
        .map(|g| {
            serde_json::json!({
                "name": g.name,
                "markets": g.markets.iter().map(|m| m.0).collect::<Vec<_>>(),
            })
        })
        .collect();

    // Serialize markets
    let markets_json: Vec<_> = problem
        .markets
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id.0,
                "name": m.name,
            })
        })
        .collect();

    // Per-solver detailed results
    let solvers_json: Vec<_> = solver_results
        .iter()
        .map(|(name, result, witness)| {
            // Per-fill details with order context
            let fills_json: Vec<_> = result
                .fills
                .iter()
                .map(|f| {
                    let order = order_map.get(&f.order_id);
                    let welfare = order.map(|o| f.welfare(o)).unwrap_or(0);
                    let is_mm = mm_order_ids.contains(&f.order_id);
                    let is_bundle = order.map(|o| o.num_markets > 1).unwrap_or(false);
                    let markets: Vec<u32> = order
                        .map(|o| {
                            (0..o.num_markets as usize)
                                .map(|i| o.markets[i].0)
                                .collect()
                        })
                        .unwrap_or_default();

                    serde_json::json!({
                        "order_id": f.order_id,
                        "fill_qty": f.fill_qty,
                        "fill_price": f.fill_price,
                        "fill_price_cents": f.fill_price as f64 / 1e7,
                        "welfare": welfare,
                        "welfare_dollars": welfare as f64 / 1e9,
                        "is_mm": is_mm,
                        "is_bundle": is_bundle,
                        "markets": markets,
                        "limit_price": order.map(|o| o.limit_price).unwrap_or(0),
                        "limit_price_cents": order.map(|o| o.limit_price as f64 / 1e7).unwrap_or(0.0),
                        "max_fill": order.map(|o| o.max_fill).unwrap_or(0),
                        "is_seller": order.map(|o| o.is_seller()).unwrap_or(false),
                    })
                })
                .collect();

            // Clearing prices
            let prices_json: HashMap<String, _> = witness
                .clearing_prices
                .iter()
                .map(|(mid, prices)| {
                    let pcts: Vec<f64> = prices.iter().map(|&p| p as f64 / 1e7).collect();
                    (
                        format!("M{}", mid.0),
                        serde_json::json!({ "nanos": prices, "pct": pcts }),
                    )
                })
                .collect();

            // Per-market fill volume and welfare
            let mut market_vol: HashMap<MarketId, u64> = HashMap::new();
            let mut market_welfare: HashMap<MarketId, i64> = HashMap::new();
            let mut market_fills: HashMap<MarketId, usize> = HashMap::new();
            for f in &result.fills {
                if let Some(order) = order_map.get(&f.order_id) {
                    let w = f.welfare(order);
                    for mid in order.active_markets() {
                        *market_vol.entry(mid).or_default() += f.fill_qty;
                        *market_welfare.entry(mid).or_default() += w;
                        *market_fills.entry(mid).or_default() += 1;
                    }
                }
            }
            let market_stats_json: HashMap<String, _> = market_vol
                .keys()
                .map(|mid| {
                    let vol = market_vol.get(mid).copied().unwrap_or(0);
                    let w = market_welfare.get(mid).copied().unwrap_or(0);
                    let fills = market_fills.get(mid).copied().unwrap_or(0);
                    (
                        format!("M{}", mid.0),
                        serde_json::json!({
                            "volume": vol,
                            "welfare": w,
                            "welfare_dollars": w as f64 / 1e9,
                            "fills": fills,
                        }),
                    )
                })
                .collect();

            // Unfilled orders
            let filled_ids: HashSet<u64> = result.fills.iter().map(|f| f.order_id).collect();
            let unfilled: Vec<_> = problem
                .orders
                .iter()
                .filter(|o| !filled_ids.contains(&o.id))
                .map(|o| {
                    serde_json::json!({
                        "id": o.id,
                        "limit_price_cents": o.limit_price as f64 / 1e7,
                        "max_fill": o.max_fill,
                        "is_bundle": o.num_markets > 1,
                        "is_aon": o.is_all_or_none(),
                        "is_mm": mm_order_ids.contains(&o.id),
                        "is_seller": o.is_seller(),
                        "markets": (0..o.num_markets as usize).map(|i| o.markets[i].0).collect::<Vec<_>>(),
                    })
                })
                .collect();

            // Welfare breakdown
            let user_welfare: i64 = result
                .fills
                .iter()
                .filter(|f| !mm_order_ids.contains(&f.order_id))
                .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
                .sum();
            let mm_welfare: i64 = result
                .fills
                .iter()
                .filter(|f| mm_order_ids.contains(&f.order_id))
                .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
                .sum();
            let bundle_welfare: i64 = result
                .fills
                .iter()
                .filter_map(|f| {
                    order_map.get(&f.order_id).and_then(|o| {
                        if o.num_markets > 1 {
                            Some(f.welfare(o))
                        } else {
                            None
                        }
                    })
                })
                .sum();

            serde_json::json!({
                "solver": name,
                "total_welfare": result.total_welfare,
                "total_welfare_dollars": result.total_welfare as f64 / 1e9,
                "orders_filled": result.orders_filled,
                "orders_unfilled_liquidity": result.orders_unfilled_liquidity,
                "orders_unfilled_aon": result.orders_unfilled_aon,
                "total_quantity_filled": result.total_quantity_filled,
                "welfare_breakdown": {
                    "user_dollars": user_welfare as f64 / 1e9,
                    "mm_dollars": mm_welfare as f64 / 1e9,
                    "bundle_dollars": bundle_welfare as f64 / 1e9,
                },
                "fills": fills_json,
                "clearing_prices": prices_json,
                "market_stats": market_stats_json,
                "unfilled_orders": unfilled,
            })
        })
        .collect();

    let root = serde_json::json!({
        "problem": {
            "num_markets": problem.markets.len(),
            "num_orders": problem.orders.len(),
            "num_mm_constraints": problem.mm_constraints.len(),
            "num_market_groups": problem.market_groups.len(),
        },
        "orders": orders_json,
        "markets": markets_json,
        "market_groups": groups_json,
        "solvers": solvers_json,
    });

    serde_json::to_string_pretty(&root).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}
