//! Canonical full-witness byte schema used by `witness_root`.

use std::collections::HashMap;

use matching_engine::{MarketGroup, MarketId, MmConstraint, MmSide, Nanos};

use crate::event_schema::{
    fill_leaf_value, order_accepted_leaf_value, order_rejected_leaf_value, system_event_leaf_value,
};
use crate::snapshot_schema::{
    append_i64, append_market_id, append_string, append_u32, append_u64, append_witness_account,
    append_witness_pre_state_sidecar, append_witness_state_sidecar,
};
use crate::types::{AccountSnapshot, BlockWitness, DepositAccumulatorWitness, WitnessBlockHeader};

pub const WITNESS_FORMAT_VERSION: u8 = 3;

pub fn canonical_witness_bytes(witness: &BlockWitness) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(WITNESS_FORMAT_VERSION);
    append_header(&mut out, &witness.header);
    match &witness.previous_header {
        Some(previous) => {
            out.push(1);
            append_header(&mut out, previous);
        }
        None => out.push(0),
    }

    let mut orders: Vec<_> = witness.orders.iter().collect();
    orders.sort_by_key(|order| order.order.id);
    append_u64(&mut out, orders.len() as u64);
    for order in orders {
        out.extend_from_slice(&order_accepted_leaf_value(order));
    }

    let mut rejections: Vec<_> = witness.rejections.iter().collect();
    rejections.sort_by_key(|rejection| rejection.order.id);
    append_u64(&mut out, rejections.len() as u64);
    for rejection in rejections {
        out.extend_from_slice(&order_rejected_leaf_value(rejection));
    }

    append_u64(&mut out, witness.system_events.len() as u64);
    for event in &witness.system_events {
        out.extend_from_slice(&system_event_leaf_value(event));
    }

    append_deposit_accumulator(&mut out, &witness.deposit_accumulator);

    append_u64(&mut out, witness.fills.len() as u64);
    for fill in &witness.fills {
        out.extend_from_slice(&fill_leaf_value(fill));
    }

    append_clearing_prices(&mut out, &witness.clearing_prices);
    append_i64(&mut out, witness.total_welfare);
    append_i64(&mut out, witness.minting_cost);
    append_mm_constraints(&mut out, &witness.mm_constraints);
    append_market_groups(&mut out, &witness.market_groups);
    append_account_section(&mut out, &witness.pre_state);
    append_account_section(&mut out, &witness.post_system_state);
    append_account_section(&mut out, &witness.post_state);
    append_witness_state_sidecar(&mut out, &witness.state_sidecar);
    append_witness_pre_state_sidecar(&mut out, &witness.pre_state_sidecar);

    let mut resolved_markets = witness.resolved_markets.clone();
    resolved_markets.sort_by_key(|market| market.0);
    append_u64(&mut out, resolved_markets.len() as u64);
    for market in resolved_markets {
        append_market_id(&mut out, market);
    }

    out
}

fn append_header(out: &mut Vec<u8>, header: &WitnessBlockHeader) {
    append_u64(out, header.height);
    out.extend_from_slice(&header.parent_hash);
    out.extend_from_slice(&header.state_root);
    out.extend_from_slice(&header.events_root);
    append_u32(out, header.order_count);
    append_u32(out, header.fill_count);
    append_u64(out, header.timestamp_ms);
}

fn append_clearing_prices(out: &mut Vec<u8>, clearing_prices: &HashMap<MarketId, Vec<Nanos>>) {
    let mut prices: Vec<_> = clearing_prices.iter().collect();
    prices.sort_by_key(|(market, _)| market.0);
    append_u64(out, prices.len() as u64);
    for (market, outcomes) in prices {
        append_market_id(out, *market);
        append_u32(out, outcomes.len() as u32);
        for price in outcomes {
            append_u64(out, price.0);
        }
    }
}

fn append_mm_constraints(out: &mut Vec<u8>, constraints: &[MmConstraint]) {
    let mut constraints: Vec<_> = constraints.iter().collect();
    constraints.sort_by_key(|constraint| constraint.mm_id.0);
    append_u64(out, constraints.len() as u64);
    for constraint in constraints {
        append_u64(out, constraint.mm_id.0);
        append_u64(out, constraint.max_capital.0);

        let mut order_ids = constraint.order_ids.clone();
        order_ids.sort_unstable();
        append_u64(out, order_ids.len() as u64);
        for order_id in order_ids {
            append_u64(out, order_id);
        }

        let mut sides: Vec<_> = constraint.order_sides.iter().collect();
        sides.sort_by_key(|(order_id, _)| **order_id);
        append_u64(out, sides.len() as u64);
        for (order_id, side) in sides {
            append_u64(out, *order_id);
            append_mm_side(out, *side);
        }
    }
}

fn append_mm_side(out: &mut Vec<u8>, side: MmSide) {
    out.push(match side {
        MmSide::SellYes => 0,
        MmSide::BuyYes => 1,
        MmSide::SellNo => 2,
        MmSide::BuyNo => 3,
    });
}

fn append_market_groups(out: &mut Vec<u8>, groups: &[MarketGroup]) {
    let mut groups: Vec<_> = groups.iter().collect();
    groups.sort_by(|left, right| {
        let left_first = left
            .markets
            .iter()
            .map(|market| market.0)
            .min()
            .unwrap_or(u32::MAX);
        let right_first = right
            .markets
            .iter()
            .map(|market| market.0)
            .min()
            .unwrap_or(u32::MAX);
        left_first
            .cmp(&right_first)
            .then(left.name.cmp(&right.name))
    });
    append_u64(out, groups.len() as u64);
    for group in groups {
        append_string(out, &group.name);
        let mut markets = group.markets.clone();
        markets.sort_by_key(|market| market.0);
        append_u64(out, markets.len() as u64);
        for market in markets {
            append_market_id(out, market);
        }
    }
}

fn append_account_section(out: &mut Vec<u8>, accounts: &[AccountSnapshot]) {
    let mut accounts: Vec<_> = accounts.iter().collect();
    accounts.sort_by_key(|account| account.id);
    append_u64(out, accounts.len() as u64);
    for account in accounts {
        append_witness_account(out, account);
    }
}

fn append_deposit_accumulator(out: &mut Vec<u8>, accumulator: &DepositAccumulatorWitness) {
    out.extend_from_slice(b"sybil/witness/deposit-accumulator");
    for hash in accumulator.pre_frontier {
        out.extend_from_slice(&hash);
    }
    append_u64(out, accumulator.pre_count);
    append_u64(out, accumulator.new_deposits.len() as u64);
    for deposit in &accumulator.new_deposits {
        out.extend_from_slice(b"sybil/witness/l1-deposit");
        append_u64(out, deposit.deposit_id);
        append_u64(out, deposit.chain_id);
        out.extend_from_slice(&deposit.vault_address);
        out.extend_from_slice(&deposit.token_address);
        out.extend_from_slice(&deposit.sender);
        out.extend_from_slice(&deposit.sybil_account_key);
        append_u64(out, deposit.amount_token_units);
        out.extend_from_slice(&deposit.deposit_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DepositAccumulatorWitness, StateSidecarSnapshot, WitnessBlockHeader};

    #[test]
    fn canonical_witness_bytes_are_stable_for_empty_witness() {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root: [1u8; 32],
                events_root: [2u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };

        let bytes = canonical_witness_bytes(&witness);
        assert_eq!(bytes[0], WITNESS_FORMAT_VERSION);
        assert_eq!(bytes.len(), 1533);
    }
}
