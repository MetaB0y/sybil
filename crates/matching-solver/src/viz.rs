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

use matching_engine::{MarketId, Nanos, Problem, NANOS_PER_DOLLAR};

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
    /// YES price at START of iteration (before fills)
    pub yes_price: f64,
    /// NO price at START of iteration (before fills)
    pub no_price: f64,
    /// YES price at END of iteration (after fills) - equals next iteration's start price
    pub yes_price_end: f64,
    /// NO price at END of iteration (after fills) - equals next iteration's start price
    pub no_price_end: f64,
    /// Volume traded in this market (cumulative through this iteration)
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
    pub negrisk_secs: f64,
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
    /// Side: "bid" (buying YES) or "ask" (selling YES / buying NO)
    /// For bundles, this is based on the first market's payoff
    pub side: String,
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
    /// Negrisk arbitrage phase (exploits price < $1 for mutually exclusive outcomes)
    NegriskArbitrage,
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
    /// Negrisk arbitrage (exploits price inconsistencies)
    NegriskArbitrage {
        opportunities_found: usize,
        total_shares: u64,
        welfare_added: f64,
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
                    qty: level.available_qty(),
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
                    qty: level.available_qty(),
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
        _market_names: &HashMap<MarketId, String>,
        fills_count: usize,
        welfare: i64,
        elapsed_secs: f64,
    ) -> Self {
        PhaseSnapshot {
            phase,
            iteration,
            liquidity: LiquiditySnapshot { books: Vec::new() },
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
    #[allow(clippy::too_many_arguments)]
    pub fn capture_with_phase_data(
        phase: PipelinePhase,
        iteration: usize,
        _market_names: &HashMap<MarketId, String>,
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
            liquidity: LiquiditySnapshot { books: Vec::new() },
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

        // Build iteration snapshots with per-iteration per-market data
        let num_iterations = result.iteration_stats.len();
        let iterations: Vec<IterationSnapshot> = result
            .iteration_stats
            .iter()
            .enumerate()
            .map(|(idx, stat)| {
                // Compute cumulative per-market volume/welfare up to this iteration
                // Uses fill_end_idx to slice fills up to this point
                let mut market_volumes: HashMap<MarketId, u64> = HashMap::new();
                let mut market_welfare: HashMap<MarketId, i64> = HashMap::new();

                for fill in result.result.fills.iter().take(stat.fill_end_idx) {
                    if let Some(order) = order_map.get(&fill.order_id) {
                        // Attribute volume to ALL markets in the order (including bundles)
                        // This is standard practice: each leg of a bundle consumes liquidity
                        // and represents shares changing hands in that market
                        for &market_id in order.markets.iter().take(order.num_markets as usize) {
                            *market_volumes.entry(market_id).or_insert(0) += fill.fill_qty;
                        }
                        // Welfare is attributed to first market only to avoid double-counting
                        // (welfare is the total consumer surplus, not per-market)
                        if order.num_markets >= 1 {
                            let market_id = order.markets[0];
                            *market_welfare.entry(market_id).or_insert(0) += fill.welfare(order);
                        }
                    }
                }

                // Get next iteration's prices for "end" prices (or same as current for final iteration)
                let next_prices: &HashMap<MarketId, Vec<Nanos>> = if idx + 1 < num_iterations {
                    &result.iteration_stats[idx + 1].market_prices
                } else {
                    &stat.market_prices // Final iteration: end = start
                };

                // Get per-iteration market prices (start = this iteration, end = next iteration)
                let market_prices: HashMap<String, MarketPrices> = stat
                    .market_prices
                    .iter()
                    .filter_map(|(market_id, prices)| {
                        let name = market_names.get(market_id)?.clone();
                        let yes_price = prices.first().copied().unwrap_or(0) as f64
                            / NANOS_PER_DOLLAR as f64;
                        let no_price = prices.get(1).copied().unwrap_or(0) as f64
                            / NANOS_PER_DOLLAR as f64;

                        // End prices from next iteration (or same for final)
                        let next_market_prices = next_prices.get(market_id);
                        let yes_price_end = next_market_prices
                            .and_then(|p| p.first().copied())
                            .unwrap_or(0) as f64
                            / NANOS_PER_DOLLAR as f64;
                        let no_price_end = next_market_prices
                            .and_then(|p| p.get(1).copied())
                            .unwrap_or(0) as f64
                            / NANOS_PER_DOLLAR as f64;

                        // Use cumulative volume/welfare up to this iteration
                        let volume = market_volumes.get(market_id).copied().unwrap_or(0);
                        let welfare = market_welfare.get(market_id).copied().unwrap_or(0);

                        Some((
                            name,
                            MarketPrices {
                                yes_price,
                                no_price,
                                yes_price_end,
                                no_price_end,
                                volume,
                                welfare,
                            },
                        ))
                    })
                    .collect();

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

        // Count unique filled orders (an order may appear in multiple fills)
        let filled_order_ids: std::collections::HashSet<u64> =
            result.result.fills.iter().map(|f| f.order_id).collect();
        let orders_filled = filled_order_ids.len();

        // Unfilled = total orders - filled (pipeline doesn't track unfilled explicitly)
        let orders_unfilled = problem.orders.len().saturating_sub(orders_filled);

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

                // Determine side based on payoffs
                // payoffs[0] > 0 means long first outcome (YES for single-market) = "bid"
                // payoffs[0] < 0 or payoffs[1] > 0 means short first / long second = "ask"
                let side = if order.num_states > 0 && order.payoffs[0] > 0 {
                    "bid".to_string()
                } else {
                    "ask".to_string()
                };

                OrderSnapshot {
                    id: order.id,
                    markets: order_markets,
                    order_type,
                    side,
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
                negrisk_secs: result.phase_times.negrisk_secs,
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

    /// Create a snapshot with phase data (for viz feature).
    #[cfg(feature = "viz")]
    pub fn from_pipeline_result_with_phases(
        result: &PipelineResult,
        problem: &Problem,
        scenario_name: impl Into<String>,
        phase_snapshots: Vec<PhaseSnapshot>,
    ) -> Self {
        let mut snapshot = Self::from_pipeline_result(result, problem, scenario_name);
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
                negrisk_secs: 0.05,
                allocation_secs: 0.02,
                partial_solving_secs: 0.03,
                combining_secs: 0.01,
                total_secs: 0.21,
            },
            orders: vec![OrderSnapshot {
                id: 1,
                markets: vec!["market_a".to_string()],
                order_type: "single".to_string(),
                side: "bid".to_string(),
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
                fill_start_idx: 0,
                fill_end_idx: 5,
                market_prices: HashMap::new(),
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
                fill_start_idx: 5,
                fill_end_idx: 8,
                market_prices: HashMap::new(),
            },
        ];

        let summary = ascii::convergence_summary(&stats);
        assert!(summary.contains("Iter"));
        assert!(summary.contains("Welfare"));
    }
}
