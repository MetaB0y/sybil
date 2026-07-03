//! JSON export of solver comparison results for offline analysis.

use std::collections::{HashMap, HashSet};

use matching_engine::{MarketId, Order, Problem};
use sybil_verifier::BlockWitness;

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
        "max_fill": order.max_fill,
        "is_seller": order.is_seller(),
        "num_markets": order.num_markets,
    })
}

/// Build a detailed JSON comparison of solver results for offline analysis.
pub fn build_comparison_json(
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
            serde_json::json!({
                "solver": name,
                "total_welfare": result.total_welfare(),
                "total_welfare_dollars": result.total_welfare() as f64 / 1e9,
                "orders_filled": result.orders_filled,
                "orders_unfilled_liquidity": result.orders_unfilled_liquidity,
                "total_quantity_filled": result.total_quantity_filled,
                "welfare_breakdown": {
                    "user_dollars": user_welfare as f64 / 1e9,
                    "mm_dollars": mm_welfare as f64 / 1e9,
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
