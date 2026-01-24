//! Visualization and export utilities for pipeline analysis.
//!
//! This module provides:
//! - JSON export of pipeline results via `VizSnapshot`
//! - ASCII convergence tables for CLI visualization
//!
//! # Usage
//!
//! ```ignore
//! use matching_solver::viz::VizSnapshot;
//!
//! let result = pipeline.solve(&problem);
//! let snapshot = VizSnapshot::from_pipeline_result(&result, &problem, "my_scenario");
//!
//! // Export to JSON
//! let json = snapshot.to_json();
//! std::fs::write("/tmp/snapshot.json", json).unwrap();
//!
//! // Print convergence table
//! println!("{}", viz::ascii::convergence_summary(&result.iteration_stats));
//! ```

use std::collections::HashMap;

use serde::Serialize;

use matching_engine::{MarketId, Problem, NANOS_PER_DOLLAR};

use crate::pipeline::{IterationStats, PipelineResult};

/// Complete snapshot of a pipeline run for visualization and analysis.
#[derive(Clone, Debug, Serialize)]
pub struct VizSnapshot {
    /// Name/description of the scenario
    pub scenario_name: String,

    /// Pipeline configuration summary
    pub config: VizConfig,

    /// Per-iteration snapshots showing convergence
    pub iterations: Vec<IterationSnapshot>,

    /// Final result summary
    pub final_result: FinalSnapshot,

    /// Phase timing breakdown
    pub phase_times: PipelineTimes,
}

/// Configuration summary for the snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct VizConfig {
    pub num_markets: usize,
    pub num_orders: usize,
    pub num_mm_constraints: usize,
    pub pipeline_iterations: usize,
}

/// Snapshot of a single iteration.
#[derive(Clone, Debug, Serialize)]
pub struct IterationSnapshot {
    pub iteration: usize,
    pub welfare: i64,
    pub welfare_delta: i64,
    pub volume: u64,
    pub volume_delta: u64,
    pub fills: usize,
    pub fills_delta: usize,
    pub price_discovery_fills: usize,
    pub bundle_fills: usize,
    /// Per-market price snapshots (market name -> prices)
    pub market_prices: HashMap<String, MarketPrices>,
    /// MM allocation snapshots
    pub mm_allocations: Vec<MmSnapshot>,
}

/// Prices for a single market.
#[derive(Clone, Debug, Serialize)]
pub struct MarketPrices {
    /// YES price as fraction (0.0-1.0)
    pub yes_price: f64,
    /// NO price as fraction (0.0-1.0)
    pub no_price: f64,
    /// Volume traded in this market
    pub volume: u64,
    /// Welfare from this market
    pub welfare: i64,
}

/// Snapshot of MM allocation.
#[derive(Clone, Debug, Serialize)]
pub struct MmSnapshot {
    pub mm_id: u64,
    pub activated_orders: usize,
    pub capital_used: f64,
    pub budget: f64,
    pub utilization: f64,
}

/// Final result summary.
#[derive(Clone, Debug, Serialize)]
pub struct FinalSnapshot {
    pub total_welfare: i64,
    pub total_welfare_dollars: f64,
    pub total_volume: u64,
    pub total_fills: usize,
    pub orders_filled: usize,
    pub orders_unfilled: usize,
    pub fill_rate: f64,
}

/// Phase timing breakdown.
#[derive(Clone, Debug, Serialize)]
pub struct PipelineTimes {
    pub price_discovery_secs: f64,
    pub price_projection_secs: f64,
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
    pub total_secs: f64,
}

impl VizSnapshot {
    /// Create a snapshot from a pipeline result.
    pub fn from_pipeline_result(
        result: &PipelineResult,
        problem: &Problem,
        scenario_name: impl Into<String>,
    ) -> Self {
        // Build market name lookup
        let market_names: HashMap<MarketId, String> = problem
            .markets
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();

        // Build iteration snapshots
        let iterations: Vec<IterationSnapshot> = result
            .iteration_stats
            .iter()
            .map(|stat| {
                // Get market prices from price discovery if available
                let market_prices = if let Some(ref pd) = result.price_discovery {
                    pd.prices
                        .iter()
                        .filter_map(|(market_id, prices)| {
                            let name = market_names.get(market_id)?.clone();
                            let yes_price = prices.first().copied().unwrap_or(0) as f64
                                / NANOS_PER_DOLLAR as f64;
                            let no_price = prices.get(1).copied().unwrap_or(0) as f64
                                / NANOS_PER_DOLLAR as f64;

                            // Get volume/welfare from market solution if available
                            let (volume, welfare) = pd
                                .market_solutions
                                .get(market_id)
                                .map(|sol| {
                                    let vol: u64 = sol.fills.iter().map(|f| f.fill_qty).sum();
                                    (vol, sol.welfare)
                                })
                                .unwrap_or((0, 0));

                            Some((
                                name,
                                MarketPrices {
                                    yes_price,
                                    no_price,
                                    volume,
                                    welfare,
                                },
                            ))
                        })
                        .collect()
                } else {
                    HashMap::new()
                };

                // Get MM allocations
                let mm_allocations = if let Some(ref alloc) = result.allocation {
                    alloc
                        .mm_allocations
                        .iter()
                        .map(|mm| MmSnapshot {
                            mm_id: mm.mm_id.0,
                            activated_orders: mm.activated_orders.len(),
                            capital_used: mm.capital_used as f64 / NANOS_PER_DOLLAR as f64,
                            budget: mm.budget as f64 / NANOS_PER_DOLLAR as f64,
                            utilization: mm.utilization,
                        })
                        .collect()
                } else {
                    Vec::new()
                };

                IterationSnapshot {
                    iteration: stat.iteration,
                    welfare: stat.welfare,
                    welfare_delta: stat.welfare_delta,
                    volume: stat.volume,
                    volume_delta: stat.volume_delta,
                    fills: stat.fills,
                    fills_delta: stat.fills_delta,
                    price_discovery_fills: stat.price_discovery_fills,
                    bundle_fills: stat.bundle_fills,
                    market_prices,
                    mm_allocations,
                }
            })
            .collect();

        // Build final result
        let total_volume: u64 = result.result.fills.iter().map(|f| f.fill_qty).sum();
        let orders_filled = result.result.orders_filled;
        let orders_unfilled =
            result.result.orders_unfilled_liquidity + result.result.orders_unfilled_aon;

        let final_result = FinalSnapshot {
            total_welfare: result.result.total_welfare,
            total_welfare_dollars: result.result.total_welfare as f64 / NANOS_PER_DOLLAR as f64,
            total_volume,
            total_fills: result.result.fills.len(),
            orders_filled,
            orders_unfilled,
            fill_rate: if orders_filled + orders_unfilled > 0 {
                orders_filled as f64 / (orders_filled + orders_unfilled) as f64
            } else {
                0.0
            },
        };

        VizSnapshot {
            scenario_name: scenario_name.into(),
            config: VizConfig {
                num_markets: problem.markets.len(),
                num_orders: problem.orders.len(),
                num_mm_constraints: problem.mm_constraints.len(),
                pipeline_iterations: result.iterations,
            },
            iterations,
            final_result,
            phase_times: PipelineTimes {
                price_discovery_secs: result.phase_times.price_discovery_secs,
                price_projection_secs: result.phase_times.price_projection_secs,
                allocation_secs: result.phase_times.allocation_secs,
                partial_solving_secs: result.phase_times.partial_solving_secs,
                combining_secs: result.phase_times.combining_secs,
                total_secs: result.total_time_secs,
            },
        }
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    }

    /// Serialize to compact JSON string.
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    }
}

/// ASCII table utilities for CLI visualization.
pub mod ascii {
    use super::*;

    /// Generate a convergence summary table.
    pub fn convergence_summary(stats: &[IterationStats]) -> String {
        if stats.is_empty() {
            return "No iteration data available.".to_string();
        }

        let mut output = String::new();
        output.push_str("\n=== Convergence Summary ===\n\n");

        output.push_str("  Iter │  Welfare ($)  │ Δ Welfare │   Volume   │ Fills\n");
        output.push_str("  ─────┼───────────────┼───────────┼────────────┼──────\n");

        for stat in stats {
            let welfare_dollars = stat.welfare as f64 / 1e9;
            let delta_str = if stat.welfare_delta > 0 {
                format!("+${:.2}", stat.welfare_delta as f64 / 1e9)
            } else if stat.welfare_delta < 0 {
                format!("-${:.2}", (-stat.welfare_delta) as f64 / 1e9)
            } else {
                "    —".to_string()
            };

            output.push_str(&format!(
                "  {:>4} │ {:>13.2} │ {:>9} │ {:>10} │ {:>5}\n",
                stat.iteration,
                welfare_dollars,
                delta_str,
                format_qty(stat.volume),
                stat.fills
            ));
        }

        // Add convergence indicator
        if let Some(last) = stats.last() {
            if last.welfare_delta == 0 && stats.len() > 1 {
                output.push_str("\n  Status: CONVERGED\n");
            } else {
                output.push_str(&format!(
                    "\n  Status: {} iterations (Δ = ${:.2})\n",
                    stats.len(),
                    last.welfare_delta as f64 / 1e9
                ));
            }
        }

        output
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viz_snapshot_serialization() {
        let snapshot = VizSnapshot {
            scenario_name: "test".to_string(),
            config: VizConfig {
                num_markets: 10,
                num_orders: 100,
                num_mm_constraints: 2,
                pipeline_iterations: 3,
            },
            iterations: vec![IterationSnapshot {
                iteration: 1,
                welfare: 1_000_000_000,
                welfare_delta: 1_000_000_000,
                volume: 500,
                volume_delta: 500,
                fills: 10,
                fills_delta: 10,
                price_discovery_fills: 8,
                bundle_fills: 2,
                market_prices: HashMap::new(),
                mm_allocations: vec![],
            }],
            final_result: FinalSnapshot {
                total_welfare: 1_000_000_000,
                total_welfare_dollars: 1.0,
                total_volume: 500,
                total_fills: 10,
                orders_filled: 10,
                orders_unfilled: 5,
                fill_rate: 0.67,
            },
            phase_times: PipelineTimes {
                price_discovery_secs: 0.1,
                price_projection_secs: 0.05,
                allocation_secs: 0.02,
                partial_solving_secs: 0.03,
                combining_secs: 0.01,
                total_secs: 0.21,
            },
        };

        let json = snapshot.to_json();
        assert!(json.contains("\"scenario_name\": \"test\""));
        assert!(json.contains("\"num_markets\": 10"));
    }

    #[test]
    fn test_convergence_summary_empty() {
        let summary = ascii::convergence_summary(&[]);
        assert!(summary.contains("No iteration data"));
    }

    #[test]
    fn test_convergence_summary_with_data() {
        let stats = vec![
            IterationStats {
                iteration: 1,
                welfare: 1_000_000_000,
                volume: 100,
                fills: 5,
                welfare_delta: 1_000_000_000,
                volume_delta: 100,
                fills_delta: 5,
                price_discovery_fills: 4,
                bundle_fills: 1,
            },
            IterationStats {
                iteration: 2,
                welfare: 1_500_000_000,
                volume: 150,
                fills: 8,
                welfare_delta: 500_000_000,
                volume_delta: 50,
                fills_delta: 3,
                price_discovery_fills: 2,
                bundle_fills: 1,
            },
        ];

        let summary = ascii::convergence_summary(&stats);
        assert!(summary.contains("Iter"));
        assert!(summary.contains("Welfare"));
    }
}
