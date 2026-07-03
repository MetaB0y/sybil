//! Visualization and export utilities for solver analysis.
//!
//! `VizSnapshot` is the JSON surface consumed by the Streamlit dashboard and
//! matching-sim exports. It intentionally contains only fields populated by
//! live solvers.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use matching_engine::{MarketId, Problem, NANOS_PER_DOLLAR};

use crate::result::PipelineResult;

/// Complete snapshot of a solver run for visualization and analysis.
#[derive(Clone, Debug, Serialize)]
pub struct VizSnapshot {
    /// Name/description of the scenario.
    pub scenario_name: String,
    /// Solver configuration summary.
    pub config: VizConfig,
    /// Final result summary.
    pub final_result: FinalSnapshot,
    /// Phase timing breakdown.
    pub phase_times: PipelineTimes,
    /// All orders with metadata.
    pub orders: Vec<OrderSnapshot>,
    /// Fill details.
    pub fills_by_iteration: Vec<IterationFills>,
}

/// Configuration summary for the snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct VizConfig {
    pub num_markets: usize,
    pub num_orders: usize,
    pub num_mm_constraints: usize,
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
    pub allocation_secs: f64,
    pub partial_solving_secs: f64,
    pub combining_secs: f64,
    pub total_secs: f64,
}

/// Snapshot of a single order with metadata.
#[derive(Clone, Debug, Serialize)]
pub struct OrderSnapshot {
    pub id: u64,
    /// Market names this order spans.
    pub markets: Vec<String>,
    /// Order type: "single", "bundle", or "spread".
    pub order_type: String,
    /// Side: "bid" (buying YES) or "ask" (selling YES / buying NO).
    pub side: String,
    /// Whether this is a market maker order.
    pub is_mm: bool,
    /// Limit price in dollars.
    pub limit_price: f64,
    /// Maximum quantity.
    pub max_qty: u64,
}

/// Fills grouped for compatibility with existing dashboard loaders.
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
    /// Fill price in dollars.
    pub fill_price: f64,
    /// Welfare contribution in dollars.
    pub welfare: f64,
    /// Source of the fill: "price_discovery" or "bundle".
    pub source: String,
}

impl VizSnapshot {
    /// Create a snapshot from a pipeline result.
    pub fn from_pipeline_result(
        result: &PipelineResult,
        problem: &Problem,
        scenario_name: impl Into<String>,
    ) -> Self {
        let market_names: HashMap<MarketId, String> = problem
            .markets
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();

        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        let order_map: HashMap<u64, &matching_engine::Order> =
            problem.orders.iter().map(|o| (o.id, o)).collect();

        let total_volume: u64 = result.result.fills.iter().map(|f| f.fill_qty.0).sum();

        let filled_order_ids: HashSet<u64> =
            result.result.fills.iter().map(|f| f.order_id).collect();
        let orders_filled = filled_order_ids.len();
        let orders_unfilled = problem.orders.len().saturating_sub(orders_filled);

        let final_result = FinalSnapshot {
            total_welfare: result.result.total_welfare(),
            total_welfare_dollars: result.result.total_welfare() as f64 / NANOS_PER_DOLLAR as f64,
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
                    let has_positive = order
                        .payoffs
                        .iter()
                        .take(order.num_states as usize)
                        .any(|&p| p > 0);
                    let has_negative = order
                        .payoffs
                        .iter()
                        .take(order.num_states as usize)
                        .any(|&p| p < 0);
                    if has_positive && has_negative {
                        "spread".to_string()
                    } else {
                        "bundle".to_string()
                    }
                } else {
                    "bundle".to_string()
                };

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
                    is_mm: mm_order_ids.contains(&order.id),
                    limit_price: order.limit_price.0 as f64 / NANOS_PER_DOLLAR as f64,
                    max_qty: order.max_fill.0,
                }
            })
            .collect();

        let fills_by_iteration = if result.result.fills.is_empty() {
            Vec::new()
        } else {
            let fills = result
                .result
                .fills
                .iter()
                .map(|fill| {
                    let welfare = order_map
                        .get(&fill.order_id)
                        .map(|o| o.welfare_contribution(fill.fill_price, fill.fill_qty))
                        .unwrap_or(0);

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
                        fill_qty: fill.fill_qty.0,
                        fill_price: fill.fill_price.0 as f64 / NANOS_PER_DOLLAR as f64,
                        welfare: welfare as f64 / NANOS_PER_DOLLAR as f64,
                        source,
                    }
                })
                .collect();

            vec![IterationFills {
                iteration: 0,
                fills,
            }]
        };

        VizSnapshot {
            scenario_name: scenario_name.into(),
            config: VizConfig {
                num_markets: problem.markets.len(),
                num_orders: problem.orders.len(),
                num_mm_constraints: problem.mm_constraints.len(),
            },
            final_result,
            phase_times: PipelineTimes {
                price_discovery_secs: result.phase_times.price_discovery_secs,
                allocation_secs: result.phase_times.allocation_secs,
                partial_solving_secs: result.phase_times.partial_solving_secs,
                combining_secs: result.phase_times.combining_secs,
                total_secs: result.total_time_secs,
            },
            orders,
            fills_by_iteration,
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
            },
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
                is_mm: false,
                limit_price: 0.60,
                max_qty: 100,
            }],
            fills_by_iteration: vec![IterationFills {
                iteration: 0,
                fills: vec![FillSnapshot {
                    order_id: 1,
                    fill_qty: 50,
                    fill_price: 0.55,
                    welfare: 0.025,
                    source: "price_discovery".to_string(),
                }],
            }],
        };

        let json = snapshot.to_json();
        assert!(json.contains("\"scenario_name\": \"test\""));
        assert!(json.contains("\"num_markets\": 10"));
        assert!(json.contains("\"orders\""));
        assert!(json.contains("\"fills_by_iteration\""));
    }
}
