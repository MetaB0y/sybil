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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DepositAccumulatorWitness, StateSidecarSnapshot, WitnessBlockHeader};

    fn header(height: u64) -> WitnessBlockHeader {
        WitnessBlockHeader {
            height,
            parent_hash: [0; 32],
            state_root: [0; 32],
            events_root: crate::test_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
        }
    }

    fn entry(key: [u8; 32], amount: i64) -> QuarantineEntrySnapshot {
        QuarantineEntrySnapshot {
            sybil_account_key: key,
            amount,
        }
    }

    fn witness(
        pre: Vec<QuarantineEntrySnapshot>,
        events: Vec<SystemEventWitness>,
        post: Vec<QuarantineEntrySnapshot>,
    ) -> BlockWitness {
        let mut pre_state_sidecar = StateSidecarSnapshot::default();
        pre_state_sidecar.bridge.quarantine = pre;
        let mut state_sidecar = StateSidecarSnapshot::default();
        state_sidecar.bridge.quarantine = post;
        BlockWitness {
            header: header(2),
            previous_header: Some(header(1)),
            genesis_hash: [0; 32],
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: events,
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
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: Vec::new(),
        }
    }

    fn quarantined(key: [u8; 32], amount: i64) -> SystemEventWitness {
        SystemEventWitness::DepositQuarantined {
            amount,
            deposit_id: 1,
            deposit_root: [7; 32],
            sybil_account_key: key,
        }
    }

    fn claimed(account_id: u64, key: [u8; 32], amount: i64) -> SystemEventWitness {
        SystemEventWitness::QuarantineClaimed {
            account_id,
            amount,
            sybil_account_key: key,
        }
    }

    fn assert_invalid(witness: &BlockWitness, expected: &str) {
        let result = verify_quarantine_transition(witness);
        assert!(!result.valid);
        assert!(
            result
                .violations
                .iter()
                .any(|violation| violation.details.contains(expected)),
            "expected {expected:?}, got {:?}",
            result.violations
        );
    }

    #[test]
    fn ledger_digest_is_order_independent_and_field_sensitive() {
        let a = entry([1; 32], 10);
        let b = entry([2; 32], 20);
        let expected = quarantine_ledger_digest(&[a.clone(), b.clone()]);

        assert_eq!(expected, quarantine_ledger_digest(&[b.clone(), a.clone()]));
        assert_ne!(expected, [0; 32]);
        assert_ne!(expected, [1; 32]);
        assert_ne!(expected, quarantine_ledger_digest(std::slice::from_ref(&a)));
        assert_ne!(expected, quarantine_ledger_digest(&[a, entry([2; 32], 21)]));
        assert_ne!(expected, quarantine_ledger_digest(&[entry([3; 32], 10), b]));
    }

    #[test]
    fn ledger_map_requires_positive_unique_entries() {
        let key = [1; 32];
        assert_eq!(
            ledger_map(&[entry(key, 7)], "test").unwrap(),
            BTreeMap::from([(key, 7)])
        );
        assert!(ledger_map(&[entry(key, 0)], "test").is_err());
        assert!(ledger_map(&[entry(key, -1)], "test").is_err());
        assert!(ledger_map(&[entry(key, 1), entry(key, 2)], "test").is_err());
    }

    #[test]
    fn quarantine_requires_positive_checked_accumulation() {
        let key = [1; 32];
        let mut ledger = BTreeMap::new();
        quarantine(&mut ledger, key, 7).unwrap();
        quarantine(&mut ledger, key, 5).unwrap();
        assert_eq!(ledger.get(&key), Some(&12));
        assert!(quarantine(&mut ledger, key, 0).is_err());
        assert!(quarantine(&mut ledger, key, -1).is_err());

        ledger.insert(key, i64::MAX);
        assert!(quarantine(&mut ledger, key, 1).is_err());
    }

    #[test]
    fn claim_requires_account_key_presence_and_exact_amount() {
        let account_id = 7;
        let key = bridge_account_key(account_id);
        assert_ne!(key, [0; 32]);
        assert_ne!(key, [1; 32]);
        assert_ne!(key, bridge_account_key(account_id + 1));

        let mut valid = BTreeMap::from([(key, 10)]);
        claim(&mut valid, account_id, key, 10).unwrap();
        assert!(valid.is_empty());

        assert!(claim(&mut BTreeMap::new(), account_id, key, 10).is_err());
        assert!(claim(&mut BTreeMap::from([(key, 10)]), account_id + 1, key, 10).is_err());
        assert!(claim(&mut BTreeMap::from([(key, 10)]), account_id, key, 9).is_err());
    }

    #[test]
    fn transition_replays_quarantine_and_claim_events() {
        let account_id = 7;
        let key = bridge_account_key(account_id);

        let deposit = witness(Vec::new(), vec![quarantined(key, 10)], vec![entry(key, 10)]);
        assert!(verify_quarantine_transition(&deposit).valid);

        let claim = witness(
            vec![entry(key, 10)],
            vec![claimed(account_id, key, 10)],
            Vec::new(),
        );
        assert!(verify_quarantine_transition(&claim).valid);
    }

    #[test]
    fn transition_rejects_invalid_events_and_post_state() {
        let account_id = 7;
        let key = bridge_account_key(account_id);

        assert_invalid(
            &witness(Vec::new(), vec![quarantined(key, 0)], Vec::new()),
            "must be positive",
        );
        assert_invalid(
            &witness(
                vec![entry(key, 10)],
                vec![claimed(account_id, key, 9)],
                Vec::new(),
            ),
            "does not equal parked amount",
        );
        assert_invalid(
            &witness(Vec::new(), vec![quarantined(key, 10)], Vec::new()),
            "post quarantine ledger does not match",
        );
    }

    #[test]
    fn transition_rejects_invalid_snapshots_and_nonempty_genesis() {
        let key = bridge_account_key(7);

        assert_invalid(
            &witness(vec![entry(key, 0)], Vec::new(), Vec::new()),
            "pre quarantine entry has non-positive amount",
        );
        assert_invalid(
            &witness(vec![entry(key, 1), entry(key, 2)], Vec::new(), Vec::new()),
            "pre quarantine ledger contains a duplicate key",
        );
        assert_invalid(
            &witness(Vec::new(), Vec::new(), vec![entry(key, 0)]),
            "post quarantine entry has non-positive amount",
        );

        let mut genesis = witness(vec![entry(key, 1)], Vec::new(), vec![entry(key, 1)]);
        genesis.previous_header = None;
        assert_invalid(
            &genesis,
            "genesis pre-state quarantine ledger must be empty",
        );
    }
}
