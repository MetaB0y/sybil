//! Canonical per-block event leaf schema committed by `BlockHeader.events_root`.

use matching_engine::Fill;

use crate::canonical::append_order;
use crate::types::{RejectionReason, SystemEventWitness, WitnessOrder, WitnessRejection};

/// Return canonical event leaf bytes in the section order committed by `events_root`.
pub fn event_leaf_values(
    system_events: &[SystemEventWitness],
    orders: &[WitnessOrder],
    rejections: &[WitnessRejection],
    fills: &[Fill],
) -> Vec<Vec<u8>> {
    let mut events =
        Vec::with_capacity(system_events.len() + orders.len() + rejections.len() + fills.len());
    events.extend(system_events.iter().map(system_event_leaf_value));
    events.extend(orders.iter().map(order_accepted_leaf_value));
    events.extend(rejections.iter().map(order_rejected_leaf_value));
    events.extend(fills.iter().map(fill_leaf_value));
    events
}

pub fn system_event_leaf_value(event: &SystemEventWitness) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/system");
    match event {
        SystemEventWitness::CreateAccount {
            account_id,
            initial_balance,
        } => {
            value.push(0);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&initial_balance.to_le_bytes());
        }
        SystemEventWitness::Deposit { account_id, amount } => {
            value.push(1);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
        }
        SystemEventWitness::L1Deposit {
            account_id,
            amount,
            deposit_id,
            deposit_root,
            sybil_account_key,
        } => {
            value.push(2);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
            value.extend_from_slice(&deposit_id.to_le_bytes());
            value.extend_from_slice(deposit_root);
            value.extend_from_slice(sybil_account_key);
        }
        SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            withdrawal_id,
            recipient,
            token,
            amount_token_units,
            expiry_height,
            nullifier,
        } => {
            value.push(3);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
            value.extend_from_slice(&withdrawal_id.to_le_bytes());
            value.extend_from_slice(recipient);
            value.extend_from_slice(token);
            value.extend_from_slice(&amount_token_units.to_le_bytes());
            value.extend_from_slice(&expiry_height.to_le_bytes());
            value.extend_from_slice(nullifier);
        }
        SystemEventWitness::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => {
            value.push(4);
            value.extend_from_slice(&market_id.0.to_le_bytes());
            value.extend_from_slice(&payout_nanos.to_le_bytes());
            let mut affected_accounts = affected_accounts.clone();
            affected_accounts.sort_unstable();
            value.extend_from_slice(&(affected_accounts.len() as u64).to_le_bytes());
            for account_id in affected_accounts {
                value.extend_from_slice(&account_id.to_le_bytes());
            }
        }
    }
    value
}

pub fn order_accepted_leaf_value(event: &WitnessOrder) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/order-accepted");
    value.extend_from_slice(&event.account_id.to_le_bytes());
    value.push(u8::from(event.is_mm));
    append_order(&mut value, &event.order);
    value
}

pub fn order_rejected_leaf_value(event: &WitnessRejection) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/order-rejected");
    value.extend_from_slice(&event.account_id.to_le_bytes());
    append_order(&mut value, &event.order);
    append_rejection_reason(&mut value, &event.reason);
    value
}

pub fn fill_leaf_value(fill: &Fill) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/fill");
    value.extend_from_slice(&fill.order_id.to_le_bytes());
    value.extend_from_slice(&fill.fill_qty.to_le_bytes());
    value.extend_from_slice(&fill.fill_price.to_le_bytes());
    value.extend_from_slice(&fill.account_id.to_le_bytes());
    value
}

fn append_rejection_reason(value: &mut Vec<u8>, reason: &RejectionReason) {
    match reason {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => {
            value.push(0);
            value.extend_from_slice(&required.to_le_bytes());
            value.extend_from_slice(&available.to_le_bytes());
        }
        RejectionReason::InsufficientPosition {
            market,
            outcome,
            required,
            available,
        } => {
            value.push(1);
            value.extend_from_slice(&market.0.to_le_bytes());
            value.push(*outcome);
            value.extend_from_slice(&required.to_le_bytes());
            value.extend_from_slice(&available.to_le_bytes());
        }
        RejectionReason::AccountNotFound => value.push(2),
        RejectionReason::CompleteSetFormation => value.push(3),
        RejectionReason::Expired {
            current_block,
            expires_at_block,
        } => {
            value.push(4);
            value.extend_from_slice(&current_block.to_le_bytes());
            value.extend_from_slice(&expires_at_block.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SystemEventWitness;

    #[test]
    fn event_leaf_values_encode_deposit() {
        let system_events = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = event_leaf_values(&system_events, &[], &[], &[]);
        let mut expected = b"sybil/event/system".to_vec();
        expected.push(1);
        expected.extend_from_slice(&7u64.to_le_bytes());
        expected.extend_from_slice(&50u64.to_le_bytes());

        assert_eq!(events, vec![expected]);
    }
}
