//! Replay account-value effects of system events from authenticated pre-state.

use std::collections::{BTreeMap, BTreeSet};

use matching_engine::{notional_nanos, Nanos, Qty, NANOS_PER_DOLLAR};

use crate::types::{AccountSnapshot, BlockWitness, SystemEventWitness};
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
        if let Err(details) = apply_event(&mut accounts, event) {
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
) -> Result<(), String> {
    match event {
        SystemEventWitness::CreateAccount {
            account_id,
            initial_balance,
        } => {
            if accounts.contains_key(account_id) {
                return Err(format!("CreateAccount duplicated account {account_id}"));
            }
            accounts.insert(
                *account_id,
                AccountSnapshot {
                    id: *account_id,
                    balance: *initial_balance,
                    total_deposited: *initial_balance,
                    positions: Vec::new(),
                    events_digest: [0; 32],
                    keys_digest: crate::empty_account_keys_digest(*account_id),
                },
            );
        }
        SystemEventWitness::Deposit { account_id, amount }
        | SystemEventWitness::L1Deposit {
            account_id, amount, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
            account.total_deposited = checked_add(account.total_deposited, *amount, *account_id)?;
        }
        SystemEventWitness::WithdrawalCreated {
            account_id, amount, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = account
                .balance
                .checked_sub(*amount)
                .ok_or_else(|| format!("account {account_id} withdrawal debit overflowed"))?;
        }
        SystemEventWitness::WithdrawalRefunded {
            account_id, amount, ..
        } => {
            let account = account_mut(accounts, *account_id)?;
            account.balance = checked_add(account.balance, *amount, *account_id)?;
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
            }
        }
        SystemEventWitness::WithdrawalFinalized { .. }
        | SystemEventWitness::L1BlockObserved { .. }
        | SystemEventWitness::OrderCancelled { .. }
        | SystemEventWitness::MarketGroupExtended { .. } => {}
    }
    Ok(())
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
