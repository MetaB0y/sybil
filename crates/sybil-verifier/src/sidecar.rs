//! Layer 5: derivable sidecar verification.
//!
//! The current witness carries only the committed post-sidecar, not a
//! pre-sidecar. This layer therefore checks sidecar facts derivable from the
//! witness itself and records the remaining pre->post transition as a witness
//! schema gap rather than trusting sequencer state.

use std::collections::{BTreeMap, BTreeSet};

use matching_engine::{MarketId, NANOS_PER_DOLLAR};
use sybil_l1_protocol::DepositLeaf;

use crate::types::{
    AccountReservationSnapshot, BlockWitness, L1DepositWitness, MarketStatusSnapshot,
    RestingOrderSnapshot, SystemEventWitness, WithdrawalSnapshot,
};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

pub const SIDECAR_WITNESS_GAPS: &[&str] = &[
    "resting orders deleted together with their aggregate reservation cannot be detected without pre_state_sidecar",
    "pre-existing withdrawals deleted without a WithdrawalCreated event cannot be detected without pre_state_sidecar",
    "market metadata/group edits and status flips for markets absent from resolved_markets/events cannot be detected without pre_state_sidecar",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReservationTotals {
    reserved_balance: i64,
    reserved_positions: BTreeMap<(MarketId, u8), i64>,
}

impl ReservationTotals {
    fn add_balance(&mut self, delta: i64) -> Result<(), String> {
        self.reserved_balance = self
            .reserved_balance
            .checked_add(delta)
            .ok_or_else(|| "reservation balance overflowed".to_string())?;
        Ok(())
    }

    fn add_position(&mut self, market: MarketId, outcome: u8, delta: i64) -> Result<(), String> {
        if delta == 0 {
            return Ok(());
        }
        let entry = self
            .reserved_positions
            .entry((market, outcome))
            .or_insert(0);
        *entry = entry.checked_add(delta).ok_or_else(|| {
            format!(
                "reservation position overflowed for {:?}/{}",
                market, outcome
            )
        })?;
        if *entry == 0 {
            self.reserved_positions.remove(&(market, outcome));
        }
        Ok(())
    }

    fn is_zero(&self) -> bool {
        self.reserved_balance == 0 && self.reserved_positions.is_empty()
    }
}

/// Verify sidecar facts derivable from the current witness.
pub fn verify_sidecar(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();

    verify_resting_order_reservations(witness, &mut violations);
    verify_withdrawal_events(witness, &mut violations);
    verify_deposit_prefix(witness, &mut violations);
    verify_market_statuses(witness, &mut violations);

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats: VerificationStats::default(),
    }
}

fn verify_resting_order_reservations(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let expected = match reservations_from_resting_orders(&witness.state_sidecar.resting_orders) {
        Ok(reservations) => reservations,
        Err(details) => {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details,
            });
            return;
        }
    };
    let claimed =
        match reservations_from_account_snapshots(&witness.state_sidecar.account_reservations) {
            Ok(reservations) => reservations,
            Err(violation) => {
                violations.push(violation);
                return;
            }
        };

    if expected != claimed {
        violations.push(Violation {
            kind: ViolationKind::SidecarReservationMismatch,
            details: format!(
                "account reservation sidecar does not equal resting-order reservation rollup: expected {:?}, claimed {:?}",
                expected, claimed
            ),
        });
    }
}

fn reservations_from_resting_orders(
    resting_orders: &[RestingOrderSnapshot],
) -> Result<BTreeMap<u64, ReservationTotals>, String> {
    let mut by_account: BTreeMap<u64, ReservationTotals> = BTreeMap::new();
    let mut seen_orders = BTreeSet::new();
    for resting in resting_orders {
        if !seen_orders.insert(resting.order.id) {
            return Err(format!(
                "duplicate resting order id {} in sidecar",
                resting.order.id
            ));
        }
        let totals = by_account.entry(resting.account_id).or_default();
        totals.add_balance(resting.reserved_balance)?;
        for &(market, outcome, qty) in &resting.reserved_positions {
            totals.add_position(market, outcome, qty)?;
        }
    }
    by_account.retain(|_, totals| !totals.is_zero());
    Ok(by_account)
}

fn reservations_from_account_snapshots(
    reservations: &[AccountReservationSnapshot],
) -> Result<BTreeMap<u64, ReservationTotals>, Violation> {
    let mut by_account: BTreeMap<u64, ReservationTotals> = BTreeMap::new();
    for reservation in reservations {
        if by_account.contains_key(&reservation.account_id) {
            return Err(Violation {
                kind: ViolationKind::SidecarReservationMismatch,
                details: format!(
                    "duplicate account reservation leaf for account {}",
                    reservation.account_id
                ),
            });
        }
        let mut totals = ReservationTotals {
            reserved_balance: reservation.reserved_balance,
            reserved_positions: BTreeMap::new(),
        };
        for &(market, outcome, qty) in &reservation.reserved_positions {
            totals
                .add_position(market, outcome, qty)
                .map_err(|details| Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details,
                })?;
        }
        if !totals.is_zero() {
            by_account.insert(reservation.account_id, totals);
        }
    }
    Ok(by_account)
}

fn verify_withdrawal_events(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let mut withdrawals_by_id: BTreeMap<u64, &WithdrawalSnapshot> = BTreeMap::new();
    for withdrawal in &witness.state_sidecar.bridge.withdrawals {
        if withdrawals_by_id
            .insert(withdrawal.withdrawal_id, withdrawal)
            .is_some()
        {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("duplicate withdrawal id {}", withdrawal.withdrawal_id),
            });
        }
    }

    if let Some(max_withdrawal_id) = withdrawals_by_id.keys().next_back().copied() {
        if witness.state_sidecar.bridge.next_withdrawal_id <= max_withdrawal_id {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "next_withdrawal_id {} must be greater than committed withdrawal id {}",
                    witness.state_sidecar.bridge.next_withdrawal_id, max_withdrawal_id
                ),
            });
        }
    }

    for event in &witness.system_events {
        let SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            withdrawal_id,
            recipient,
            token,
            amount_token_units,
            expiry_height,
            nullifier,
        } = event
        else {
            continue;
        };

        let Ok(amount_nanos) = u64::try_from(*amount) else {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "withdrawal {} has negative amount {}",
                    withdrawal_id, amount
                ),
            });
            continue;
        };

        let Some(withdrawal) = withdrawals_by_id.get(withdrawal_id).copied() else {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "WithdrawalCreated event {} missing from committed withdrawal leaves",
                    withdrawal_id
                ),
            });
            continue;
        };

        if withdrawal.account_id != *account_id
            || withdrawal.recipient != *recipient
            || withdrawal.token != *token
            || withdrawal.amount_token_units != *amount_token_units
            || withdrawal.amount_nanos != amount_nanos
            || withdrawal.expiry_height != *expiry_height
            || withdrawal.nullifier != *nullifier
        {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "WithdrawalCreated event {} does not match committed withdrawal leaf",
                    withdrawal_id
                ),
            });
        }
    }
}

fn verify_deposit_prefix(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let cursor = witness.state_sidecar.bridge.deposit_cursor;
    if cursor != witness.l1_deposits.len() as u64 {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: format!(
                "deposit_cursor {} != l1_deposits.len() {}",
                cursor,
                witness.l1_deposits.len()
            ),
        });
    }

    for (index, deposit) in witness.l1_deposits.iter().enumerate() {
        let expected_id = index as u64 + 1;
        if deposit.deposit_id != expected_id {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositCursorMismatch,
                details: format!(
                    "l1_deposits[{}].deposit_id {} != expected {}",
                    index, deposit.deposit_id, expected_id
                ),
            });
        }
    }

    let leaves = witness
        .l1_deposits
        .iter()
        .map(deposit_leaf_from_witness)
        .collect::<Vec<_>>();
    let prefix_roots = sybil_l1_protocol::deposit_prefix_roots(&leaves);
    for (deposit, expected_root) in witness.l1_deposits.iter().zip(prefix_roots.iter()) {
        if deposit.deposit_root != *expected_root {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositRootMismatch,
                details: format!(
                    "deposit {} root does not match recomputed prefix root",
                    deposit.deposit_id
                ),
            });
        }
    }
    let expected_root = prefix_roots
        .last()
        .copied()
        .unwrap_or_else(sybil_l1_protocol::empty_deposit_root);
    if witness.state_sidecar.bridge.deposit_root != expected_root {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositRootMismatch,
            details: "bridge deposit_root does not match recomputed L1 deposit prefix".to_string(),
        });
    }
}

fn deposit_leaf_from_witness(deposit: &L1DepositWitness) -> DepositLeaf {
    DepositLeaf {
        chain_id: deposit.chain_id,
        vault_address: deposit.vault_address,
        deposit_id: deposit.deposit_id,
        token_address: deposit.token_address,
        sender: deposit.sender,
        sybil_account_key: deposit.sybil_account_key,
        amount_token_units: deposit.amount_token_units,
    }
}

fn verify_market_statuses(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let markets_by_id = witness
        .state_sidecar
        .markets
        .iter()
        .map(|market| (market.market_id, market))
        .collect::<BTreeMap<_, _>>();

    for event in &witness.system_events {
        let SystemEventWitness::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts: _,
        } = event
        else {
            continue;
        };

        if payout_nanos.0 > NANOS_PER_DOLLAR {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Market {:?}: payout_nanos {} exceeds NANOS_PER_DOLLAR {}",
                    market_id, payout_nanos, NANOS_PER_DOLLAR
                ),
            });
            continue;
        }

        match markets_by_id.get(market_id).map(|market| &market.status) {
            Some(MarketStatusSnapshot::Resolved { record })
                if record.market_id == *market_id && record.payout_nanos == *payout_nanos => {}
            Some(status) => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "MarketResolved event for {:?} not reflected in committed status {:?}",
                    market_id, status
                ),
            }),
            None => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "MarketResolved event for {:?} missing from committed market sidecar",
                    market_id
                ),
            }),
        }
    }

    for market_id in &witness.resolved_markets {
        match markets_by_id.get(market_id).map(|market| &market.status) {
            Some(MarketStatusSnapshot::Resolved { .. } | MarketStatusSnapshot::Voided) => {}
            Some(status) => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "resolved_markets contains {:?} but committed status is {:?}",
                    market_id, status
                ),
            }),
            None => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "resolved_markets contains {:?} but market is missing from sidecar",
                    market_id
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{Nanos, Order, Qty};

    use crate::types::{
        BridgeStateSnapshot, MarketSnapshot, OracleSourceSnapshot, ResolutionRecordSnapshot,
        StateSidecarSnapshot, WitnessBlockHeader,
    };

    fn empty_header() -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            events_root: [0u8; 32],
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1_000,
        }
    }

    fn empty_sidecar() -> StateSidecarSnapshot {
        StateSidecarSnapshot {
            bridge: BridgeStateSnapshot {
                deposit_root: sybil_l1_protocol::empty_deposit_root(),
                ..BridgeStateSnapshot::default()
            },
            ..StateSidecarSnapshot::default()
        }
    }

    fn witness_with_sidecar(sidecar: StateSidecarSnapshot) -> BlockWitness {
        BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            l1_deposits: vec![],
            fills: vec![],
            clearing_prices: BTreeMap::new().into_iter().collect(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar: sidecar,
            resolved_markets: vec![],
        }
    }

    fn resting_order() -> RestingOrderSnapshot {
        let mut order = Order::new(42);
        order.markets[0] = MarketId::new(3);
        order.num_markets = 1;
        order.num_states = 2;
        order.payoffs[0] = 1;
        order.limit_price = Nanos(500_000_000);
        order.max_fill = Qty(10);
        RestingOrderSnapshot {
            order,
            account_id: 7,
            created_at: 1,
            expires_at_block: 10,
            reserved_balance: 123,
            reserved_positions: vec![(MarketId::new(3), 0, 4)],
        }
    }

    fn reservation() -> AccountReservationSnapshot {
        AccountReservationSnapshot {
            account_id: 7,
            reserved_balance: 123,
            reserved_positions: vec![(MarketId::new(3), 0, 4)],
        }
    }

    fn withdrawal_leaf() -> WithdrawalSnapshot {
        WithdrawalSnapshot {
            withdrawal_id: 9,
            account_id: 7,
            recipient: [1u8; 20],
            token: [2u8; 20],
            amount_token_units: 50,
            amount_nanos: 50_000,
            expiry_height: 20,
            nullifier: [3u8; 32],
        }
    }

    fn withdrawal_event() -> SystemEventWitness {
        let withdrawal = withdrawal_leaf();
        SystemEventWitness::WithdrawalCreated {
            account_id: withdrawal.account_id,
            amount: withdrawal.amount_nanos as i64,
            withdrawal_id: withdrawal.withdrawal_id,
            recipient: withdrawal.recipient,
            token: withdrawal.token,
            amount_token_units: withdrawal.amount_token_units,
            expiry_height: withdrawal.expiry_height,
            nullifier: withdrawal.nullifier,
        }
    }

    fn l1_deposit_prefix(count: u64) -> Vec<L1DepositWitness> {
        let mut deposits = (1..=count)
            .map(|deposit_id| L1DepositWitness {
                deposit_id,
                chain_id: 31_337,
                vault_address: [0x11; 20],
                token_address: [0x22; 20],
                sender: [deposit_id as u8; 20],
                sybil_account_key: [0x33; 32],
                amount_token_units: 1_000 + deposit_id,
                deposit_root: [0u8; 32],
            })
            .collect::<Vec<_>>();
        let leaves = deposits
            .iter()
            .map(deposit_leaf_from_witness)
            .collect::<Vec<_>>();
        let roots = sybil_l1_protocol::deposit_prefix_roots(&leaves);
        for (deposit, root) in deposits.iter_mut().zip(roots) {
            deposit.deposit_root = root;
        }
        deposits
    }

    fn resolved_market() -> MarketSnapshot {
        MarketSnapshot {
            market_id: MarketId::new(5),
            name: "M5".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Resolved {
                record: ResolutionRecordSnapshot {
                    market_id: MarketId::new(5),
                    payout_nanos: Nanos(NANOS_PER_DOLLAR),
                    resolved_by: OracleSourceSnapshot::Admin,
                    resolved_at_ms: 1_000,
                    proposal: None,
                    challenge: None,
                },
            },
            metadata_digest: [5u8; 32],
            resolution_template: "admin".to_string(),
        }
    }

    #[test]
    fn sidecar_drop_resting_order_fails() {
        let mut sidecar = empty_sidecar();
        sidecar.resting_orders = vec![resting_order()];
        sidecar.account_reservations = vec![reservation()];
        assert!(verify_sidecar(&witness_with_sidecar(sidecar.clone())).valid);

        sidecar.resting_orders.clear();
        let result = verify_sidecar(&witness_with_sidecar(sidecar));
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SidecarReservationMismatch));
    }

    #[test]
    fn sidecar_zero_reservation_fails() {
        let mut sidecar = empty_sidecar();
        sidecar.resting_orders = vec![resting_order()];
        sidecar.account_reservations = vec![AccountReservationSnapshot {
            account_id: 7,
            reserved_balance: 0,
            reserved_positions: vec![],
        }];
        let result = verify_sidecar(&witness_with_sidecar(sidecar));
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SidecarReservationMismatch));
    }

    #[test]
    fn sidecar_delete_withdrawal_leaf_fails() {
        let mut sidecar = empty_sidecar();
        sidecar.bridge.next_withdrawal_id = 10;
        sidecar.bridge.withdrawals = vec![withdrawal_leaf()];
        let mut witness = witness_with_sidecar(sidecar.clone());
        witness.system_events = vec![withdrawal_event()];
        assert!(verify_sidecar(&witness).valid);

        witness.state_sidecar.bridge.withdrawals.clear();
        let result = verify_sidecar(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SidecarWithdrawalMismatch));
    }

    #[test]
    fn sidecar_corrupt_deposit_cursor_fails() {
        let deposits = l1_deposit_prefix(2);
        let mut sidecar = empty_sidecar();
        sidecar.bridge.deposit_cursor = 2;
        sidecar.bridge.deposit_root = deposits.last().expect("deposit prefix").deposit_root;
        let mut witness = witness_with_sidecar(sidecar);
        witness.l1_deposits = deposits;
        assert!(verify_sidecar(&witness).valid);

        witness.state_sidecar.bridge.deposit_cursor = 1;
        let result = verify_sidecar(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SidecarDepositCursorMismatch));
    }

    #[test]
    fn sidecar_flip_market_status_fails() {
        let mut sidecar = empty_sidecar();
        sidecar.markets = vec![resolved_market()];
        let mut witness = witness_with_sidecar(sidecar);
        witness.system_events = vec![SystemEventWitness::MarketResolved {
            market_id: MarketId::new(5),
            payout_nanos: Nanos(NANOS_PER_DOLLAR),
            affected_accounts: vec![],
        }];
        witness.resolved_markets = vec![MarketId::new(5)];
        assert!(verify_sidecar(&witness).valid);

        witness.state_sidecar.markets[0].status = MarketStatusSnapshot::Active;
        let result = verify_sidecar(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch));
    }
}
