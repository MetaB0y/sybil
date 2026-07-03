//! Canonical typed-state leaf schema committed by `BlockHeader.state_root`.

use sha2::{Digest as _, Sha256};

use crate::snapshot_schema::{
    append_state_account_leaf_value, append_state_account_reservation_leaf_value,
    append_state_market_group_leaf_value, append_state_market_leaf_value,
    append_state_resting_order_leaf_value, append_state_withdrawal_leaf_value,
};
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, MarketGroupSnapshot, MarketSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot,
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
    append_state_account_leaf_value(&mut value, account);
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
    append_state_market_leaf_value(&mut value, market);
    value
}

fn market_group_leaf_value(group: &MarketGroupSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    append_state_market_group_leaf_value(&mut value, group);
    value
}

fn withdrawal_leaf_value(withdrawal: &WithdrawalSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    append_state_withdrawal_leaf_value(&mut value, withdrawal);
    value
}

fn resting_order_leaf_value(resting: &RestingOrderSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    append_state_resting_order_leaf_value(&mut value, resting);
    value
}

fn account_reservation_leaf_value(reservation: &AccountReservationSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    append_state_account_reservation_leaf_value(&mut value, reservation);
    value
}
