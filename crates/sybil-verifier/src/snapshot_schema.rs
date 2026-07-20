//! Shared snapshot byte visitor for state-root and witness schemas.

use matching_engine::MarketId;

use crate::canonical::append_order;
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, BridgeStateSnapshot, MarketGroupSnapshot,
    MarketSnapshot, MarketStatusSnapshot, OracleSourceSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot,
};

#[derive(Clone, Copy)]
enum PositionEncoding {
    StateRoot,
    Witness,
}

struct SnapshotByteVisitor<'a> {
    out: &'a mut Vec<u8>,
}

impl<'a> SnapshotByteVisitor<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out }
    }

    fn append_account_fields(&mut self, account: &AccountSnapshot, positions: PositionEncoding) {
        append_u64(self.out, account.id);
        append_i64(self.out, account.balance);
        append_i64(self.out, account.total_deposited);
        self.append_positions(&account.positions, positions);
        self.out.extend_from_slice(&account.events_digest);
        self.out.extend_from_slice(&account.keys_digest);
        append_u64(self.out, account.last_trading_nonce);
    }

    fn append_positions(&mut self, positions: &[(MarketId, u8, i64)], encoding: PositionEncoding) {
        let mut positions = positions.to_vec();
        match encoding {
            PositionEncoding::StateRoot => {
                positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
                positions.retain(|(_, _, qty)| *qty != 0);
            }
            PositionEncoding::Witness => {
                positions.sort_by_key(|(market, outcome, qty)| (market.0, *outcome, *qty));
            }
        }

        append_u64(self.out, positions.len() as u64);
        for (market, outcome, qty) in positions {
            append_market_id(self.out, market);
            self.out.push(outcome);
            append_i64(self.out, qty);
        }
    }

    fn append_market_snapshot_fields(&mut self, market: &MarketSnapshot) {
        append_market_id(self.out, market.market_id);
        append_string(self.out, &market.name);
        self.out.push(market.num_outcomes);
        self.append_market_status(&market.status);
        self.out.extend_from_slice(&market.metadata_digest);
        append_string(self.out, &market.resolution_template);
        append_u64(self.out, market.last_clearing_prices.len() as u64);
        for price in &market.last_clearing_prices {
            append_u64(self.out, price.0);
        }
    }

    fn append_market_group_fields(&mut self, group: &MarketGroupSnapshot) {
        append_u64(self.out, group.group_id);
        append_string(self.out, &group.name);
        append_optional_string(self.out, group.creation_key.as_deref());

        let mut markets = group.markets.clone();
        markets.sort_by_key(|market| market.0);
        append_u64(self.out, markets.len() as u64);
        for market in markets {
            append_market_id(self.out, market);
        }
    }

    fn append_withdrawal_fields(&mut self, withdrawal: &WithdrawalSnapshot) {
        append_u64(self.out, withdrawal.withdrawal_id);
        append_u64(self.out, withdrawal.account_id);
        self.out.extend_from_slice(&withdrawal.recipient);
        self.out.extend_from_slice(&withdrawal.token);
        append_u64(self.out, withdrawal.amount_token_units);
        append_u64(self.out, withdrawal.amount_nanos);
        append_u64(self.out, withdrawal.expiry_height);
        self.out.extend_from_slice(&withdrawal.nullifier);
    }

    fn append_market_status(&mut self, status: &MarketStatusSnapshot) {
        match status {
            MarketStatusSnapshot::Active => self.out.push(0),
            MarketStatusSnapshot::Resolved { record } => {
                self.out.push(1);
                self.append_resolution_record(record);
            }
        }
    }

    fn append_resolution_record(&mut self, record: &ResolutionRecordSnapshot) {
        append_u64(self.out, record.payout_nanos.0);
        self.append_oracle_source(&record.resolved_by);
        append_u64(self.out, record.resolved_at_ms);
    }

    fn append_oracle_source(&mut self, source: &OracleSourceSnapshot) {
        match source {
            OracleSourceSnapshot::Admin => self.out.push(0),
            OracleSourceSnapshot::DataFeed(feed_id) => {
                self.out.push(1);
                append_u64(self.out, *feed_id);
            }
        }
    }
}

fn append_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            append_string(out, value);
        }
        None => out.push(0),
    }
}

pub(crate) fn append_state_account_leaf_value(out: &mut Vec<u8>, account: &AccountSnapshot) {
    out.extend_from_slice(b"sybil/state/acct");
    SnapshotByteVisitor::new(out).append_account_fields(account, PositionEncoding::StateRoot);
}

pub(crate) fn append_witness_account(out: &mut Vec<u8>, account: &AccountSnapshot) {
    out.extend_from_slice(b"sybil/witness/account");
    SnapshotByteVisitor::new(out).append_account_fields(account, PositionEncoding::Witness);
}

pub(crate) fn append_state_market_leaf_value(out: &mut Vec<u8>, market: &MarketSnapshot) {
    out.extend_from_slice(b"sybil/state/market");
    SnapshotByteVisitor::new(out).append_market_snapshot_fields(market);
}

pub(crate) fn append_witness_market_snapshot(out: &mut Vec<u8>, market: &MarketSnapshot) {
    SnapshotByteVisitor::new(out).append_market_snapshot_fields(market);
}

pub(crate) fn append_state_market_group_leaf_value(out: &mut Vec<u8>, group: &MarketGroupSnapshot) {
    out.extend_from_slice(b"sybil/state/market-group");
    SnapshotByteVisitor::new(out).append_market_group_fields(group);
}

pub(crate) fn append_witness_market_group_snapshot(out: &mut Vec<u8>, group: &MarketGroupSnapshot) {
    SnapshotByteVisitor::new(out).append_market_group_fields(group);
}

pub(crate) fn append_state_withdrawal_leaf_value(
    out: &mut Vec<u8>,
    withdrawal: &WithdrawalSnapshot,
) {
    out.extend_from_slice(b"sybil/state/withdrawal");
    SnapshotByteVisitor::new(out).append_withdrawal_fields(withdrawal);
}

pub(crate) fn append_witness_withdrawal(out: &mut Vec<u8>, withdrawal: &WithdrawalSnapshot) {
    SnapshotByteVisitor::new(out).append_withdrawal_fields(withdrawal);
}

pub(crate) fn append_state_resting_order_leaf_value(
    out: &mut Vec<u8>,
    resting: &RestingOrderSnapshot,
) {
    out.extend_from_slice(b"sybil/state/order");
    append_u64(out, resting.account_id);
    append_u64(out, resting.created_at);
    append_u64(out, resting.expires_at_block);
    append_i64(out, resting.reserved_balance);
    SnapshotByteVisitor::new(out)
        .append_positions(&resting.reserved_positions, PositionEncoding::StateRoot);
    append_order(out, &resting.order);
}

pub(crate) fn append_witness_resting_order(out: &mut Vec<u8>, resting: &RestingOrderSnapshot) {
    append_order(out, &resting.order);
    append_u64(out, resting.account_id);
    append_u64(out, resting.created_at);
    append_u64(out, resting.expires_at_block);
    append_i64(out, resting.reserved_balance);
    SnapshotByteVisitor::new(out)
        .append_positions(&resting.reserved_positions, PositionEncoding::Witness);
}

pub(crate) fn append_state_account_reservation_leaf_value(
    out: &mut Vec<u8>,
    reservation: &AccountReservationSnapshot,
) {
    out.extend_from_slice(b"sybil/state/acct-resv");
    append_u64(out, reservation.account_id);
    append_i64(out, reservation.reserved_balance);
    SnapshotByteVisitor::new(out)
        .append_positions(&reservation.reserved_positions, PositionEncoding::StateRoot);
}

pub(crate) fn append_witness_account_reservation(
    out: &mut Vec<u8>,
    reservation: &AccountReservationSnapshot,
) {
    append_u64(out, reservation.account_id);
    append_i64(out, reservation.reserved_balance);
    SnapshotByteVisitor::new(out)
        .append_positions(&reservation.reserved_positions, PositionEncoding::Witness);
}

pub(crate) fn append_witness_state_sidecar(out: &mut Vec<u8>, sidecar: &StateSidecarSnapshot) {
    append_witness_state_sidecar_with_domain(out, b"sybil/witness/state-sidecar", sidecar);
}

pub(crate) fn append_witness_pre_state_sidecar(out: &mut Vec<u8>, sidecar: &StateSidecarSnapshot) {
    append_witness_state_sidecar_with_domain(out, b"sybil/witness/pre-state-sidecar", sidecar);
}

fn append_witness_state_sidecar_with_domain(
    out: &mut Vec<u8>,
    domain: &[u8],
    sidecar: &StateSidecarSnapshot,
) {
    out.extend_from_slice(domain);
    append_witness_bridge(out, &sidecar.bridge);

    let mut markets: Vec<_> = sidecar.markets.iter().collect();
    markets.sort_by_key(|market| market.market_id.0);
    append_u64(out, markets.len() as u64);
    for market in markets {
        append_witness_market_snapshot(out, market);
    }

    let mut groups: Vec<_> = sidecar.market_groups.iter().collect();
    groups.sort_by_key(|group| group.group_id);
    append_u64(out, groups.len() as u64);
    for group in groups {
        append_witness_market_group_snapshot(out, group);
    }

    let mut resting_orders: Vec<_> = sidecar.resting_orders.iter().collect();
    resting_orders.sort_by_key(|resting| resting.order.id);
    append_u64(out, resting_orders.len() as u64);
    for resting in resting_orders {
        append_witness_resting_order(out, resting);
    }

    let mut reservations: Vec<_> = sidecar.account_reservations.iter().collect();
    reservations.sort_by_key(|reservation| reservation.account_id);
    append_u64(out, reservations.len() as u64);
    for reservation in reservations {
        append_witness_account_reservation(out, reservation);
    }
}

fn append_witness_bridge(out: &mut Vec<u8>, bridge: &BridgeStateSnapshot) {
    append_u64(out, bridge.deposit_cursor);
    out.extend_from_slice(&bridge.deposit_root);
    append_u64(out, bridge.observed_l1_height);
    append_u64(out, bridge.next_withdrawal_id);

    let mut withdrawals: Vec<_> = bridge.withdrawals.iter().collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    append_u64(out, withdrawals.len() as u64);
    for withdrawal in withdrawals {
        append_witness_withdrawal(out, withdrawal);
    }

    let mut quarantine = bridge.quarantine.clone();
    quarantine.sort_by_key(|entry| entry.sybil_account_key);
    append_u64(out, quarantine.len() as u64);
    for entry in quarantine {
        out.extend_from_slice(&entry.sybil_account_key);
        append_i64(out, entry.amount);
    }
}

pub(crate) fn append_market_id(out: &mut Vec<u8>, market: MarketId) {
    append_u32(out, market.0);
}

pub(crate) fn append_string(out: &mut Vec<u8>, value: &str) {
    append_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

pub(crate) fn append_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn append_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn append_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}
