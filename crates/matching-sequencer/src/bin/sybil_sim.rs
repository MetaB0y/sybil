use clap::Parser;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

use matching_engine::NANOS_PER_DOLLAR;
use matching_sequencer::scenario::Scenario;
use matching_sequencer::simulation::SimulationRunner;

#[derive(Parser)]
#[command(name = "sybil-sim", about = "Agent-based prediction market simulation")]
struct Cli {
    /// Scenario: coin_flip, election, two_events_with_leak, quick, standard, stress
    #[arg(long, default_value = "standard")]
    scenario: String,

    /// Number of informed traders
    #[arg(long)]
    informed: Option<usize>,

    /// Number of noise traders
    #[arg(long)]
    noise: Option<usize>,

    /// Number of market makers
    #[arg(long)]
    mm: Option<usize>,

    /// Number of batches
    #[arg(long)]
    batches: Option<usize>,

    /// Random seed
    #[arg(long)]
    seed: Option<u64>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    let mut scenario = match cli.scenario.as_str() {
        "coin_flip" => Scenario::coin_flip(),
        "election" => Scenario::election(),
        "two_events_with_leak" => Scenario::two_events_with_leak(),
        "quick" => Scenario::quick(),
        "standard" => Scenario::standard(),
        "stress" => Scenario::stress(),
        other => {
            eprintln!(
                "Unknown scenario: {}. Available: coin_flip, election, two_events_with_leak, quick, standard, stress.",
                other
            );
            std::process::exit(1);
        }
    };

    // Apply CLI overrides
    if let Some(i) = cli.informed {
        scenario.num_informed = i;
    }
    if let Some(n) = cli.noise {
        scenario.num_noise = n;
    }
    if let Some(m) = cli.mm {
        scenario.num_mm = m;
    }
    if let Some(b) = cli.batches {
        scenario.num_batches = b;
    }
    if let Some(s) = cli.seed {
        scenario.seed = s;
    }

    let num_batches = scenario.num_batches;

    println!("=== Sybil Simulation (Scenario: {}) ===", scenario.name);
    for (i, event) in scenario.events.iter().enumerate() {
        let outcomes: Vec<&str> = event.outcomes.iter().map(|o| o.name.as_str()).collect();
        println!(
            "Event {}: {} [{}]",
            i,
            event.name,
            outcomes.join(", ")
        );
        if let Some(b) = event.resolve_at_batch {
            println!("  Resolves at batch {}", b);
        }
    }
    if !scenario.news.is_empty() {
        println!("News items: {}", scenario.news.len());
    }
    println!(
        "Informed: {}, Noise: {}, MMs: {}, Batches: {}",
        scenario.num_informed, scenario.num_noise, scenario.num_mm, num_batches
    );
    println!("Seed: {}", scenario.seed);
    println!();

    let mut runner = SimulationRunner::from_scenario(&scenario);
    let result = runner.run(num_batches);

    // Print per-batch summary
    if cli.verbose {
        print_batch_table(&result);
    }

    // Print agent PnL tables
    print_agent_pnl(&result);
    print_resolved_pnl(&result);

    // Print event-aware price discovery
    print_event_price_discovery(&result, &result.scenario, &result.event_map);

    print_summary(&result);
}

fn print_batch_table(result: &matching_sequencer::SimulationResult) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        "Batch", "Welfare", "Volume", "Orders", "Filled", "Fill%", "Rejections", "Price Error",
    ]);

    for bm in &result.batch_metrics {
        let price_err = matching_sequencer::metrics::price_convergence(
            &bm.clearing_prices,
            &result.true_probs,
        );
        table.add_row(vec![
            Cell::new(bm.batch),
            Cell::new(format_nanos_dollars(bm.total_welfare)),
            Cell::new(bm.total_volume),
            Cell::new(bm.orders_submitted),
            Cell::new(bm.orders_filled),
            Cell::new(format!("{:.1}%", bm.fill_rate() * 100.0)),
            Cell::new(bm.rejections),
            Cell::new(format!("{:.4}", price_err)),
        ]);
    }

    println!("Per-Batch Results:");
    println!("{table}");
    println!();
}

fn print_agent_pnl(result: &matching_sequencer::SimulationResult) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        "Agent",
        "Initial",
        "Cash",
        "Position Value",
        "Total PnL",
    ]);

    for pnl in &result.agent_pnl {
        let pnl_color = if pnl.total_pnl >= 0 {
            Color::Green
        } else {
            Color::Red
        };
        table.add_row(vec![
            Cell::new(&pnl.name),
            Cell::new(format_nanos_dollars(pnl.initial_balance)),
            Cell::new(format_nanos_dollars(pnl.final_balance)),
            Cell::new(format_nanos_dollars(pnl.position_value)),
            Cell::new(format_nanos_dollars(pnl.total_pnl)).fg(pnl_color),
        ]);
    }

    println!("Agent PnL (pre-resolution, positions valued at last price):");
    println!("{table}");
    println!();
}

fn print_resolved_pnl(result: &matching_sequencer::SimulationResult) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec!["Agent", "Initial", "Final", "Realized PnL"]);

    for pnl in &result.resolved_pnl {
        let pnl_color = if pnl.total_pnl >= 0 {
            Color::Green
        } else {
            Color::Red
        };
        table.add_row(vec![
            Cell::new(&pnl.name),
            Cell::new(format_nanos_dollars(pnl.initial_balance)),
            Cell::new(format_nanos_dollars(pnl.final_balance)),
            Cell::new(format_nanos_dollars(pnl.total_pnl)).fg(pnl_color),
        ]);
    }

    println!("Agent PnL (after market resolution):");
    println!("{table}");
    println!();
}

fn print_event_price_discovery(
    result: &matching_sequencer::SimulationResult,
    scenario: &Scenario,
    event_map: &matching_sequencer::scenario::EventMarketMap,
) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        "Event",
        "Outcome",
        "Market",
        "True P",
        "Final Price",
        "Error",
        "Winner",
    ]);

    let last_prices = result.price_history.last();

    for (event_idx, event) in scenario.events.iter().enumerate() {
        let market_ids = &event_map.event_markets[event_idx];

        if event.outcomes.len() == 2 && market_ids.len() == 1 {
            // Binary event: single market
            let mid = market_ids[0];
            let true_p = event.true_probs[0];
            let (final_price_str, error_str) = format_market_price(last_prices, &mid, true_p);

            let winner_marker = if event.winner == 0 { "<-" } else { "" };
            table.add_row(vec![
                Cell::new(&event.name),
                Cell::new(&event.outcomes[0].name),
                Cell::new(format!("{}", mid)),
                Cell::new(format!("{:.4}", true_p)),
                Cell::new(final_price_str),
                Cell::new(error_str),
                Cell::new(winner_marker),
            ]);

            let winner_marker = if event.winner == 1 { "<-" } else { "" };
            table.add_row(vec![
                Cell::new(""),
                Cell::new(&event.outcomes[1].name),
                Cell::new(format!("({})", mid)),
                Cell::new(format!("{:.4}", event.true_probs[1])),
                Cell::new("(complement)"),
                Cell::new(""),
                Cell::new(winner_marker),
            ]);
        } else {
            // Multi-outcome event: one market per outcome
            for (outcome_idx, outcome) in event.outcomes.iter().enumerate() {
                let mid = market_ids[outcome_idx];
                let true_p = event.true_probs[outcome_idx];
                let (final_price_str, error_str) = format_market_price(last_prices, &mid, true_p);

                let event_label = if outcome_idx == 0 {
                    event.name.as_str()
                } else {
                    ""
                };

                let winner_marker = if outcome_idx == event.winner {
                    "<-"
                } else {
                    ""
                };

                table.add_row(vec![
                    Cell::new(event_label),
                    Cell::new(&outcome.name),
                    Cell::new(format!("{}", mid)),
                    Cell::new(format!("{:.4}", true_p)),
                    Cell::new(final_price_str),
                    Cell::new(error_str),
                    Cell::new(winner_marker),
                ]);
            }
        }
    }

    println!("Price Discovery (Event View):");
    println!("{table}");
    println!();
}

fn format_market_price(
    last_prices: Option<&std::collections::HashMap<matching_engine::MarketId, Vec<u64>>>,
    mid: &matching_engine::MarketId,
    true_p: f64,
) -> (String, String) {
    if let Some(prices) = last_prices {
        if let Some(ps) = prices.get(mid) {
            let p = ps[0] as f64 / NANOS_PER_DOLLAR as f64;
            (format!("{:.4}", p), format!("{:.4}", (p - true_p).abs()))
        } else {
            ("N/A".to_string(), "N/A".to_string())
        }
    } else {
        ("N/A".to_string(), "N/A".to_string())
    }
}

fn print_summary(result: &matching_sequencer::SimulationResult) {
    let total_welfare: i64 = result.batch_metrics.iter().map(|b| b.total_welfare).sum();
    let total_volume: u64 = result.batch_metrics.iter().map(|b| b.total_volume).sum();
    let total_orders: usize = result
        .batch_metrics
        .iter()
        .map(|b| b.orders_submitted)
        .sum();
    let total_fills: usize = result.batch_metrics.iter().map(|b| b.orders_filled).sum();

    println!("=== Summary ===");
    println!("Total welfare: {}", format_nanos_dollars(total_welfare));
    println!("Total volume: {}", total_volume);
    println!("Total orders: {} ({} filled)", total_orders, total_fills);
    println!(
        "Overall fill rate: {:.1}%",
        if total_orders > 0 {
            total_fills as f64 / total_orders as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("Final price error: {:.4}", result.final_price_error);
}

fn format_nanos_dollars(nanos: i64) -> String {
    let dollars = nanos as f64 / NANOS_PER_DOLLAR as f64;
    if dollars.abs() >= 1000.0 {
        format!("${:.0}", dollars)
    } else if dollars.abs() >= 1.0 {
        format!("${:.2}", dollars)
    } else {
        format!("${:.4}", dollars)
    }
}
