//! Proven signing-key set transitions.

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{AccountSnapshot, BlockWitness, KeyRecord, SystemEventWitness};
use crate::violations::{VerificationResult, Violation, ViolationKind};

pub fn verify_key_transitions(witness: &BlockWitness) -> VerificationResult {
    match verify(witness) {
        Ok(()) => VerificationResult::from_violations(Vec::new()),
        Err(details) => VerificationResult::from_violations(vec![Violation {
            kind: ViolationKind::KeyTransitionMismatch,
            details,
        }]),
    }
}

fn verify(witness: &BlockWitness) -> Result<(), String> {
    let pre_accounts = account_map(&witness.pre_state, "pre_state")?;
    let post_system_accounts = account_map(&witness.post_system_state, "post_system_state")?;
    let post_accounts = account_map(&witness.post_state, "post_state")?;
    let mut post_keys = post_key_map(witness, &post_accounts)?;

    weld_post_keys(&post_keys, &post_system_accounts, "post_system_state")?;
    weld_post_keys(&post_keys, &post_accounts, "post_state")?;
    ensure_global_pubkey_uniqueness(&post_keys, "post key universe")?;

    let key_op_count = witness
        .system_events
        .iter()
        .filter(|event| {
            matches!(
                event,
                SystemEventWitness::KeyRegistered { .. } | SystemEventWitness::KeyRevoked { .. }
            )
        })
        .count();
    if key_op_count > crate::MAX_KEY_OPS_PER_BLOCK {
        return Err(format!(
            "block has {key_op_count} key ops, exceeding MAX_KEY_OPS_PER_BLOCK={}",
            crate::MAX_KEY_OPS_PER_BLOCK
        ));
    }
    verify_key_event_order(&witness.system_events)?;

    let touched = key_touched_accounts(&witness.system_events);

    // Reverse the witnessed mutations over the authenticated post universe to
    // recover the block-start universe, including register/revoke cycles.
    for event in witness.system_events.iter().rev() {
        match event {
            SystemEventWitness::KeyRegistered {
                account_id, key, ..
            } => {
                let keys = post_keys.get_mut(account_id).ok_or_else(|| {
                    format!("KeyRegistered account {account_id} has no opened post-state leaf")
                })?;
                if !keys.remove(key) {
                    return Err(format!(
                        "reverse KeyRegistered for account {account_id} did not find the registered key"
                    ));
                }
            }
            SystemEventWitness::KeyRevoked {
                account_id, key, ..
            } => {
                let keys = post_keys.get_mut(account_id).ok_or_else(|| {
                    format!("KeyRevoked account {account_id} has no opened post-state leaf")
                })?;
                if !keys.insert(*key) {
                    return Err(format!(
                        "reverse KeyRevoked for account {account_id} found the key still active"
                    ));
                }
            }
            SystemEventWitness::CreateAccount {
                account_id,
                initial_keys,
                ..
            } => {
                validate_key_list(*account_id, initial_keys)?;
                let expected: BTreeSet<_> = initial_keys.iter().copied().collect();
                let actual = post_keys.get(account_id).ok_or_else(|| {
                    format!("CreateAccount {account_id} has no opened post-state leaf")
                })?;
                if actual != &expected {
                    return Err(format!(
                        "reverse CreateAccount {account_id} did not recover its initial key set"
                    ));
                }
                post_keys.remove(account_id);
            }
            _ => {}
        }
    }

    ensure_global_pubkey_uniqueness(&post_keys, "recovered pre key universe")?;
    for account_id in &touched {
        let created = witness.system_events.iter().any(|event| {
            matches!(event, SystemEventWitness::CreateAccount { account_id: id, .. } if id == account_id)
        });
        if created {
            if pre_accounts.contains_key(account_id) {
                return Err(format!(
                    "CreateAccount {account_id} already exists in authenticated pre-state"
                ));
            }
            continue;
        }
        let pre = pre_accounts.get(account_id).ok_or_else(|| {
            format!("key event account {account_id} has no opened pre-state leaf")
        })?;
        let keys = post_keys.get(account_id).ok_or_else(|| {
            format!("key event account {account_id} is missing from recovered pre keys")
        })?;
        let digest = crate::account_keys_digest(*account_id, keys.iter().copied());
        if pre.keys_digest != digest {
            return Err(format!(
                "account {account_id} recovered pre key set does not match authenticated pre keys_digest"
            ));
        }
    }

    for (account_id, post) in &post_accounts {
        if touched.contains(account_id) {
            continue;
        }
        let pre = pre_accounts.get(account_id).ok_or_else(|| {
            format!("untouched account {account_id} is absent from authenticated pre-state")
        })?;
        if pre.keys_digest != post.keys_digest {
            return Err(format!(
                "account {account_id} changed keys_digest without a witnessed key event"
            ));
        }
    }

    // Forward replay is the semantic check. The same running maps enforce
    // uniqueness for CreateAccount.initial_keys and KeyRegistered.
    let mut running = post_keys;
    let mut pubkeys = pubkey_owner_map(&running)?;
    let mut running_events: BTreeMap<u64, [u8; 32]> = pre_accounts
        .iter()
        .map(|(account_id, account)| (*account_id, account.events_digest))
        .collect();
    for event in &witness.system_events {
        match event {
            SystemEventWitness::CreateAccount {
                account_id,
                initial_balance,
                initial_keys,
                ..
            } => {
                if running.contains_key(account_id) {
                    return Err(format!("CreateAccount duplicated account {account_id}"));
                }
                validate_key_list(*account_id, initial_keys)?;
                let mut keys = BTreeSet::new();
                for key in initial_keys {
                    insert_globally_unique_key(*account_id, *key, &mut keys, &mut pubkeys)?;
                }
                running.insert(*account_id, keys);
                let encoded = crate::system::encode_create_account_event(
                    *initial_balance,
                    witness.header.height,
                );
                running_events.insert(
                    *account_id,
                    crate::system::update_digest(&[0; 32], &encoded),
                );
            }
            SystemEventWitness::KeyRegistered {
                account_id,
                key,
                authorization,
            } => {
                validate_key_record(key)?;
                let keys = running.get_mut(account_id).ok_or_else(|| {
                    format!("KeyRegistered account {account_id} has no running account")
                })?;
                let events_digest = *running_events.get(account_id).ok_or_else(|| {
                    format!("KeyRegistered account {account_id} has no running events digest")
                })?;
                let keys_digest = crate::account_keys_digest(*account_id, keys.iter().copied());
                let canonical = crate::canonical_key_registration_bytes(
                    witness.genesis_hash,
                    *account_id,
                    key,
                    keys_digest,
                    events_digest,
                );
                crate::verify_keyop_auth(authorization, keys.iter(), &canonical)
                    .map_err(|details| format!("account {account_id} registration: {details}"))?;
                if keys.len() >= crate::MAX_KEYS_PER_ACCOUNT {
                    return Err(format!(
                        "account {account_id} exceeds MAX_KEYS_PER_ACCOUNT={} on registration",
                        crate::MAX_KEYS_PER_ACCOUNT
                    ));
                }
                insert_globally_unique_key(*account_id, *key, keys, &mut pubkeys)?;
                let encoded = crate::system::encode_key_event(0x0a, key, witness.header.height);
                running_events.insert(
                    *account_id,
                    crate::system::update_digest(&events_digest, &encoded),
                );
            }
            SystemEventWitness::KeyRevoked {
                account_id,
                key,
                authorization,
            } => {
                validate_key_record(key)?;
                let keys = running.get_mut(account_id).ok_or_else(|| {
                    format!("KeyRevoked account {account_id} has no running account")
                })?;
                let events_digest = *running_events.get(account_id).ok_or_else(|| {
                    format!("KeyRevoked account {account_id} has no running events digest")
                })?;
                let keys_digest = crate::account_keys_digest(*account_id, keys.iter().copied());
                let canonical = crate::canonical_key_revocation_bytes(
                    witness.genesis_hash,
                    *account_id,
                    key,
                    keys_digest,
                    events_digest,
                );
                crate::verify_keyop_auth(authorization, keys.iter(), &canonical)
                    .map_err(|details| format!("account {account_id} revocation: {details}"))?;
                if keys.len() <= 1 {
                    return Err(format!(
                        "KeyRevoked would remove account {account_id}'s last active key"
                    ));
                }
                if !keys.remove(key) {
                    return Err(format!(
                        "KeyRevoked target is not active on account {account_id}"
                    ));
                }
                pubkeys.remove(&key.pubkey_sec1);
                let encoded = crate::system::encode_key_event(0x0b, key, witness.header.height);
                running_events.insert(
                    *account_id,
                    crate::system::update_digest(&events_digest, &encoded),
                );
            }
            _ => {}
        }
    }

    let expected_post = normalized_post_key_map(witness, &post_accounts)?;
    if running != expected_post {
        return Err("forward key-event replay does not produce witness.account_keys".to_string());
    }

    Ok(())
}

fn account_map<'a>(
    accounts: &'a [AccountSnapshot],
    section: &str,
) -> Result<BTreeMap<u64, &'a AccountSnapshot>, String> {
    let mut out = BTreeMap::new();
    for account in accounts {
        if out.insert(account.id, account).is_some() {
            return Err(format!("duplicate account {} in {section}", account.id));
        }
    }
    Ok(out)
}

fn post_key_map(
    witness: &BlockWitness,
    post_accounts: &BTreeMap<u64, &AccountSnapshot>,
) -> Result<BTreeMap<u64, BTreeSet<KeyRecord>>, String> {
    normalized_post_key_map(witness, post_accounts)
}

fn normalized_post_key_map(
    witness: &BlockWitness,
    post_accounts: &BTreeMap<u64, &AccountSnapshot>,
) -> Result<BTreeMap<u64, BTreeSet<KeyRecord>>, String> {
    let mut out: BTreeMap<u64, BTreeSet<KeyRecord>> = post_accounts
        .keys()
        .copied()
        .map(|account_id| (account_id, BTreeSet::new()))
        .collect();
    let mut seen = BTreeSet::new();
    for (account_id, keys) in &witness.account_keys {
        if !seen.insert(*account_id) {
            return Err(format!(
                "duplicate account {account_id} in witness.account_keys"
            ));
        }
        if !post_accounts.contains_key(account_id) {
            return Err(format!(
                "witness.account_keys opens unknown post-state account {account_id}"
            ));
        }
        validate_key_list(*account_id, keys)?;
        out.insert(*account_id, keys.iter().copied().collect());
    }
    Ok(out)
}

fn weld_post_keys(
    keys: &BTreeMap<u64, BTreeSet<KeyRecord>>,
    accounts: &BTreeMap<u64, &AccountSnapshot>,
    section: &str,
) -> Result<(), String> {
    for (account_id, account) in accounts {
        let active = keys.get(account_id).ok_or_else(|| {
            format!("account {account_id} in {section} is absent from the post key universe")
        })?;
        let digest = crate::account_keys_digest(*account_id, active.iter().copied());
        if account.keys_digest != digest {
            return Err(format!(
                "account {account_id} {section} keys_digest does not match witness.account_keys"
            ));
        }
    }
    Ok(())
}

fn validate_key_list(account_id: u64, keys: &[KeyRecord]) -> Result<(), String> {
    if keys.len() > crate::MAX_KEYS_PER_ACCOUNT {
        return Err(format!(
            "account {account_id} has {} keys, exceeding MAX_KEYS_PER_ACCOUNT={}",
            keys.len(),
            crate::MAX_KEYS_PER_ACCOUNT
        ));
    }
    let mut pubkeys = BTreeSet::new();
    for key in keys {
        validate_key_record(key)?;
        if !pubkeys.insert(key.pubkey_sec1) {
            return Err(format!("account {account_id} contains a duplicate pubkey"));
        }
    }
    Ok(())
}

fn validate_key_record(key: &KeyRecord) -> Result<(), String> {
    if key.auth_scheme > 1 {
        return Err(format!("unsupported key auth_scheme {}", key.auth_scheme));
    }
    if !matches!(key.pubkey_sec1[0], 0x02 | 0x03) {
        return Err("key is not a compressed SEC1 P-256 point".to_string());
    }
    if key.capability_mask != KeyRecord::FULL_CAPABILITY_MASK {
        return Err("key capability_mask is not full-authority in v6".to_string());
    }
    Ok(())
}

fn ensure_global_pubkey_uniqueness(
    keys: &BTreeMap<u64, BTreeSet<KeyRecord>>,
    context: &str,
) -> Result<(), String> {
    pubkey_owner_map(keys)
        .map(|_| ())
        .map_err(|details| format!("{context}: {details}"))
}

fn pubkey_owner_map(
    keys: &BTreeMap<u64, BTreeSet<KeyRecord>>,
) -> Result<BTreeMap<[u8; 33], u64>, String> {
    let mut out = BTreeMap::new();
    for (account_id, records) in keys {
        for record in records {
            if let Some(previous) = out.insert(record.pubkey_sec1, *account_id) {
                return Err(format!(
                    "pubkey is active on both account {previous} and account {account_id}"
                ));
            }
        }
    }
    Ok(out)
}

fn insert_globally_unique_key(
    account_id: u64,
    key: KeyRecord,
    account_keys: &mut BTreeSet<KeyRecord>,
    pubkeys: &mut BTreeMap<[u8; 33], u64>,
) -> Result<(), String> {
    if let Some(owner) = pubkeys.get(&key.pubkey_sec1) {
        return Err(format!(
            "key registration on account {account_id} collides with account {owner}"
        ));
    }
    if !account_keys.insert(key) {
        return Err(format!(
            "duplicate key registration on account {account_id}"
        ));
    }
    pubkeys.insert(key.pubkey_sec1, account_id);
    Ok(())
}

fn key_touched_accounts(events: &[SystemEventWitness]) -> BTreeSet<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::CreateAccount { account_id, .. }
            | SystemEventWitness::KeyRegistered { account_id, .. }
            | SystemEventWitness::KeyRevoked { account_id, .. } => Some(*account_id),
            _ => None,
        })
        .collect()
}

fn verify_key_event_order(events: &[SystemEventWitness]) -> Result<(), String> {
    let mut saw_non_key_event = BTreeSet::new();
    for event in events {
        let is_key_phase = matches!(
            event,
            SystemEventWitness::CreateAccount { .. }
                | SystemEventWitness::KeyRegistered { .. }
                | SystemEventWitness::KeyRevoked { .. }
        );
        for account_id in event_account_ids(event) {
            if is_key_phase {
                if saw_non_key_event.contains(&account_id) {
                    return Err(format!(
                        "key event for account {account_id} follows another same-account system event"
                    ));
                }
            } else {
                saw_non_key_event.insert(account_id);
            }
        }
    }
    Ok(())
}

fn event_account_ids(event: &SystemEventWitness) -> Vec<u64> {
    match event {
        SystemEventWitness::CreateAccount { account_id, .. }
        | SystemEventWitness::Deposit { account_id, .. }
        | SystemEventWitness::L1Deposit { account_id, .. }
        | SystemEventWitness::WithdrawalCreated { account_id, .. }
        | SystemEventWitness::WithdrawalRefunded { account_id, .. }
        | SystemEventWitness::WithdrawalFinalized { account_id, .. }
        | SystemEventWitness::OrderCancelled { account_id, .. }
        | SystemEventWitness::KeyRegistered { account_id, .. }
        | SystemEventWitness::KeyRevoked { account_id, .. } => vec![*account_id],
        SystemEventWitness::QuarantineClaimed { account_id, .. } => vec![*account_id],
        SystemEventWitness::MarketResolved {
            affected_accounts, ..
        } => affected_accounts.clone(),
        SystemEventWitness::L1BlockObserved { .. }
        | SystemEventWitness::MarketGroupExtended { .. }
        | SystemEventWitness::DepositQuarantined { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use p256::ecdsa::signature::Signer as _;
    use p256::ecdsa::{Signature, SigningKey};

    use super::*;
    use crate::KeyOpAuth;
    use crate::{DepositAccumulatorWitness, StateSidecarSnapshot, WitnessBlockHeader};

    fn key(byte: u8, auth_scheme: u8) -> KeyRecord {
        let signing = SigningKey::from_slice(&[byte; 32]).unwrap();
        let mut pubkey_sec1 = [0u8; 33];
        pubkey_sec1.copy_from_slice(signing.verifying_key().to_encoded_point(true).as_bytes());
        KeyRecord {
            auth_scheme,
            pubkey_sec1,
            capability_mask: KeyRecord::FULL_CAPABILITY_MASK,
        }
    }

    fn dummy_auth(signer: KeyRecord) -> KeyOpAuth {
        KeyOpAuth::RawP256 {
            signer_pubkey: signer.pubkey_sec1,
            signature: [0u8; 64],
        }
    }

    fn signed_auth(signer_byte: u8, canonical: &[u8]) -> KeyOpAuth {
        let signing = SigningKey::from_slice(&[signer_byte; 32]).unwrap();
        let signature: Signature = signing.sign(canonical);
        KeyOpAuth::RawP256 {
            signer_pubkey: key(signer_byte, 0).pubkey_sec1,
            signature: signature.to_bytes().into(),
        }
    }

    fn account(account_id: u64, keys: &[KeyRecord]) -> AccountSnapshot {
        AccountSnapshot {
            id: account_id,
            balance: 0,
            total_deposited: 0,
            positions: Vec::new(),
            events_digest: [0u8; 32],
            keys_digest: crate::account_keys_digest(account_id, keys.iter().copied()),
        }
    }

    fn witness(
        pre_keys: &[KeyRecord],
        post_keys: &[KeyRecord],
        system_events: Vec<SystemEventWitness>,
    ) -> BlockWitness {
        let pre = account(7, pre_keys);
        let post = account(7, post_keys);
        BlockWitness {
            header: WitnessBlockHeader {
                height: 2,
                parent_hash: [0u8; 32],
                state_root: [0u8; 32],
                events_root: [0u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 0,
            },
            previous_header: Some(WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root: [0u8; 32],
                events_root: [0u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 0,
            }),
            genesis_hash: [0x42; 32],
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events,
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: Vec::new(),
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: vec![pre],
            post_system_state: vec![post.clone()],
            post_state: vec![post],
            account_keys: if post_keys.is_empty() {
                Vec::new()
            } else {
                vec![(7, post_keys.to_vec())]
            },
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        }
    }

    #[test]
    fn rejects_digest_swap_without_key_events() {
        let old = key(1, 0);
        let attacker = key(2, 0);
        let result = verify_key_transitions(&witness(&[old], &[attacker], Vec::new()));
        assert!(!result.valid);
        assert!(result.violations[0]
            .details
            .contains("without a witnessed key event"));
    }

    #[test]
    fn rejects_event_stream_that_does_not_produce_claimed_digest() {
        let old = key(1, 0);
        let claimed = key(2, 0);
        let witnessed = key(3, 0);
        let result = verify_key_transitions(&witness(
            &[old],
            &[old, claimed],
            vec![SystemEventWitness::KeyRegistered {
                account_id: 7,
                key: witnessed,
                authorization: dummy_auth(old),
            }],
        ));
        assert!(!result.valid);
        assert!(result.violations[0]
            .details
            .contains("did not find the registered key"));
    }

    #[test]
    fn rejects_key_event_without_opened_account_leaf() {
        let old = key(1, 0);
        let added = key(2, 0);
        let mut witness = witness(&[old], &[old], Vec::new());
        witness.system_events = vec![SystemEventWitness::KeyRegistered {
            account_id: 8,
            key: added,
            authorization: dummy_auth(old),
        }];
        let result = verify_key_transitions(&witness);
        assert!(!result.valid);
        assert!(result.violations[0]
            .details
            .contains("no opened post-state leaf"));
    }

    #[test]
    fn register_then_revoke_across_blocks_round_trips_canonical_witness() {
        let primary = key(1, 0);
        let agent = key(2, 0);
        let mut register = witness(
            &[primary],
            &[primary, agent],
            vec![SystemEventWitness::KeyRegistered {
                account_id: 7,
                key: agent,
                authorization: dummy_auth(primary),
            }],
        );
        let canonical = crate::canonical_key_registration_bytes(
            register.genesis_hash,
            7,
            &agent,
            crate::account_keys_digest(7, [primary]),
            [0; 32],
        );
        if let SystemEventWitness::KeyRegistered { authorization, .. } =
            &mut register.system_events[0]
        {
            *authorization = signed_auth(1, &canonical);
        }
        assert!(verify_key_transitions(&register).valid);

        let bytes = crate::witness_schema::canonical_witness_bytes(&register);
        let restored = crate::witness_schema::decode_canonical_witness_bytes(&bytes).unwrap();
        assert!(verify_key_transitions(&restored).valid);

        let mut revoke = witness(
            &[primary, agent],
            &[agent],
            vec![SystemEventWitness::KeyRevoked {
                account_id: 7,
                key: primary,
                authorization: dummy_auth(agent),
            }],
        );
        let canonical = crate::canonical_key_revocation_bytes(
            revoke.genesis_hash,
            7,
            &primary,
            crate::account_keys_digest(7, [primary, agent]),
            [0; 32],
        );
        if let SystemEventWitness::KeyRevoked { authorization, .. } = &mut revoke.system_events[0] {
            *authorization = signed_auth(2, &canonical);
        }
        assert!(verify_key_transitions(&revoke).valid);
    }
}
