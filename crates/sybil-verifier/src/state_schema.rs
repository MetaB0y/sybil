//! Canonical typed-state leaf schema committed by `BlockHeader.state_root`.

use sha2::{Digest as _, Sha256};

use crate::canonical::append_order;
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, ChallengeSnapshot, MarketGroupSnapshot,
    MarketSnapshot, MarketStatusSnapshot, OracleSourceSnapshot, ResolutionProposalSnapshot,
    ResolutionRecordSnapshot, RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot,
};

/// Return the sorted typed key/value leaves committed by `state_root`.
pub fn state_root_leaves(
    accounts: &[AccountSnapshot],
    sidecar: &StateSidecarSnapshot,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut leaves = Vec::new();

    let mut sorted_accounts: Vec<&AccountSnapshot> = accounts.iter().collect();
    sorted_accounts.sort_by_key(|account| account.id);
    for account in sorted_accounts {
        leaves.push((account_leaf_key(account.id), account_leaf_value(account)));
    }

    leaves.push((
        b"sys/deposit_cursor".to_vec(),
        sys_u64_leaf_value(b"deposit_cursor", sidecar.bridge.deposit_cursor),
    ));
    leaves.push((
        b"sys/deposit_root".to_vec(),
        sys_bytes32_leaf_value(b"deposit_root", &sidecar.bridge.deposit_root),
    ));
    leaves.push((
        b"sys/next_withdrawal_id".to_vec(),
        sys_u64_leaf_value(b"next_withdrawal_id", sidecar.bridge.next_withdrawal_id),
    ));

    let mut markets: Vec<&MarketSnapshot> = sidecar.markets.iter().collect();
    markets.sort_by_key(|market| market.market_id.0);
    for market in markets {
        leaves.push((market_leaf_key(market.market_id), market_leaf_value(market)));
    }

    let mut market_groups: Vec<&MarketGroupSnapshot> = sidecar.market_groups.iter().collect();
    market_groups.sort_by_key(|group| group.group_id);
    for group in market_groups {
        leaves.push((
            market_group_leaf_key(group.group_id),
            market_group_leaf_value(group),
        ));
    }

    let mut withdrawals: Vec<&WithdrawalSnapshot> = sidecar.bridge.withdrawals.iter().collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    for withdrawal in withdrawals {
        leaves.push((
            withdrawal_leaf_key(withdrawal.withdrawal_id),
            withdrawal_leaf_value(withdrawal),
        ));
    }

    let mut resting_orders: Vec<&RestingOrderSnapshot> = sidecar.resting_orders.iter().collect();
    resting_orders.sort_by_key(|resting| resting.order.id);
    for resting in resting_orders {
        leaves.push((
            resting_order_leaf_key(resting.order.id),
            resting_order_leaf_value(resting),
        ));
    }

    let mut reservations: Vec<&AccountReservationSnapshot> =
        sidecar.account_reservations.iter().collect();
    reservations.sort_by_key(|reservation| reservation.account_id);
    for reservation in reservations {
        leaves.push((
            account_reservation_leaf_key(reservation.account_id),
            account_reservation_leaf_value(reservation),
        ));
    }

    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));
    leaves
}

pub fn account_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(13);
    key.extend_from_slice(b"acct/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

pub fn market_leaf_key(market_id: matching_engine::MarketId) -> Vec<u8> {
    let mut key = Vec::with_capacity(11);
    key.extend_from_slice(b"market/");
    key.extend_from_slice(&market_id.0.to_be_bytes());
    key
}

pub fn market_group_leaf_key(group_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(21);
    key.extend_from_slice(b"market_group/");
    key.extend_from_slice(&group_id.to_be_bytes());
    key
}

pub fn withdrawal_leaf_key(withdrawal_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(19);
    key.extend_from_slice(b"withdrawal/");
    key.extend_from_slice(&withdrawal_id.to_be_bytes());
    key
}

pub fn resting_order_leaf_key(order_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(14);
    key.extend_from_slice(b"order/");
    key.extend_from_slice(&order_id.to_be_bytes());
    key
}

pub fn account_reservation_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(18);
    key.extend_from_slice(b"acct_resv/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

fn account_leaf_value(account: &AccountSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/acct");
    value.extend_from_slice(&account.id.to_le_bytes());
    value.extend_from_slice(&account.balance.to_le_bytes());
    value.extend_from_slice(&account.total_deposited.to_le_bytes());

    let mut positions = account.positions.clone();
    positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    positions.retain(|(_, _, qty)| *qty != 0);
    value.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    for (market, outcome, qty) in positions {
        value.extend_from_slice(&market.0.to_le_bytes());
        value.push(outcome);
        value.extend_from_slice(&qty.to_le_bytes());
    }

    value.extend_from_slice(&account.events_digest);
    value
}

fn sys_u64_leaf_value(name: &[u8], raw: u64) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 8);
    value.extend_from_slice(b"sybil/state/sys");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(&raw.to_le_bytes());
    value
}

fn sys_bytes32_leaf_value(name: &[u8], raw: &[u8; 32]) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 32);
    value.extend_from_slice(b"sybil/state/sys");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(raw);
    value
}

/// Canonical digest for sequencer-layer market metadata.
///
/// The market leaf stores this digest instead of large text fields. A caller
/// proving metadata can reveal the raw metadata bytes and recompute this
/// digest against the committed market leaf.
pub fn market_metadata_digest(payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sybil/state/market-meta");
    hasher.update((payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().into()
}

fn market_leaf_value(market: &MarketSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/market");
    value.extend_from_slice(&market.market_id.0.to_le_bytes());
    append_string(&mut value, &market.name);
    value.push(market.num_outcomes);
    append_market_status(&mut value, &market.status);
    value.extend_from_slice(&market.metadata_digest);
    append_string(&mut value, &market.resolution_template);
    value
}

fn market_group_leaf_value(group: &MarketGroupSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/market-group");
    value.extend_from_slice(&group.group_id.to_le_bytes());
    append_string(&mut value, &group.name);

    let mut markets = group.markets.clone();
    markets.sort_by_key(|market| market.0);
    value.extend_from_slice(&(markets.len() as u64).to_le_bytes());
    for market in markets {
        value.extend_from_slice(&market.0.to_le_bytes());
    }
    value
}

fn withdrawal_leaf_value(withdrawal: &WithdrawalSnapshot) -> Vec<u8> {
    let mut value = Vec::with_capacity(25 + 8 + 8 + 20 + 20 + 8 + 8 + 8 + 32);
    value.extend_from_slice(b"sybil/state/withdrawal");
    value.extend_from_slice(&withdrawal.withdrawal_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.account_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.recipient);
    value.extend_from_slice(&withdrawal.token);
    value.extend_from_slice(&withdrawal.amount_token_units.to_le_bytes());
    value.extend_from_slice(&withdrawal.amount_nanos.to_le_bytes());
    value.extend_from_slice(&withdrawal.expiry_height.to_le_bytes());
    value.extend_from_slice(&withdrawal.nullifier);
    value
}

fn resting_order_leaf_value(resting: &RestingOrderSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/order");
    value.extend_from_slice(&resting.account_id.to_le_bytes());
    value.extend_from_slice(&resting.created_at.to_le_bytes());
    value.extend_from_slice(&resting.expires_at_block.to_le_bytes());
    value.extend_from_slice(&resting.reserved_balance.to_le_bytes());
    append_position_reservations(&mut value, &resting.reserved_positions);
    append_order(&mut value, &resting.order);
    value
}

fn account_reservation_leaf_value(reservation: &AccountReservationSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/acct-resv");
    value.extend_from_slice(&reservation.account_id.to_le_bytes());
    value.extend_from_slice(&reservation.reserved_balance.to_le_bytes());
    append_position_reservations(&mut value, &reservation.reserved_positions);
    value
}

fn append_position_reservations(
    value: &mut Vec<u8>,
    positions: &[(matching_engine::MarketId, u8, i64)],
) {
    let mut positions = positions.to_vec();
    positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    positions.retain(|(_, _, qty)| *qty != 0);
    value.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    for (market, outcome, qty) in positions {
        value.extend_from_slice(&market.0.to_le_bytes());
        value.push(outcome);
        value.extend_from_slice(&qty.to_le_bytes());
    }
}

fn append_string(value: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    value.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    value.extend_from_slice(bytes);
}

fn append_option_string(value: &mut Vec<u8>, text: &Option<String>) {
    match text {
        None => value.push(0),
        Some(text) => {
            value.push(1);
            append_string(value, text);
        }
    }
}

fn append_market_status(value: &mut Vec<u8>, status: &MarketStatusSnapshot) {
    match status {
        MarketStatusSnapshot::Active => value.push(0),
        MarketStatusSnapshot::Proposed {
            proposal,
            challenge_deadline_ms,
        } => {
            value.push(1);
            append_resolution_proposal(value, proposal);
            value.extend_from_slice(&challenge_deadline_ms.to_le_bytes());
        }
        MarketStatusSnapshot::Challenged {
            proposal,
            challenge,
        } => {
            value.push(2);
            append_resolution_proposal(value, proposal);
            append_challenge(value, challenge);
        }
        MarketStatusSnapshot::Resolved { record } => {
            value.push(3);
            append_resolution_record(value, record);
        }
        MarketStatusSnapshot::Voided => value.push(4),
    }
}

fn append_resolution_proposal(value: &mut Vec<u8>, proposal: &ResolutionProposalSnapshot) {
    value.extend_from_slice(&proposal.id.to_le_bytes());
    value.extend_from_slice(&proposal.market_id.0.to_le_bytes());
    value.extend_from_slice(&proposal.payout_nanos.to_le_bytes());
    append_oracle_source(value, &proposal.source);
    value.extend_from_slice(&proposal.proposed_at_ms.to_le_bytes());
    append_option_string(value, &proposal.reason);
}

fn append_challenge(value: &mut Vec<u8>, challenge: &ChallengeSnapshot) {
    value.extend_from_slice(&challenge.id.to_le_bytes());
    value.extend_from_slice(&challenge.challenger.to_le_bytes());
    value.extend_from_slice(&challenge.proposal_id.to_le_bytes());
    value.extend_from_slice(&challenge.bond_amount.to_le_bytes());
    value.extend_from_slice(&challenge.proposed_payout_nanos.to_le_bytes());
    append_string(value, &challenge.reason);
    value.extend_from_slice(&challenge.challenged_at_ms.to_le_bytes());
}

fn append_resolution_record(value: &mut Vec<u8>, record: &ResolutionRecordSnapshot) {
    value.extend_from_slice(&record.market_id.0.to_le_bytes());
    value.extend_from_slice(&record.payout_nanos.to_le_bytes());
    append_oracle_source(value, &record.resolved_by);
    value.extend_from_slice(&record.resolved_at_ms.to_le_bytes());
    append_optional_resolution_proposal(value, &record.proposal);
    append_optional_challenge(value, &record.challenge);
}

fn append_optional_resolution_proposal(
    value: &mut Vec<u8>,
    proposal: &Option<ResolutionProposalSnapshot>,
) {
    match proposal {
        None => value.push(0),
        Some(proposal) => {
            value.push(1);
            append_resolution_proposal(value, proposal);
        }
    }
}

fn append_optional_challenge(value: &mut Vec<u8>, challenge: &Option<ChallengeSnapshot>) {
    match challenge {
        None => value.push(0),
        Some(challenge) => {
            value.push(1);
            append_challenge(value, challenge);
        }
    }
}

fn append_oracle_source(value: &mut Vec<u8>, source: &OracleSourceSnapshot) {
    match source {
        OracleSourceSnapshot::Admin => value.push(0),
        OracleSourceSnapshot::DataFeed(feed_id) => {
            value.push(1);
            value.extend_from_slice(&feed_id.to_le_bytes());
        }
        OracleSourceSnapshot::AutomatedL0 => value.push(2),
    }
}
