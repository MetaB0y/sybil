//! Canonical full-witness byte schema used by `witness_root`.

use std::collections::HashMap;

use matching_engine::{MarketGroup, MarketId, MmConstraint, MmSide, Nanos};

use crate::canonical::append_order;
use crate::event_schema::{
    fill_leaf_value, order_accepted_leaf_value, order_rejected_leaf_value, system_event_leaf_value,
};
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
    OracleSourceSnapshot, ResolutionProposalSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot, WitnessBlockHeader,
};

pub const WITNESS_FORMAT_VERSION: u8 = 1;

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
    append_state_sidecar(&mut out, &witness.state_sidecar);

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
            append_u64(out, *price);
        }
    }
}

fn append_mm_constraints(out: &mut Vec<u8>, constraints: &[MmConstraint]) {
    let mut constraints: Vec<_> = constraints.iter().collect();
    constraints.sort_by_key(|constraint| constraint.mm_id.0);
    append_u64(out, constraints.len() as u64);
    for constraint in constraints {
        append_u64(out, constraint.mm_id.0);
        append_u64(out, constraint.max_capital);

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
        append_account(out, account);
    }
}

fn append_account(out: &mut Vec<u8>, account: &AccountSnapshot) {
    out.extend_from_slice(b"sybil/witness/account");
    append_u64(out, account.id);
    append_i64(out, account.balance);
    append_i64(out, account.total_deposited);

    let mut positions = account.positions.clone();
    positions.sort_by_key(|(market, outcome, qty)| (market.0, *outcome, *qty));
    append_u64(out, positions.len() as u64);
    for (market, outcome, qty) in positions {
        append_market_id(out, market);
        out.push(outcome);
        append_i64(out, qty);
    }

    out.extend_from_slice(&account.events_digest);
}

fn append_state_sidecar(out: &mut Vec<u8>, sidecar: &StateSidecarSnapshot) {
    out.extend_from_slice(b"sybil/witness/state-sidecar");
    append_bridge(out, &sidecar.bridge);

    let mut markets: Vec<_> = sidecar.markets.iter().collect();
    markets.sort_by_key(|market| market.market_id.0);
    append_u64(out, markets.len() as u64);
    for market in markets {
        append_market_snapshot(out, market);
    }

    let mut groups: Vec<_> = sidecar.market_groups.iter().collect();
    groups.sort_by_key(|group| group.group_id);
    append_u64(out, groups.len() as u64);
    for group in groups {
        append_market_group_snapshot(out, group);
    }

    let mut resting_orders: Vec<_> = sidecar.resting_orders.iter().collect();
    resting_orders.sort_by_key(|resting| resting.order.id);
    append_u64(out, resting_orders.len() as u64);
    for resting in resting_orders {
        append_resting_order(out, resting);
    }

    let mut reservations: Vec<_> = sidecar.account_reservations.iter().collect();
    reservations.sort_by_key(|reservation| reservation.account_id);
    append_u64(out, reservations.len() as u64);
    for reservation in reservations {
        append_account_reservation(out, reservation);
    }
}

fn append_bridge(out: &mut Vec<u8>, bridge: &BridgeStateSnapshot) {
    append_u64(out, bridge.deposit_cursor);
    out.extend_from_slice(&bridge.deposit_root);
    append_u64(out, bridge.next_withdrawal_id);

    let mut withdrawals: Vec<_> = bridge.withdrawals.iter().collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    append_u64(out, withdrawals.len() as u64);
    for withdrawal in withdrawals {
        append_withdrawal(out, withdrawal);
    }
}

fn append_withdrawal(out: &mut Vec<u8>, withdrawal: &WithdrawalSnapshot) {
    append_u64(out, withdrawal.withdrawal_id);
    append_u64(out, withdrawal.account_id);
    out.extend_from_slice(&withdrawal.recipient);
    out.extend_from_slice(&withdrawal.token);
    append_u64(out, withdrawal.amount_token_units);
    append_u64(out, withdrawal.amount_nanos);
    append_u64(out, withdrawal.expiry_height);
    out.extend_from_slice(&withdrawal.nullifier);
}

fn append_market_snapshot(out: &mut Vec<u8>, market: &MarketSnapshot) {
    append_market_id(out, market.market_id);
    append_string(out, &market.name);
    out.push(market.num_outcomes);
    append_market_status(out, &market.status);
    out.extend_from_slice(&market.metadata_digest);
    append_string(out, &market.resolution_template);
}

fn append_market_group_snapshot(out: &mut Vec<u8>, group: &MarketGroupSnapshot) {
    append_u64(out, group.group_id);
    append_string(out, &group.name);
    let mut markets = group.markets.clone();
    markets.sort_by_key(|market| market.0);
    append_u64(out, markets.len() as u64);
    for market in markets {
        append_market_id(out, market);
    }
}

fn append_market_status(out: &mut Vec<u8>, status: &MarketStatusSnapshot) {
    match status {
        MarketStatusSnapshot::Active => out.push(0),
        MarketStatusSnapshot::Proposed {
            proposal,
            challenge_deadline_ms,
        } => {
            out.push(1);
            append_resolution_proposal(out, proposal);
            append_u64(out, *challenge_deadline_ms);
        }
        MarketStatusSnapshot::Challenged {
            proposal,
            challenge,
        } => {
            out.push(2);
            append_resolution_proposal(out, proposal);
            append_challenge(out, challenge);
        }
        MarketStatusSnapshot::Resolved { record } => {
            out.push(3);
            append_resolution_record(out, record);
        }
        MarketStatusSnapshot::Voided => out.push(4),
    }
}

fn append_resolution_proposal(out: &mut Vec<u8>, proposal: &ResolutionProposalSnapshot) {
    append_u64(out, proposal.id);
    append_market_id(out, proposal.market_id);
    append_u64(out, proposal.payout_nanos);
    append_oracle_source(out, &proposal.source);
    append_u64(out, proposal.proposed_at_ms);
    append_option_string(out, proposal.reason.as_deref());
}

fn append_challenge(out: &mut Vec<u8>, challenge: &ChallengeSnapshot) {
    append_u64(out, challenge.id);
    append_u64(out, challenge.challenger);
    append_u64(out, challenge.proposal_id);
    append_u64(out, challenge.bond_amount);
    append_u64(out, challenge.proposed_payout_nanos);
    append_string(out, &challenge.reason);
    append_u64(out, challenge.challenged_at_ms);
}

fn append_resolution_record(out: &mut Vec<u8>, record: &ResolutionRecordSnapshot) {
    append_market_id(out, record.market_id);
    append_u64(out, record.payout_nanos);
    append_oracle_source(out, &record.resolved_by);
    append_u64(out, record.resolved_at_ms);
    append_option(out, record.proposal.as_ref(), append_resolution_proposal);
    append_option(out, record.challenge.as_ref(), append_challenge);
}

fn append_oracle_source(out: &mut Vec<u8>, source: &OracleSourceSnapshot) {
    match source {
        OracleSourceSnapshot::Admin => out.push(0),
        OracleSourceSnapshot::DataFeed(feed_id) => {
            out.push(1);
            append_u64(out, *feed_id);
        }
        OracleSourceSnapshot::AutomatedL0 => out.push(2),
    }
}

fn append_resting_order(out: &mut Vec<u8>, resting: &RestingOrderSnapshot) {
    append_order(out, &resting.order);
    append_u64(out, resting.account_id);
    append_u64(out, resting.created_at);
    append_u64(out, resting.expires_at_block);
    append_i64(out, resting.reserved_balance);
    append_position_reservations(out, &resting.reserved_positions);
}

fn append_account_reservation(out: &mut Vec<u8>, reservation: &AccountReservationSnapshot) {
    append_u64(out, reservation.account_id);
    append_i64(out, reservation.reserved_balance);
    append_position_reservations(out, &reservation.reserved_positions);
}

fn append_position_reservations(out: &mut Vec<u8>, positions: &[(MarketId, u8, i64)]) {
    let mut positions = positions.to_vec();
    positions.sort_by_key(|(market, outcome, qty)| (market.0, *outcome, *qty));
    append_u64(out, positions.len() as u64);
    for (market, outcome, qty) in positions {
        append_market_id(out, market);
        out.push(outcome);
        append_i64(out, qty);
    }
}

fn append_market_id(out: &mut Vec<u8>, market: MarketId) {
    append_u32(out, market.0);
}

fn append_string(out: &mut Vec<u8>, value: &str) {
    append_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn append_option_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            append_string(out, value);
        }
        None => out.push(0),
    }
}

fn append_option<T>(out: &mut Vec<u8>, value: Option<&T>, append: fn(&mut Vec<u8>, &T)) {
    match value {
        Some(value) => {
            out.push(1);
            append(out, value);
        }
        None => out.push(0),
    }
}

fn append_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StateSidecarSnapshot, WitnessBlockHeader};

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
            resolved_markets: vec![],
        };

        let bytes = canonical_witness_bytes(&witness);
        assert_eq!(bytes[0], WITNESS_FORMAT_VERSION);
        assert_eq!(bytes.len(), 341);
    }
}
