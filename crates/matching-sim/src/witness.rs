//! Block-witness construction for standalone verification.
//!
//! The sim has no accounts or settlement, so only Layer 1 (match verification)
//! is meaningful. Layers 2–4 (settlement, block, orders) require a full
//! sequencer and are therefore left empty in the witnesses built here.

use std::collections::HashMap;

use matching_engine::{MarketId, Problem};
use matching_solver::MatchingResult;
use sybil_verifier::{BlockWitness, WitnessBlockHeader, WitnessOrder};

#[cfg(any(feature = "lp", feature = "conic"))]
use matching_solver::PipelineResult;

/// Shared `BlockWitness` builder used by every solver path.
///
/// Callers differ only in how they obtain `clearing_prices` (the pipeline reads
/// them from price discovery; MILP carries them directly), so that is the sole
/// parameter that varies — everything else is derived from the `Problem` and the
/// [`MatchingResult`].
pub fn build_witness(
    problem: &Problem,
    result: &MatchingResult,
    clearing_prices: HashMap<MarketId, Vec<u64>>,
) -> BlockWitness {
    let witness_orders: Vec<WitnessOrder> = problem
        .orders
        .iter()
        .map(|o| WitnessOrder {
            order: o.clone(),
            account_id: 0, // not meaningful in sim
            is_mm: problem
                .mm_constraints
                .iter()
                .any(|mm| mm.order_ids.contains(&o.id)),
        })
        .collect();

    let mut witness = BlockWitness {
        header: WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            events_root: [0u8; 32],
            order_count: problem.orders.len() as u32,
            fill_count: result.fills.len() as u32,
            timestamp_ms: 0,
        },
        previous_header: None,
        orders: witness_orders,
        rejections: vec![],
        system_events: vec![],
        fills: result.fills.clone(),
        clearing_prices,
        total_welfare: result.total_welfare(),
        minting_cost: result.minting_cost,
        mm_constraints: problem.mm_constraints.clone(),
        market_groups: problem.market_groups.clone(),
        pre_state: vec![],
        post_system_state: vec![],
        post_state: vec![],
        state_sidecar: Default::default(),
        resolved_markets: vec![],
    };
    witness.header.events_root = sybil_verifier::event_commitment::compute_events_root(&witness);
    witness
}

/// Build a `BlockWitness` from a `PipelineResult` using real orders and fills.
#[cfg(any(feature = "lp", feature = "conic"))]
pub fn witness_from_pipeline(problem: &Problem, result: &PipelineResult) -> BlockWitness {
    let clearing_prices = result
        .price_discovery
        .as_ref()
        .map(|pd| pd.prices.clone())
        .unwrap_or_default();
    build_witness(problem, &result.result, clearing_prices)
}

/// Build a `BlockWitness` from a `MilpResult` using real orders and fills.
pub fn witness_from_milp(problem: &Problem, result: &matching_solver::MilpResult) -> BlockWitness {
    build_witness(problem, &result.result, result.clearing_prices.clone())
}
