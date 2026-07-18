//! Shared assembly for exact and coordinated component solves.
//!
//! Component algorithms own partitioning and coordination. This module owns
//! only the neutral operation of merging disjoint results and re-establishing
//! the original problem's integer budget, minting, and welfare invariants.

use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, Problem};

use crate::MatchingResult;
use crate::result::{
    PipelineResult, PipelineTimings, PriceDiscoveryResult, SolverDiagnostics, TerminationStatus,
};

/// Aggregate disjoint component results, then enforce the original problem's
/// global integer invariants and recompute welfare.
pub(crate) fn assemble_component_results(
    problem: &Problem,
    component_results: Vec<PipelineResult>,
) -> PipelineResult {
    let mut result = aggregate_results(component_results);

    let prices = match crate::lp_solver::stabilize_integer_result(&mut result.result, problem) {
        Ok(prices) => prices,
        Err(error) => {
            result.result = MatchingResult::new();
            result.price_discovery = None;
            result.diagnostics = SolverDiagnostics {
                algorithm: "component-assembly".to_string(),
                status: TerminationStatus::PostProcessingFailure,
                message: Some(error),
                ..Default::default()
            };
            return result;
        }
    };
    if let Some(price_discovery) = result.price_discovery.as_mut() {
        price_discovery.prices = prices;
        price_discovery.total_fills = result.result.fills.len();
        price_discovery.total_welfare = result.result.total_welfare();
    }

    result
}

fn aggregate_results(component_results: Vec<PipelineResult>) -> PipelineResult {
    let mut merged = MatchingResult::new();
    let mut prices: HashMap<MarketId, Vec<Nanos>> = HashMap::new();
    let mut total_solve_time = 0.0f64;

    for result in &component_results {
        // Component order and market sets are disjoint by construction.
        merged.fills.extend(result.result.fills.iter().cloned());
        merged.gross_welfare += result.result.gross_welfare;
        merged.minting_cost += result.result.minting_cost;
        merged.orders_filled += result.result.orders_filled;
        merged.orders_unfilled_liquidity += result.result.orders_unfilled_liquidity;
        merged.total_quantity_filled += result.result.total_quantity_filled;

        if let Some(price_discovery) = &result.price_discovery {
            for (market_id, market_prices) in &price_discovery.prices {
                prices.insert(*market_id, market_prices.clone());
            }
        }

        total_solve_time += result.total_time_secs;
    }
    // Component numbering is an implementation detail. Settlement and account
    // event digests must not depend on HashMap iteration order during
    // partition construction, so restore the canonical admitted-order order.
    merged.fills.sort_by_key(|fill| fill.order_id);

    let mut result = PipelineResult::empty();
    result.result = merged;
    result.price_discovery = Some(PriceDiscoveryResult {
        total_welfare: result.result.total_welfare(),
        total_fills: result.result.fills.len(),
        prices,
    });
    result.phase_times = PipelineTimings {
        price_discovery_secs: total_solve_time,
        ..Default::default()
    };
    result
}

#[cfg(test)]
mod tests {
    use matching_engine::{Fill, Qty};

    use super::*;

    #[test]
    fn aggregation_canonicalizes_fill_order() {
        let mut later = PipelineResult::empty();
        later
            .result
            .fills
            .push(Fill::new(20, Qty(1), Nanos(500_000_000)));
        let mut earlier = PipelineResult::empty();
        earlier
            .result
            .fills
            .push(Fill::new(10, Qty(1), Nanos(500_000_000)));

        let result = aggregate_results(vec![later, earlier]);
        let order_ids: Vec<_> = result
            .result
            .fills
            .iter()
            .map(|fill| fill.order_id)
            .collect();

        assert_eq!(order_ids, [10, 20]);
    }
}
