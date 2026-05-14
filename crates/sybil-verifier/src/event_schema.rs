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
        SystemEventWitness::OrderCancelled {
            account_id,
            order_id,
            market_ids,
            side,
            remaining_quantity,
        } => {
            value.push(5);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&order_id.to_le_bytes());
            let mut market_ids = market_ids.clone();
            market_ids.sort_unstable();
            value.extend_from_slice(&(market_ids.len() as u64).to_le_bytes());
            for mid in market_ids {
                value.extend_from_slice(&mid.0.to_le_bytes());
            }
            value.push(side.to_byte());
            value.extend_from_slice(&remaining_quantity.to_le_bytes());
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
    use matching_engine::{MarketId, OrderDirection};

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

    /// Tag byte 5 is the next slot after `MarketResolved=4`. The verifier and
    /// the FE-facing API rely on this byte being stable. Changing it breaks
    /// historical `events_root` verification.
    #[test]
    fn order_cancelled_tag_byte_5() {
        let event = SystemEventWitness::OrderCancelled {
            account_id: 42,
            order_id: 1234,
            market_ids: vec![MarketId::new(7), MarketId::new(3)],
            side: OrderDirection::BuyNo,
            remaining_quantity: 9,
        };
        let leaf = system_event_leaf_value(&event);
        // Prefix is the literal "sybil/event/system" — 18 bytes.
        assert_eq!(&leaf[..18], b"sybil/event/system");
        // Followed by the variant tag.
        assert_eq!(leaf[18], 5, "OrderCancelled must be tag byte 5");
    }

    /// Full byte-by-byte encoding of an OrderCancelled leaf. If this breaks,
    /// it almost certainly means a verifier-incompatible encoding change —
    /// re-derive carefully before updating the expected bytes.
    #[test]
    fn order_cancelled_leaf_encoding_is_stable() {
        let event = SystemEventWitness::OrderCancelled {
            account_id: 42,
            order_id: 1234,
            market_ids: vec![MarketId::new(7), MarketId::new(3)],
            side: OrderDirection::BuyNo,
            remaining_quantity: 9,
        };
        let leaf = system_event_leaf_value(&event);

        let mut expected = b"sybil/event/system".to_vec();
        expected.push(5);
        expected.extend_from_slice(&42u64.to_le_bytes());
        expected.extend_from_slice(&1234u64.to_le_bytes());
        // Sorted: [3, 7]
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(&3u32.to_le_bytes());
        expected.extend_from_slice(&7u32.to_le_bytes());
        expected.push(OrderDirection::BuyNo.to_byte());
        expected.extend_from_slice(&9u64.to_le_bytes());

        assert_eq!(leaf, expected);
    }

}
