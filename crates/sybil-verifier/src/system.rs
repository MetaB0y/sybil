//! Replay account-value effects of system events from authenticated pre-state.

use std::collections::{BTreeMap, BTreeSet};

use matching_engine::{
    MarketId, NANOS_PER_DOLLAR, Nanos, OrderDirection, Qty, collateralize_complete_set,
    notional_nanos, redeem_complete_set,
};

use crate::types::{
    AccountSnapshot, BlockWitness, KeyRecord, SystemEventWitness, WithdrawalRefundReasonWitness,
};
use crate::violations::{VerificationResult, Violation, ViolationKind};

pub fn verify_system_transition(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let mut accounts: BTreeMap<u64, AccountSnapshot> = witness
        .pre_state
        .iter()
        .cloned()
        .map(|account| (account.id, account))
        .collect();

    for event in &witness.system_events {
        if let Err(details) = apply_event(&mut accounts, event, witness.header.height) {
            violations.push(Violation {
                kind: ViolationKind::SystemStateMismatch,
                details,
            });
        }
    }

    let claimed: BTreeMap<u64, &AccountSnapshot> = witness
        .post_system_state
        .iter()
        .map(|account| (account.id, account))
        .collect();
    let ids: BTreeSet<u64> = accounts.keys().chain(claimed.keys()).copied().collect();
    for account_id in ids {
        match (accounts.get(&account_id), claimed.get(&account_id)) {
            (Some(expected), Some(actual)) => {
                if expected.balance != actual.balance
                    || expected.total_deposited != actual.total_deposited
                    || normalized_positions(expected) != normalized_positions(actual)
                    || expected.events_digest != actual.events_digest
                    || expected.last_trading_nonce != actual.last_trading_nonce
                {
                    violations.push(Violation {
                        kind: ViolationKind::SystemStateMismatch,
                        details: format!(
                            "account {account_id} post-system value state does not match system-event replay"
                        ),
                    });
                }
            }
            (Some(_), None) => violations.push(Violation {
                kind: ViolationKind::SystemStateMismatch,
                details: format!("account {account_id} missing from post-system state"),
            }),
            (None, Some(_)) => violations.push(Violation {
                kind: ViolationKind::SystemStateMismatch,
                details: format!("unexpected account {account_id} in post-system state"),
            }),
            (None, None) => unreachable!(),
        }
    }

    VerificationResult::from_violations(violations)
}

fn apply_event(
    accounts: &mut BTreeMap<u64, AccountSnapshot>,
    event: &SystemEventWitness,
    block_height: u64,
) -> Result<(), String> {
    match event {
        SystemEventWitness::CreateAccount {
            account_id,
            initial_balance,
            initial_keys,
        } => {
            if accounts.contains_key(account_id) {
                return Err(format!("CreateAccount duplicated account {account_id}"));
            }
            let encoded = encode_create_account_event(*initial_balance, block_height);
            accounts.insert(
                *account_id,
                AccountSnapshot {
                    id: *account_id,
                    balance: *initial_balance,
                    total_deposited: *initial_balance,
                    positions: Vec::new(),
                    events_digest: update_digest(&[0; 32], &encoded),
                    keys_digest: crate::account_keys_digest(
                        *account_id,
                        initial_keys.iter().copied(),
                    ),
                    last_trading_nonce: 0,
                },
            );
        }
        SystemEventWitness::Deposit { account_id, amount } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
            account.total_deposited = checked_add(account.total_deposited, *amount, *account_id)?;
            let encoded = encode_deposit_event(*amount, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::L1Deposit {
            account_id,
            amount,
            deposit_id,
            deposit_root,
            ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
            account.total_deposited = checked_add(account.total_deposited, *amount, *account_id)?;
            let encoded = encode_l1_deposit_event(*deposit_id, *amount, deposit_root, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            withdrawal_id,
            nullifier,
            ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = account
                .balance
                .checked_sub(*amount)
                .ok_or_else(|| format!("account {account_id} withdrawal debit overflowed"))?;
            let encoded =
                encode_withdrawal_created_event(*withdrawal_id, *amount, nullifier, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::WithdrawalRefunded {
            account_id,
            withdrawal_id,
            amount,
            reason,
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
            let encoded =
                encode_withdrawal_refunded_event(*withdrawal_id, *amount, reason, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => {
            let no_payout = Nanos(NANOS_PER_DOLLAR.saturating_sub(payout_nanos.0));
            for account_id in affected_accounts {
                let account = account_mut(accounts, *account_id)?;
                let yes = remove_position(account, *market_id, 0);
                let no = remove_position(account, *market_id, 1);
                let yes_value = signed_notional(*payout_nanos, yes)?;
                let no_value = signed_notional(no_payout, no)?;
                account.balance = checked_add(account.balance, yes_value, *account_id)?;
                account.balance = checked_add(account.balance, no_value, *account_id)?;
                let encoded = encode_resolution_event(*market_id, *payout_nanos, block_height);
                account.events_digest = update_digest(&account.events_digest, &encoded);
            }
        }
        SystemEventWitness::OrderCancelled {
            account_id,
            order_id,
            market_ids,
            side,
            remaining_quantity,
        } => {
            let account = account_mut(accounts, *account_id)?;
            let encoded = encode_order_cancelled_event(
                *order_id,
                market_ids,
                *side,
                *remaining_quantity,
                block_height,
            );
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::KeyRegistered {
            account_id, key, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            let encoded = encode_key_event(0x0a, key, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::KeyRevoked {
            account_id, key, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            let encoded = encode_key_event(0x0b, key, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::QuarantineClaimed {
            account_id, amount, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
            account.total_deposited = checked_add(account.total_deposited, *amount, *account_id)?;
            let encoded = encode_quarantine_claimed_event(*amount, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::CompleteSetCollateralized {
            account_id,
            market_id,
            quantity,
        }
        | SystemEventWitness::CompleteSetRedeemed {
            account_id,
            market_id,
            quantity,
        } => {
            if *quantity == 0 {
                return Err("complete-set quantity must be positive".to_string());
            }
            let collateralize =
                matches!(event, SystemEventWitness::CompleteSetCollateralized { .. });
            let delta = if collateralize {
                collateralize_complete_set(Qty(*quantity))
            } else {
                redeem_complete_set(Qty(*quantity))
            }
            .map_err(|_| "complete-set arithmetic overflowed".to_string())?;
            let account = account_mut(accounts, *account_id)?;
            if collateralize && account.balance < -delta.balance_delta {
                return Err(format!(
                    "account {account_id} lacks cash for complete-set collateralization"
                ));
            }
            if !collateralize {
                let required = i64::try_from(*quantity)
                    .map_err(|_| "complete-set quantity exceeds i64".to_string())?;
                let yes = position(account, *market_id, 0);
                let no = position(account, *market_id, 1);
                if yes < required || no < required {
                    return Err(format!("account {account_id} lacks complete-set inventory"));
                }
            }
            account.balance = checked_add(account.balance, delta.balance_delta, *account_id)?;
            add_position(account, *market_id, 0, delta.yes_delta)?;
            add_position(account, *market_id, 1, delta.no_delta)?;
            let encoded =
                encode_complete_set_event(collateralize, *market_id, *quantity, block_height);
            account.events_digest = update_digest(&account.events_digest, &encoded);
        }
        SystemEventWitness::DepositQuarantined { .. } => {}
        SystemEventWitness::ClientActionAuthorized(action) => {
            let (account_id, nonce) = match action {
                crate::ClientActionWitness::Order {
                    account_id, nonce, ..
                }
                | crate::ClientActionWitness::Cancel {
                    account_id, nonce, ..
                } => (*account_id, *nonce),
            };
            let account = account_mut(accounts, account_id)?;
            if nonce <= account.last_trading_nonce {
                return Err(format!(
                    "account {account_id} client-action nonce {nonce} is not above prior trading nonce {}",
                    account.last_trading_nonce
                ));
            }
            account.last_trading_nonce = nonce;
        }
        SystemEventWitness::WithdrawalFinalized { .. }
        | SystemEventWitness::L1BlockObserved { .. }
        | SystemEventWitness::MarketGroupExtended { .. }
        | SystemEventWitness::LiquidityUniverseActivated { .. } => {}
    }
    Ok(())
}

fn position(account: &AccountSnapshot, market_id: MarketId, outcome: u8) -> i64 {
    account
        .positions
        .iter()
        .find(|(market, side, _)| *market == market_id && *side == outcome)
        .map(|(_, _, quantity)| *quantity)
        .unwrap_or(0)
}

fn add_position(
    account: &mut AccountSnapshot,
    market_id: MarketId,
    outcome: u8,
    delta: i64,
) -> Result<(), String> {
    if let Some((_, _, quantity)) = account
        .positions
        .iter_mut()
        .find(|(market, side, _)| *market == market_id && *side == outcome)
    {
        *quantity = quantity
            .checked_add(delta)
            .ok_or_else(|| "complete-set position overflowed".to_string())?;
    } else if delta != 0 {
        account.positions.push((market_id, outcome, delta));
    }
    account.positions.retain(|(_, _, quantity)| *quantity != 0);
    account
        .positions
        .sort_by_key(|(market, side, _)| (market.0, *side));
    Ok(())
}

fn encode_complete_set_event(
    collateralize: bool,
    market_id: MarketId,
    quantity: u64,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(if collateralize { 0x0d } else { 0x0e }, 21);
    bytes.extend_from_slice(&market_id.0.to_le_bytes());
    bytes.extend_from_slice(&quantity.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_quarantine_claimed_event(amount: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x0c, 17);
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn account_mut(
    accounts: &mut BTreeMap<u64, AccountSnapshot>,
    account_id: u64,
) -> Result<&mut AccountSnapshot, String> {
    accounts
        .get_mut(&account_id)
        .ok_or_else(|| format!("system event references missing account {account_id}"))
}

fn checked_add(value: i64, delta: i64, account_id: u64) -> Result<i64, String> {
    value
        .checked_add(delta)
        .ok_or_else(|| format!("account {account_id} system balance arithmetic overflowed"))
}

pub(crate) fn update_digest(current: &[u8; 32], event_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(current);
    hasher.update(event_bytes);
    *hasher.finalize().as_bytes()
}

fn encode_fill_prefix(tag: u8, capacity: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.push(tag);
    bytes
}

pub(crate) fn encode_create_account_event(initial_balance: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x04, 17);
    bytes.extend_from_slice(&initial_balance.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_deposit_event(amount: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x02, 17);
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_l1_deposit_event(
    deposit_id: u64,
    amount: i64,
    deposit_root: &[u8; 32],
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x06, 57);
    bytes.extend_from_slice(&deposit_id.to_le_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(deposit_root);
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_withdrawal_created_event(
    withdrawal_id: u64,
    amount: i64,
    nullifier: &[u8; 32],
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x07, 57);
    bytes.extend_from_slice(&withdrawal_id.to_le_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(nullifier);
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_withdrawal_refunded_event(
    withdrawal_id: u64,
    amount: i64,
    reason: &WithdrawalRefundReasonWitness,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x09, 34);
    bytes.extend_from_slice(&withdrawal_id.to_le_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    match reason {
        WithdrawalRefundReasonWitness::L1Cancelled => bytes.push(0),
        WithdrawalRefundReasonWitness::L1Expired { observed_l1_height } => {
            bytes.push(1);
            bytes.extend_from_slice(&observed_l1_height.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_resolution_event(market_id: MarketId, payout_nanos: Nanos, block_height: u64) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x03, 21);
    bytes.extend_from_slice(&market_id.0.to_le_bytes());
    bytes.extend_from_slice(&payout_nanos.0.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn encode_order_cancelled_event(
    order_id: u64,
    market_ids: &[MarketId],
    side: OrderDirection,
    remaining_quantity: u64,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(0x08, 34 + market_ids.len() * 4);
    bytes.extend_from_slice(&order_id.to_le_bytes());
    bytes.extend_from_slice(&(market_ids.len() as u64).to_le_bytes());
    for market_id in market_ids {
        bytes.extend_from_slice(&market_id.0.to_le_bytes());
    }
    bytes.push(side.to_byte());
    bytes.extend_from_slice(&remaining_quantity.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub(crate) fn encode_key_event(tag: u8, key: &KeyRecord, block_height: u64) -> Vec<u8> {
    let mut bytes = encode_fill_prefix(tag, 47);
    bytes.push(key.auth_scheme);
    bytes.extend_from_slice(&key.pubkey_sec1);
    bytes.extend_from_slice(&key.capability_mask.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

fn remove_position(
    account: &mut AccountSnapshot,
    market: matching_engine::MarketId,
    outcome: u8,
) -> i64 {
    let mut quantity = 0;
    account
        .positions
        .retain(|(position_market, position_outcome, position_quantity)| {
            if *position_market == market && *position_outcome == outcome {
                quantity = *position_quantity;
                false
            } else {
                true
            }
        });
    quantity
}

fn signed_notional(price: Nanos, quantity: i64) -> Result<i64, String> {
    let nanos = notional_nanos(price, Qty(quantity.unsigned_abs())).0;
    let signed = i64::try_from(nanos).map_err(|_| "resolution notional exceeds i64".to_string())?;
    signed
        .checked_mul(quantity.signum())
        .ok_or_else(|| "resolution signed notional overflowed".to_string())
}

fn normalized_positions(
    account: &AccountSnapshot,
) -> BTreeMap<(matching_engine::MarketId, u8), i64> {
    account
        .positions
        .iter()
        .filter(|(_, _, quantity)| *quantity != 0)
        .map(|(market, outcome, quantity)| ((*market, *outcome), *quantity))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientActionWitness, KeyOpAuth};

    fn account(last_trading_nonce: u64) -> AccountSnapshot {
        AccountSnapshot {
            id: 7,
            balance: 0,
            total_deposited: 0,
            positions: Vec::new(),
            events_digest: [0; 32],
            keys_digest: [0; 32],
            last_trading_nonce,
        }
    }

    fn client_action(nonce: u64) -> SystemEventWitness {
        SystemEventWitness::ClientActionAuthorized(ClientActionWitness::Order {
            account_id: 7,
            order: matching_engine::Order::new(1),
            nonce,
            authorization: KeyOpAuth::RawP256 {
                signer_pubkey: [2; 33],
                signature: [0; 64],
            },
        })
    }

    #[test]
    fn client_action_nonce_allows_gaps_and_rejects_same_block_replay() {
        let mut accounts = BTreeMap::from([(7, account(0))]);
        apply_event(&mut accounts, &client_action(10), 1).unwrap();
        assert_eq!(accounts[&7].last_trading_nonce, 10);
        assert!(apply_event(&mut accounts, &client_action(9), 1).is_err());
        assert!(apply_event(&mut accounts, &client_action(10), 1).is_err());
    }

    #[test]
    fn client_action_nonce_replay_fails_against_prior_block_state() {
        let mut accounts = BTreeMap::from([(7, account(10))]);
        assert!(apply_event(&mut accounts, &client_action(10), 2).is_err());
        assert_eq!(accounts[&7].last_trading_nonce, 10);
    }
}
