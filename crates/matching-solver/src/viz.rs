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

use std::collections::{HashMap, HashSet};

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

    /// All orders with metadata
    pub orders: Vec<OrderSnapshot>,

    /// Per-iteration fill details
    pub fills_by_iteration: Vec<IterationFills>,

    /// Phase snapshots for detailed analysis (viz feature only)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub phase_snapshots: Vec<PhaseSnapshot>,

    /// Initial liquidity before solving (viz feature only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_liquidity: Option<LiquiditySnapshot>,

    /// Final liquidity after solving (viz feature only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_liquidity: Option<LiquiditySnapshot>,
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

/// Snapshot of a single order with metadata.
#[derive(Clone, Debug, Serialize)]
pub struct OrderSnapshot {
    pub id: u64,
    /// Market names this order spans
    pub markets: Vec<String>,
    /// Order type: "single", "bundle", or "spread"
    pub order_type: String,
    /// Whether this is an all-or-none order
    pub is_aon: bool,
    /// Whether this is a market maker order
    pub is_mm: bool,
    /// Limit price in dollars
    pub limit_price: f64,
    /// Maximum quantity
    pub max_qty: u64,
}

/// Fills for a single iteration.
#[derive(Clone, Debug, Serialize)]
pub struct IterationFills {
    pub iteration: usize,
    pub fills: Vec<FillSnapshot>,
}

/// Snapshot of a single fill.
#[derive(Clone, Debug, Serialize)]
pub struct FillSnapshot {
    pub order_id: u64,
    pub fill_qty: u64,
    /// Fill price in dollars
    pub fill_price: f64,
    /// Welfare contribution in dollars
    pub welfare: f64,
    /// Source of the fill: "price_discovery" or "bundle"
    pub source: String,
}

// ============================================================================
// Orderbook and Phase Snapshots (viz feature)
// ============================================================================

/// Orderbook level for visualization.
#[derive(Clone, Debug, Serialize)]
pub struct BookLevelSnapshot {
    /// Price as fraction 0.0-1.0
    pub price: f64,
    /// Quantity at this level
    pub qty: u64,
    /// Cumulative quantity for depth chart
    pub cumulative_qty: u64,
}

/// Single outcome orderbook snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct OutcomeBookSnapshot {
    pub market_name: String,
    /// 0=YES, 1=NO
    pub outcome: u8,
    pub bids: Vec<BookLevelSnapshot>,
    pub asks: Vec<BookLevelSnapshot>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub spread: Option<f64>,
    pub mid_price: Option<f64>,
    pub total_bid_qty: u64,
    pub total_ask_qty: u64,
}

/// Complete liquidity snapshot across all markets/outcomes.
#[derive(Clone, Debug, Serialize)]
pub struct LiquiditySnapshot {
    pub books: Vec<OutcomeBookSnapshot>,
}

/// Phase identifier for pipeline stages.
#[derive(Clone, Debug, Serialize)]
pub enum PipelinePhase {
    Initial,
    PriceDiscovery,
    PriceProjection,
    MmAllocation,
    /// After single-market fills are merged (confirmed single-market orders)
    Merged,
    /// Bundle/arbitrage matching phase
    BundleMatching,
    Final,
}

/// Snapshot at a phase boundary.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseSnapshot {
    pub phase: PipelinePhase,
    pub iteration: usize,
    pub liquidity: LiquiditySnapshot,
    /// Cumulative confirmed fills (from result.result)
    pub fills_count: usize,
    /// Cumulative confirmed welfare (from result.result)
    pub welfare: i64,
    pub elapsed_secs: f64,
    /// Fills produced by THIS phase (phase-specific output)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_fills: Option<usize>,
    /// Welfare produced by THIS phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_welfare: Option<i64>,
    /// Additional phase-specific data (e.g., violations fixed, orders activated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_metadata: Option<PhaseMetadata>,
}

/// Phase-specific metadata.
#[derive(Clone, Debug, Serialize)]
pub enum PhaseMetadata {
    PriceDiscovery {
        markets_priced: usize,
    },
    PriceProjection {
        violations_fixed: usize,
        max_adjustment: f64,
        iterations: usize,
    },
    MmAllocation {
        orders_activated: usize,
        mm_count: usize,
    },
    /// Merged single-market fills
    Merged {
        single_market_fills: usize,
    },
    /// Bundle/arbitrage matching
    BundleMatching {
        solver_name: String,
    },
}

impl LiquiditySnapshot {
    /// Create a liquidity snapshot from a LiquidityPool.
    pub fn from_liquidity_pool(
        pool: &matching_engine::LiquidityPool,
        market_names: &HashMap<MarketId, String>,
    ) -> Self {
        let mut books = Vec::new();

        for (&(market_id, outcome), book) in &pool.books {
            let market_name = market_names
                .get(&market_id)
                .cloned()
                .unwrap_or_else(|| format!("market_{}", market_id.0));

            // Extract ask levels (sorted by price ascending)
            let mut asks: Vec<BookLevelSnapshot> = book
                .asks()
                .iter()
                .map(|level| BookLevelSnapshot {
                    price: level.price as f64 / NANOS_PER_DOLLAR as f64,
                    qty: level.available_qty,
                    cumulative_qty: 0, // Will be filled below
                })
                .collect();

            // Calculate cumulative quantities for asks (ascending price order)
            let mut cumulative = 0u64;
            for ask in &mut asks {
                cumulative += ask.qty;
                ask.cumulative_qty = cumulative;
            }

            // Extract bid levels (sorted by price descending - best bid first)
            let mut bids: Vec<BookLevelSnapshot> = book
                .bids()
                .iter()
                .map(|level| BookLevelSnapshot {
                    price: level.price as f64 / NANOS_PER_DOLLAR as f64,
                    qty: level.available_qty,
                    cumulative_qty: 0, // Will be filled below
                })
                .collect();

            // Calculate cumulative quantities for bids (descending price order)
            let mut cumulative_bids = 0u64;
            for bid in &mut bids {
                cumulative_bids += bid.qty;
                bid.cumulative_qty = cumulative_bids;
            }

            let best_ask = asks.first().map(|a| a.price);
            let best_bid = bids.first().map(|b| b.price);
            let spread = match (best_bid, best_ask) {
                (Some(bid), Some(ask)) => Some(ask - bid),
                _ => None,
            };
            let mid_price = match (best_bid, best_ask) {
                (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
                _ => best_ask, // Use best ask as proxy for mid if no bids
            };

            let total_ask_qty: u64 = asks.iter().map(|a| a.qty).sum();
            let total_bid_qty: u64 = bids.iter().map(|b: &BookLevelSnapshot| b.qty).sum();

            books.push(OutcomeBookSnapshot {
                market_name,
                outcome,
                bids,
                asks,
                best_bid,
                best_ask,
                spread,
                mid_price,
                total_bid_qty,
                total_ask_qty,
            });
        }

        // Sort books by market name and outcome for consistent ordering
        books.sort_by(|a, b| {
            a.market_name
                .cmp(&b.market_name)
                .then_with(|| a.outcome.cmp(&b.outcome))
        });

        LiquiditySnapshot { books }
    }
}

impl PhaseSnapshot {
    /// Capture a phase snapshot.
    #[cfg(feature = "viz")]
    pub fn capture(
        phase: PipelinePhase,
        iteration: usize,
        liquidity: &matching_engine::LiquidityPool,
        market_names: &HashMap<MarketId, String>,
        fills_count: usize,
        welfare: i64,
        elapsed_secs: f64,
    ) -> Self {
        PhaseSnapshot {
            phase,
            iteration,
            liquidity: LiquiditySnapshot::from_liquidity_pool(liquidity, market_names),
            fills_count,
            welfare,
            elapsed_secs,
            phase_fills: None,
            phase_welfare: None,
            phase_metadata: None,
        }
    }

    /// Capture with phase-specific data.
    #[cfg(feature = "viz")]
    pub fn capture_with_phase_data(
        phase: PipelinePhase,
        iteration: usize,
        liquidity: &matching_engine::LiquidityPool,
        market_names: &HashMap<MarketId, String>,
        fills_count: usize,
        welfare: i64,
        elapsed_secs: f64,
        phase_fills: Option<usize>,
        phase_welfare: Option<i64>,
        phase_metadata: Option<PhaseMetadata>,
    ) -> Self {
        PhaseSnapshot {
            phase,
            iteration,
            liquidity: LiquiditySnapshot::from_liquidity_pool(liquidity, market_names),
            fills_count,
            welfare,
            elapsed_secs,
            phase_fills,
            phase_welfare,
            phase_metadata,
        }
    }
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

        // Build MM order IDs set for identifying MM orders
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        // Build order lookup map
        let order_map: HashMap<u64, &matching_engine::Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

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

        // Build order snapshots
        let orders: Vec<OrderSnapshot> = problem
            .orders
            .iter()
            .map(|order| {
                let order_markets: Vec<String> = order
                    .active_markets()
                    .filter_map(|m| market_names.get(&m).cloned())
                    .collect();

                let order_type = if order.num_markets == 1 {
                    "single".to_string()
                } else if order.num_markets == 2 {
                    // Check if it's a spread (opposite payoffs) or bundle (same direction)
                    let has_positive = order.payoffs.iter().take(order.num_states as usize).any(|&p| p > 0);
                    let has_negative = order.payoffs.iter().take(order.num_states as usize).any(|&p| p < 0);
                    if has_positive && has_negative {
                        "spread".to_string()
                    } else {
                        "bundle".to_string()
                    }
                } else {
                    "bundle".to_string()
                };

                OrderSnapshot {
                    id: order.id,
                    markets: order_markets,
                    order_type,
                    is_aon: order.is_all_or_none(),
                    is_mm: mm_order_ids.contains(&order.id),
                    limit_price: order.limit_price as f64 / NANOS_PER_DOLLAR as f64,
                    max_qty: order.max_fill,
                }
            })
            .collect();

        // Build fills by iteration
        // Note: Currently we only have final fills, not per-iteration tracking.
        // All fills are placed in the final iteration.
        let fills_by_iteration = if !result.result.fills.is_empty() {
            let fills: Vec<FillSnapshot> = result
                .result
                .fills
                .iter()
                .map(|fill| {
                    let welfare = order_map
                        .get(&fill.order_id)
                        .map(|o| o.welfare_contribution(fill.fill_price, fill.fill_qty))
                        .unwrap_or(0);

                    // Determine source based on order type
                    let source = order_map
                        .get(&fill.order_id)
                        .map(|o| {
                            if o.num_markets > 1 {
                                "bundle".to_string()
                            } else {
                                "price_discovery".to_string()
                            }
                        })
                        .unwrap_or_else(|| "unknown".to_string());

                    FillSnapshot {
                        order_id: fill.order_id,
                        fill_qty: fill.fill_qty,
                        fill_price: fill.fill_price as f64 / NANOS_PER_DOLLAR as f64,
                        welfare: welfare as f64 / NANOS_PER_DOLLAR as f64,
                        source,
                    }
                })
                .collect();

            vec![IterationFills {
                iteration: result.iterations,
                fills,
            }]
        } else {
            Vec::new()
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
            orders,
            fills_by_iteration,
            phase_snapshots: Vec::new(),
            initial_liquidity: None,
            final_liquidity: None,
        }
    }

    /// Create a snapshot with liquidity data (for viz feature).
    #[cfg(feature = "viz")]
    pub fn from_pipeline_result_with_liquidity(
        result: &PipelineResult,
        problem: &Problem,
        scenario_name: impl Into<String>,
        initial_liquidity: &matching_engine::LiquidityPool,
        phase_snapshots: Vec<PhaseSnapshot>,
    ) -> Self {
        let mut snapshot = Self::from_pipeline_result(result, problem, scenario_name);

        // Build market name lookup
        let market_names: HashMap<MarketId, String> = problem
            .markets
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();

        snapshot.initial_liquidity = Some(LiquiditySnapshot::from_liquidity_pool(
            initial_liquidity,
            &market_names,
        ));
        snapshot.final_liquidity = Some(LiquiditySnapshot::from_liquidity_pool(
            &result.result.remaining_liquidity,
            &market_names,
        ));
        snapshot.phase_snapshots = phase_snapshots;

        snapshot
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
            orders: vec![OrderSnapshot {
                id: 1,
                markets: vec!["market_a".to_string()],
                order_type: "single".to_string(),
                is_aon: false,
                is_mm: false,
                limit_price: 0.60,
                max_qty: 100,
            }],
            fills_by_iteration: vec![IterationFills {
                iteration: 1,
                fills: vec![FillSnapshot {
                    order_id: 1,
                    fill_qty: 50,
                    fill_price: 0.55,
                    welfare: 0.025,
                    source: "price_discovery".to_string(),
                }],
            }],
            phase_snapshots: Vec::new(),
            initial_liquidity: None,
            final_liquidity: None,
        };

        let json = snapshot.to_json();
        assert!(json.contains("\"scenario_name\": \"test\""));
        assert!(json.contains("\"num_markets\": 10"));
        assert!(json.contains("\"orders\""));
        assert!(json.contains("\"fills_by_iteration\""));
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
