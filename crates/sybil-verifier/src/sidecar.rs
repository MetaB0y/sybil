//! Layer 5: non-account sidecar transition verification.

use std::collections::{BTreeMap, BTreeSet};

use matching_engine::{MarketId, NANOS_PER_DOLLAR, Order, Qty, ceil_mul_ratio};
use sybil_l1_protocol::DepositLeaf;

use crate::match_verifier::price_is_in_protocol_range;
use crate::types::{
    AccountReservationSnapshot, BlockWitness, L1DepositWitness, MarketGroupSnapshot,
    MarketSnapshot, MarketStatusSnapshot, RestingOrderSnapshot, SystemEventWitness,
    WithdrawalSnapshot,
};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

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

    verify_resting_order_reservations(
        "pre",
        &witness.pre_state_sidecar.resting_orders,
        &witness.pre_state_sidecar.account_reservations,
        &mut violations,
    );
    verify_resting_order_reservations(
        "post",
        &witness.state_sidecar.resting_orders,
        &witness.state_sidecar.account_reservations,
        &mut violations,
    );
    verify_resting_order_transition(witness, &mut violations);
    verify_withdrawal_transition(witness, &mut violations);
    verify_deposit_accumulator(witness, &mut violations);
    let quarantine = crate::quarantine::verify_quarantine_transition(witness);
    violations.extend(quarantine.violations);
    verify_market_transition(witness, &mut violations);
    verify_market_group_transition(witness, &mut violations);

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats: VerificationStats::default(),
    }
}

fn verify_resting_order_reservations(
    label: &str,
    resting_orders: &[RestingOrderSnapshot],
    reservations: &[AccountReservationSnapshot],
    violations: &mut Vec<Violation>,
) {
    let expected = match reservations_from_resting_orders(resting_orders) {
        Ok(reservations) => reservations,
        Err(details) => {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details,
            });
            return;
        }
    };
    let claimed = match reservations_from_account_snapshots(reservations) {
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
                "{label} account reservation sidecar does not equal resting-order reservation rollup: expected {:?}, claimed {:?}",
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

fn verify_resting_order_transition(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    if witness.previous_header.is_none() {
        return;
    }

    let pre_orders = keyed_resting_orders(&witness.pre_state_sidecar.resting_orders, violations);
    let post_orders = keyed_resting_orders(&witness.state_sidecar.resting_orders, violations);
    let accepted_orders = witness
        .orders
        .iter()
        .map(|order| (order.order.id, order))
        .collect::<BTreeMap<_, _>>();
    let fill_qty_by_order = fill_qty_by_order(witness, violations);
    let cancelled_orders = witness
        .system_events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let resolved_markets = resolved_market_events(witness);

    for (&order_id, pre) in &pre_orders {
        let filled = fill_qty_by_order.get(&order_id).copied().unwrap_or(0);
        match post_orders.get(&order_id) {
            Some(post) => {
                if let Some(expected) = expected_post_resting(pre, filled, witness.header.height) {
                    if !resting_orders_equal(&expected, post) {
                        violations.push(Violation {
                            kind: ViolationKind::SidecarRestingOrderMismatch,
                            details: format!(
                                "resting order {order_id} post leaf does not match the deterministic pre+fill transition"
                            ),
                        });
                    }
                } else {
                    violations.push(Violation {
                        kind: ViolationKind::SidecarRestingOrderMismatch,
                        details: format!(
                            "resting order {order_id} remains in post sidecar despite full fill or expiry"
                        ),
                    });
                }
            }
            None => {
                let explained = cancelled_orders.contains(&order_id)
                    || filled >= pre.order.max_fill.0
                    || witness.header.height >= pre.expires_at_block
                    || pre
                        .order
                        .active_markets()
                        .any(|market| resolved_markets.contains(&market));
                if !explained {
                    violations.push(Violation {
                        kind: ViolationKind::SidecarRestingOrderMismatch,
                        details: format!(
                            "pre-existing resting order {order_id} was deleted without fill, expiry, cancellation, or market resolution"
                        ),
                    });
                }
            }
        }
    }

    for (&order_id, post) in &post_orders {
        if pre_orders.contains_key(&order_id) {
            continue;
        }

        let Some(accepted) = accepted_orders.get(&order_id) else {
            violations.push(Violation {
                kind: ViolationKind::SidecarRestingOrderMismatch,
                details: format!("new post resting order {order_id} has no accepted order event"),
            });
            continue;
        };
        if accepted.is_mm {
            violations.push(Violation {
                kind: ViolationKind::SidecarRestingOrderMismatch,
                details: format!("MM order {order_id} must not appear as a resting order"),
            });
        }
        if post.account_id != accepted.account_id
            || !order_shape_matches_except_qty(&post.order, &accepted.order)
        {
            violations.push(Violation {
                kind: ViolationKind::SidecarRestingOrderMismatch,
                details: format!(
                    "new post resting order {order_id} does not match its accepted order"
                ),
            });
        }
    }
}

fn verify_withdrawal_transition(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let pre = keyed_withdrawals(&witness.pre_state_sidecar.bridge.withdrawals, violations);
    let post = keyed_withdrawals(&witness.state_sidecar.bridge.withdrawals, violations);

    if let Some(max_withdrawal_id) = post.keys().next_back().copied()
        && witness.state_sidecar.bridge.next_withdrawal_id <= max_withdrawal_id
    {
        violations.push(Violation {
            kind: ViolationKind::SidecarWithdrawalMismatch,
            details: format!(
                "next_withdrawal_id {} must be greater than committed withdrawal id {}",
                witness.state_sidecar.bridge.next_withdrawal_id, max_withdrawal_id
            ),
        });
    }

    let mut created = BTreeMap::new();
    let mut terminal = BTreeMap::new();
    let mut observed_heights = BTreeSet::new();
    for event in &witness.system_events {
        match event {
            SystemEventWitness::WithdrawalCreated { withdrawal_id, .. } => {
                if created.insert(*withdrawal_id, event).is_some() {
                    violations.push(Violation {
                        kind: ViolationKind::SidecarWithdrawalMismatch,
                        details: format!("duplicate WithdrawalCreated event {withdrawal_id}"),
                    });
                }
            }
            SystemEventWitness::WithdrawalRefunded { withdrawal_id, .. }
            | SystemEventWitness::WithdrawalFinalized { withdrawal_id, .. } => {
                if terminal.insert(*withdrawal_id, event).is_some() {
                    violations.push(Violation {
                        kind: ViolationKind::SidecarWithdrawalMismatch,
                        details: format!("duplicate terminal event for withdrawal {withdrawal_id}"),
                    });
                }
            }
            SystemEventWitness::L1BlockObserved { height } if !observed_heights.insert(*height) => {
                violations.push(Violation {
                    kind: ViolationKind::SidecarWithdrawalMismatch,
                    details: format!("duplicate L1 height observation {height}"),
                });
            }
            _ => {}
        }
    }

    let mut expected_observed_height = witness.pre_state_sidecar.bridge.observed_l1_height;
    for event in &witness.system_events {
        if let SystemEventWitness::L1BlockObserved { height } = event {
            if *height <= expected_observed_height {
                violations.push(Violation {
                    kind: ViolationKind::SidecarWithdrawalMismatch,
                    details: format!(
                        "L1 height observation {height} did not advance {expected_observed_height}"
                    ),
                });
            } else {
                expected_observed_height = *height;
            }
        }
    }
    if witness.state_sidecar.bridge.observed_l1_height != expected_observed_height {
        violations.push(Violation {
            kind: ViolationKind::SidecarWithdrawalMismatch,
            details: format!(
                "post observed_l1_height {} != event-derived {}",
                witness.state_sidecar.bridge.observed_l1_height, expected_observed_height
            ),
        });
    }

    for (&withdrawal_id, pre_leaf) in &pre {
        match (
            terminal.contains_key(&withdrawal_id),
            post.get(&withdrawal_id),
        ) {
            (true, None) => {}
            (true, Some(_)) => violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("terminal withdrawal {withdrawal_id} was not pruned"),
            }),
            (false, Some(post_leaf)) if *post_leaf == *pre_leaf => {}
            (false, Some(_)) => violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("pre-existing withdrawal {withdrawal_id} was silently edited"),
            }),
            (false, None) => violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "pre-existing withdrawal {withdrawal_id} was deleted without a terminal event"
                ),
            }),
        }
    }

    for (&withdrawal_id, event) in &created {
        let SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            recipient,
            token,
            amount_token_units,
            expiry_height,
            nullifier,
            ..
        } = event
        else {
            unreachable!()
        };
        let Ok(amount_nanos) = u64::try_from(*amount) else {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("withdrawal {withdrawal_id} has negative amount {amount}"),
            });
            continue;
        };
        if terminal.contains_key(&withdrawal_id) {
            if post.contains_key(&withdrawal_id) {
                violations.push(Violation {
                    kind: ViolationKind::SidecarWithdrawalMismatch,
                    details: format!(
                        "same-block terminal withdrawal {withdrawal_id} was not pruned"
                    ),
                });
            }
            continue;
        }
        let Some(withdrawal) = post.get(&withdrawal_id).copied() else {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("WithdrawalCreated event {withdrawal_id} missing from committed withdrawal leaves"),
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
                details: format!("WithdrawalCreated event {withdrawal_id} does not match committed withdrawal leaf"),
            });
        }
    }

    for (&withdrawal_id, event) in &terminal {
        let source = pre
            .get(&withdrawal_id)
            .copied()
            .or_else(|| post.get(&withdrawal_id).copied());
        let created_event = created.get(&withdrawal_id).copied();
        let (leaf_account, leaf_amount, leaf_expiry) = if let Some(leaf) = source {
            (leaf.account_id, leaf.amount_nanos, leaf.expiry_height)
        } else if let Some(SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            expiry_height,
            ..
        }) = created_event
        {
            (*account_id, amount.unsigned_abs(), *expiry_height)
        } else {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("terminal event references unknown withdrawal {withdrawal_id}"),
            });
            continue;
        };

        let (account_id, amount) = match event {
            SystemEventWitness::WithdrawalRefunded {
                account_id,
                amount,
                reason,
                ..
            } => {
                if let crate::types::WithdrawalRefundReasonWitness::L1Expired { observed_l1_height } =
                    reason
                    && (*observed_l1_height <= leaf_expiry
                        || !observed_heights.contains(observed_l1_height))
                {
                    violations.push(Violation {
                            kind: ViolationKind::SidecarWithdrawalMismatch,
                            details: format!(
                                "withdrawal {withdrawal_id} expiry refund at L1 height {observed_l1_height} is not justified by expiry {leaf_expiry}"
                            ),
                        });
                }
                (*account_id, *amount)
            }
            SystemEventWitness::WithdrawalFinalized {
                account_id, amount, ..
            } => (*account_id, *amount),
            _ => unreachable!(),
        };
        if account_id != leaf_account || u64::try_from(amount).ok() != Some(leaf_amount) {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!(
                    "terminal event {withdrawal_id} does not match withdrawal owner/amount"
                ),
            });
        }
    }

    let created_count = created.len() as u64;
    let expected_next = witness
        .pre_state_sidecar
        .bridge
        .next_withdrawal_id
        .saturating_add(created_count);
    if witness.state_sidecar.bridge.next_withdrawal_id != expected_next {
        violations.push(Violation {
            kind: ViolationKind::SidecarWithdrawalMismatch,
            details: format!(
                "next_withdrawal_id {} != pre {} + created {}",
                witness.state_sidecar.bridge.next_withdrawal_id,
                witness.pre_state_sidecar.bridge.next_withdrawal_id,
                created_count
            ),
        });
    }
}

fn verify_deposit_accumulator(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let accumulator = &witness.deposit_accumulator;
    let pre_bridge = &witness.pre_state_sidecar.bridge;
    let post_bridge = &witness.state_sidecar.bridge;

    if accumulator.pre_count != pre_bridge.deposit_cursor {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: format!(
                "deposit accumulator pre_count {} != pre bridge cursor {}",
                accumulator.pre_count, pre_bridge.deposit_cursor
            ),
        });
    }

    match sybil_l1_protocol::deposit_root_from_frontier(
        &accumulator.pre_frontier,
        accumulator.pre_count,
    ) {
        Some(root) if root == pre_bridge.deposit_root => {}
        Some(root) => violations.push(Violation {
            kind: ViolationKind::SidecarDepositRootMismatch,
            details: format!(
                "deposit pre-frontier root {:?} != pre bridge root {:?}",
                root, pre_bridge.deposit_root
            ),
        }),
        None => violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: format!(
                "deposit pre_count {} exceeds tree capacity",
                accumulator.pre_count
            ),
        }),
    }

    let expected_post_count = match accumulator
        .pre_count
        .checked_add(accumulator.new_deposits.len() as u64)
    {
        Some(count) => count,
        None => {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositCursorMismatch,
                details: "deposit post count overflowed".to_string(),
            });
            return;
        }
    };
    if post_bridge.deposit_cursor != expected_post_count {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: format!(
                "post bridge cursor {} != pre_count {} + new deposits {}",
                post_bridge.deposit_cursor,
                accumulator.pre_count,
                accumulator.new_deposits.len()
            ),
        });
    }

    for (index, deposit) in accumulator.new_deposits.iter().enumerate() {
        let expected_id = accumulator.pre_count + index as u64 + 1;
        if deposit.deposit_id != expected_id {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositCursorMismatch,
                details: format!(
                    "new_deposits[{index}].deposit_id {} != expected {expected_id}",
                    deposit.deposit_id
                ),
            });
        }
    }

    let leaves = accumulator
        .new_deposits
        .iter()
        .map(deposit_leaf_from_witness)
        .collect::<Vec<_>>();
    let Some(prefix_roots) = sybil_l1_protocol::deposit_frontier_prefix_roots(
        &accumulator.pre_frontier,
        accumulator.pre_count,
        &leaves,
    ) else {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: "deposit frontier fold exceeds tree capacity".to_string(),
        });
        return;
    };
    for (deposit, expected_root) in accumulator.new_deposits.iter().zip(prefix_roots.iter()) {
        if deposit.deposit_root != *expected_root {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositRootMismatch,
                details: format!(
                    "deposit {} root does not match recomputed frontier root",
                    deposit.deposit_id
                ),
            });
        }
    }
    let expected_root = prefix_roots
        .last()
        .copied()
        .unwrap_or(pre_bridge.deposit_root);
    if post_bridge.deposit_root != expected_root {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositRootMismatch,
            details: "post bridge deposit_root does not match folded deposit frontier".to_string(),
        });
    }

    verify_l1_deposit_events_match_delta(witness, &prefix_roots, violations);
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

fn verify_l1_deposit_events_match_delta(
    witness: &BlockWitness,
    prefix_roots: &[[u8; 32]],
    violations: &mut Vec<Violation>,
) {
    let disposition_events = witness
        .system_events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::L1Deposit {
                account_id,
                amount,
                deposit_id,
                deposit_root,
                sybil_account_key,
            } => Some((
                Some(*account_id),
                *amount,
                *deposit_id,
                *deposit_root,
                *sybil_account_key,
            )),
            SystemEventWitness::DepositQuarantined {
                amount,
                deposit_id,
                deposit_root,
                sybil_account_key,
            } => Some((
                None,
                *amount,
                *deposit_id,
                *deposit_root,
                *sybil_account_key,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    if disposition_events.len() != witness.deposit_accumulator.new_deposits.len() {
        violations.push(Violation {
            kind: ViolationKind::SidecarDepositCursorMismatch,
            details: format!(
                "deposit disposition event count {} != deposit delta length {}",
                disposition_events.len(),
                witness.deposit_accumulator.new_deposits.len()
            ),
        });
    }

    for (index, (account_id, amount, deposit_id, deposit_root, sybil_account_key)) in
        disposition_events.into_iter().enumerate()
    {
        let Some(deposit) = witness.deposit_accumulator.new_deposits.get(index) else {
            continue;
        };
        if deposit.deposit_id != deposit_id {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositCursorMismatch,
                details: format!(
                    "L1Deposit event id {deposit_id} != delta deposit id {}",
                    deposit.deposit_id
                ),
            });
        }
        let expected_root = prefix_roots
            .get(index)
            .copied()
            .unwrap_or(deposit.deposit_root);
        if deposit_root != expected_root {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositRootMismatch,
                details: format!("L1Deposit event {deposit_id} carries wrong frontier root"),
            });
        }
        if deposit.sybil_account_key != sybil_account_key {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositRootMismatch,
                details: format!("deposit disposition event {deposit_id} has wrong account key"),
            });
        }
        if let Some(account_id) = account_id {
            let expected_key = bridge_account_key(account_id);
            if sybil_account_key != expected_key {
                violations.push(Violation {
                    kind: ViolationKind::SidecarDepositRootMismatch,
                    details: format!("L1Deposit event {deposit_id} has wrong account key"),
                });
            }
        }
        let Some(expected_amount) = deposit_amount_nanos(deposit) else {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!("deposit {deposit_id} amount overflows nanos"),
            });
            continue;
        };
        if amount != expected_amount {
            violations.push(Violation {
                kind: ViolationKind::SidecarDepositRootMismatch,
                details: format!(
                    "L1Deposit event {deposit_id} amount {amount} != leaf amount {expected_amount}"
                ),
            });
        }
    }
}

fn verify_market_transition(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let pre_markets = keyed_markets(&witness.pre_state_sidecar.markets, violations);
    let post_markets = keyed_markets(&witness.state_sidecar.markets, violations);
    let resolution_events = market_resolution_events(witness, violations);

    if witness.previous_header.is_some() {
        for (&market_id, pre_market) in &pre_markets {
            let Some(post_market) = post_markets.get(&market_id) else {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!("pre-existing market {:?} was deleted", market_id),
                });
                continue;
            };
            if pre_market.name != post_market.name
                || pre_market.num_outcomes != post_market.num_outcomes
                || pre_market.metadata_digest != post_market.metadata_digest
                || pre_market.resolution_template != post_market.resolution_template
            {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!("market {:?} metadata was silently edited", market_id),
                });
            }

            match resolution_events.get(&market_id).copied() {
                Some(payout_nanos) => match &post_market.status {
                    MarketStatusSnapshot::Resolved { record }
                        if record.payout_nanos == payout_nanos => {}
                    status => violations.push(Violation {
                        kind: ViolationKind::SidecarMarketStatusMismatch,
                        details: format!(
                            "MarketResolved event for {:?} not reflected in committed status {:?}",
                            market_id, status
                        ),
                    }),
                },
                None if pre_market.status != post_market.status => violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!("market {:?} status was silently edited", market_id),
                }),
                None => {}
            }
        }

        for (&market_id, prices) in &witness.clearing_prices {
            match post_markets.get(&market_id) {
                Some(post_market) if post_market.last_clearing_prices == *prices => {}
                Some(post_market) => violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!(
                        "market {:?} committed last clearing prices {:?} != witnessed prices {:?}",
                        market_id, post_market.last_clearing_prices, prices
                    ),
                }),
                None => violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!(
                        "market {:?} has witnessed clearing prices but no committed market leaf",
                        market_id
                    ),
                }),
            }
        }

        for (&market_id, pre_market) in &pre_markets {
            if witness.clearing_prices.contains_key(&market_id) {
                continue;
            }
            let Some(post_market) = post_markets.get(&market_id) else {
                continue;
            };
            if post_market.last_clearing_prices != pre_market.last_clearing_prices {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!(
                        "market {:?} last clearing prices changed without a witnessed clearing entry",
                        market_id
                    ),
                });
            }
        }

        // A newly introduced market with no clearing entry has no prior price
        // state to carry, so it must begin in the never-cleared representation.
        for (&market_id, post_market) in &post_markets {
            if !pre_markets.contains_key(&market_id)
                && !witness.clearing_prices.contains_key(&market_id)
                && !post_market.last_clearing_prices.is_empty()
            {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!(
                        "new market {:?} has last clearing prices without a witnessed clearing entry",
                        market_id
                    ),
                });
            }
        }
    }

    for (event_market, payout_nanos) in &resolution_events {
        match post_markets.get(event_market).map(|market| &market.status) {
            Some(MarketStatusSnapshot::Resolved { record })
                if record.payout_nanos == *payout_nanos => {}
            Some(status) => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "MarketResolved event for {:?} not reflected in committed status {:?}",
                    event_market, status
                ),
            }),
            None => violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "MarketResolved event for {:?} missing from committed market sidecar",
                    event_market
                ),
            }),
        }
    }

    for market_id in &witness.resolved_markets {
        match post_markets.get(market_id).map(|market| &market.status) {
            Some(MarketStatusSnapshot::Resolved { .. }) => {}
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

fn verify_market_group_transition(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let pre_groups = keyed_market_groups(&witness.pre_state_sidecar.market_groups, violations);
    let post_groups = keyed_market_groups(&witness.state_sidecar.market_groups, violations);
    let resolved_markets = resolved_market_events(witness);
    let extensions = witness
        .system_events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::MarketGroupExtended {
                group_id,
                market_id,
            } => Some((*group_id, *market_id)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();

    if witness.previous_header.is_none() {
        return;
    }

    for (&group_id, pre_group) in &pre_groups {
        let expected_markets_after_resolution = pre_group
            .markets
            .iter()
            .copied()
            .filter(|market| !resolved_markets.contains(market))
            .collect::<Vec<_>>();
        let expected_absent = expected_markets_after_resolution.len() < 2;

        let Some(post_group) = post_groups.get(&group_id) else {
            if !expected_absent {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketGroupMismatch,
                    details: format!("pre-existing market group {group_id} was deleted"),
                });
            }
            continue;
        };
        if pre_group.name != post_group.name {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketGroupMismatch,
                details: format!("market group {group_id} name was silently edited"),
            });
        }
        if pre_group.creation_key != post_group.creation_key {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketGroupMismatch,
                details: format!("market group {group_id} creation identity was silently edited"),
            });
        }

        let allowed_additions = extensions
            .iter()
            .filter_map(|(event_group_id, market_id)| {
                (*event_group_id == group_id).then_some(*market_id)
            })
            .collect::<BTreeSet<_>>();
        let expected_markets = expected_markets_after_resolution
            .into_iter()
            .chain(allowed_additions.iter().copied())
            .collect::<BTreeSet<_>>();
        let post_markets = post_group.markets.iter().copied().collect::<BTreeSet<_>>();
        if post_markets != expected_markets {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketGroupMismatch,
                details: format!(
                    "market group {group_id} membership changed without matching resolution/extension event"
                ),
            });
        }
    }
}

fn market_resolution_events(
    witness: &BlockWitness,
    violations: &mut Vec<Violation>,
) -> BTreeMap<MarketId, matching_engine::Nanos> {
    let mut resolved = BTreeMap::new();
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

        if resolved.insert(*market_id, *payout_nanos).is_some() {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!("duplicate MarketResolved event for {:?}", market_id),
            });
        }
    }
    resolved
}

fn resolved_market_events(witness: &BlockWitness) -> BTreeSet<MarketId> {
    witness
        .system_events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::MarketResolved { market_id, .. } => Some(*market_id),
            _ => None,
        })
        .collect()
}

fn keyed_resting_orders<'a>(
    resting_orders: &'a [RestingOrderSnapshot],
    violations: &mut Vec<Violation>,
) -> BTreeMap<u64, &'a RestingOrderSnapshot> {
    let mut out = BTreeMap::new();
    for resting in resting_orders {
        if out.insert(resting.order.id, resting).is_some() {
            violations.push(Violation {
                kind: ViolationKind::SidecarRestingOrderMismatch,
                details: format!("duplicate resting order id {}", resting.order.id),
            });
        }
    }
    out
}

fn keyed_withdrawals<'a>(
    withdrawals: &'a [WithdrawalSnapshot],
    violations: &mut Vec<Violation>,
) -> BTreeMap<u64, &'a WithdrawalSnapshot> {
    let mut out = BTreeMap::new();
    for withdrawal in withdrawals {
        if out.insert(withdrawal.withdrawal_id, withdrawal).is_some() {
            violations.push(Violation {
                kind: ViolationKind::SidecarWithdrawalMismatch,
                details: format!("duplicate withdrawal id {}", withdrawal.withdrawal_id),
            });
        }
    }
    out
}

fn keyed_markets<'a>(
    markets: &'a [MarketSnapshot],
    violations: &mut Vec<Violation>,
) -> BTreeMap<MarketId, &'a MarketSnapshot> {
    let mut out = BTreeMap::new();
    for market in markets {
        let price_count = market.last_clearing_prices.len();
        if price_count != 0 && price_count != usize::from(market.num_outcomes) {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!(
                    "market {:?} last clearing price count {} is neither zero nor num_outcomes {}",
                    market.market_id, price_count, market.num_outcomes
                ),
            });
        }
        for (outcome, price) in market.last_clearing_prices.iter().copied().enumerate() {
            if !price_is_in_protocol_range(price) {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketStatusMismatch,
                    details: format!(
                        "market {:?} outcome {} last clearing price {} exceeds NANOS_PER_DOLLAR {}",
                        market.market_id, outcome, price, NANOS_PER_DOLLAR
                    ),
                });
            }
        }
        if out.insert(market.market_id, market).is_some() {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketStatusMismatch,
                details: format!("duplicate market id {:?}", market.market_id),
            });
        }
    }
    out
}

fn keyed_market_groups<'a>(
    groups: &'a [MarketGroupSnapshot],
    violations: &mut Vec<Violation>,
) -> BTreeMap<u64, &'a MarketGroupSnapshot> {
    let mut out = BTreeMap::new();
    let mut creation_keys = BTreeSet::new();
    for group in groups {
        if let Some(key) = group.creation_key.as_deref() {
            if !matching_engine::operator_creation_key_is_valid(key) {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketGroupMismatch,
                    details: format!(
                        "market group {} has invalid creation key {key:?}",
                        group.group_id
                    ),
                });
            } else if !creation_keys.insert(key) {
                violations.push(Violation {
                    kind: ViolationKind::SidecarMarketGroupMismatch,
                    details: format!("duplicate market group creation key {key:?}"),
                });
            }
        }
        if out.insert(group.group_id, group).is_some() {
            violations.push(Violation {
                kind: ViolationKind::SidecarMarketGroupMismatch,
                details: format!("duplicate market group id {}", group.group_id),
            });
        }
    }
    out
}

fn fill_qty_by_order(
    witness: &BlockWitness,
    violations: &mut Vec<Violation>,
) -> BTreeMap<u64, u64> {
    let mut out = BTreeMap::new();
    for fill in &witness.fills {
        let entry = out.entry(fill.order_id).or_insert(0u64);
        match entry.checked_add(fill.fill_qty.0) {
            Some(qty) => *entry = qty,
            None => violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!("fill quantity overflow for order {}", fill.order_id),
            }),
        }
    }
    out
}

fn expected_post_resting(
    pre: &RestingOrderSnapshot,
    filled: u64,
    height: u64,
) -> Option<RestingOrderSnapshot> {
    if filled >= pre.order.max_fill.0 || height >= pre.expires_at_block {
        return None;
    }
    if filled == 0 {
        return Some(pre.clone());
    }

    let remaining = pre.order.max_fill.0 - filled;
    let max_fill = pre.order.max_fill.0;
    let mut order = pre.order.clone();
    order.max_fill = Qty(remaining);
    let reserved_balance = ceil_mul_ratio(pre.reserved_balance as u64, remaining, max_fill) as i64;
    let reserved_positions = pre
        .reserved_positions
        .iter()
        .map(|&(market, outcome, qty)| {
            (
                market,
                outcome,
                ceil_mul_ratio(qty as u64, remaining, max_fill) as i64,
            )
        })
        .collect();
    Some(RestingOrderSnapshot {
        order,
        account_id: pre.account_id,
        created_at: pre.created_at,
        expires_at_block: pre.expires_at_block,
        reserved_balance,
        reserved_positions,
    })
}

fn resting_orders_equal(left: &RestingOrderSnapshot, right: &RestingOrderSnapshot) -> bool {
    left.account_id == right.account_id
        && left.created_at == right.created_at
        && left.expires_at_block == right.expires_at_block
        && left.reserved_balance == right.reserved_balance
        && left.reserved_positions == right.reserved_positions
        && orders_equal(&left.order, &right.order)
}

fn order_shape_matches_except_qty(left: &Order, right: &Order) -> bool {
    left.id == right.id
        && left.markets == right.markets
        && left.num_markets == right.num_markets
        && left.payoffs == right.payoffs
        && left.num_states == right.num_states
        && left.limit_price == right.limit_price
        && left.condition == right.condition
        && left.expires_at_block == right.expires_at_block
}

fn orders_equal(left: &Order, right: &Order) -> bool {
    order_shape_matches_except_qty(left, right) && left.max_fill == right.max_fill
}

fn deposit_amount_nanos(deposit: &L1DepositWitness) -> Option<i64> {
    let amount = deposit.amount_token_units.checked_mul(1_000)?;
    i64::try_from(amount).ok()
}

fn bridge_account_key(account_id: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/bridge/account-key/v1");
    hasher.update(&account_id.to_le_bytes());
    *hasher.finalize().as_bytes()
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
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: BTreeMap::new().into_iter().collect(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            account_keys: vec![],
            state_sidecar: sidecar,
            pre_state_sidecar: empty_sidecar(),
            resolved_markets: vec![],
        }
    }

    fn witness_with_pre_post_sidecars(
        pre_state_sidecar: StateSidecarSnapshot,
        state_sidecar: StateSidecarSnapshot,
    ) -> BlockWitness {
        BlockWitness {
            header: WitnessBlockHeader {
                height: 2,
                ..empty_header()
            },
            previous_header: Some(empty_header()),
            genesis_hash: [0u8; 32],
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: BTreeMap::new().into_iter().collect(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            account_keys: vec![],
            state_sidecar,
            pre_state_sidecar,
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
            withdrawal_id: 0,
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
                sybil_account_key: bridge_account_key(7),
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
                    payout_nanos: Nanos(NANOS_PER_DOLLAR),
                    resolved_by: OracleSourceSnapshot::Admin,
                    resolved_at_ms: 1_000,
                },
            },
            metadata_digest: [5u8; 32],
            resolution_template: "admin".to_string(),
            last_clearing_prices: vec![],
        }
    }

    fn active_market(market_id: MarketId) -> MarketSnapshot {
        MarketSnapshot {
            market_id,
            name: format!("M{}", market_id.0),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [market_id.0 as u8; 32],
            resolution_template: "admin".to_string(),
            last_clearing_prices: vec![],
        }
    }

    fn market_group(group_id: u64, markets: Vec<MarketId>) -> MarketGroupSnapshot {
        MarketGroupSnapshot {
            group_id,
            name: format!("G{group_id}"),
            creation_key: None,
            markets,
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
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarReservationMismatch)
        );
    }

    #[test]
    fn sidecar_resting_order_and_reservation_co_deletion_fails_closed() {
        let mut pre = empty_sidecar();
        pre.resting_orders = vec![resting_order()];
        pre.account_reservations = vec![reservation()];
        let post = empty_sidecar();

        let result = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarRestingOrderMismatch)
        );
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
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarReservationMismatch)
        );
    }

    #[test]
    fn sidecar_delete_withdrawal_leaf_fails() {
        let mut sidecar = empty_sidecar();
        sidecar.bridge.next_withdrawal_id = 1;
        sidecar.bridge.withdrawals = vec![withdrawal_leaf()];
        let mut witness = witness_with_sidecar(sidecar.clone());
        witness.system_events = vec![withdrawal_event()];
        assert!(verify_sidecar(&witness).valid);

        witness.state_sidecar.bridge.withdrawals.clear();
        let result = verify_sidecar(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarWithdrawalMismatch)
        );
    }

    #[test]
    fn sidecar_pre_existing_withdrawal_deletion_fails_closed() {
        let mut pre = empty_sidecar();
        pre.bridge.next_withdrawal_id = 10;
        pre.bridge.withdrawals = vec![withdrawal_leaf()];
        let mut post = empty_sidecar();
        post.bridge.next_withdrawal_id = 10;

        let result = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarWithdrawalMismatch)
        );
    }

    #[test]
    fn sidecar_corrupt_deposit_cursor_fails() {
        let deposits = l1_deposit_prefix(2);
        let mut sidecar = empty_sidecar();
        sidecar.bridge.deposit_cursor = 2;
        sidecar.bridge.deposit_root = deposits.last().expect("deposit prefix").deposit_root;
        let mut witness = witness_with_sidecar(sidecar);
        witness.system_events = deposits
            .iter()
            .map(|deposit| SystemEventWitness::L1Deposit {
                account_id: 7,
                amount: deposit_amount_nanos(deposit).expect("small deposit amount"),
                deposit_id: deposit.deposit_id,
                deposit_root: deposit.deposit_root,
                sybil_account_key: deposit.sybil_account_key,
            })
            .collect();
        witness.deposit_accumulator.new_deposits = deposits;
        assert!(verify_sidecar(&witness).valid);

        witness.state_sidecar.bridge.deposit_cursor = 1;
        let result = verify_sidecar(&witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarDepositCursorMismatch)
        );
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
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch)
        );
    }

    #[test]
    fn sidecar_silent_market_edit_fails_closed() {
        let mut pre = empty_sidecar();
        pre.markets = vec![active_market(MarketId::new(11))];
        let mut post = pre.clone();
        post.markets[0].metadata_digest = [0xaa; 32];

        let result = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch)
        );
    }

    #[test]
    fn sidecar_cleared_market_price_must_match_witness() {
        let market_id = MarketId::new(11);
        let mut pre = empty_sidecar();
        pre.markets = vec![active_market(market_id)];
        let mut post = pre.clone();
        post.markets[0].last_clearing_prices = vec![Nanos(600_000_000), Nanos(400_000_000)];
        let mut witness = witness_with_pre_post_sidecars(pre, post);
        witness
            .clearing_prices
            .insert(market_id, vec![Nanos(550_000_000), Nanos(450_000_000)]);

        let result = verify_sidecar(&witness);

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch)
        );
    }

    #[test]
    fn sidecar_market_price_cannot_mutate_without_clearing_entry() {
        let market_id = MarketId::new(11);
        let mut pre = empty_sidecar();
        let mut market = active_market(market_id);
        market.last_clearing_prices = vec![Nanos(550_000_000), Nanos(450_000_000)];
        pre.markets = vec![market];
        let mut post = pre.clone();
        post.markets[0].last_clearing_prices = vec![Nanos(600_000_000), Nanos(400_000_000)];

        let result = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch)
        );
    }

    #[test]
    fn sidecar_honest_market_price_transitions_are_accepted() {
        let cleared_id = MarketId::new(11);
        let carried_id = MarketId::new(12);
        let mut cleared = active_market(cleared_id);
        cleared.last_clearing_prices = vec![Nanos(500_000_000), Nanos(500_000_000)];
        let mut carried = active_market(carried_id);
        carried.last_clearing_prices = vec![Nanos(300_000_000), Nanos(700_000_000)];
        let mut pre = empty_sidecar();
        pre.markets = vec![cleared, carried];
        let mut post = pre.clone();
        let new_prices = vec![Nanos(550_000_000), Nanos(450_000_000)];
        post.markets[0].last_clearing_prices = new_prices.clone();
        let mut witness = witness_with_pre_post_sidecars(pre, post);
        witness.clearing_prices.insert(cleared_id, new_prices);

        let result = verify_sidecar(&witness);

        assert!(result.valid, "violations: {:?}", result.violations);
    }

    #[test]
    fn sidecar_market_price_shape_fails_closed() {
        let market_id = MarketId::new(11);
        let mut wrong_count = empty_sidecar();
        let mut market = active_market(market_id);
        market.last_clearing_prices = vec![Nanos(500_000_000)];
        wrong_count.markets = vec![market];
        let result = verify_sidecar(&witness_with_sidecar(wrong_count));
        assert!(!result.valid);

        let mut out_of_range = empty_sidecar();
        let mut market = active_market(market_id);
        market.last_clearing_prices = vec![Nanos(NANOS_PER_DOLLAR + 1), Nanos(0)];
        out_of_range.markets = vec![market];
        let result = verify_sidecar(&witness_with_sidecar(out_of_range));
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketStatusMismatch)
        );
    }

    #[test]
    fn sidecar_silent_market_group_membership_extension_fails_closed() {
        let mut pre = empty_sidecar();
        pre.market_groups = vec![market_group(3, vec![MarketId::new(1), MarketId::new(2)])];
        let mut post = pre.clone();
        post.market_groups[0].markets.push(MarketId::new(4));

        let result = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));

        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::SidecarMarketGroupMismatch)
        );
    }

    #[test]
    fn sidecar_market_group_creation_identity_is_valid_unique_and_immutable() {
        let mut pre = empty_sidecar();
        let mut group = market_group(3, vec![MarketId::new(1), MarketId::new(2)]);
        group.creation_key = Some("native:event".to_string());
        pre.market_groups = vec![group];
        let mut post = pre.clone();
        post.market_groups[0].creation_key = Some("native:other".to_string());

        let changed = verify_sidecar(&witness_with_pre_post_sidecars(pre, post));
        assert!(!changed.valid);
        assert!(changed.violations.iter().any(|violation| {
            violation.kind == ViolationKind::SidecarMarketGroupMismatch
                && violation.details.contains("creation identity")
        }));

        let mut duplicate = empty_sidecar();
        let mut first = market_group(1, vec![MarketId::new(1), MarketId::new(2)]);
        first.creation_key = Some("native:event".to_string());
        let mut second = market_group(2, vec![MarketId::new(3), MarketId::new(4)]);
        second.creation_key = first.creation_key.clone();
        duplicate.market_groups = vec![first, second];
        let duplicate = verify_sidecar(&witness_with_sidecar(duplicate));
        assert!(!duplicate.valid);
        assert!(duplicate.violations.iter().any(|violation| {
            violation.kind == ViolationKind::SidecarMarketGroupMismatch
                && violation
                    .details
                    .contains("duplicate market group creation key")
        }));
    }

    #[test]
    fn sidecar_market_group_extension_event_allows_membership_change() {
        let mut pre = empty_sidecar();
        pre.market_groups = vec![market_group(3, vec![MarketId::new(1), MarketId::new(2)])];
        let mut post = pre.clone();
        post.market_groups[0].markets.push(MarketId::new(4));
        let mut witness = witness_with_pre_post_sidecars(pre, post);
        witness.system_events = vec![SystemEventWitness::MarketGroupExtended {
            group_id: 3,
            market_id: MarketId::new(4),
        }];

        let result = verify_sidecar(&witness);

        assert!(result.valid, "violations: {:?}", result.violations);
    }
}
