use super::super::*;
use super::current_timestamp_ms;

impl SequencerActorState {
    fn validate_keyop_state_binding(
        &self,
        account_id: AccountId,
        bound_keys_digest: [u8; 32],
        bound_events_digest: [u8; 32],
    ) -> Result<(), SequencerError> {
        let account = self
            .sequencer
            .accounts
            .get(account_id)
            .ok_or(SequencerError::UnknownSigner)?;
        if account.keys_digest != bound_keys_digest || account.events_digest != bound_events_digest
        {
            return Err(SequencerError::KeyOpStateStale { account_id });
        }
        Ok(())
    }

    pub(super) async fn persist_control_plane(
        &self,
        command: &crate::store::ControlPlaneCommand,
    ) -> Result<(), SequencerError> {
        if let Some(store) = &self.store {
            store
                .append_control_plane_command(command)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        Ok(())
    }

    pub(super) async fn accept_replay_nonce(
        &mut self,
        account_id: AccountId,
        nonce: u64,
    ) -> Result<(), SequencerError> {
        self.sequencer.validate_replay_nonce(account_id, nonce)?;
        self.persist_control_plane(&ControlPlaneCommand::AdvanceReplayNonce { account_id, nonce })
            .await?;
        self.sequencer.advance_replay_nonce(account_id, nonce)
    }

    pub(super) async fn handle_create_account(
        &mut self,
        initial_balance: i64,
    ) -> Result<Account, SequencerError> {
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::CreateAccountAt {
            initial_balance,
            timestamp_ms,
        })
        .await?;
        let account_id = self
            .sequencer
            .create_account_at(initial_balance, timestamp_ms);
        Ok(self
            .sequencer
            .accounts
            .get(account_id)
            .cloned()
            .expect("created account should exist"))
    }

    pub(super) async fn handle_fund_account(
        &mut self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        if self.sequencer.accounts.get(account_id).is_none() {
            return self.sequencer.fund_account(account_id, amount);
        }
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::FundAccount {
            account_id,
            amount,
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .fund_account_at(account_id, amount, timestamp_ms)
    }

    pub(super) async fn handle_register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: PublicKey,
        auth_scheme: AccountAuthScheme,
    ) -> Result<(), SequencerError> {
        self.sequencer
            .can_register_first_pubkey(account_id, &pubkey)?;
        self.persist_control_plane(&ControlPlaneCommand::RegisterPubkey {
            account_id,
            compressed_pubkey: pubkey.compressed_bytes(),
            auth_scheme,
        })
        .await?;
        self.sequencer.register_first_pubkey_with_meta(
            account_id,
            pubkey,
            RegisteredPubkey::primary(account_id, auth_scheme),
        )
    }

    pub(super) async fn handle_create_market(
        &mut self,
        name: String,
    ) -> Result<MarketId, SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarket { name: name.clone() })
            .await?;
        Ok(self.sequencer.create_market(name))
    }

    pub(super) async fn handle_create_market_with_metadata(
        &mut self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarketWithMetadata {
            name: name.clone(),
            metadata: metadata.clone(),
        })
        .await?;
        Ok(self.sequencer.create_market_with_metadata(name, metadata))
    }

    pub(super) async fn handle_create_market_group(
        &mut self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<(u64, MarketGroup), SequencerError> {
        self.persist_control_plane(&ControlPlaneCommand::CreateMarketGroup {
            name: name.clone(),
            market_ids: market_ids.clone(),
        })
        .await?;
        Ok(self.sequencer.create_market_group(name, market_ids))
    }

    pub(super) async fn handle_extend_market_group(
        &mut self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(MarketGroup, bool), SequencerError> {
        self.sequencer
            .can_extend_market_group(group_id, market_id)?;
        self.persist_control_plane(&ControlPlaneCommand::ExtendMarketGroup {
            group_id,
            market_id,
        })
        .await?;
        self.sequencer.extend_market_group(group_id, market_id)
    }

    pub(super) async fn handle_resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.resolve_market(market_id, payout_nanos, timestamp_ms)?;
        self.persist_control_plane(&ControlPlaneCommand::ResolveMarket {
            market_id,
            payout_nanos,
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .resolve_market(market_id, payout_nanos, timestamp_ms)
    }

    pub(super) async fn handle_resolve_market_attested(
        &mut self,
        market_id: MarketId,
        signed: SignedAttestation,
    ) -> Result<ResolutionRecord, SequencerError> {
        crate::crypto::verify_signed_attestation(&signed)?;
        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.resolve_market_attested(market_id, &signed, timestamp_ms)?;
        self.persist_control_plane(&ControlPlaneCommand::ResolveMarketAttested {
            market_id,
            signed: signed.clone(),
            timestamp_ms,
        })
        .await?;
        self.sequencer
            .resolve_market_attested(market_id, &signed, timestamp_ms)
    }

    pub(super) async fn handle_register_feed(
        &mut self,
        pubkey: FeedPubkey,
        name: String,
    ) -> Result<FeedId, SequencerError> {
        if let Some(feed) = self.sequencer.feed_by_pubkey(&pubkey) {
            return Ok(feed.id);
        }
        let timestamp_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::RegisterFeed {
            pubkey: pubkey.clone(),
            name: name.clone(),
            timestamp_ms,
        })
        .await?;
        Ok(self.sequencer.register_feed(pubkey, name, timestamp_ms))
    }

    pub(super) async fn handle_install_template(
        &mut self,
        template: sybil_oracle::ResolutionTemplate,
    ) -> Result<(), SequencerError> {
        if self
            .sequencer
            .market_lifecycle()
            .templates()
            .get(&template.id)
            .is_some_and(|existing| existing == &template)
        {
            return Ok(());
        }
        self.persist_control_plane(&ControlPlaneCommand::InstallTemplate {
            template: template.clone(),
        })
        .await?;
        self.sequencer.install_template(template);
        Ok(())
    }

    // --- SYB-60 account management signed mutations ---
    //
    // All four verify the P256 signature (or accept an already-verified
    // WebAuthn intent), confirm the signer key belongs to the target account,
    // then persist a control-plane WAL row before applying the in-memory change.
    // Key operations bind the running key/event digests; profile and API-key
    // operations retain the ordinary replay nonce.

    pub(super) async fn handle_register_pubkey_with_meta(
        &mut self,
        account_id: AccountId,
        pubkey: PublicKey,
        meta: RegisteredPubkey,
    ) -> Result<(), SequencerError> {
        self.sequencer
            .can_register_first_pubkey(account_id, &pubkey)?;
        self.persist_control_plane(&ControlPlaneCommand::RegisterPubkeyWithMeta {
            account_id,
            compressed_pubkey: pubkey.compressed_bytes(),
            auth_scheme: meta.auth_scheme,
            label: meta.label.clone(),
            scope: meta.scope,
            created_at_ms: meta.created_at_ms,
        })
        .await?;
        self.sequencer
            .register_first_pubkey_with_meta(account_id, pubkey, meta)
    }

    /// Register a NEW signing key authorized by an existing account key (SYB-229).
    ///
    /// Unlike the first-key bootstrap (`handle_register_pubkey_with_meta`, service
    /// tier), this path requires a signature by a key that already belongs to the
    /// account. The genesis domain is checked by the caller
    /// (`handle_signed_key_registration`); WebAuthn intents are pre-verified at the
    /// API edge, exactly like the other SYB-60 mutations.
    pub(super) async fn handle_signed_key_registration(
        &mut self,
        signed: SignedKeyRegistration,
    ) -> Result<(), SequencerError> {
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_key_registration(&signed, genesis_hash)?;
        let authorization = raw_key_op_auth(&signed.signer, &signed.signature);
        self.handle_authenticated_key_registration(AuthenticatedKeyRegistration {
            account_id: signed.account_id,
            new_pubkey: signed.new_pubkey,
            new_auth_scheme: signed.new_auth_scheme,
            label: signed.label,
            scope: signed.scope,
            bound_keys_digest: signed.bound_keys_digest,
            bound_events_digest: signed.bound_events_digest,
            signer: signed.signer,
            authorization,
        })
        .await
    }

    pub(super) async fn handle_authenticated_key_registration(
        &mut self,
        authenticated: AuthenticatedKeyRegistration,
    ) -> Result<(), SequencerError> {
        // The signer must already be an active key on the target account. This
        // also implies the account has >= 1 key, so the signed path can never be
        // used to bootstrap the very first key (that stays on the service tier).
        self.resolve_signer_account(&authenticated.signer, authenticated.account_id)?;
        self.validate_keyop_state_binding(
            authenticated.account_id,
            authenticated.bound_keys_digest,
            authenticated.bound_events_digest,
        )?;
        // Reject a duplicate registration before writing the WAL. The digest
        // binding has already rejected stale or replayed key operations.
        self.sequencer
            .can_register_pubkey(authenticated.account_id, &authenticated.new_pubkey)?;
        let meta = RegisteredPubkey {
            account_id: authenticated.account_id,
            auth_scheme: authenticated.new_auth_scheme,
            label: authenticated.label.clone(),
            scope: authenticated.scope,
            created_at_ms: current_timestamp_ms(),
        };
        self.persist_control_plane(&ControlPlaneCommand::RegisterPubkeyAuthorized {
            account_id: authenticated.account_id,
            compressed_pubkey: authenticated.new_pubkey.compressed_bytes(),
            auth_scheme: meta.auth_scheme,
            label: meta.label.clone(),
            scope: meta.scope,
            created_at_ms: meta.created_at_ms,
            authorization: authenticated.authorization.clone(),
        })
        .await?;
        self.sequencer.register_pubkey_with_meta_authorized(
            authenticated.account_id,
            authenticated.new_pubkey,
            meta,
            authenticated.authorization,
        )
    }

    /// Resolve the account for a verified signer and confirm it matches the
    /// account named in the request (shared by all SYB-60 mutations).
    pub(super) fn resolve_signer_account(
        &self,
        signer: &PublicKey,
        claimed: AccountId,
    ) -> Result<(), SequencerError> {
        let account_id = self
            .sequencer
            .lookup_pubkey(signer)
            .ok_or(SequencerError::UnknownSigner)?;
        if account_id != claimed {
            return Err(SequencerError::SignerAccountMismatch);
        }
        Ok(())
    }

    pub(super) async fn handle_signed_profile_update(
        &mut self,
        signed: SignedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        verify_signed_profile_update(&signed)?;
        self.handle_authenticated_profile_update(AuthenticatedProfileUpdate {
            account_id: signed.account_id,
            display_name: signed.display_name,
            avatar_seed: signed.avatar_seed,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_profile_update(
        &mut self,
        authenticated: AuthenticatedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        self.resolve_signer_account(&authenticated.signer, authenticated.account_id)?;
        self.accept_replay_nonce(authenticated.account_id, authenticated.nonce)
            .await?;
        self.persist_control_plane(&ControlPlaneCommand::SetProfile {
            account_id: authenticated.account_id,
            display_name: authenticated.display_name.clone(),
            avatar_seed: authenticated.avatar_seed.clone(),
        })
        .await?;
        self.sequencer.set_profile(
            authenticated.account_id,
            authenticated.display_name,
            authenticated.avatar_seed,
        )
    }

    pub(super) async fn handle_signed_key_revocation(
        &mut self,
        signed: SignedKeyRevocation,
    ) -> Result<(), SequencerError> {
        // Domain-separate the revocation signature by the chain genesis (SYB-231),
        // exactly like the signed key-registration path, so a captured revocation
        // cannot replay against a fresh-genesis redeploy.
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_key_revocation(&signed, genesis_hash)?;
        let authorization = raw_key_op_auth(&signed.signer, &signed.signature);
        self.handle_authenticated_key_revocation(AuthenticatedKeyRevocation {
            account_id: signed.account_id,
            target_key: signed.target_key,
            bound_keys_digest: signed.bound_keys_digest,
            bound_events_digest: signed.bound_events_digest,
            signer: signed.signer,
            authorization,
        })
        .await
    }

    pub(super) async fn handle_authenticated_key_revocation(
        &mut self,
        authenticated: AuthenticatedKeyRevocation,
    ) -> Result<(), SequencerError> {
        self.resolve_signer_account(&authenticated.signer, authenticated.account_id)?;
        self.validate_keyop_state_binding(
            authenticated.account_id,
            authenticated.bound_keys_digest,
            authenticated.bound_events_digest,
        )?;
        let target = PublicKey::from_compressed_bytes(&authenticated.target_key.pubkey_sec1)
            .ok_or(SequencerError::KeyNotFound)?;
        let registered = self
            .sequencer
            .lookup_registered_pubkey(&target)
            .ok_or(SequencerError::KeyNotFound)?;
        if crate::digest::key_record(&target, &registered) != authenticated.target_key {
            return Err(SequencerError::KeyNotFound);
        }
        // Validate the revocation (ownership + last-key lockout) before writing
        // the WAL. The digest binding has already rejected stale/replayed ops.
        self.sequencer
            .can_revoke_signing_key(authenticated.account_id, &target)?;
        self.persist_control_plane(&ControlPlaneCommand::RevokeSigningKey {
            account_id: authenticated.account_id,
            compressed_pubkey: authenticated.target_key.pubkey_sec1.to_vec(),
            authorization: authenticated.authorization.clone(),
        })
        .await?;
        self.sequencer.revoke_signing_key(
            authenticated.account_id,
            &target,
            authenticated.authorization,
        )
    }

    pub(super) async fn handle_signed_api_key_create(
        &mut self,
        signed: SignedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        verify_signed_api_key_create(&signed)?;
        self.handle_authenticated_api_key_create(AuthenticatedApiKeyCreate {
            account_id: signed.account_id,
            label: signed.label,
            token_hash: signed.token_hash,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_api_key_create(
        &mut self,
        authenticated: AuthenticatedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        self.resolve_signer_account(&authenticated.signer, authenticated.account_id)?;
        self.accept_replay_nonce(authenticated.account_id, authenticated.nonce)
            .await?;
        let created_at_ms = current_timestamp_ms();
        self.persist_control_plane(&ControlPlaneCommand::CreateApiKey {
            account_id: authenticated.account_id,
            token_hash: authenticated.token_hash,
            label: authenticated.label.clone(),
            created_at_ms,
        })
        .await?;
        self.sequencer.create_api_key(
            authenticated.account_id,
            authenticated.token_hash,
            authenticated.label,
            created_at_ms,
        )
    }

    pub(super) async fn handle_signed_api_key_revoke(
        &mut self,
        signed: SignedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        verify_signed_api_key_revoke(&signed)?;
        self.handle_authenticated_api_key_revoke(AuthenticatedApiKeyRevoke {
            account_id: signed.account_id,
            api_key_id: signed.api_key_id,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_api_key_revoke(
        &mut self,
        authenticated: AuthenticatedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        self.resolve_signer_account(&authenticated.signer, authenticated.account_id)?;
        let revoked_at_ms = current_timestamp_ms();
        self.sequencer.can_revoke_api_key(
            authenticated.account_id,
            authenticated.api_key_id,
            revoked_at_ms,
        )?;
        self.accept_replay_nonce(authenticated.account_id, authenticated.nonce)
            .await?;
        self.persist_control_plane(&ControlPlaneCommand::RevokeApiKey {
            account_id: authenticated.account_id,
            api_key_id: authenticated.api_key_id,
            revoked_at_ms,
        })
        .await?;
        self.sequencer.revoke_api_key(
            authenticated.account_id,
            authenticated.api_key_id,
            revoked_at_ms,
        )
    }
}

fn raw_key_op_auth(
    signer: &PublicKey,
    signature: &p256::ecdsa::Signature,
) -> sybil_verifier::KeyOpAuth {
    let compressed = signer.compressed_bytes();
    let mut signer_pubkey = [0u8; 33];
    signer_pubkey.copy_from_slice(&compressed);
    sybil_verifier::KeyOpAuth::RawP256 {
        signer_pubkey,
        signature: signature.to_bytes().into(),
    }
}
