use clap::Parser;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

use matching_engine::NANOS_PER_DOLLAR;
use matching_sequencer::config::SimulationConfig;
use matching_sequencer::simulation::SimulationRunner;

#[derive(Parser)]
#[command(name = "sybil-sim", about = "Agent-based prediction market simulation")]
struct Cli {
    /// Preset configuration: quick, standard, stress
    #[arg(long)]
    preset: Option<String>,

    /// Number of binary markets
    #[arg(long)]
    markets: Option<usize>,

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

    // Start from preset or default
    let mut config = match cli.preset.as_deref() {
        Some("quick") => SimulationConfig::quick(),
        Some("standard") => SimulationConfig::standard(),
        Some("stress") => SimulationConfig::stress(),
        Some(other) => {
            eprintln!("Unknown preset: {}. Use quick, standard, or stress.", other);
            std::process::exit(1);
        }
        None => SimulationConfig::standard(),
    };

    // Override with CLI args
    if let Some(m) = cli.markets {
        config.num_markets = m;
    }
    if let Some(i) = cli.informed {
        config.num_informed = i;
    }
    if let Some(n) = cli.noise {
        config.num_noise = n;
    }
    if let Some(m) = cli.mm {
        config.num_mm = m;
    }
    if let Some(b) = cli.batches {
        config.num_batches = b;
    }
    if let Some(s) = cli.seed {
        config.seed = s;
    }
    config.verbose = cli.verbose;

    let num_batches = config.num_batches;

    println!("=== Sybil Simulation ===");
    println!(
        "Markets: {}, Informed: {}, Noise: {}, MMs: {}, Batches: {}",
        config.num_markets, config.num_informed, config.num_noise, config.num_mm, num_batches
    );
    println!("Seed: {}", config.seed);
    println!();

    let mut runner = SimulationRunner::from_config(&config);
    let result = runner.run(num_batches);

    // Print per-batch summary
    if config.verbose {
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

    // Print agent PnL (pre-resolution)
    {
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

    // Print resolved PnL
    {
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

    // Print true probabilities and final prices
    {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec!["Market", "True P(YES)", "Final Price", "Error"]);

        let last_prices = result.price_history.last();

        let mut market_ids: Vec<_> = result.true_probs.keys().collect();
        market_ids.sort();

        for &market_id in &market_ids {
            let true_p = result.true_probs[market_id];
            let (final_price, error) = if let Some(prices) = last_prices {
                if let Some(ps) = prices.get(market_id) {
                    let p = ps[0] as f64 / NANOS_PER_DOLLAR as f64;
                    (format!("{:.4}", p), format!("{:.4}", (p - true_p).abs()))
                } else {
                    ("N/A".to_string(), "N/A".to_string())
                }
            } else {
                ("N/A".to_string(), "N/A".to_string())
            };

            table.add_row(vec![
                Cell::new(format!("{}", market_id)),
                Cell::new(format!("{:.4}", true_p)),
                Cell::new(final_price),
                Cell::new(error),
            ]);
        }

        println!("Price Discovery:");
        println!("{table}");
        println!();
    }

    // Summary stats
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
