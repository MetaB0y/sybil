use matching_engine::{MarketId, MintAdjustment, Nanos, OrderDirection, Qty};
use std::collections::HashMap;

use crate::account::{AccountId, AccountStore};
use crate::crypto::{PublicKey, RegisteredPubkey};

pub fn update_digest(current: &[u8; 32], event_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(current);
    hasher.update(event_bytes);
    *hasher.finalize().as_bytes()
}

pub fn account_keys_digest(
    account_id: AccountId,
    pubkey_registry: &HashMap<PublicKey, RegisteredPubkey>,
) -> [u8; 32] {
    let keys = pubkey_registry
        .iter()
        .filter(|(_, registered)| registered.account_id == account_id)
        .map(|(pubkey, registered)| {
            let compressed = pubkey.compressed_bytes();
            let mut pubkey_sec1 = [0u8; 33];
            pubkey_sec1.copy_from_slice(&compressed);
            sybil_verifier::AccountKeyDigestRecord {
                auth_scheme: registered.auth_scheme.canonical_byte(),
                pubkey_sec1,
            }
        });

    sybil_verifier::account_keys_digest(account_id.0, keys)
}

pub fn refresh_account_keys_digest(
    accounts: &mut AccountStore,
    account_id: AccountId,
    pubkey_registry: &HashMap<PublicKey, RegisteredPubkey>,
) {
    let keys_digest = account_keys_digest(account_id, pubkey_registry);
    if let Some(account) = accounts.get_mut(account_id) {
        account.keys_digest = keys_digest;
    }
}

pub fn refresh_all_account_keys_digests(
    accounts: &mut AccountStore,
    pubkey_registry: &HashMap<PublicKey, RegisteredPubkey>,
) {
    let account_ids: Vec<AccountId> = accounts.iter().map(|(account_id, _)| *account_id).collect();
    for account_id in account_ids {
        refresh_account_keys_digest(accounts, account_id, pubkey_registry);
    }
}

pub fn encode_fill_event(
    order_id: u64,
    fill_qty: Qty,
    fill_price: Nanos,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 * 4);
    bytes.push(0x01);
    bytes.extend_from_slice(&order_id.to_le_bytes());
    bytes.extend_from_slice(&fill_qty.0.to_le_bytes());
    bytes.extend_from_slice(&fill_price.0.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_deposit_event(amount: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8);
    bytes.push(0x02);
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_l1_deposit_event(
    deposit_id: u64,
    amount: i64,
    deposit_root: &[u8; 32],
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8 + 32 + 8);
    bytes.push(0x06);
    bytes.extend_from_slice(&deposit_id.to_le_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(deposit_root);
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_withdrawal_created_event(
    withdrawal_id: u64,
    amount: i64,
    nullifier: &[u8; 32],
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8 + 32 + 8);
    bytes.push(0x07);
    bytes.extend_from_slice(&withdrawal_id.to_le_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(nullifier);
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_resolution_event(
    market_id: MarketId,
    payout_nanos: Nanos,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 4 + 8 + 8);
    bytes.push(0x03);
    bytes.extend_from_slice(&market_id.0.to_le_bytes());
    bytes.extend_from_slice(&payout_nanos.0.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_create_account_event(initial_balance: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8);
    bytes.push(0x04);
    bytes.extend_from_slice(&initial_balance.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_order_cancelled_event(
    order_id: u64,
    market_ids: &[MarketId],
    side: OrderDirection,
    remaining_quantity: u64,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8 + market_ids.len() * 4 + 1 + 8 + 8);
    bytes.push(0x08);
    bytes.extend_from_slice(&order_id.to_le_bytes());
    bytes.extend_from_slice(&(market_ids.len() as u64).to_le_bytes());
    for mid in market_ids {
        bytes.extend_from_slice(&mid.0.to_le_bytes());
    }
    bytes.push(side.to_byte());
    bytes.extend_from_slice(&remaining_quantity.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_mint_event(adjustments: &[MintAdjustment], block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + adjustments.len() * (4 + 1 + 8 + 8));
    bytes.push(0x05);
    bytes.extend_from_slice(&(adjustments.len() as u64).to_le_bytes());
    for adjustment in adjustments {
        bytes.extend_from_slice(&adjustment.market_id.0.to_le_bytes());
        bytes.push(adjustment.outcome);
        bytes.extend_from_slice(&adjustment.position_delta.to_le_bytes());
        bytes.extend_from_slice(&adjustment.balance_delta.to_le_bytes());
    }
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digest_deterministic() {
        let event = encode_fill_event(7, Qty(10), Nanos(500_000_000), 12);
        assert_eq!(
            update_digest(&[0u8; 32], &event),
            update_digest(&[0u8; 32], &event)
        );
    }

    #[test]
    fn test_digest_sensitive_to_event_bytes() {
        let fill = encode_fill_event(7, Qty(10), Nanos(500_000_000), 12);
        let deposit = encode_deposit_event(500_000_000, 12);
        assert_ne!(
            update_digest(&[0u8; 32], &fill),
            update_digest(&[0u8; 32], &deposit)
        );
    }

    /// Tag byte 0x08 is the next slot after `withdrawal_created=0x07`. The
    /// per-account `events_digest` commits this byte; changing it would
    /// retroactively diverge every account's history.
    #[test]
    fn order_cancelled_event_uses_tag_0x08() {
        let bytes =
            encode_order_cancelled_event(1234, &[MarketId::new(3)], OrderDirection::BuyYes, 5, 42);
        assert_eq!(bytes[0], 0x08);
    }

    #[test]
    fn order_cancelled_event_encoding_is_stable() {
        let bytes = encode_order_cancelled_event(
            1234,
            &[MarketId::new(3), MarketId::new(7)],
            OrderDirection::SellNo,
            9,
            100,
        );
        let mut expected: Vec<u8> = Vec::new();
        expected.push(0x08);
        expected.extend_from_slice(&1234u64.to_le_bytes());
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(&3u32.to_le_bytes());
        expected.extend_from_slice(&7u32.to_le_bytes());
        expected.push(OrderDirection::SellNo.to_byte());
        expected.extend_from_slice(&9u64.to_le_bytes());
        expected.extend_from_slice(&100u64.to_le_bytes());
        assert_eq!(bytes, expected);
    }
}
