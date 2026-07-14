//! Canonical ordinary client-action bytes shared by admission and guest replay.

use matching_engine::{ConditionDir, Order};
use sybil_signing::{
    ConditionDir as CanonicalConditionDir, MarketId as CanonicalMarketId, Order as CanonicalOrder,
    PriceCondition as CanonicalPriceCondition,
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
        }
    }
    Ok(())
}
