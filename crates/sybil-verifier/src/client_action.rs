//! Canonical ordinary client-action bytes shared by admission and guest replay.

use matching_engine::{ConditionDir, MmSide, Nanos, Order};
use sybil_signing::{
    ConditionDir as CanonicalConditionDir, MarketId as CanonicalMarketId,
    MmBundle as CanonicalMmBundle, MmBundleCancel as CanonicalMmBundleCancel,
    MmBundleOrder as CanonicalMmBundleOrder, MmBundleReplace as CanonicalMmBundleReplace,
    MmSide as CanonicalMmSide, Order as CanonicalOrder, PriceCondition as CanonicalPriceCondition,
};

use crate::{
    BlockWitness, ClientActionWitness, SystemEventWitness, VerificationResult, Violation,
    ViolationKind,
};

fn to_canonical_order(order: &Order, nonce: u64) -> CanonicalOrder {
    let mut markets = [CanonicalMarketId::NONE; sybil_signing::MAX_MARKETS_PER_ORDER];
    for (dst, src) in markets.iter_mut().zip(order.markets.iter()) {
        *dst = CanonicalMarketId(src.0);
    }

    let condition = order
        .condition
        .as_ref()
        .map(|condition| CanonicalPriceCondition {
            market: CanonicalMarketId(condition.market.0),
            threshold: condition.threshold.0,
            direction: match condition.direction {
                ConditionDir::Above => CanonicalConditionDir::Above,
                ConditionDir::Below => CanonicalConditionDir::Below,
            },
        });

    CanonicalOrder {
        markets,
        num_markets: order.num_markets,
        payoffs: order.payoffs,
        num_states: order.num_states,
        limit_price: order.limit_price.0,
        max_fill: order.max_fill.0,
        condition,
        expires_at_block: order.expires_at_block,
        nonce,
    }
}

/// Canonical bytes signed by raw P256 and hashed into a WebAuthn challenge.
/// The server-assigned `Order.id` is intentionally excluded.
pub fn canonical_order_bytes(order: &Order, nonce: u64, genesis_hash: [u8; 32]) -> Vec<u8> {
    sybil_signing::canonical_order_bytes(&to_canonical_order(order, nonce), genesis_hash)
}

pub fn canonical_cancel_bytes(
    account_id: u64,
    order_id: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
) -> Vec<u8> {
    sybil_signing::canonical_cancel_bytes(account_id, order_id, nonce, genesis_hash)
}

fn to_canonical_mm_side(side: MmSide) -> CanonicalMmSide {
    match side {
        MmSide::SellYes => CanonicalMmSide::SellYes,
        MmSide::BuyYes => CanonicalMmSide::BuyYes,
        MmSide::SellNo => CanonicalMmSide::SellNo,
        MmSide::BuyNo => CanonicalMmSide::BuyNo,
    }
}

fn to_canonical_mm_bundle_order(order: &Order, side: MmSide) -> CanonicalMmBundleOrder {
    let canonical = to_canonical_order(order, 0);
    CanonicalMmBundleOrder {
        markets: canonical.markets,
        num_markets: canonical.num_markets,
        payoffs: canonical.payoffs,
        num_states: canonical.num_states,
        limit_price: canonical.limit_price,
        max_fill: canonical.max_fill,
        condition: canonical.condition,
        expires_at_block: canonical.expires_at_block,
        side: to_canonical_mm_side(side),
    }
}

/// Canonical bytes for a signed atomic MM bundle. Server-assigned order ids
/// are deliberately excluded; the signed side vector is positional.
#[allow(
    clippy::too_many_arguments,
    reason = "the verifier canonicalizer exposes the complete signed protocol tuple"
)]
pub fn canonical_mm_bundle_bytes(
    account_id: u64,
    bundle_id: [u8; 32],
    revision: u64,
    orders: &[Order],
    order_sides: &[MmSide],
    max_capital: Nanos,
    nonce: u64,
    genesis_hash: [u8; 32],
) -> Result<Vec<u8>, String> {
    if orders.len() != order_sides.len() {
        return Err("MM bundle orders and sides have different lengths".to_string());
    }
    let orders = orders
        .iter()
        .zip(order_sides)
        .map(|(order, side)| to_canonical_mm_bundle_order(order, *side))
        .collect();
    Ok(sybil_signing::canonical_mm_bundle_bytes(
        &CanonicalMmBundle {
            account_id,
            bundle_id,
            revision,
            orders,
            max_capital: max_capital.0,
            nonce,
        },
        genesis_hash,
    ))
}

#[allow(
    clippy::too_many_arguments,
    reason = "the verifier canonicalizer exposes the complete replacement protocol tuple"
)]
pub fn canonical_mm_bundle_replace_bytes(
    account_id: u64,
    bundle_id: [u8; 32],
    expected_revision: u64,
    new_revision: u64,
    orders: &[Order],
    order_sides: &[MmSide],
    max_capital: Nanos,
    nonce: u64,
    genesis_hash: [u8; 32],
) -> Result<Vec<u8>, String> {
    if new_revision
        != expected_revision
            .checked_add(1)
            .ok_or("MM bundle revision exhausted")?
    {
        return Err("replacement revision must be exactly expected revision plus one".to_string());
    }
    if orders.len() != order_sides.len() {
        return Err("MM bundle orders and sides have different lengths".to_string());
    }
    let orders = orders
        .iter()
        .zip(order_sides)
        .map(|(order, side)| to_canonical_mm_bundle_order(order, *side))
        .collect();
    Ok(sybil_signing::canonical_mm_bundle_replace_bytes(
        &CanonicalMmBundleReplace {
            expected_revision,
            replacement: CanonicalMmBundle {
                account_id,
                bundle_id,
                revision: new_revision,
                orders,
                max_capital: max_capital.0,
                nonce,
            },
        },
        genesis_hash,
    ))
}

pub fn canonical_mm_bundle_cancel_bytes(
    account_id: u64,
    bundle_id: [u8; 32],
    expected_revision: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
) -> Vec<u8> {
    sybil_signing::canonical_mm_bundle_cancel_bytes(
        &CanonicalMmBundleCancel {
            account_id,
            bundle_id,
            expected_revision,
            nonce,
        },
        genesis_hash,
    )
}

/// Bind every authorization event to the order/cancel effect it authorized.
/// Signature/key membership is checked by `key_transition`; nonce replay is
/// checked by `system`. This pass prevents a valid envelope from being carried
/// without the corresponding sequencer action.
pub fn verify_client_action_bindings(witness: &BlockWitness) -> VerificationResult {
    match verify_bindings(witness) {
        Ok(()) => VerificationResult::from_violations(Vec::new()),
        Err(details) => VerificationResult::from_violations(vec![Violation {
            kind: ViolationKind::ClientActionMismatch,
            details,
        }]),
    }
}

fn verify_bindings(witness: &BlockWitness) -> Result<(), String> {
    use std::collections::{BTreeMap, BTreeSet};

    let pre_resting: BTreeSet<u64> = witness
        .pre_state_sidecar
        .resting_orders
        .iter()
        .map(|resting| resting.order.id)
        .collect();
    let mut order_results: BTreeMap<u64, (u64, &Order)> = BTreeMap::new();
    let mut accepted_mm = BTreeSet::new();
    let mut rejected_ids = BTreeSet::new();
    for accepted in &witness.orders {
        if order_results
            .insert(accepted.order.id, (accepted.account_id, &accepted.order))
            .is_some()
        {
            return Err(format!(
                "duplicate order result for authorized order {}",
                accepted.order.id
            ));
        }
        if accepted.is_mm {
            accepted_mm.insert(accepted.order.id);
        }
    }
    for rejected in &witness.rejections {
        if order_results
            .insert(rejected.order.id, (rejected.account_id, &rejected.order))
            .is_some()
        {
            return Err(format!(
                "duplicate order result for authorized order {}",
                rejected.order.id
            ));
        }
        rejected_ids.insert(rejected.order.id);
    }
    for resting in &witness.state_sidecar.resting_orders {
        order_results
            .entry(resting.order.id)
            .or_insert((resting.account_id, &resting.order));
    }

    let mut authorized_orders: BTreeMap<u64, (usize, u64)> = BTreeMap::new();
    for (index, event) in witness.system_events.iter().enumerate() {
        let SystemEventWitness::ClientActionAuthorized(action) = event else {
            continue;
        };
        match action {
            ClientActionWitness::Order {
                account_id, order, ..
            } => {
                if pre_resting.contains(&order.id) {
                    return Err(format!(
                        "authorized order {} already existed in authenticated pre-state",
                        order.id
                    ));
                }
                if authorized_orders
                    .insert(order.id, (index, *account_id))
                    .is_some()
                {
                    return Err(format!("order {} was authorized more than once", order.id));
                }
                let has_result =
                    order_results
                        .get(&order.id)
                        .is_some_and(|(result_account, result_order)| {
                            *result_account == *account_id && *result_order == order
                        });
                let cancelled_later = witness.system_events[index + 1..].iter().any(|event| {
                    matches!(
                        event,
                        SystemEventWitness::OrderCancelled {
                            account_id: cancelled_account,
                            order_id,
                            ..
                        } if cancelled_account == account_id && *order_id == order.id
                    )
                });
                let resolved_later = witness.system_events[index + 1..].iter().any(|event| {
                    matches!(
                        event,
                        SystemEventWitness::MarketResolved { market_id, .. }
                            if order.active_markets().any(|active| active == *market_id)
                    )
                });
                if !has_result && !cancelled_later && !resolved_later {
                    return Err(format!(
                        "authorized order {} has no matching block or sidecar effect",
                        order.id
                    ));
                }
            }
            ClientActionWitness::Cancel {
                account_id,
                order_id,
                ..
            } => {
                // The order may have been admitted through an internal
                // unsigned path and cancelled before the next block, leaving
                // no pre/post sidecar leaf. The account-bound signature plus
                // the exact later cancellation effect is sufficient: a
                // nonexistent cancel is at worst an authorized account-event
                // no-op and cannot remove another account's order.
                let effect = witness.system_events[index + 1..].iter().any(|event| {
                    matches!(
                        event,
                        SystemEventWitness::OrderCancelled {
                            account_id: cancelled_account,
                            order_id: cancelled_order,
                            ..
                        } if cancelled_account == account_id && cancelled_order == order_id
                    )
                });
                if !effect {
                    return Err(format!(
                        "authorized cancel for order {order_id} has no later cancellation effect"
                    ));
                }
            }
            ClientActionWitness::MmBundle {
                account_id,
                revision,
                orders,
                order_sides,
                max_capital,
                ..
            } => {
                if *revision != 0 {
                    return Err(format!(
                        "initial authorized MM bundle has nonzero revision {revision}"
                    ));
                }
                verify_mm_bundle_binding(
                    witness,
                    &pre_resting,
                    &order_results,
                    &accepted_mm,
                    &rejected_ids,
                    &mut authorized_orders,
                    index,
                    *account_id,
                    orders,
                    order_sides,
                    *max_capital,
                )?;
            }
            ClientActionWitness::MmBundleReplace {
                account_id,
                expected_revision,
                new_revision,
                orders,
                order_sides,
                max_capital,
                ..
            } => {
                if *new_revision
                    != expected_revision
                        .checked_add(1)
                        .ok_or("authorized MM replacement revision exhausted")?
                {
                    return Err(
                        "authorized MM replacement revision is not exactly expected plus one"
                            .into(),
                    );
                }
                verify_mm_bundle_binding(
                    witness,
                    &pre_resting,
                    &order_results,
                    &accepted_mm,
                    &rejected_ids,
                    &mut authorized_orders,
                    index,
                    *account_id,
                    orders,
                    order_sides,
                    *max_capital,
                )?;
            }
            ClientActionWitness::MmBundleCancel { account_id, .. } => {
                let later_bundle = witness.system_events[index + 1..].iter().any(|event| {
                    matches!(
                        event,
                        SystemEventWitness::ClientActionAuthorized(
                            ClientActionWitness::MmBundle {
                                account_id: later_account,
                                ..
                            } | ClientActionWitness::MmBundleReplace {
                                account_id: later_account,
                                ..
                            }
                        ) if later_account == account_id
                    )
                });
                if !later_bundle
                    && witness
                        .mm_constraints
                        .iter()
                        .any(|constraint| constraint.mm_id.0 == *account_id)
                {
                    return Err(format!(
                        "authorized MM cancellation for account {account_id} retained a bundle constraint"
                    ));
                }
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the helper binds one signed bundle across each independent witness index"
)]
fn verify_mm_bundle_binding(
    witness: &BlockWitness,
    pre_resting: &std::collections::BTreeSet<u64>,
    order_results: &std::collections::BTreeMap<u64, (u64, &Order)>,
    accepted_mm: &std::collections::BTreeSet<u64>,
    rejected_ids: &std::collections::BTreeSet<u64>,
    authorized_orders: &mut std::collections::BTreeMap<u64, (usize, u64)>,
    event_index: usize,
    account_id: u64,
    orders: &[Order],
    order_sides: &[MmSide],
    max_capital: Nanos,
) -> Result<(), String> {
    if orders.is_empty() || orders.len() != order_sides.len() {
        return Err("authorized MM bundle has empty or mismatched orders/sides".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for order in orders {
        if pre_resting.contains(&order.id) {
            return Err(format!(
                "authorized MM order {} already existed in authenticated pre-state",
                order.id
            ));
        }
        if !ids.insert(order.id)
            || authorized_orders
                .insert(order.id, (event_index, account_id))
                .is_some()
        {
            return Err(format!(
                "MM order {} was authorized more than once",
                order.id
            ));
        }
        let has_result =
            order_results
                .get(&order.id)
                .is_some_and(|(result_account, result_order)| {
                    *result_account == account_id && *result_order == order
                });
        if !has_result {
            return Err(format!(
                "authorized MM order {} has no matching block result",
                order.id
            ));
        }
    }
    let accepted = ids.iter().filter(|id| accepted_mm.contains(id)).count();
    let rejected = ids.iter().filter(|id| rejected_ids.contains(id)).count();
    if accepted != ids.len() && rejected != ids.len() {
        return Err(format!(
            "authorized MM bundle was partially admitted: {accepted} accepted, {rejected} rejected"
        ));
    }
    let exact_constraint = witness.mm_constraints.iter().any(|constraint| {
        constraint.mm_id.0 == account_id
            && constraint.max_capital == max_capital
            && constraint.order_ids.len() == orders.len()
            && constraint.order_sides.len() == orders.len()
            && orders.iter().zip(order_sides).all(|(order, side)| {
                constraint.order_ids.contains(&order.id)
                    && constraint.order_sides.get(&order.id) == Some(side)
            })
    });
    if !exact_constraint {
        return Err("authorized MM bundle has no exact shared-budget constraint".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{
        MarketId, MarketSet, MmConstraint, MmId, Nanos, OrderDirection, outcome_buy, shares_to_qty,
    };

    use crate::{
        ClientActionAuth, DepositAccumulatorWitness, RejectionReason, RestingOrderSnapshot,
        StateSidecarSnapshot, WitnessBlockHeader, WitnessOrder, WitnessRejection,
    };

    fn header() -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0; 32],
            state_root: [0; 32],
            events_root: crate::test_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
        }
    }

    fn witness() -> BlockWitness {
        BlockWitness {
            header: header(),
            previous_header: None,
            genesis_hash: [0; 32],
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: Vec::new(),
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: Vec::new(),
            clearing_prices: Default::default(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: Vec::new(),
            post_system_state: Vec::new(),
            post_state: Vec::new(),
            account_keys: Vec::new(),
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        }
    }

    fn order(id: u64, market: MarketId) -> Order {
        let mut markets = MarketSet::new();
        let actual_market = markets.add_binary("binding");
        assert_eq!(actual_market, market);
        outcome_buy(
            &markets,
            id,
            actual_market,
            0,
            500_000_000,
            shares_to_qty(1).0,
        )
    }

    fn auth() -> ClientActionAuth {
        ClientActionAuth::RawP256 {
            signer_pubkey: [0; 33],
            signature: [0; 64],
        }
    }

    fn authorized_order(account_id: u64, order: Order) -> SystemEventWitness {
        SystemEventWitness::ClientActionAuthorized(ClientActionWitness::Order {
            account_id,
            order,
            nonce: 1,
            authorization: auth(),
        })
    }

    fn authorized_cancel(account_id: u64, order_id: u64) -> SystemEventWitness {
        SystemEventWitness::ClientActionAuthorized(ClientActionWitness::Cancel {
            account_id,
            order_id,
            nonce: 1,
            authorization: auth(),
        })
    }

    fn authorized_mm_replace(
        account_id: u64,
        order: Order,
        max_capital: Nanos,
    ) -> SystemEventWitness {
        SystemEventWitness::ClientActionAuthorized(ClientActionWitness::MmBundleReplace {
            account_id,
            bundle_id: [0x44; 32],
            expected_revision: 7,
            new_revision: 8,
            orders: vec![order],
            order_sides: vec![MmSide::BuyYes],
            max_capital,
            nonce: 9,
            authorization: auth(),
        })
    }

    fn authorized_mm_cancel(account_id: u64) -> SystemEventWitness {
        SystemEventWitness::ClientActionAuthorized(ClientActionWitness::MmBundleCancel {
            account_id,
            bundle_id: [0x44; 32],
            expected_revision: 8,
            nonce: 10,
            authorization: auth(),
        })
    }

    fn cancellation(account_id: u64, order_id: u64) -> SystemEventWitness {
        SystemEventWitness::OrderCancelled {
            account_id,
            order_id,
            market_ids: vec![MarketId(0)],
            side: OrderDirection::BuyYes,
            remaining_quantity: shares_to_qty(1).0,
        }
    }

    fn resting(order: Order, account_id: u64) -> RestingOrderSnapshot {
        RestingOrderSnapshot {
            order,
            account_id,
            created_at: 1,
            expires_at_block: 10,
            reserved_balance: 500_000_000,
            reserved_positions: Vec::new(),
        }
    }

    fn assert_mismatch(witness: &BlockWitness, expected: &str) {
        let result = verify_client_action_bindings(witness);
        assert!(!result.valid);
        assert_eq!(
            result.violations[0].kind,
            ViolationKind::ClientActionMismatch
        );
        assert!(
            result.violations[0].details.contains(expected),
            "unexpected violation: {}",
            result.violations[0].details
        );
    }

    #[test]
    fn empty_witness_has_no_binding_work() {
        assert!(verify_client_action_bindings(&witness()).valid);
    }

    #[test]
    fn authorized_order_accepts_matching_accepted_rejected_or_resting_effect() {
        let expected = order(7, MarketId(0));

        let mut accepted = witness();
        accepted.orders.push(WitnessOrder {
            order: expected.clone(),
            account_id: 11,
            is_mm: false,
        });
        accepted
            .system_events
            .push(authorized_order(11, expected.clone()));
        assert!(verify_client_action_bindings(&accepted).valid);

        let mut rejected = witness();
        rejected.rejections.push(WitnessRejection {
            order: expected.clone(),
            account_id: 11,
            reason: RejectionReason::AccountNotFound,
        });
        rejected
            .system_events
            .push(authorized_order(11, expected.clone()));
        assert!(verify_client_action_bindings(&rejected).valid);

        let mut resting_effect = witness();
        resting_effect
            .state_sidecar
            .resting_orders
            .push(resting(expected.clone(), 11));
        resting_effect
            .system_events
            .push(authorized_order(11, expected));
        assert!(verify_client_action_bindings(&resting_effect).valid);
    }

    #[test]
    fn authorized_order_requires_exact_account_and_order() {
        let expected = order(7, MarketId(0));

        let mut wrong_account = witness();
        wrong_account.orders.push(WitnessOrder {
            order: expected.clone(),
            account_id: 12,
            is_mm: false,
        });
        wrong_account
            .system_events
            .push(authorized_order(11, expected.clone()));
        assert_mismatch(&wrong_account, "no matching block or sidecar effect");

        let mut different_order = expected.clone();
        different_order.limit_price = Nanos(400_000_000);
        let mut wrong_order = witness();
        wrong_order.orders.push(WitnessOrder {
            order: different_order,
            account_id: 11,
            is_mm: false,
        });
        wrong_order
            .system_events
            .push(authorized_order(11, expected));
        assert_mismatch(&wrong_order, "no matching block or sidecar effect");
    }

    #[test]
    fn duplicate_order_results_and_authorizations_are_rejected() {
        let expected = order(7, MarketId(0));

        let mut duplicate_result = witness();
        duplicate_result.orders.push(WitnessOrder {
            order: expected.clone(),
            account_id: 11,
            is_mm: false,
        });
        duplicate_result.rejections.push(WitnessRejection {
            order: expected.clone(),
            account_id: 11,
            reason: RejectionReason::AccountNotFound,
        });
        assert_mismatch(&duplicate_result, "duplicate order result");

        let mut duplicate_authorization = witness();
        duplicate_authorization.orders.push(WitnessOrder {
            order: expected.clone(),
            account_id: 11,
            is_mm: false,
        });
        duplicate_authorization
            .system_events
            .push(authorized_order(11, expected.clone()));
        duplicate_authorization
            .system_events
            .push(authorized_order(11, expected));
        assert_mismatch(&duplicate_authorization, "authorized more than once");
    }

    #[test]
    fn authorized_order_cannot_reauthorize_authenticated_resting_order() {
        let expected = order(7, MarketId(0));
        let mut candidate = witness();
        candidate
            .pre_state_sidecar
            .resting_orders
            .push(resting(expected.clone(), 11));
        candidate.system_events.push(authorized_order(11, expected));
        assert_mismatch(&candidate, "already existed in authenticated pre-state");
    }

    #[test]
    fn authorized_order_accepts_only_later_cancel_or_resolution_effect() {
        let expected = order(7, MarketId(0));

        let mut cancelled = witness();
        cancelled
            .system_events
            .push(authorized_order(11, expected.clone()));
        cancelled.system_events.push(cancellation(11, 7));
        assert!(verify_client_action_bindings(&cancelled).valid);

        let mut resolved = witness();
        resolved
            .system_events
            .push(authorized_order(11, expected.clone()));
        resolved
            .system_events
            .push(SystemEventWitness::MarketResolved {
                market_id: MarketId(0),
                payout_nanos: Nanos(1_000_000_000),
                affected_accounts: Vec::new(),
            });
        assert!(verify_client_action_bindings(&resolved).valid);

        let mut earlier_cancel = witness();
        earlier_cancel.system_events.push(cancellation(11, 7));
        earlier_cancel
            .system_events
            .push(authorized_order(11, expected.clone()));
        assert_mismatch(&earlier_cancel, "no matching block or sidecar effect");

        let mut unrelated_resolution = witness();
        unrelated_resolution
            .system_events
            .push(authorized_order(11, expected));
        unrelated_resolution
            .system_events
            .push(SystemEventWitness::MarketResolved {
                market_id: MarketId(9),
                payout_nanos: Nanos(1_000_000_000),
                affected_accounts: Vec::new(),
            });
        assert_mismatch(&unrelated_resolution, "no matching block or sidecar effect");
    }

    #[test]
    fn authorized_cancel_requires_exact_later_effect() {
        let mut valid = witness();
        valid.system_events.push(authorized_cancel(11, 7));
        valid.system_events.push(cancellation(11, 7));
        assert!(verify_client_action_bindings(&valid).valid);

        for (cancelled_account, cancelled_order) in [(12, 7), (11, 8)] {
            let mut wrong_effect = witness();
            wrong_effect.system_events.push(authorized_cancel(11, 7));
            wrong_effect
                .system_events
                .push(cancellation(cancelled_account, cancelled_order));
            assert_mismatch(&wrong_effect, "has no later cancellation effect");
        }

        let mut earlier_effect = witness();
        earlier_effect.system_events.push(cancellation(11, 7));
        earlier_effect.system_events.push(authorized_cancel(11, 7));
        assert_mismatch(&earlier_effect, "has no later cancellation effect");
    }

    #[test]
    fn authorized_mm_replace_requires_exact_atomic_result_and_shared_budget() {
        let expected = order(7, MarketId(0));
        let budget = Nanos(2_000_000_000);
        let mut valid = witness();
        valid.orders.push(WitnessOrder {
            order: expected.clone(),
            account_id: 11,
            is_mm: true,
        });
        let mut constraint = MmConstraint::new(MmId(11), budget);
        constraint.add_order(expected.id, MmSide::BuyYes);
        valid.mm_constraints.push(constraint);
        valid
            .system_events
            .push(authorized_mm_replace(11, expected.clone(), budget));
        assert!(verify_client_action_bindings(&valid).valid);

        let mut wrong_budget = valid.clone();
        wrong_budget.mm_constraints[0].max_capital = Nanos(budget.0 + 1);
        assert_mismatch(&wrong_budget, "no exact shared-budget constraint");

        let mut partial_result = valid.clone();
        partial_result.orders.clear();
        assert_mismatch(&partial_result, "has no matching block result");

        let mut skipped_revision = valid;
        let SystemEventWitness::ClientActionAuthorized(ClientActionWitness::MmBundleReplace {
            new_revision,
            ..
        }) = &mut skipped_revision.system_events[0]
        else {
            panic!("replacement authorization fixture");
        };
        *new_revision = 9;
        assert_mismatch(&skipped_revision, "not exactly expected plus one");
    }

    #[test]
    fn authorized_mm_cancel_requires_the_account_constraint_to_be_absent() {
        let mut valid = witness();
        valid.system_events.push(authorized_mm_cancel(11));
        assert!(verify_client_action_bindings(&valid).valid);

        let mut retained = valid;
        retained
            .mm_constraints
            .push(MmConstraint::new(MmId(11), Nanos(1_000_000_000)));
        assert_mismatch(&retained, "retained a bundle constraint");

        let mut unrelated = witness();
        unrelated
            .mm_constraints
            .push(MmConstraint::new(MmId(12), Nanos(1_000_000_000)));
        unrelated.system_events.push(authorized_mm_cancel(11));
        assert!(verify_client_action_bindings(&unrelated).valid);
    }
}
