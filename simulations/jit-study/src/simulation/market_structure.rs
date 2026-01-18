//! Market structure comparison framework
//!
//! Compares different market structures:
//! - Clob + Traditional MM: continuous trading, capital locked
//! - Private FBA + JIT: batched, JIT fills unfilled demand
//! - Private FBA + Taxed Displacement: JIT can displace with tax

use super::{Simulation, SimulationConfig, Bps, Qty};
use super::agents::{AgentId, JitMM, JitStrategy};
use super::tax::{NoTax, FixedRateTax};
use super::metrics::AggregateMetrics;

/// Defines the market structure being simulated
#[derive(Debug, Clone)]
pub enum MarketStructure {
    /// Traditional continuous limit order book
    /// Modeled as "instant FBA" with passive MMs only - no JIT
    Clob {
        /// Spread MMs post around true value
        mm_spread_bps: Bps,
        /// Capital locked per MM (continuously)
        mm_capital_per_market: u64,
    },

    /// Private FBA with JIT liquidity
    PrivateFBA {
        /// Batch duration in ms (e.g., 60000 = 1 minute)
        batch_duration_ms: u64,
        /// JIT window duration in ms (e.g., 1000 = 1 second)
        jit_window_ms: u64,
        /// JIT strategy (backrun-only vs displacement allowed)
        jit_strategy: JitStrategy,
        /// Displacement tax rate (only used if DisplacementAllowed)
        displacement_tax_bps: Bps,
    },
}

impl MarketStructure {
    /// Create a Clob configuration
    pub fn clob(mm_spread_bps: Bps, mm_capital_per_market: u64) -> Self {
        MarketStructure::Clob {
            mm_spread_bps,
            mm_capital_per_market,
        }
    }

    /// Create a Private FBA with backrun-only JIT
    pub fn fba_backrun(batch_duration_ms: u64, jit_window_ms: u64) -> Self {
        MarketStructure::PrivateFBA {
            batch_duration_ms,
            jit_window_ms,
            jit_strategy: JitStrategy::BackrunOnly,
            displacement_tax_bps: 0,
        }
    }

    /// Create a Private FBA with taxed displacement
    pub fn fba_taxed_displacement(batch_duration_ms: u64, jit_window_ms: u64, tax_bps: Bps) -> Self {
        MarketStructure::PrivateFBA {
            batch_duration_ms,
            jit_window_ms,
            jit_strategy: JitStrategy::DisplacementAllowed,
            displacement_tax_bps: tax_bps,
        }
    }

    /// Get a descriptive name for this market structure
    pub fn name(&self) -> String {
        match self {
            MarketStructure::Clob { mm_spread_bps, .. } => {
                format!("Clob ({}bps spread)", mm_spread_bps)
            }
            MarketStructure::PrivateFBA { jit_strategy, displacement_tax_bps, .. } => {
                match jit_strategy {
                    JitStrategy::BackrunOnly => "FBA+Backrun".to_string(),
                    JitStrategy::DisplacementAllowed => {
                        format!("FBA+TaxedDisp({}bps)", displacement_tax_bps)
                    }
                }
            }
        }
    }

    /// Calculate capital efficiency factor
    /// Clob: capital locked 100% of time (factor = 1.0)
    /// FBA+JIT: capital locked only during JIT window (factor = jit_window / batch_duration)
    pub fn jit_capital_efficiency_factor(&self) -> f64 {
        match self {
            MarketStructure::Clob { .. } => 1.0,
            MarketStructure::PrivateFBA { batch_duration_ms, jit_window_ms, .. } => {
                *jit_window_ms as f64 / *batch_duration_ms as f64
            }
        }
    }
}

/// Results from comparing market structures
#[derive(Debug, Clone)]
pub struct MarketStructureMetrics {
    pub structure_name: String,
    pub metrics: AggregateMetrics,
    /// Adjusted capital efficiency accounting for time locked
    pub adjusted_jit_capital_efficiency: f64,
}

/// Configuration for market structure comparison
#[derive(Debug, Clone)]
pub struct ComparisonConfig {
    pub num_rounds: u64,
    pub num_passive_mms: usize,
    pub num_noise_traders: usize,
    pub true_value_mean: Bps,
    pub true_value_volatility: Bps,
    pub mm_spread_bps: Bps,
    pub mm_order_size: Qty,
    pub jit_profit_threshold_bps: Bps,
    pub noise_size_mean: Qty,
    pub noise_size_stddev: Qty,
    pub seed: u64,
}

impl Default for ComparisonConfig {
    fn default() -> Self {
        ComparisonConfig {
            num_rounds: 5_000,
            num_passive_mms: 5,
            num_noise_traders: 10,
            true_value_mean: 5000,
            true_value_volatility: 100,
            mm_spread_bps: 100,
            mm_order_size: 100,
            jit_profit_threshold_bps: 10,
            noise_size_mean: 100,
            noise_size_stddev: 50,
            seed: 42,
        }
    }
}

/// Run a simulation for a specific market structure
pub fn run_market_structure(
    structure: &MarketStructure,
    config: &ComparisonConfig,
) -> MarketStructureMetrics {
    let sim_config = SimulationConfig {
        num_rounds: config.num_rounds,
        num_passive_lps: config.num_passive_mms,
        num_jit_mms: match structure {
            MarketStructure::Clob { .. } => 0,  // No JIT in Clob
            MarketStructure::PrivateFBA { .. } => 1,
        },
        num_noise_traders: config.num_noise_traders,
        true_value_mean: config.true_value_mean,
        true_value_volatility: config.true_value_volatility,
        lp_spread_bps: config.mm_spread_bps,
        lp_order_size: config.mm_order_size,
        jit_profit_threshold_bps: config.jit_profit_threshold_bps,
        noise_size_mean: config.noise_size_mean,
        noise_size_stddev: config.noise_size_stddev,
        seed: config.seed,
    };

    match structure {
        MarketStructure::Clob { mm_capital_per_market, .. } => {
            // Run with NoTax (no JIT MMs anyway)
            let mut sim = Simulation::new(sim_config.clone(), NoTax);
            // Override: no JIT MMs for Clob
            sim.jit_mms.clear();
            sim.run();

            let total_passive_capital = *mm_capital_per_market * config.num_passive_mms as u64;
            let metrics = sim.metrics.aggregate_with_capital(total_passive_capital, 0);

            MarketStructureMetrics {
                structure_name: structure.name(),
                adjusted_jit_capital_efficiency: 0.0, // No JIT in Clob
                metrics,
            }
        }
        MarketStructure::PrivateFBA { jit_strategy, displacement_tax_bps, .. } => {
            // Create JIT MM with correct strategy
            let jit_mm = JitMM::with_strategy(
                AgentId(config.num_passive_mms as u64),
                *jit_strategy,
                config.jit_profit_threshold_bps,
            );

            match jit_strategy {
                JitStrategy::BackrunOnly => {
                    // No tax for backrun-only
                    let mut sim = Simulation::new(sim_config.clone(), NoTax);
                    sim.jit_mms = vec![jit_mm];
                    sim.run();

                    let total_passive_capital = config.mm_order_size * 2 * config.num_passive_mms as u64;
                    let jit_capital = 10000; // JIT MM capital pool
                    let metrics = sim.metrics.aggregate_with_capital(total_passive_capital, jit_capital);

                    let efficiency_factor = structure.jit_capital_efficiency_factor();
                    let adjusted_efficiency = metrics.jit_mm_capital_efficiency / efficiency_factor;

                    MarketStructureMetrics {
                        structure_name: structure.name(),
                        adjusted_jit_capital_efficiency: adjusted_efficiency,
                        metrics,
                    }
                }
                JitStrategy::DisplacementAllowed => {
                    // Use fixed rate tax
                    let tax = FixedRateTax::new(*displacement_tax_bps);
                    let mut sim = Simulation::new(sim_config.clone(), tax);
                    sim.jit_mms = vec![jit_mm];
                    sim.run();

                    let total_passive_capital = config.mm_order_size * 2 * config.num_passive_mms as u64;
                    let jit_capital = 10000;
                    let metrics = sim.metrics.aggregate_with_capital(total_passive_capital, jit_capital);

                    let efficiency_factor = structure.jit_capital_efficiency_factor();
                    let adjusted_efficiency = metrics.jit_mm_capital_efficiency / efficiency_factor;

                    MarketStructureMetrics {
                        structure_name: structure.name(),
                        adjusted_jit_capital_efficiency: adjusted_efficiency,
                        metrics,
                    }
                }
            }
        }
    }
}

/// Compare multiple market structures
pub fn compare_structures(
    structures: &[MarketStructure],
    config: &ComparisonConfig,
) -> Vec<MarketStructureMetrics> {
    structures.iter()
        .map(|s| run_market_structure(s, config))
        .collect()
}

/// Print comparison results in a table format
pub fn print_comparison_table(results: &[MarketStructureMetrics]) {
    println!("\n=== MARKET STRUCTURE COMPARISON ===\n");

    // Header
    println!("{:<25} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "Structure", "Batch", "JIT Window", "Price Impact", "Fill Rate", "Volume/Rnd");
    println!("{}", "-".repeat(90));

    for result in results {
        let batch_dur = match &result.structure_name {
            n if n.starts_with("Clob") => "0s (cont)".to_string(),
            _ => "60s".to_string(),
        };
        let jit_window = match &result.structure_name {
            n if n.starts_with("Clob") => "N/A".to_string(),
            _ => "1s".to_string(),
        };

        println!("{:<25} {:>12} {:>12} {:>10.1} bps {:>10.1}% {:>12.1}",
            result.structure_name,
            batch_dur,
            jit_window,
            result.metrics.avg_price_impact_bps,
            result.metrics.fill_rate * 100.0,
            result.metrics.volume_per_round,
        );
    }

    println!("\n{:<25} {:>12} {:>12} {:>12} {:>14}",
        "Structure", "Passive P&L", "JIT P&L", "Total Welfare", "Cap Efficiency");
    println!("{}", "-".repeat(85));

    for result in results {
        let cap_eff = if result.adjusted_jit_capital_efficiency > 0.0 {
            format!("{:.4}%", result.adjusted_jit_capital_efficiency * 100.0)
        } else {
            format!("{:.4}%", result.metrics.passive_mm_capital_efficiency * 100.0)
        };

        println!("{:<25} {:>12.1} {:>12.1} {:>12} {:>14}",
            result.structure_name,
            result.metrics.mean_passive_lp_pnl_per_round,
            result.metrics.mean_jit_mm_pnl_per_round,
            result.metrics.total_welfare,
            cap_eff,
        );
    }
}

/// Print insights from comparison
pub fn print_comparison_insights(results: &[MarketStructureMetrics]) {
    println!("\n=== INSIGHTS ===\n");

    // Find best for each metric
    let best_price_impact = results.iter()
        .min_by(|a, b| a.metrics.avg_price_impact_bps.partial_cmp(&b.metrics.avg_price_impact_bps).unwrap())
        .unwrap();

    let best_fill_rate = results.iter()
        .max_by(|a, b| a.metrics.fill_rate.partial_cmp(&b.metrics.fill_rate).unwrap())
        .unwrap();

    let best_welfare = results.iter()
        .max_by(|a, b| a.metrics.total_welfare.cmp(&b.metrics.total_welfare))
        .unwrap();

    let best_passive_mm = results.iter()
        .max_by(|a, b| a.metrics.total_passive_lp_pnl.cmp(&b.metrics.total_passive_lp_pnl))
        .unwrap();

    println!("Best for users (lowest price impact): {}", best_price_impact.structure_name);
    println!("Best fill rate: {}", best_fill_rate.structure_name);
    println!("Best total welfare: {}", best_welfare.structure_name);
    println!("Best for passive MMs: {}", best_passive_mm.structure_name);

    // Capital efficiency comparison
    let clob_result = results.iter().find(|r| r.structure_name.starts_with("Clob"));
    let fba_backrun = results.iter().find(|r| r.structure_name == "FBA+Backrun");

    if let (Some(clob), Some(fba)) = (clob_result, fba_backrun) {
        if clob.metrics.passive_mm_capital_efficiency > 0.0 && fba.adjusted_jit_capital_efficiency > 0.0 {
            let ratio = fba.adjusted_jit_capital_efficiency / clob.metrics.passive_mm_capital_efficiency;
            println!("\nCapital efficiency ratio (FBA+JIT vs Clob): {:.1}x", ratio);
        }
    }

    println!("\n--- SUMMARY ---");
    println!("- FBA batching typically reduces price impact vs Clob (batch aggregation)");
    println!("- JIT MM capital efficiency is ~60x better (1s lock vs 60s continuous)");
    println!("- Backrun-only preserves passive MM profitability");
    println!("- Taxed displacement may increase fill rate but hurts passive MMs");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_structure_names() {
        let clob = MarketStructure::clob(100, 1000);
        assert!(clob.name().contains("Clob"));

        let fba_backrun = MarketStructure::fba_backrun(60000, 1000);
        assert_eq!(fba_backrun.name(), "FBA+Backrun");

        let fba_taxed = MarketStructure::fba_taxed_displacement(60000, 1000, 100);
        assert!(fba_taxed.name().contains("TaxedDisp"));
    }

    #[test]
    fn test_capital_efficiency_factor() {
        let clob = MarketStructure::clob(100, 1000);
        assert_eq!(clob.jit_capital_efficiency_factor(), 1.0);

        let fba = MarketStructure::fba_backrun(60000, 1000);
        let factor = fba.jit_capital_efficiency_factor();
        assert!((factor - 1.0/60.0).abs() < 0.001);
    }
}
