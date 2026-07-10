use super::*;

impl BlockSequencer {
    pub fn register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
    ) -> Result<(), SequencerError> {
        self.register_pubkey_with_scheme(
            account_id,
            pubkey,
            crate::crypto::AccountAuthScheme::RawP256,
        )
    }

    pub fn register_pubkey_with_scheme(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
        auth_scheme: crate::crypto::AccountAuthScheme,
    ) -> Result<(), SequencerError> {
        self.register_pubkey_with_meta(
            account_id,
            pubkey,
            crate::crypto::RegisteredPubkey::primary(account_id, auth_scheme),
        )
    }

    /// Register a signing key carrying full metadata (label/scope/created_at).
    ///
    /// The metadata's `account_id` is authoritative and overwrites whatever the
    /// caller passed for `account_id` to keep the two in lockstep.
    pub fn register_pubkey_with_meta(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
        mut meta: crate::crypto::RegisteredPubkey,
    ) -> Result<(), SequencerError> {
        if self.height != 0 {
            return Err(SequencerError::FirstKeyMustBeInitial);
        }
        self.can_register_pubkey(account_id, &pubkey)?;
        self.validate_quarantine_claim_for_account(account_id)?;
        meta.account_id = account_id;
        self.apply_pubkey_registration(account_id, pubkey, meta);
        self.claim_quarantine_for_account(account_id)?;
        Ok(())
    }

    /// Preflight a signing-key registration without mutating account state.
    pub fn can_register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: &crate::crypto::PublicKey,
    ) -> Result<(), SequencerError> {
        if self.accounts.get(account_id).is_none() {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        }
        if self.pubkey_registry.contains_key(pubkey) {
            return Err(SequencerError::AccountAlreadyRegistered);
        }
        if self
            .pubkey_registry
            .values()
            .filter(|registered| registered.account_id == account_id)
            .count()
            >= sybil_verifier::MAX_KEYS_PER_ACCOUNT
        {
            return Err(SequencerError::SigningKeyLimit);
        }
        Ok(())
    }

    /// Preflight the unsigned first-key bootstrap.
    ///
    /// This is deliberately stricter than signed key registration: the target
    /// account must have no signing keys at all. The actor invokes this before
    /// appending the control-plane WAL command, and the apply method below
    /// invokes the same preflight again so validation cannot drift.
    pub fn can_register_first_pubkey(
        &self,
        account_id: AccountId,
        pubkey: &crate::crypto::PublicKey,
    ) -> Result<(), SequencerError> {
        self.can_register_pubkey(account_id, pubkey)?;
        if self
            .pubkey_registry
            .values()
            .any(|registered| registered.account_id == account_id)
        {
            return Err(SequencerError::AccountAlreadyRegistered);
        }
        Ok(())
    }

    /// Atomically apply the unsigned first-key bootstrap.
    pub fn register_first_pubkey_with_meta(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
        mut meta: crate::crypto::RegisteredPubkey,
    ) -> Result<(), SequencerError> {
        self.can_register_first_pubkey(account_id, &pubkey)?;
        self.validate_quarantine_claim_for_account(account_id)?;
        meta.account_id = account_id;
        let key = crate::digest::key_record(&pubkey, &meta);
        let pending_create = self
            .pending_system_events
            .iter_mut()
            .rev()
            .find_map(|event| match event {
                SystemEvent::CreateAccount {
                    account_id: created_id,
                    initial_keys,
                    ..
                } if *created_id == account_id => Some(initial_keys),
                _ => None,
            });
        if let Some(initial_keys) = pending_create {
            initial_keys.push(key);
        } else if self.height != 0 {
            return Err(SequencerError::FirstKeyMustBeInitial);
        }
        self.apply_pubkey_registration(account_id, pubkey, meta);
        self.claim_quarantine_for_account(account_id)?;
        Ok(())
    }

    /// Apply an existing-key-authorized registration and stage the exact v6
    /// system event for the next block.
    pub fn register_pubkey_with_meta_authorized(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
        mut meta: crate::crypto::RegisteredPubkey,
        authorization: sybil_verifier::KeyOpAuth,
    ) -> Result<(), SequencerError> {
        self.can_register_pubkey(account_id, &pubkey)?;
        self.validate_quarantine_claim_for_account(account_id)?;
        self.capture_system_account_baseline(account_id);
        meta.account_id = account_id;
        let key = crate::digest::key_record(&pubkey, &meta);
        self.apply_pubkey_registration(account_id, pubkey, meta);
        let account = self
            .accounts
            .get_mut(account_id)
            .expect("key-registration preflight requires the account");
        let encoded =
            crate::digest::encode_key_registered_event(&key, self.height.saturating_add(1));
        account.events_digest = crate::digest::update_digest(&account.events_digest, &encoded);
        self.record_system_event(SystemEvent::KeyRegistered {
            account_id,
            key,
            authorization,
        });
        self.claim_quarantine_for_account(account_id)?;
        Ok(())
    }

    fn apply_pubkey_registration(
        &mut self,
        account_id: AccountId,
        pubkey: crate::crypto::PublicKey,
        meta: crate::crypto::RegisteredPubkey,
    ) {
        self.pubkey_registry.insert(pubkey, meta);
        crate::digest::refresh_account_keys_digest(
            &mut self.accounts,
            account_id,
            &self.pubkey_registry,
        );
    }

    /// Revoke a registered signing key (SYB-60).
    ///
    /// LOCKOUT PROTECTION: refuses to remove an account's last remaining signing
    /// key. Because every mutation (orders/cancels/withdrawals/profile/key/api-key
    /// changes) is P256-signed, an account with zero registered keys would be
    /// permanently frozen — unable even to register a replacement. The invariant
    /// enforced here is simply "at least one signing key must remain after
    /// revocation", which also covers the self-revocation case: a key may revoke
    /// itself only while a second key exists.
    pub fn revoke_signing_key(
        &mut self,
        account_id: AccountId,
        target: &crate::crypto::PublicKey,
        authorization: sybil_verifier::KeyOpAuth,
    ) -> Result<(), SequencerError> {
        self.can_revoke_signing_key(account_id, target)?;
        self.capture_system_account_baseline(account_id);
        let registered = self
            .pubkey_registry
            .get(target)
            .expect("revocation preflight requires the target key")
            .clone();
        let key = crate::digest::key_record(target, &registered);
        self.pubkey_registry.remove(target);
        crate::digest::refresh_account_keys_digest(
            &mut self.accounts,
            account_id,
            &self.pubkey_registry,
        );
        let account = self
            .accounts
            .get_mut(account_id)
            .expect("revocation preflight requires the account");
        let encoded = crate::digest::encode_key_revoked_event(&key, self.height.saturating_add(1));
        account.events_digest = crate::digest::update_digest(&account.events_digest, &encoded);
        self.record_system_event(SystemEvent::KeyRevoked {
            account_id,
            key,
            authorization,
        });
        Ok(())
    }

    pub fn can_revoke_signing_key(
        &self,
        account_id: AccountId,
        target: &crate::crypto::PublicKey,
    ) -> Result<(), SequencerError> {
        let registered = self
            .pubkey_registry
            .get(target)
            .ok_or(SequencerError::KeyNotFound)?;
        if registered.account_id != account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        let remaining = self
            .pubkey_registry
            .values()
            .filter(|r| r.account_id == account_id)
            .count();
        if remaining <= 1 {
            return Err(SequencerError::LastSigningKey);
        }
        Ok(())
    }

    pub fn lookup_pubkey(&self, pubkey: &crate::crypto::PublicKey) -> Option<AccountId> {
        self.pubkey_registry
            .get(pubkey)
            .map(|registered| registered.account_id)
    }

    pub fn lookup_registered_pubkey(
        &self,
        pubkey: &crate::crypto::PublicKey,
    ) -> Option<crate::crypto::RegisteredPubkey> {
        self.pubkey_registry.get(pubkey).cloned()
    }

    /// All registered signing keys for an account, with metadata (SYB-60).
    /// Returned as (compressed SEC1 bytes, registration) pairs, sorted by
    /// `created_at_ms` then key bytes for a stable listing order.
    pub fn signing_keys_for_account(
        &self,
        account_id: AccountId,
    ) -> Vec<(Vec<u8>, crate::crypto::RegisteredPubkey)> {
        let mut keys: Vec<(Vec<u8>, crate::crypto::RegisteredPubkey)> = self
            .pubkey_registry
            .iter()
            .filter(|(_, r)| r.account_id == account_id)
            .map(|(pk, r)| (pk.compressed_bytes(), r.clone()))
            .collect();
        keys.sort_by(|a, b| {
            a.1.created_at_ms
                .cmp(&b.1.created_at_ms)
                .then_with(|| a.0.cmp(&b.0))
        });
        keys
    }

    // --- Profile (SYB-60) ---

    /// Set or clear an account's opt-in profile. Passing `None` for a field
    /// clears it. Validation (length/charset) happens at the API edge.
    pub fn set_profile(
        &mut self,
        account_id: AccountId,
        display_name: Option<String>,
        avatar_seed: Option<String>,
    ) -> Result<Account, SequencerError> {
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        account.profile.display_name = display_name;
        account.profile.avatar_seed = avatar_seed;
        Ok(account.clone())
    }

    // --- Read API keys (SYB-60) ---

    /// Register a new read-scoped bearer API key from its precomputed
    /// blake3 hash. Returns the assigned key id. The plaintext token never
    /// reaches this layer.
    pub fn create_api_key(
        &mut self,
        account_id: AccountId,
        token_hash: [u8; 32],
        label: Option<String>,
        created_at_ms: u64,
    ) -> Result<u64, SequencerError> {
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        // Collisions are astronomically unlikely, but a duplicate hash would
        // corrupt the reverse index, so reject rather than shadow it.
        if account.api_keys.iter().any(|k| k.hash == token_hash) {
            return Err(SequencerError::AccountAlreadyRegistered);
        }
        let id = account.next_api_key_id;
        account.next_api_key_id = account.next_api_key_id.saturating_add(1);
        account.api_keys.push(crate::account::ApiKeyRecord {
            id,
            hash: token_hash,
            label,
            created_at_ms,
            revoked_at_ms: None,
        });
        self.api_key_index.insert(token_hash, account_id);
        Ok(id)
    }

    /// Revoke a read API key by id. Idempotent-safe: an already-revoked or
    /// unknown id returns `ApiKeyNotFound` only when truly absent.
    pub fn revoke_api_key(
        &mut self,
        account_id: AccountId,
        api_key_id: u64,
        revoked_at_ms: u64,
    ) -> Result<(), SequencerError> {
        let hash = self.api_key_hash_for_revocation(account_id, api_key_id)?;
        let account = self
            .accounts
            .get_mut(account_id)
            .expect("account exists: validated above");
        let record = account
            .api_keys
            .iter_mut()
            .find(|k| k.id == api_key_id)
            .expect("API key exists: validated above");
        if record.revoked_at_ms.is_none() {
            record.revoked_at_ms = Some(revoked_at_ms);
        }
        self.api_key_index.remove(&hash);
        Ok(())
    }

    pub fn can_revoke_api_key(
        &self,
        account_id: AccountId,
        api_key_id: u64,
        revoked_at_ms: u64,
    ) -> Result<(), SequencerError> {
        let _ = revoked_at_ms;
        self.api_key_hash_for_revocation(account_id, api_key_id)
            .map(|_| ())
    }

    fn api_key_hash_for_revocation(
        &self,
        account_id: AccountId,
        api_key_id: u64,
    ) -> Result<[u8; 32], SequencerError> {
        self.accounts
            .get(account_id)
            .and_then(|account| account.api_keys.iter().find(|key| key.id == api_key_id))
            .map(|record| record.hash)
            .ok_or(SequencerError::ApiKeyNotFound)
    }

    /// Resolve a bearer token hash to its owning account, if the key is active
    /// (SYB-60). Used by the read-only bearer extractor.
    pub fn lookup_api_key(&self, token_hash: &[u8; 32]) -> Option<AccountId> {
        self.api_key_index.get(token_hash).copied()
    }

    /// List an account's API keys (metadata only — never the hash/token).
    pub fn api_keys_for_account(&self, account_id: AccountId) -> Vec<crate::account::ApiKeyRecord> {
        self.accounts
            .get(account_id)
            .map(|a| a.api_keys.clone())
            .unwrap_or_default()
    }

    pub fn validate_replay_nonce(
        &self,
        account_id: AccountId,
        nonce: u64,
    ) -> Result<(), SequencerError> {
        let Some(account) = self.accounts.get(account_id) else {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        };
        if nonce <= account.last_nonce {
            return Err(SequencerError::ReplayNonceStale {
                account_id,
                nonce,
                last_nonce: account.last_nonce,
            });
        }
        Ok(())
    }

    pub fn advance_replay_nonce(
        &mut self,
        account_id: AccountId,
        nonce: u64,
    ) -> Result<(), SequencerError> {
        self.validate_replay_nonce(account_id, nonce)?;
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        account.last_nonce = nonce;
        Ok(())
    }

    pub fn create_account(&mut self, initial_balance: i64) -> AccountId {
        self.create_account_at(initial_balance, current_timestamp_ms())
    }

    pub fn create_account_at(&mut self, initial_balance: i64, timestamp_ms: u64) -> AccountId {
        let account_id = self.accounts.create_account(initial_balance);
        self.capture_missing_system_account(account_id);
        self.record_system_event(SystemEvent::CreateAccount {
            account_id,
            initial_balance,
            initial_keys: Vec::new(),
        });
        {
            use crate::aggregates::{HistoryEvent, HistoryKind};
            let mut e = HistoryEvent::new(
                account_id,
                HistoryKind::Created,
                self.height.saturating_add(1),
                timestamp_ms,
            );
            e.amount_nanos = Some(initial_balance);
            self.analytics.record_history(e);
        }
        account_id
    }

    pub fn fund_account(
        &mut self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        self.fund_account_at(account_id, amount, current_timestamp_ms())
    }

    pub fn fund_account_at(
        &mut self,
        account_id: AccountId,
        amount: i64,
        timestamp_ms: u64,
    ) -> Result<Account, SequencerError> {
        self.capture_system_account_baseline(account_id);
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;

        account.balance += amount;
        account.total_deposited += amount;
        let updated = account.clone();
        self.record_system_event(SystemEvent::Deposit { account_id, amount });
        {
            use crate::aggregates::{HistoryEvent, HistoryKind};
            let mut e = HistoryEvent::new(
                account_id,
                HistoryKind::Deposit,
                self.height.saturating_add(1),
                timestamp_ms,
            );
            e.amount_nanos = Some(amount);
            self.analytics.record_history(e);
        }
        self.note_first_deposit_at(account_id, timestamp_ms);
        Ok(updated)
    }

    /// Stamp the first observed deposit time for an account. Subsequent
    /// deposits are ignored. Off-block sidecar; never enters `state_root`.
    pub(super) fn note_first_deposit(&mut self, account_id: AccountId) {
        self.analytics.note_first_deposit(account_id);
    }

    pub(super) fn note_first_deposit_at(&mut self, account_id: AccountId, timestamp_ms: u64) {
        self.analytics
            .note_first_deposit_at(account_id, timestamp_ms);
    }
}
