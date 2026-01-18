//! Metrics collection for the displacement tax simulation
//!
//! Tracks per-round data and computes aggregate statistics.

use serde::{Deserialize, Serialize};

use super::{Bps, Qty};

/// Metrics for a single round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundMetrics {
    pub round: u64,
    pub true_value: Bps,
    pub clearing_price: Bps,
    pub total_volume: Qty,
    pub jit_participated: bool,
    pub jit_volume: Qty,
    pub displacement_qty: Qty,
    pub tax_collected: u64,
    pub tax_rate_bps: Bps,
    pub passive_lp_pnl: i64,
    pub jit_mm_pnl: i64,

    // Welfare metrics
    /// Price impact: |clearing_price - true_value| in basis points
    #[serde(default)]
    pub price_impact_bps: f64,
    /// Number of user orders submitted
    #[serde(default)]
    pub user_orders_submitted: u64,
    /// Number of user orders filled (at least partially)
    #[serde(default)]
    pub user_orders_filled: u64,
    /// Total user order quantity
    #[serde(default)]
    pub user_order_qty: Qty,
    /// Total user quantity filled
    #[serde(default)]
    pub user_qty_filled: Qty,
}

/// Aggregate statistics across rounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateMetrics {
    pub total_rounds: u64,
    pub jit_participation_rate: f64,
    pub mean_jit_volume: f64,
    pub mean_displacement: f64,
    pub total_tax_collected: u64,
    pub mean_tax_rate_bps: f64,
    pub total_passive_lp_pnl: i64,
    pub total_jit_mm_pnl: i64,
    pub mean_passive_lp_pnl_per_round: f64,
    pub mean_jit_mm_pnl_per_round: f64,
    pub total_welfare: i64,

    // Welfare metrics
    /// Average price impact in basis points
    pub avg_price_impact_bps: f64,
    /// Fill rate: % of user order quantity that got filled
    pub fill_rate: f64,
    /// Total volume across all rounds
    pub total_volume: Qty,
    /// Average volume per round
    pub volume_per_round: f64,
    /// MM capital efficiency: profit / (capital_locked * time)
    /// For passive MM: continuous lock. For JIT MM: 1s per minute (1/60)
    pub passive_mm_capital_efficiency: f64,
    pub jit_mm_capital_efficiency: f64,
}

/// Collects and aggregates metrics
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    pub rounds: Vec<RoundMetrics>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        MetricsCollector { rounds: Vec::new() }
    }

    pub fn record_round(&mut self, metrics: RoundMetrics) {
        self.rounds.push(metrics);
    }

    pub fn aggregate(&self) -> AggregateMetrics {
        self.aggregate_with_capital(200, 10000) // Default: 200 per passive MM, 10000 total for JIT
    }

    /// Aggregate with explicit capital amounts for efficiency calculation
    pub fn aggregate_with_capital(&self, passive_mm_capital: u64, jit_mm_capital: u64) -> AggregateMetrics {
        let total_rounds = self.rounds.len() as u64;

        if total_rounds == 0 {
            return AggregateMetrics {
                total_rounds: 0,
                jit_participation_rate: 0.0,
                mean_jit_volume: 0.0,
                mean_displacement: 0.0,
                total_tax_collected: 0,
                mean_tax_rate_bps: 0.0,
                total_passive_lp_pnl: 0,
                total_jit_mm_pnl: 0,
                mean_passive_lp_pnl_per_round: 0.0,
                mean_jit_mm_pnl_per_round: 0.0,
                total_welfare: 0,
                avg_price_impact_bps: 0.0,
                fill_rate: 0.0,
                total_volume: 0,
                volume_per_round: 0.0,
                passive_mm_capital_efficiency: 0.0,
                jit_mm_capital_efficiency: 0.0,
            };
        }

        let jit_participated_count = self.rounds.iter().filter(|r| r.jit_participated).count();
        let jit_participation_rate = jit_participated_count as f64 / total_rounds as f64;

        let total_jit_volume: Qty = self.rounds.iter().map(|r| r.jit_volume).sum();
        let mean_jit_volume = total_jit_volume as f64 / total_rounds as f64;

        let total_displacement: Qty = self.rounds.iter().map(|r| r.displacement_qty).sum();
        let mean_displacement = total_displacement as f64 / total_rounds as f64;

        let total_tax_collected: u64 = self.rounds.iter().map(|r| r.tax_collected).sum();

        let total_tax_rate: u64 = self.rounds.iter().map(|r| r.tax_rate_bps).sum();
        let mean_tax_rate_bps = total_tax_rate as f64 / total_rounds as f64;

        let total_passive_lp_pnl: i64 = self.rounds.iter().map(|r| r.passive_lp_pnl).sum();
        let total_jit_mm_pnl: i64 = self.rounds.iter().map(|r| r.jit_mm_pnl).sum();

        let mean_passive_lp_pnl_per_round = total_passive_lp_pnl as f64 / total_rounds as f64;
        let mean_jit_mm_pnl_per_round = total_jit_mm_pnl as f64 / total_rounds as f64;

        let total_welfare = total_passive_lp_pnl + total_jit_mm_pnl;

        // Welfare metrics
        let total_price_impact: f64 = self.rounds.iter().map(|r| r.price_impact_bps).sum();
        let avg_price_impact_bps = total_price_impact / total_rounds as f64;

        let total_user_qty: Qty = self.rounds.iter().map(|r| r.user_order_qty).sum();
        let total_user_filled: Qty = self.rounds.iter().map(|r| r.user_qty_filled).sum();
        let fill_rate = if total_user_qty > 0 {
            total_user_filled as f64 / total_user_qty as f64
        } else {
            // If no user orders, use total volume as proxy (noise traders act as users)
            1.0 // Assume all gets filled
        };

        let total_volume: Qty = self.rounds.iter().map(|r| r.total_volume).sum();
        let volume_per_round = total_volume as f64 / total_rounds as f64;

        // Capital efficiency: profit / (capital * time)
        // Passive MM: capital locked continuously (time = 1.0)
        // JIT MM: capital locked only during JIT window (time = 1/60 for 1s window in 60s batch)
        let passive_mm_capital_efficiency = if passive_mm_capital > 0 && total_rounds > 0 {
            // Efficiency per round = profit_per_round / capital_locked
            (total_passive_lp_pnl as f64 / total_rounds as f64) / passive_mm_capital as f64
        } else {
            0.0
        };

        // JIT MM capital efficiency is ~60x better because capital only locked 1s per 60s batch
        // profit / (capital * effective_time_fraction)
        // where effective_time_fraction = 1/60 for 1s JIT window in 60s batch
        let jit_time_fraction = 1.0 / 60.0; // JIT window is 1s out of 60s batch
        let jit_mm_capital_efficiency = if jit_mm_capital > 0 && total_rounds > 0 {
            (total_jit_mm_pnl as f64 / total_rounds as f64) / (jit_mm_capital as f64 * jit_time_fraction)
        } else {
            0.0
        };

        AggregateMetrics {
            total_rounds,
            jit_participation_rate,
            mean_jit_volume,
            mean_displacement,
            total_tax_collected,
            mean_tax_rate_bps,
            total_passive_lp_pnl,
            total_jit_mm_pnl,
            mean_passive_lp_pnl_per_round,
            mean_jit_mm_pnl_per_round,
            total_welfare,
            avg_price_impact_bps,
            fill_rate,
            total_volume,
            volume_per_round,
            passive_mm_capital_efficiency,
            jit_mm_capital_efficiency,
        }
    }

}

/// Results from a parameter sweep
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepResult {
    pub parameter_name: String,
    pub parameter_value: f64,
    pub tax_mechanism: String,
    pub metrics: AggregateMetrics,
}

/// Collect sweep results
#[derive(Debug, Clone)]
pub struct SweepCollector {
    pub results: Vec<SweepResult>,
}

impl SweepCollector {
    pub fn new() -> Self {
        SweepCollector { results: Vec::new() }
    }

    pub fn add_result(&mut self, result: SweepResult) {
        self.results.push(result);
    }

    /// Export sweep results to CSV
    pub fn export_csv(&self, path: &str) -> std::io::Result<()> {
        let mut wtr = csv::Writer::from_path(path)?;

        // Write header
        wtr.write_record([
            "parameter_name",
            "parameter_value",
            "tax_mechanism",
            "total_rounds",
            "jit_participation_rate",
            "mean_jit_volume",
            "mean_displacement",
            "total_tax_collected",
            "mean_tax_rate_bps",
            "total_passive_lp_pnl",
            "total_jit_mm_pnl",
            "mean_passive_lp_pnl_per_round",
            "mean_jit_mm_pnl_per_round",
            "total_welfare",
        ])?;

        for result in &self.results {
            wtr.write_record(&[
                result.parameter_name.clone(),
                result.parameter_value.to_string(),
                result.tax_mechanism.clone(),
                result.metrics.total_rounds.to_string(),
                format!("{:.4}", result.metrics.jit_participation_rate),
                format!("{:.2}", result.metrics.mean_jit_volume),
                format!("{:.2}", result.metrics.mean_displacement),
                result.metrics.total_tax_collected.to_string(),
                format!("{:.2}", result.metrics.mean_tax_rate_bps),
                result.metrics.total_passive_lp_pnl.to_string(),
                result.metrics.total_jit_mm_pnl.to_string(),
                format!("{:.2}", result.metrics.mean_passive_lp_pnl_per_round),
                format!("{:.2}", result.metrics.mean_jit_mm_pnl_per_round),
                result.metrics.total_welfare.to_string(),
            ])?;
        }

        wtr.flush()?;
        Ok(())
    }

    /// Print summary table
    pub fn print_summary(&self) {
        println!("\n=== SWEEP RESULTS ===\n");

        println!("{:<20} {:<10} {:<8} {:<12} {:<12} {:<12}",
            "Tax Mechanism", "Param", "JIT%", "Displace", "LP P&L", "JIT P&L");
        println!("{}", "-".repeat(80));

        for result in &self.results {
            println!("{:<20} {:<10.0} {:<8.1} {:<12.1} {:<12.0} {:<12.0}",
                result.tax_mechanism,
                result.parameter_value,
                result.metrics.jit_participation_rate * 100.0,
                result.metrics.mean_displacement,
                result.metrics.mean_passive_lp_pnl_per_round,
                result.metrics.mean_jit_mm_pnl_per_round,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector() {
        let mut collector = MetricsCollector::new();

        collector.record_round(RoundMetrics {
            round: 1,
            true_value: 5000,
            clearing_price: 5000,
            total_volume: 100,
            jit_participated: true,
            jit_volume: 50,
            displacement_qty: 25,
            tax_collected: 10,
            tax_rate_bps: 100,
            passive_lp_pnl: -100,
            jit_mm_pnl: 50,
            price_impact_bps: 0.0,
            user_orders_submitted: 10,
            user_orders_filled: 8,
            user_order_qty: 500,
            user_qty_filled: 400,
        });

        collector.record_round(RoundMetrics {
            round: 2,
            true_value: 5100,
            clearing_price: 5050,
            total_volume: 150,
            jit_participated: false,
            jit_volume: 0,
            displacement_qty: 0,
            tax_collected: 0,
            tax_rate_bps: 100,
            passive_lp_pnl: 200,
            jit_mm_pnl: 0,
            price_impact_bps: 50.0,
            user_orders_submitted: 12,
            user_orders_filled: 10,
            user_order_qty: 600,
            user_qty_filled: 500,
        });

        let agg = collector.aggregate();

        assert_eq!(agg.total_rounds, 2);
        assert_eq!(agg.jit_participation_rate, 0.5);
        assert_eq!(agg.mean_jit_volume, 25.0);
        assert_eq!(agg.total_passive_lp_pnl, 100);
        assert_eq!(agg.total_jit_mm_pnl, 50);
        assert_eq!(agg.avg_price_impact_bps, 25.0);
        assert!((agg.fill_rate - 0.818).abs() < 0.01); // 900/1100
    }

}
