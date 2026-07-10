//! Canonical commitment and replay for the system deposit-quarantine ledger.

use std::collections::BTreeMap;

use sha2::{Digest as _, Sha256};

use crate::{
    BlockWitness, QuarantineEntrySnapshot, SystemEventWitness, VerificationResult, Violation,
    ViolationKind,
};

pub const QUARANTINE_LEDGER_DIGEST_DOMAIN: &[u8] = b"sybil/state/deposit-quarantine-digest/v1";

/// Digest the complete logical ledger. Entries must be unique, positive, and
/// are sorted here so host collection order cannot affect the commitment.
pub fn quarantine_ledger_digest(entries: &[QuarantineEntrySnapshot]) -> [u8; 32] {
    let mut entries = entries.to_vec();
    entries.sort_by_key(|entry| entry.sybil_account_key);
    let mut hasher = Sha256::new();
    hasher.update(QUARANTINE_LEDGER_DIGEST_DOMAIN);
    hasher.update((entries.len() as u64).to_le_bytes());
    for entry in entries {
        hasher.update(entry.sybil_account_key);
        hasher.update(entry.amount.to_le_bytes());
    }
    hasher.finalize().into()
}

pub fn verify_quarantine_transition(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let mut ledger = match ledger_map(&witness.pre_state_sidecar.bridge.quarantine, "pre") {
        Ok(ledger) => ledger,
        Err(details) => {
            violations.push(violation(details));
            BTreeMap::new()
        }
    };

    if witness.previous_header.is_none() && !ledger.is_empty() {
        violations.push(violation(
            "genesis pre-state quarantine ledger must be empty".to_string(),
        ));
    }

    for event in &witness.system_events {
        let result = match event {
            SystemEventWitness::DepositQuarantined {
                amount,
                sybil_account_key,
                ..
            } => quarantine(&mut ledger, *sybil_account_key, *amount),
            SystemEventWitness::QuarantineClaimed {
                account_id,
                amount,
                sybil_account_key,
            } => claim(&mut ledger, *account_id, *sybil_account_key, *amount),
            _ => Ok(()),
        };
        if let Err(details) = result {
            violations.push(violation(details));
        }
    }

    match ledger_map(&witness.state_sidecar.bridge.quarantine, "post") {
        Ok(claimed) if claimed != ledger => violations.push(violation(
            "post quarantine ledger does not match witnessed quarantine/claim replay".to_string(),
        )),
        Ok(_) => {}
        Err(details) => violations.push(violation(details)),
    }

    VerificationResult::from_violations(violations)
}

fn ledger_map(
    entries: &[QuarantineEntrySnapshot],
    label: &str,
) -> Result<BTreeMap<[u8; 32], i64>, String> {
    let mut ledger = BTreeMap::new();
    for entry in entries {
        if entry.amount <= 0 {
            return Err(format!("{label} quarantine entry has non-positive amount"));
        }
        if ledger
            .insert(entry.sybil_account_key, entry.amount)
            .is_some()
        {
            return Err(format!(
                "{label} quarantine ledger contains a duplicate key"
            ));
        }
    }
    Ok(ledger)
}

fn quarantine(
    ledger: &mut BTreeMap<[u8; 32], i64>,
    key: [u8; 32],
    amount: i64,
) -> Result<(), String> {
    if amount <= 0 {
        return Err("quarantined deposit amount must be positive".to_string());
    }
    let entry = ledger.entry(key).or_default();
    *entry = entry
        .checked_add(amount)
        .ok_or_else(|| "quarantine accumulation overflowed".to_string())?;
    Ok(())
}

fn claim(
    ledger: &mut BTreeMap<[u8; 32], i64>,
    account_id: u64,
    key: [u8; 32],
    amount: i64,
) -> Result<(), String> {
    if bridge_account_key(account_id) != key {
        return Err(format!(
            "quarantine claim key does not match committed account {account_id} bridge key"
        ));
    }
    let Some(parked) = ledger.remove(&key) else {
        return Err("quarantine claim references an absent entry (double claim)".to_string());
    };
    if parked != amount {
        return Err(format!(
            "quarantine claim amount {amount} does not equal parked amount {parked}"
        ));
    }
    Ok(())
}

pub fn bridge_account_key(account_id: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/bridge/account-key/v1");
    hasher.update(&account_id.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn violation(details: String) -> Violation {
    Violation {
        kind: ViolationKind::SidecarDepositCursorMismatch,
        details,
    }
}
