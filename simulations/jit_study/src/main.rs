use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;

mod types;
mod batch;
mod jit;
mod scenarios;
mod analysis;
mod simulation;

use types::*;
use batch::*;
use jit::*;
use scenarios::*;

use simulation::{Simulation, SimulationConfig};
use simulation::tax::{FixedRateTax, DynamicTax, ProportionalHarmTax, NoTax, TaxCalculator};
use simulation::metrics::{SweepCollector, SweepResult};
use simulation::market_structure::{
    MarketStructure, ComparisonConfig, compare_structures,
    print_comparison_table, print_comparison_insights,
};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "simulate" {
        run_tax_simulation();
    } else if args.len() > 1 && args[1] == "sweep" {
        run_parameter_sweep();
    } else if args.len() > 1 && args[1] == "compare-structures" {
        run_structure_comparison();
    } else {
        run_jit_study();
    }
}

// =============================================================================
// TAX SIMULATION (Agent-Based)
// =============================================================================

fn run_tax_simulation() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║            DISPLACEMENT TAX SIMULATION                           ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let config = SimulationConfig::default();

    println!("Configuration:");
    println!("  Rounds: {}", config.num_rounds);
    println!("  Passive LPs: {}", config.num_passive_lps);
    println!("  JIT MMs: {}", config.num_jit_mms);
    println!("  Noise Traders: {}", config.num_noise_traders);
    println!("  True value mean: {} bps ({})", config.true_value_mean, config.true_value_mean as f64 / 10000.0);
    println!("  Volatility: {} bps/round", config.true_value_volatility);
    println!("  LP spread: {} bps", config.lp_spread_bps);
    println!("  JIT profit threshold: {} bps", config.jit_profit_threshold_bps);
    println!();

    println!("=== RUNNING SIMULATIONS ===\n");

    // NoTax
    print!("Running NoTax... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_no_tax(config.clone());
    print_sim_result(&metrics);

    // FixedRate 50bps
    print!("Running FixedRate 50bps... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_fixed_rate(config.clone(), 50);
    print_sim_result(&metrics);

    // FixedRate 100bps
    print!("Running FixedRate 100bps... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_fixed_rate(config.clone(), 100);
    print_sim_result(&metrics);

    // FixedRate 200bps
    print!("Running FixedRate 200bps... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_fixed_rate(config.clone(), 200);
    print_sim_result(&metrics);

    // Dynamic (target 25%)
    print!("Running Dynamic (target 25%)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_dynamic(config.clone(), 25);
    print_sim_result(&metrics);

    // Dynamic (target 50%)
    print!("Running Dynamic (target 50%)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_dynamic(config.clone(), 50);
    print_sim_result(&metrics);

    // ProportionalHarm 1.0x
    print!("Running ProportionalHarm 1.0x... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_proportional_harm(config.clone(), 100);
    print_sim_result(&metrics);

    // ProportionalHarm 1.5x
    print!("Running ProportionalHarm 1.5x... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let metrics = run_sim_proportional_harm(config.clone(), 150);
    print_sim_result(&metrics);
}

fn print_sim_result(metrics: &simulation::metrics::AggregateMetrics) {
    println!("Done.");
    println!("  JIT participation: {:.1}%", metrics.jit_participation_rate * 100.0);
    println!("  Mean displacement: {:.1}", metrics.mean_displacement);
    println!("  Passive LP P&L: {:.0}/round", metrics.mean_passive_lp_pnl_per_round);
    println!("  JIT MM P&L: {:.0}/round", metrics.mean_jit_mm_pnl_per_round);
    println!("  Total welfare: {}", metrics.total_welfare);
    println!();
}

fn run_sim_no_tax(config: SimulationConfig) -> simulation::metrics::AggregateMetrics {
    let mut sim = Simulation::new(config, NoTax);
    sim.run();
    sim.metrics.aggregate()
}

fn run_sim_fixed_rate(config: SimulationConfig, rate_bps: u64) -> simulation::metrics::AggregateMetrics {
    let mut sim = Simulation::new(config, FixedRateTax::new(rate_bps));
    sim.run();
    sim.metrics.aggregate()
}

fn run_sim_dynamic(config: SimulationConfig, target_pct: u8) -> simulation::metrics::AggregateMetrics {
    let mut sim = Simulation::new(config, DynamicTax::new(100, target_pct, 5));
    sim.run();
    sim.metrics.aggregate()
}

fn run_sim_proportional_harm(config: SimulationConfig, coeff: u64) -> simulation::metrics::AggregateMetrics {
    let mut sim = Simulation::new(config, ProportionalHarmTax::new(coeff));
    sim.run();
    sim.metrics.aggregate()
}

// =============================================================================
// PARAMETER SWEEP
// =============================================================================

fn run_parameter_sweep() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║            PARAMETER SWEEP                                       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let mut sweep_collector = SweepCollector::new();

    let base_config = SimulationConfig {
        num_rounds: 5_000, // Reduced for faster sweeps
        ..SimulationConfig::default()
    };

    // Fixed Rate Sweep: 0-500 bps (step 25)
    println!("=== FIXED RATE TAX SWEEP (0-500 bps) ===\n");

    for rate_bps in (0..=500).step_by(50) {
        print!("  Rate {}bps... ", rate_bps);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let tax = FixedRateTax::new(rate_bps as u64);
        let mut sim = Simulation::new(base_config.clone(), tax.clone());
        sim.run();
        let metrics = sim.metrics.aggregate();

        println!("JIT {:.1}%, LP P&L {:.0}",
            metrics.jit_participation_rate * 100.0,
            metrics.mean_passive_lp_pnl_per_round);

        sweep_collector.add_result(SweepResult {
            parameter_name: "rate_bps".to_string(),
            parameter_value: rate_bps as f64,
            tax_mechanism: tax.name(),
            metrics,
        });
    }

    // Dynamic Tax Sweep: target 5%-50%
    println!("\n=== DYNAMIC TAX SWEEP (target 5%-50%) ===\n");

    for target_pct in (5..=50).step_by(5) {
        print!("  Target {}%... ", target_pct);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let tax = DynamicTax::new(100, target_pct as u8, 5);
        let mut sim = Simulation::new(base_config.clone(), tax.clone());
        sim.run();
        let metrics = sim.metrics.aggregate();

        // Get final rate
        let final_rate = sim.tax_calculator.current_rate_bps();

        println!("JIT {:.1}%, Final rate {}bps, LP P&L {:.0}",
            metrics.jit_participation_rate * 100.0,
            final_rate,
            metrics.mean_passive_lp_pnl_per_round);

        sweep_collector.add_result(SweepResult {
            parameter_name: "target_pct".to_string(),
            parameter_value: target_pct as f64,
            tax_mechanism: tax.name(),
            metrics,
        });
    }

    // Proportional Harm Sweep: 0.5x-2.0x
    println!("\n=== PROPORTIONAL HARM TAX SWEEP (0.5x-2.0x) ===\n");

    for coeff_x10 in (5..=20).step_by(2) {
        let coeff = coeff_x10 as f64 / 10.0;
        print!("  Coeff {:.1}x... ", coeff);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let tax = ProportionalHarmTax::new((coeff * 100.0) as u64);
        let mut sim = Simulation::new(base_config.clone(), tax.clone());
        sim.run();
        let metrics = sim.metrics.aggregate();

        println!("JIT {:.1}%, LP P&L {:.0}",
            metrics.jit_participation_rate * 100.0,
            metrics.mean_passive_lp_pnl_per_round);

        sweep_collector.add_result(SweepResult {
            parameter_name: "coefficient".to_string(),
            parameter_value: coeff,
            tax_mechanism: tax.name(),
            metrics,
        });
    }

    // Print summary
    sweep_collector.print_summary();

    // Export results
    if let Err(e) = sweep_collector.export_csv("sweep_results.csv") {
        eprintln!("Failed to export CSV: {}", e);
    } else {
        println!("\nResults exported to sweep_results.csv");
    }

    // Print analysis
    print_sweep_analysis(&sweep_collector);
}

fn print_sweep_analysis(collector: &SweepCollector) {
    println!("\n=== ANALYSIS ===\n");

    // Find optimal fixed rate for LP profitability
    let fixed_rate_results: Vec<_> = collector.results.iter()
        .filter(|r| r.tax_mechanism.starts_with("FixedRate"))
        .collect();

    if !fixed_rate_results.is_empty() {
        let best_for_lps = fixed_rate_results.iter()
            .max_by(|a, b| a.metrics.total_passive_lp_pnl.cmp(&b.metrics.total_passive_lp_pnl))
            .unwrap();

        let best_for_welfare = fixed_rate_results.iter()
            .max_by(|a, b| a.metrics.total_welfare.cmp(&b.metrics.total_welfare))
            .unwrap();

        println!("FIXED RATE TAX:");
        println!("  Best for LP profitability: {}bps (P&L: {:.0})",
            best_for_lps.parameter_value,
            best_for_lps.metrics.mean_passive_lp_pnl_per_round);
        println!("  Best for total welfare: {}bps (welfare: {})",
            best_for_welfare.parameter_value,
            best_for_welfare.metrics.total_welfare);
        println!();
    }

    // Check if dynamic tax converges
    let dynamic_results: Vec<_> = collector.results.iter()
        .filter(|r| r.tax_mechanism.starts_with("Dynamic"))
        .collect();

    if !dynamic_results.is_empty() {
        println!("DYNAMIC TAX:");
        for result in &dynamic_results {
            let target = result.parameter_value;
            let actual = result.metrics.jit_participation_rate * 100.0;
            let diff = (actual - target).abs();
            println!("  Target {}%: Actual {:.1}% (diff: {:.1}%)",
                target, actual, diff);
        }
        println!();
    }

    // Compare mechanisms at similar JIT participation
    println!("MECHANISM COMPARISON (at ~25% JIT participation):");

    let target_jit = 0.25;
    let tolerance = 0.15;

    for result in &collector.results {
        if (result.metrics.jit_participation_rate - target_jit).abs() < tolerance {
            println!("  {}: JIT {:.1}%, LP P&L {:.0}, Welfare {}",
                result.tax_mechanism,
                result.metrics.jit_participation_rate * 100.0,
                result.metrics.mean_passive_lp_pnl_per_round,
                result.metrics.total_welfare);
        }
    }
}

// =============================================================================
// ORIGINAL JIT STUDY
// =============================================================================

// =============================================================================
// MARKET STRUCTURE COMPARISON
// =============================================================================

fn run_structure_comparison() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║            MARKET STRUCTURE COMPARISON                           ║");
    println!("║     CLOB vs Private FBA + JIT Liquidity                          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let config = ComparisonConfig::default();

    println!("Configuration:");
    println!("  Rounds: {}", config.num_rounds);
    println!("  Passive MMs: {}", config.num_passive_mms);
    println!("  Noise Traders: {}", config.num_noise_traders);
    println!("  MM spread: {} bps", config.mm_spread_bps);
    println!("  Batch duration: 60s (for FBA)");
    println!("  JIT window: 1s");
    println!();

    // Define structures to compare
    let structures = vec![
        // CLOB baseline
        MarketStructure::clob(config.mm_spread_bps, config.mm_order_size * 2),

        // FBA + Backrun-only JIT (no displacement allowed)
        MarketStructure::fba_backrun(60000, 1000),

        // FBA + Taxed Displacement (50bps tax)
        MarketStructure::fba_taxed_displacement(60000, 1000, 50),

        // FBA + Taxed Displacement (100bps tax)
        MarketStructure::fba_taxed_displacement(60000, 1000, 100),
    ];

    println!("Running {} market structure simulations...\n", structures.len());

    let results = compare_structures(&structures, &config);

    // Print results
    print_comparison_table(&results);
    print_comparison_insights(&results);

    println!("\n=== KEY QUESTIONS ANSWERED ===\n");

    // Q1: Does backrun-only provide enough liquidity?
    println!("Q1: Does backrun-only provide enough liquidity?");
    let backrun_result = results.iter().find(|r| r.structure_name == "FBA+Backrun");
    let clob_result = results.iter().find(|r| r.structure_name.starts_with("CLOB"));
    if let (Some(br), Some(clob)) = (backrun_result, clob_result) {
        let volume_ratio = br.metrics.volume_per_round / clob.metrics.volume_per_round;
        println!("   Volume ratio (FBA+Backrun / CLOB): {:.2}x", volume_ratio);
        if volume_ratio > 0.9 {
            println!("   → YES: Backrun-only provides comparable liquidity");
        } else {
            println!("   → PARTIAL: Some volume reduction vs CLOB");
        }
    }
    println!();

    // Q2: Is taxed displacement better than backrun-only?
    println!("Q2: Is taxed displacement better than backrun-only?");
    let taxed_result = results.iter().find(|r| r.structure_name.contains("TaxedDisp(100"));
    if let (Some(br), Some(taxed)) = (backrun_result, taxed_result) {
        let user_impact_diff = taxed.metrics.avg_price_impact_bps - br.metrics.avg_price_impact_bps;
        let passive_mm_diff = taxed.metrics.mean_passive_lp_pnl_per_round - br.metrics.mean_passive_lp_pnl_per_round;
        println!("   Price impact change: {:+.1} bps", user_impact_diff);
        println!("   Passive MM P&L change: {:+.1}/round", passive_mm_diff);
        if user_impact_diff < 0.0 && passive_mm_diff > -50.0 {
            println!("   → MIXED: Better for users, hurts passive MMs moderately");
        } else if passive_mm_diff < -50.0 {
            println!("   → CAUTION: Significantly hurts passive MMs");
        } else {
            println!("   → BACKRUN-ONLY appears cleaner");
        }
    }
    println!();

    // Q3: How does FBA + JIT compare to CLOB overall?
    println!("Q3: How does Private FBA + JIT compare to CLOB?");
    if let (Some(br), Some(clob)) = (backrun_result, clob_result) {
        let price_impact_diff = br.metrics.avg_price_impact_bps - clob.metrics.avg_price_impact_bps;
        println!("   Price impact: {:+.1} bps vs CLOB", price_impact_diff);

        // Capital efficiency
        let passive_eff = clob.metrics.passive_mm_capital_efficiency;
        let jit_eff = br.adjusted_jit_capital_efficiency;
        if passive_eff > 0.0 && jit_eff > 0.0 {
            println!("   Capital efficiency (JIT): {:.1}x better than CLOB MM", jit_eff / passive_eff);
        }

        println!("   → FBA+JIT trades execution speed for capital efficiency + MEV protection");
    }
}

fn run_jit_study() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                    JIT BEHAVIOR STUDY                            ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    println!("Usage:");
    println!("  cargo run                    # Run original JIT behavior study");
    println!("  cargo run -- simulate        # Run tax simulation");
    println!("  cargo run -- sweep           # Run parameter sweep");
    println!("  cargo run -- compare-structures  # Compare CLOB vs FBA+JIT");
    println!();

    // First verify correctness with a simple test
    println!("=== CORRECTNESS VERIFICATION ===\n");
    verify_batch_solver();

    // Run all scenarios
    println!("\n=== SCENARIO ANALYSIS ===\n");

    let scenarios = all_scenarios();
    let mut results: Vec<ScenarioResult> = vec![];

    for scenario in scenarios {
        let result = analyze_scenario(&scenario);
        print_scenario_result(&result);
        results.push(result);
    }

    // Summary table
    println!("\n=== SUMMARY TABLE ===\n");
    print_summary_table(&results);

    // Key insights
    println!("\n=== KEY INSIGHTS ===\n");
    print_insights(&results);
}

// =============================================================================
// CORRECTNESS VERIFICATION
// =============================================================================

fn verify_batch_solver() {
    println!("Test 1: Simple crossing");
    println!("  Buyer: 100 @ 0.60 (willing to pay up to 0.60)");
    println!("  Seller: 100 @ 0.40 (willing to sell at 0.40 or above)");
    println!("  Expected: clear at midpoint 0.50, volume = 100");

    let orders = vec![
        Order::buy(1, dec!(100), dec!(0.60)),
        Order::sell(2, dec!(100), dec!(0.40)),
    ];
    let solution = solve_batch(&orders);

    println!("  Result: price={}, volume={}", solution.clearing_price, solution.total_volume);
    assert_eq!(solution.clearing_price, dec!(0.50));
    assert_eq!(solution.total_volume, dec!(100));
    println!("  ✓ PASS\n");

    println!("Test 2: No crossing (wide spread)");
    println!("  Buyer: 100 @ 0.40 (only willing to pay 0.40)");
    println!("  Seller: 100 @ 0.60 (won't sell below 0.60)");
    println!("  Expected: no trade (volume = 0)");

    let orders = vec![
        Order::buy(1, dec!(100), dec!(0.40)),
        Order::sell(2, dec!(100), dec!(0.60)),
    ];
    let solution = solve_batch(&orders);

    println!("  Result: price={}, volume={}", solution.clearing_price, solution.total_volume);
    assert_eq!(solution.total_volume, dec!(0));
    println!("  ✓ PASS\n");

    println!("Test 3: Pro-rata fill when excess demand");
    println!("  Buyer 1: 100 @ 0.55");
    println!("  Buyer 2: 100 @ 0.55");
    println!("  Seller: 100 @ 0.45");
    println!("  Expected: both buyers fill 50 each (pro-rata)");

    let orders = vec![
        Order::buy(1, dec!(100), dec!(0.55)),
        Order::buy(2, dec!(100), dec!(0.55)),
        Order::sell(3, dec!(100), dec!(0.45)),
    ];
    let solution = solve_batch(&orders);

    println!("  Result: price={}, volume={}", solution.clearing_price, solution.total_volume);
    let buyer1_fill = solution.fills.iter().find(|f| f.order_id == 1).map(|f| f.quantity).unwrap_or(dec!(0));
    let buyer2_fill = solution.fills.iter().find(|f| f.order_id == 2).map(|f| f.quantity).unwrap_or(dec!(0));
    println!("  Buyer 1 fill: {}, Buyer 2 fill: {}", buyer1_fill, buyer2_fill);
    assert_eq!(buyer1_fill, dec!(50));
    assert_eq!(buyer2_fill, dec!(50));
    println!("  ✓ PASS\n");

    println!("Test 4: Welfare calculation");
    println!("  Buyer: 100 @ 0.60, Seller: 100 @ 0.40, Clear @ 0.50");
    println!("  Buyer surplus: (0.60 - 0.50) * 100 = 10");
    println!("  Seller surplus: (0.50 - 0.40) * 100 = 10");
    println!("  Expected total welfare: 20");

    let orders = vec![
        Order::buy(1, dec!(100), dec!(0.60)),
        Order::sell(2, dec!(100), dec!(0.40)),
    ];
    let solution = solve_batch(&orders);

    // The clearing price might not be exactly 0.50, so let's calculate expected welfare
    let price = solution.clearing_price;
    let expected_welfare = (dec!(0.60) - price) * dec!(100) + (price - dec!(0.40)) * dec!(100);
    println!("  Result: welfare={}, expected={}", solution.welfare, expected_welfare);
    // Welfare should be 20 regardless of clearing price (buyer_surplus + seller_surplus = 0.20 * 100 = 20)
    assert_eq!(solution.welfare, dec!(20));
    println!("  ✓ PASS\n");

    println!("All correctness tests passed!\n");
}

// =============================================================================
// SCENARIO ANALYSIS
// =============================================================================

#[derive(Debug)]
struct ScenarioResult {
    name: String,
    true_value: Decimal,

    // Base (no JIT)
    base_price: Decimal,
    base_volume: Decimal,
    base_welfare: Decimal,

    // JIT opportunity
    unfilled_buy: Decimal,
    unfilled_sell: Decimal,

    // Backrun result (JIT MM participates)
    backrun_possible: bool,
    backrun_volume_delta: Decimal,
    backrun_welfare_delta: Decimal,
    backrun_jit_fill: Decimal,
    backrun_mm_pnl: Decimal,  // P&L if JIT MM participates

    // Passive MM comparison
    passive_mm_pnl: Decimal,  // P&L if MM had passive orders that got filled

    // JIT option value = max(0, backrun_mm_pnl) - passive_mm_pnl
    // (JIT MM can skip bad batches, passive MM cannot)
    jit_option_value: Decimal,

    // Displacement analysis
    displacement_possible: bool,
    displacement_qty: Decimal,
    displacement_welfare_delta: Decimal,
}

fn analyze_scenario(scenario: &Scenario) -> ScenarioResult {
    let base = solve_batch(&scenario.orders);

    let opp = analyze_jit_opportunity(&base, &scenario.orders);

    let mut result = ScenarioResult {
        name: scenario.name.to_string(),
        true_value: scenario.true_value,
        base_price: base.clearing_price,
        base_volume: base.total_volume,
        base_welfare: base.welfare,
        unfilled_buy: opp.unfilled_buy,
        unfilled_sell: opp.unfilled_sell,
        backrun_possible: false,
        backrun_volume_delta: dec!(0),
        backrun_welfare_delta: dec!(0),
        backrun_jit_fill: dec!(0),
        backrun_mm_pnl: dec!(0),
        passive_mm_pnl: dec!(0),
        jit_option_value: dec!(0),
        displacement_possible: false,
        displacement_qty: dec!(0),
        displacement_welfare_delta: dec!(0),
    };

    // Analyze backrun
    if let Some(jit_order) = backrun_strategy(&opp, &base) {
        let with_jit = solve_batch_with_jit(&scenario.orders, &Some(jit_order.clone()));

        result.backrun_possible = true;
        result.backrun_volume_delta = with_jit.total_volume - base.total_volume;
        result.backrun_welfare_delta = with_jit.welfare - base.welfare;

        if let Some(fill) = with_jit.fills.iter().find(|f| f.order_id == 9999) {
            result.backrun_jit_fill = fill.quantity;

            // MM P&L: sold at clearing, true value is scenario.true_value
            // If JIT sold: profit = (exec_price - true_value) * qty
            // If JIT bought: profit = (true_value - exec_price) * qty
            let pnl = match jit_order.side {
                Side::Sell => (with_jit.clearing_price - scenario.true_value) * fill.quantity,
                Side::Buy => (scenario.true_value - with_jit.clearing_price) * fill.quantity,
            };

            // Passive MM: would have same fills, same P&L (can't skip)
            result.passive_mm_pnl = pnl;

            // JIT MM: can choose to skip if P&L < 0
            // So JIT P&L = max(0, pnl) if MM is smart and skips bad batches
            result.backrun_mm_pnl = if pnl > dec!(0) { pnl } else { dec!(0) };

            // JIT option value = what MM gains by having the option to skip
            // = JIT P&L - Passive P&L
            // = max(0, pnl) - pnl
            // = |pnl| when pnl < 0, else 0
            result.jit_option_value = result.backrun_mm_pnl - result.passive_mm_pnl;
        }
    }

    // Analyze aggressive/displacement
    if let Some(jit_order) = aggressive_strategy(&opp, &base, &scenario.orders) {
        let with_jit = solve_batch_with_jit(&scenario.orders, &Some(jit_order.clone()));

        // Count displacement
        let mut displaced = dec!(0);
        for base_fill in &base.fills {
            let new_qty = with_jit.fills
                .iter()
                .find(|f| f.order_id == base_fill.order_id)
                .map(|f| f.quantity)
                .unwrap_or(dec!(0));
            if new_qty < base_fill.quantity {
                displaced += base_fill.quantity - new_qty;
            }
        }

        if displaced > dec!(0) {
            result.displacement_possible = true;
            result.displacement_qty = displaced;
            result.displacement_welfare_delta = with_jit.welfare - base.welfare;
        }
    }

    result
}

fn print_scenario_result(r: &ScenarioResult) {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ {}{}│", r.name, " ".repeat(64 - r.name.len()));
    println!("├─────────────────────────────────────────────────────────────────┤");
    println!("│ True value: {:<52}│", format!("{:.2}", r.true_value));
    println!("│ Base: price={:.2}, volume={:.2}, welfare={:<16}│",
        r.base_price, r.base_volume, format!("{:.2}", r.base_welfare));
    println!("│ Unfilled: buy={:.2}, sell={:<30}│",
        r.unfilled_buy, format!("{:.2}", r.unfilled_sell));
    println!("├─────────────────────────────────────────────────────────────────┤");

    if r.backrun_possible {
        println!("│ BACKRUN: +{:.2} volume, +{:.2} welfare{:<20}│",
            r.backrun_volume_delta, r.backrun_welfare_delta, "");
        println!("│   JIT fills: {:.2}{:<46}│", r.backrun_jit_fill, "");
        println!("│   Passive MM P&L: {:+.2} (can't skip){:<26}│", r.passive_mm_pnl, "");
        println!("│   JIT MM P&L: {:+.2} (skips if bad){:<29}│", r.backrun_mm_pnl, "");
        println!("│   JIT OPTION VALUE: {:+.2}{:<40}│", r.jit_option_value, "");

        if r.jit_option_value > dec!(0) {
            println!("│   → JIT saves MM from loss by skipping toxic batch{:<13}│", "");
        } else if r.backrun_mm_pnl > dec!(0) {
            println!("│   → Both profit, JIT doesn't add value here{:<21}│", "");
        }
    } else if r.base_volume == dec!(0) {
        println!("│ BACKRUN: not possible (no crossing){:<28}│", "");
    } else {
        println!("│ BACKRUN: not needed (market fully cleared){:<21}│", "");
    }

    if r.displacement_possible {
        println!("│ DISPLACEMENT: {:.2} shares, welfare delta: {:+.2}{:<16}│",
            r.displacement_qty, r.displacement_welfare_delta, "");

        if r.displacement_welfare_delta < dec!(0) {
            println!("│   → WELFARE DECREASES (bad for users){:<27}│", "");
        } else {
            println!("│   → welfare preserved but passive LPs displaced{:<16}│", "");
        }
    }

    println!("└─────────────────────────────────────────────────────────────────┘\n");
}

fn print_summary_table(results: &[ScenarioResult]) {
    println!("┌────────────────────────────┬────────┬──────────┬──────────┬──────────┐");
    println!("│ Scenario                   │ Volume │ Passive  │ JIT MM   │ JIT      │");
    println!("│                            │ +delta │ MM P&L   │ P&L      │ Value    │");
    println!("├────────────────────────────┼────────┼──────────┼──────────┼──────────┤");

    for r in results {
        let name = if r.name.len() > 26 { &r.name[..26] } else { &r.name };
        let vol = if r.backrun_possible { format!("+{:.0}", r.backrun_volume_delta) } else { "-".to_string() };
        let passive = if r.backrun_possible { format!("{:+.2}", r.passive_mm_pnl) } else { "-".to_string() };
        let jit = if r.backrun_possible { format!("{:+.2}", r.backrun_mm_pnl) } else { "-".to_string() };
        let value = if r.backrun_possible { format!("{:+.2}", r.jit_option_value) } else { "-".to_string() };

        println!("│ {:<26} │ {:>6} │ {:>8} │ {:>8} │ {:>8} │",
            name, vol, passive, jit, value
        );
    }

    println!("└────────────────────────────┴────────┴──────────┴──────────┴──────────┘");
    println!("");
    println!("Passive MM P&L = if MM had passive orders, forced to fill");
    println!("JIT MM P&L = MM sees batch, skips if would lose");
    println!("JIT Value = JIT P&L - Passive P&L = value of option to skip");
}

fn print_insights(results: &[ScenarioResult]) {
    // Count patterns
    let total = results.len();
    let backrun_possible = results.iter().filter(|r| r.backrun_possible).count();
    let jit_has_value = results.iter().filter(|r| r.jit_option_value > dec!(0)).count();
    let passive_loses = results.iter().filter(|r| r.passive_mm_pnl < dec!(0)).count();
    let displacement_possible = results.iter().filter(|r| r.displacement_possible).count();
    let displacement_hurts = results.iter().filter(|r| r.displacement_welfare_delta < dec!(0)).count();

    // Sum up total values
    let total_jit_value: Decimal = results.iter().map(|r| r.jit_option_value).sum();
    let total_passive_loss: Decimal = results.iter()
        .filter(|r| r.passive_mm_pnl < dec!(0))
        .map(|r| r.passive_mm_pnl)
        .sum();

    println!("1. BACKRUN OPPORTUNITY");
    println!("   - {}/{} scenarios have backrun opportunity", backrun_possible, total);
    println!("   - Markets that fully clear don't need JIT backrun");
    println!("");

    println!("2. JIT VALUE FOR MM");
    println!("   - {}/{} scenarios: JIT has positive option value", jit_has_value, backrun_possible);
    println!("   - {}/{} scenarios: passive MM would lose", passive_loses, backrun_possible);
    println!("   - Total JIT option value: {:+.2}", total_jit_value);
    println!("   - Total passive MM losses avoided: {:+.2}", -total_passive_loss);
    println!("");
    println!("   JIT lets MM skip batches where they would lose.");
    println!("   This is pure upside for MMs — no downside to having JIT.");
    println!("");

    println!("3. BUT: CAN MM ACTUALLY DETECT TOXIC FLOW?");
    println!("   For JIT to have value, MM must KNOW which batches to skip.");
    println!("   Signals MM might use:");
    println!("   - Order size: large order = maybe informed");
    println!("   - Price aggression: limit far from mid = maybe informed");
    println!("   - One-sided flow: all buys or all sells = maybe informed");
    println!("   - Historical patterns: this trader picked me off before");
    println!("");
    println!("   With PRIVACY: MM can't see WHO is trading, harder to cherry-pick.");
    println!("   But they can still see: size, price, direction.");
    println!("");

    println!("4. DISPLACEMENT");
    println!("   - {}/{} scenarios allow displacement", displacement_possible, total);
    println!("   - {}/{} displacements hurt total welfare", displacement_hurts, displacement_possible);
    println!("   - Displacement = taking fills from passive LPs");
    println!("   - Questionable value: steals from passive, may hurt welfare");
    println!("");

    println!("5. JIT PRIMARY VALUE: CAPITAL EFFICIENCY");
    println!("   The above focuses on toxic flow avoidance.");
    println!("   But the MAIN value of JIT is CAPITAL EFFICIENCY:");
    println!("");
    println!("   Without JIT (passive MM):");
    println!("   - Want to make markets on 1000 prediction markets");
    println!("   - Each market needs capital locked in passive orders");
    println!("   - Total capital needed: 1000 × per-market capital");
    println!("");
    println!("   With JIT (flash liquidity):");
    println!("   - Keep capital in ONE pool");
    println!("   - See batch, decide to deploy");
    println!("   - Same capital serves ALL 1000 markets simultaneously");
    println!("   - Capital efficiency: potentially 1000x better!");
    println!("");
    println!("   MM doesn't lock capital waiting — deploys only when needed.");
    println!("");

    println!("6. DESIGN IMPLICATIONS");
    println!("   a) JIT backrun is unambiguously good (welfare +, MM capital efficient)");
    println!("   b) JIT displacement is questionable (hurts passive LPs, may hurt welfare)");
    println!("   c) Privacy limits MM's ability to cherry-pick (good for users)");
    println!("   d) Capital efficiency is the killer feature, not toxic flow avoidance");
}
