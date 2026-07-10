use super::*;

#[cfg(test)]
const TEST_ACTOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Cloneable handle to the sequencer actor.
#[derive(Clone)]
pub struct SequencerHandle {
    inner: SequencerHandleInner,
    supervisor: ActorRef<SequencerSupervisorMsg>,
}

impl SequencerHandle {
    async fn actor_ref(&self) -> Result<ActorRef<SequencerMsg>, SequencerError> {
        self.inner
            .actor
            .read()
            .expect("sequencer actor ref lock poisoned")
            .clone()
            .ok_or(SequencerError::ActorGone)
    }

    async fn rpc<T>(
        &self,
        build_message: impl FnOnce(RpcReplyPort<T>) -> SequencerMsg,
    ) -> Result<T, SequencerError>
    where
        T: Send + 'static,
    {
        let actor = self.actor_ref().await?;
        self.inner.mailbox_monitor.queued();
        match actor.call(build_message, None).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Err(_) => {
                self.inner.mailbox_monitor.send_failed();
                Err(SequencerError::ActorGone)
            }
            _ => Err(SequencerError::ActorGone),
        }
    }

    async fn read_query<T>(
        &self,
        query: impl FnOnce(&SequencerActorState) -> T + Send + 'static,
    ) -> Result<T, SequencerError>
    where
        T: Send + 'static,
    {
        self.rpc(|reply| SequencerMsg::Query(SequencerReadQuery::new(reply, query)))
            .await
    }

    /// Spawn with default config (1-second block interval).
    /// Prefer [`spawn_with_store`] for production use.
    pub fn spawn(sequencer: BlockSequencer) -> Self {
        Self::spawn_with_store(sequencer, None)
    }

    /// Spawn the sequencer actor with an attached persistent store.
    ///
    /// The store is used for:
    ///   1. Persisting each committed block's state (write-through on every block).
    ///   2. Reloading state when the supervisor restarts the actor after a crash.
    ///
    /// It is **not** used to hydrate the `sequencer` argument. The caller is
    /// responsible for calling [`Store::load_state`] and passing the result to
    /// [`BlockSequencer::restore`] before invoking this function — see
    /// `crates/sybil-api/src/main.rs` for the canonical hydration + spawn pattern.
    pub fn spawn_with_store(sequencer: BlockSequencer, store: Option<crate::store::Store>) -> Self {
        Self::spawn_with_store_arc(sequencer, store.map(Arc::new))
    }

    fn spawn_with_store_arc(
        sequencer: BlockSequencer,
        store: Option<Arc<crate::store::Store>>,
    ) -> Self {
        let oracle = sequencer.oracle();
        let config = sequencer.config.clone();
        let (block_broadcast, _) = broadcast::channel(64);
        let mailbox_monitor = MailboxMonitor::new(
            SEQUENCER_ACTOR_METRIC_NAME,
            config.actor_queue_warn_depth,
            config.actor_queue_error_depth,
        );
        let inner = SequencerHandleInner {
            actor: Arc::new(RwLock::new(None)),
            block_broadcast: block_broadcast.clone(),
            mailbox_monitor,
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        };
        let supervisor_args = SequencerSupervisorArgs {
            config,
            store: store.clone(),
            oracle,
            handle: inner.clone(),
        };
        let (supervisor, _) =
            ractor::ActorRuntime::spawn_instant(None, SequencerSupervisor, supervisor_args)
                .expect("failed to spawn sequencer supervisor");
        let actor_args = SequencerActorArgs {
            sequencer,
            store,
            block_broadcast,
            mailbox_monitor: inner.mailbox_monitor.clone(),
        };
        let (child, _) = ractor::ActorRuntime::spawn_linked_instant(
            None,
            SequencerActor,
            actor_args,
            supervisor.get_cell(),
        )
        .expect("failed to spawn sequencer actor");
        *inner
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = Some(child.clone());
        supervisor
            .send_message(SequencerSupervisorMsg::AdoptChild(child))
            .expect("failed to hand child actor to supervisor");
        Self { inner, supervisor }
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_store_arc_for_test(
        sequencer: BlockSequencer,
        store: Arc<crate::store::Store>,
    ) -> Self {
        Self::spawn_with_store_arc(sequencer, Some(store))
    }

    #[cfg(test)]
    async fn wait_for_actor_restart_for_test(
        &self,
        old_id: ractor::ActorId,
    ) -> Result<(), SequencerError> {
        let deadline = Instant::now() + TEST_ACTOR_TIMEOUT;
        loop {
            let actor = self
                .inner
                .actor
                .read()
                .expect("sequencer actor ref lock poisoned")
                .clone();
            if let Some(actor) = actor {
                if actor.get_id() != old_id {
                    return Ok(());
                }
            }

            if Instant::now() >= deadline {
                return Err(SequencerError::ActorGone);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn crash_actor_for_test(&self) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        let old_id = actor.get_id();
        actor
            .kill_and_wait(Some(TEST_ACTOR_TIMEOUT))
            .await
            .map_err(|error| {
                SequencerError::Persistence(format!("actor test kill failed: {error}"))
            })?;
        self.wait_for_actor_restart_for_test(old_id).await
    }

    #[cfg(test)]
    pub(crate) async fn produce_block_and_crash_for_test(
        &self,
        crashpoint: SequencerTestCrashpoint,
    ) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        let old_id = actor.get_id();
        self.inner.mailbox_monitor.queued();
        if actor
            .send_message(SequencerMsg::TestCrashOnNextBlock(crashpoint))
            .is_err()
        {
            self.inner.mailbox_monitor.send_failed();
            return Err(SequencerError::ActorGone);
        }
        self.wait_for_actor_restart_for_test(old_id).await
    }

    #[cfg(test)]
    async fn hold_next_tick_for_test(
        &self,
        hold: SequencerTestTickHold,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::TestHoldNextTick(hold, reply))
            .await
    }

    #[cfg(test)]
    async fn send_tick_for_test(&self) -> Result<(), SequencerError> {
        let actor = self.actor_ref().await?;
        self.inner.mailbox_monitor.queued();
        if actor.send_message(SequencerMsg::Tick).is_err() {
            self.inner.mailbox_monitor.send_failed();
            return Err(SequencerError::ActorGone);
        }
        Ok(())
    }

    /// Stop the sequencer actor and supervisor, waiting up to `timeout`.
    ///
    /// This uses ractor's graceful `stop_and_wait`, so the actor finishes the
    /// message it is currently handling before shutdown completes. It does not
    /// drain queued future messages.
    pub async fn stop_and_wait(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        self.inner.shutdown_requested.store(true, Ordering::Release);

        let actor = self
            .inner
            .actor
            .read()
            .expect("sequencer actor ref lock poisoned")
            .clone();

        if let Some(actor) = actor {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }

            match actor
                .stop_and_wait(Some("shutdown".to_string()), Some(remaining))
                .await
            {
                Ok(()) => self.inner.publish_actor(None),
                Err(ractor::RactorErr::Timeout) => return false,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "sequencer actor stop returned an error; continuing shutdown"
                    );
                    self.inner.publish_actor(None);
                }
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }

        match self
            .supervisor
            .stop_and_wait(Some("shutdown".to_string()), Some(remaining))
            .await
        {
            Ok(()) => true,
            Err(ractor::RactorErr::Timeout) => false,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "sequencer supervisor stop returned an error after actor shutdown"
                );
                true
            }
        }
    }

    pub async fn submit_order(
        &self,
        submission: OrderSubmission,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitOrder(submission, reply))
            .await?
    }

    /// Submit an unsigned IOC order whose concrete expiry is assigned by the
    /// sequencer actor from its committed height at admission time.
    pub async fn submit_ioc_order(
        &self,
        submission: OrderSubmission,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitIocOrder(submission, reply))
            .await?
    }

    pub async fn submit_signed_order(
        &self,
        signed: SignedOrder,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitSignedOrder(signed, reply))
            .await?
    }

    pub async fn submit_authenticated_order(
        &self,
        authenticated: AuthenticatedOrder,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitAuthenticatedOrder(authenticated, reply))
            .await?
    }

    pub async fn cancel_signed_order(&self, signed: SignedCancel) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelSignedOrder(signed, reply))
            .await?
    }

    pub async fn cancel_authenticated_order(
        &self,
        authenticated: AuthenticatedCancel,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelAuthenticatedOrder(authenticated, reply))
            .await?
    }

    pub async fn get_latest_block(&self) -> Result<Option<SealedBlock>, SequencerError> {
        if let Some(block) = self.read_query(|state| state.latest_block.clone()).await? {
            return Ok(Some(block));
        }

        let Some(height) = self.get_committed_height().await? else {
            return Ok(None);
        };
        self.get_block(height).await.map(Some)
    }

    pub async fn get_committed_height(&self) -> Result<Option<u64>, SequencerError> {
        self.read_query(|state| {
            let height = state.sequencer.height();
            (height > 0).then_some(height)
        })
        .await
    }

    pub async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Account>, SequencerError> {
        self.read_query(move |state| state.sequencer.accounts.get(account_id).cloned())
            .await
    }

    /// Read an account and its live resting-order balance reservation atomically.
    pub async fn get_account_with_reserved_balance(
        &self,
        account_id: AccountId,
    ) -> Result<Option<(Account, i64)>, SequencerError> {
        self.read_query(move |state| {
            state
                .sequencer
                .accounts
                .get(account_id)
                .cloned()
                .map(|account| {
                    let reserved = state.sequencer.reserved_balance_nanos(account_id);
                    (account, reserved)
                })
        })
        .await
    }

    /// Read the balance committed to an account's live resting orders.
    pub async fn get_reserved_balance(&self, account_id: AccountId) -> Result<i64, SequencerError> {
        self.read_query(move |state| state.sequencer.reserved_balance_nanos(account_id))
            .await
    }

    pub async fn get_state_root(&self) -> Result<[u8; 32], SequencerError> {
        self.read_query(|state| {
            crate::block::compute_complete_state_root(
                &state.sequencer.accounts,
                state.sequencer.bridge_state(),
                state.sequencer.order_book(),
                state.sequencer.markets(),
                state.sequencer.market_groups(),
                state.sequencer.market_lifecycle(),
            )
        })
        .await
    }

    pub async fn get_genesis_hash(&self) -> Result<Option<[u8; 32]>, SequencerError> {
        self.read_query(|state| state.sequencer.genesis_hash())
            .await
    }

    pub async fn get_state_proof(
        &self,
        leaf_key: Vec<u8>,
    ) -> Result<SequencerStateProof, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetStateProof(leaf_key, reply))
            .await?
    }

    pub async fn produce_block(&self) -> Result<SealedBlock, SequencerError> {
        self.rpc(SequencerMsg::ProduceBlock).await?
    }

    pub async fn create_account(&self, initial_balance: i64) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateAccount(initial_balance, reply))
            .await?
    }

    pub async fn fund_account(
        &self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::FundAccount(account_id, amount, reply))
            .await?
    }

    pub async fn submit_l1_deposit(&self, deposit: L1Deposit) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitL1Deposit(deposit, reply))
            .await?
    }

    pub async fn create_bridge_withdrawal(
        &self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateBridgeWithdrawal(request, reply))
            .await?
    }

    pub async fn create_signed_bridge_withdrawal(
        &self,
        signed: SignedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateSignedBridgeWithdrawal(signed, reply))
            .await?
    }

    pub async fn create_authenticated_bridge_withdrawal(
        &self,
        authenticated: AuthenticatedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateAuthenticatedBridgeWithdrawal(authenticated, reply))
            .await?
    }

    pub async fn apply_bridge_withdrawal_l1_event(
        &self,
        event: BridgeWithdrawalL1Event,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        self.rpc(|reply| SequencerMsg::ApplyBridgeWithdrawalL1Event(event, reply))
            .await?
    }

    pub async fn observe_bridge_l1_height(
        &self,
        height: u64,
    ) -> Result<Vec<WithdrawalLeaf>, SequencerError> {
        self.rpc(|reply| SequencerMsg::ObserveBridgeL1Height(height, reply))
            .await?
    }

    pub async fn get_bridge_state(&self) -> Result<BridgeState, SequencerError> {
        self.read_query(|state| state.sequencer.bridge_state().clone())
            .await
    }

    pub async fn get_bridge_account_key(
        &self,
        account_id: AccountId,
    ) -> Result<Option<[u8; 32]>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_key(account_id))
            .await
    }

    pub async fn get_bridge_account_id_by_key(
        &self,
        key: [u8; 32],
    ) -> Result<Option<AccountId>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_id_by_key(key))
            .await
    }

    pub async fn get_bridge_withdrawal(
        &self,
        withdrawal_id: u64,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_withdrawal(withdrawal_id).cloned())
            .await
    }

    pub async fn get_default_bridge_withdrawal_expiry(&self) -> Result<u64, SequencerError> {
        self.read_query(|state| state.sequencer.default_bridge_withdrawal_expiry_height())
            .await
    }

    pub async fn register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        self.register_pubkey_with_scheme(account_id, pubkey, AccountAuthScheme::RawP256)
            .await
    }

    pub async fn register_pubkey_with_scheme(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
        auth_scheme: AccountAuthScheme,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkey(account_id, pubkey, auth_scheme, reply))
            .await?
    }

    pub async fn lookup_registered_pubkey(
        &self,
        pubkey: PublicKey,
    ) -> Result<Option<RegisteredPubkey>, SequencerError> {
        self.read_query(move |state| state.sequencer.lookup_registered_pubkey(&pubkey))
            .await
    }

    /// Register a signing key with SYB-60 management metadata (label/scope).
    pub async fn register_pubkey_with_meta(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
        meta: RegisteredPubkey,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkeyWithMeta(account_id, pubkey, meta, reply))
            .await?
    }

    /// Register a NEW signing key from a raw-P256-signed request (SYB-229).
    pub async fn register_key_signed(
        &self,
        signed: SignedKeyRegistration,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterKeySigned(signed, reply))
            .await?
    }

    /// Register a NEW signing key from a WebAuthn-authenticated request (SYB-229).
    pub async fn register_key_authenticated(
        &self,
        authenticated: AuthenticatedKeyRegistration,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// List an account's registered signing keys with metadata (SYB-60).
    pub async fn signing_keys_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<(Vec<u8>, RegisteredPubkey)>, SequencerError> {
        self.read_query(move |state| state.sequencer.signing_keys_for_account(account_id))
            .await
    }

    /// Apply a raw-P256-signed profile update (SYB-60).
    pub async fn set_profile_signed(
        &self,
        signed: SignedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SetProfileSigned(signed, reply))
            .await?
    }

    /// Apply a WebAuthn-authenticated profile update (SYB-60).
    pub async fn set_profile_authenticated(
        &self,
        authenticated: AuthenticatedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SetProfileAuthenticated(authenticated, reply))
            .await?
    }

    /// Revoke a signing key from a raw-P256-signed request (SYB-60).
    pub async fn revoke_signing_key_signed(
        &self,
        signed: SignedKeyRevocation,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeSigningKeySigned(signed, reply))
            .await?
    }

    /// Revoke a signing key from a WebAuthn-authenticated request (SYB-60).
    pub async fn revoke_signing_key_authenticated(
        &self,
        authenticated: AuthenticatedKeyRevocation,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeSigningKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// Create a read API key from a raw-P256-signed request (SYB-60).
    pub async fn create_api_key_signed(
        &self,
        signed: SignedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateApiKeySigned(signed, reply))
            .await?
    }

    /// Create a read API key from a WebAuthn-authenticated request (SYB-60).
    pub async fn create_api_key_authenticated(
        &self,
        authenticated: AuthenticatedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateApiKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// Revoke a read API key from a raw-P256-signed request (SYB-60).
    pub async fn revoke_api_key_signed(
        &self,
        signed: SignedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeApiKeySigned(signed, reply))
            .await?
    }

    /// Revoke a read API key from a WebAuthn-authenticated request (SYB-60).
    pub async fn revoke_api_key_authenticated(
        &self,
        authenticated: AuthenticatedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeApiKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// List an account's read API keys (metadata only) (SYB-60).
    pub async fn api_keys_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<crate::account::ApiKeyRecord>, SequencerError> {
        self.read_query(move |state| state.sequencer.api_keys_for_account(account_id))
            .await
    }

    /// Resolve a bearer token hash to its owning account if the key is active
    /// (SYB-60). Read-only; used by the API bearer extractor.
    pub async fn lookup_api_key(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<AccountId>, SequencerError> {
        self.read_query(move |state| state.sequencer.lookup_api_key(&token_hash))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn list_markets(&self) -> Result<MarketSet, SequencerError> {
        self.read_query(|state| state.sequencer.markets().clone())
            .await
    }

    pub async fn create_market(&self, name: String) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarket(name, reply))
            .await?
    }

    pub async fn create_market_group(
        &self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<(u64, MarketGroup), SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketGroup(name, market_ids, reply))
            .await?
    }

    pub async fn extend_market_group(
        &self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(MarketGroup, bool), SequencerError> {
        self.rpc(|reply| SequencerMsg::ExtendMarketGroup(group_id, market_id, reply))
            .await?
    }

    pub async fn list_market_groups(&self) -> Result<Vec<MarketGroup>, SequencerError> {
        self.read_query(|state| state.sequencer.market_groups().to_vec())
            .await
    }

    pub async fn resolve_market(
        &self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        self.rpc(|reply| SequencerMsg::ResolveMarket(market_id, payout_nanos, reply))
            .await?
    }

    pub async fn resolve_market_attested(
        &self,
        market_id: MarketId,
        signed: SignedAttestation,
    ) -> Result<ResolutionRecord, SequencerError> {
        self.rpc(|reply| SequencerMsg::ResolveMarketAttested(market_id, signed, reply))
            .await?
    }

    pub async fn register_feed(
        &self,
        pubkey: FeedPubkey,
        name: String,
    ) -> Result<FeedId, SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterFeed(pubkey, name, reply))
            .await?
    }

    pub async fn get_feed(&self, id: FeedId) -> Result<Option<DataFeed>, SequencerError> {
        self.read_query(move |state| state.sequencer.feed_by_id(id).cloned())
            .await
    }

    pub async fn get_feed_by_pubkey(
        &self,
        pubkey: FeedPubkey,
    ) -> Result<Option<DataFeed>, SequencerError> {
        self.read_query(move |state| state.sequencer.feed_by_pubkey(&pubkey).cloned())
            .await
    }

    pub async fn list_feeds(&self) -> Result<Vec<DataFeed>, SequencerError> {
        self.read_query(|state| state.sequencer.lifecycle.feeds().iter().cloned().collect())
            .await
    }

    pub async fn install_template(
        &self,
        template: sybil_oracle::ResolutionTemplate,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::InstallTemplate(template, reply))
            .await?
    }

    pub async fn template_exists(&self, id: String) -> Result<bool, SequencerError> {
        self.read_query(move |state| state.sequencer.template_exists(&id))
            .await
    }

    pub async fn get_market_status(
        &self,
        market_id: MarketId,
    ) -> Result<MarketStatus, SequencerError> {
        self.read_query(move |state| state.sequencer.market_status(market_id))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_statuses(
        &self,
    ) -> Result<HashMap<MarketId, MarketStatus>, SequencerError> {
        self.read_query(|state| state.sequencer.market_statuses().clone())
            .await
    }

    pub async fn get_block(&self, height: u64) -> Result<SealedBlock, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetBlock(height, reply))
            .await?
    }

    pub async fn get_da_artifact(&self, height: u64) -> Result<DaArtifactLookup, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetDaArtifact(height, reply))
            .await?
    }

    pub async fn get_recent_blocks(&self, n: usize) -> Result<Vec<SealedBlock>, SequencerError> {
        self.read_query(move |state| {
            let cap = state.sequencer.config.block_history_capacity;
            let take = n.min(cap);
            state
                .block_history
                .iter()
                .rev()
                .take(take)
                .cloned()
                .collect()
        })
        .await
    }

    pub async fn get_block_page(
        &self,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<SealedBlock>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetBlockPage(before_height, limit, reply))
            .await?
    }

    pub async fn subscribe_blocks(
        &self,
    ) -> Result<broadcast::Receiver<SealedBlock>, SequencerError> {
        Ok(self.inner.block_broadcast.subscribe())
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_market_prices(&self) -> Result<HashMap<MarketId, Vec<Nanos>>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().last_clearing_prices().clone())
            .await
    }

    pub async fn get_portfolio(
        &self,
        account_id: AccountId,
    ) -> Result<PortfolioSummary, SequencerError> {
        self.read_query(move |state| state.sequencer.portfolio_summary(account_id))
            .await?
    }

    /// Read a portfolio and its resting-order balance reservation atomically.
    pub async fn get_portfolio_with_reserved_balance(
        &self,
        account_id: AccountId,
    ) -> Result<(PortfolioSummary, i64), SequencerError> {
        self.read_query(move |state| {
            let portfolio = state.sequencer.portfolio_summary(account_id)?;
            let reserved = state.sequencer.reserved_balance_nanos(account_id);
            Ok((portfolio, reserved))
        })
        .await?
    }

    /// Read the components of the private account summary from one actor state.
    pub async fn get_account_summary_with_reserved_balance(
        &self,
        account_id: AccountId,
    ) -> Result<Option<(Account, PortfolioSummary, i64)>, SequencerError> {
        self.read_query(move |state| {
            let account = state.sequencer.accounts.get(account_id).cloned()?;
            let portfolio = state.sequencer.portfolio_summary(account_id).ok()?;
            let reserved = state.sequencer.reserved_balance_nanos(account_id);
            Some((account, portfolio, reserved))
        })
        .await
    }

    pub async fn create_market_with_metadata(
        &self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketWithMetadata(name, metadata, reply))
            .await?
    }

    pub async fn get_market_metadata(
        &self,
        market_id: MarketId,
    ) -> Result<Option<MarketMetadata>, SequencerError> {
        self.read_query(move |state| state.sequencer.market_metadata(market_id).cloned())
            .await
    }

    pub async fn get_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<PriceHistoryPage, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetPriceHistory(market_id, from_ms, to_ms, before_height, limit, reply)
        })
        .await?
    }

    pub async fn get_price_candles(
        &self,
        market_id: MarketId,
        resolution_secs: u32,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<PriceCandlePage, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetPriceCandles(
                market_id,
                resolution_secs,
                from_ms,
                to_ms,
                before_ms,
                limit,
                reply,
            )
        })
        .await?
    }

    pub async fn get_account_fills(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountFills(account_id, market_id, limit, offset, reply))
            .await
    }

    pub async fn get_account_fills_after(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        after: Option<AccountFillCursor>,
        limit: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::GetAccountFillsAfter(account_id, market_id, after, limit, reply)
        })
        .await
    }

    /// Equity series for an account, restricted to points with
    /// `timestamp_ms >= since_ms` (pass `0` for the full series). The range is
    /// applied in the durable store scan rather than by the caller.
    pub async fn get_equity_series(
        &self,
        account_id: AccountId,
        since_ms: u64,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetEquitySeries(account_id, since_ms, reply))
            .await
    }

    /// Ranked leaderboard (SYB-59) over a window. `since_ms == 0` is all-time;
    /// a non-zero `since_ms` computes per-account windowed PnL from the durable
    /// equity store. Returns at most `limit` rows, PnL-descending with an
    /// account-id tie-break.
    pub async fn leaderboard(
        &self,
        since_ms: u64,
        limit: usize,
    ) -> Result<Vec<LeaderboardRow>, SequencerError> {
        self.rpc(|reply| SequencerMsg::Leaderboard(since_ms, limit, reply))
            .await
    }

    pub async fn get_account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountEvents(account_id, limit, before, category, reply))
            .await
    }

    pub async fn list_auto_resolution_records(
        &self,
    ) -> Result<Vec<AutoResolutionRecord>, SequencerError> {
        self.rpc(SequencerMsg::ListAutoResolutionRecords).await?
    }

    pub async fn put_auto_resolution_record(
        &self,
        record: AutoResolutionRecord,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::PutAutoResolutionRecord(record, reply))
            .await?
    }

    pub async fn get_pending_orders(
        &self,
        account_id: Option<AccountId>,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.read_query(move |state| state.sequencer.pending_orders_info(account_id))
            .await
    }

    pub async fn get_market_order_book(
        &self,
        market_id: MarketId,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.read_query(move |state| state.sequencer.market_orderbook(market_id))
            .await
    }

    pub async fn search_markets(
        &self,
        query: MarketSearchQuery,
    ) -> Result<Vec<MarketSearchResult>, SequencerError> {
        self.read_query(move |state| state.handle_search_markets(query))
            .await
    }

    pub async fn get_market_volume(&self, market_id: MarketId) -> Result<u64, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().market_volume(market_id))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_volumes(&self) -> Result<HashMap<MarketId, u64>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().market_volumes().clone())
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_metadata(
        &self,
    ) -> Result<HashMap<MarketId, MarketMetadata>, SequencerError> {
        self.read_query(|state| state.sequencer.market_metadata_all().clone())
            .await
    }

    pub async fn pause_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::PauseBlockProduction).await
    }

    pub async fn resume_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::ResumeBlockProduction).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_trader_counts(&self) -> Result<HashMap<MarketId, u32>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().all_trader_counts())
            .await
    }

    /// Platform-wide unique-trader counts `(all_time, last_24h)`. Caller
    /// passes `now_ms` so test paths can synthesise the cutoff.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_trader_counts(
        &self,
        now_ms: u64,
    ) -> Result<(u32, u32), SequencerError> {
        self.read_query(move |state| {
            let all_time = state.sequencer.analytics().platform_trader_count();
            let last_24h = state
                .sequencer
                .analytics()
                .platform_trader_24h_count(now_ms);
            (all_time, last_24h)
        })
        .await
    }

    #[tracing::instrument(skip_all, fields(markets = market_ids.len()))]
    pub async fn get_event_trader_count(
        &self,
        market_ids: Vec<MarketId>,
    ) -> Result<u32, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().event_trader_count(&market_ids))
            .await
    }

    #[tracing::instrument(skip_all, fields(market_id = market_id.0))]
    pub async fn get_open_batch_placers(&self, market_id: MarketId) -> Result<u32, SequencerError> {
        self.read_query(move |state| state.sequencer.open_batch_unique_placers(market_id))
            .await
    }

    /// Platform-wide volume `(all_time, last_24h)` with caller-supplied
    /// `now_ms` so 24h-bucket cutoff is deterministic for tests.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_volumes(&self, now_ms: u64) -> Result<(u64, u64), SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().platform_volumes(now_ms))
            .await
    }

    /// All-market 24h volume map — single round-trip companion to
    /// `get_all_market_volumes` (the cumulative variant).
    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_volumes_24h(
        &self,
        now_ms: u64,
    ) -> Result<HashMap<MarketId, u64>, SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().all_market_volumes_24h(now_ms))
            .await
    }

    /// All-market clearing prices `n` hours ago in one shot — populates
    /// `MarketResponse.{yes,no}_price_24h_ago_nanos` in a single round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_prices_n_hours_ago(
        &self,
        n: u64,
        now_ms: u64,
    ) -> Result<HashMap<MarketId, (u64, u64)>, SequencerError> {
        self.read_query(move |state| {
            state
                .sequencer
                .analytics()
                .all_market_prices_n_hours_ago(n, now_ms)
        })
        .await
    }

    /// All-market liquidity averages + the band width currently in effect.
    /// Bulks `MarketResponse.liquidity_avg10_nanos` + `.liquidity_band_nanos`
    /// into one round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_liquidity_snapshot(
        &self,
    ) -> Result<(HashMap<MarketId, u64>, u64), SequencerError> {
        self.read_query(|state| {
            let liq = state.sequencer.analytics().all_liquidity_avg10();
            let band = state.sequencer.liquidity_band_nanos();
            (liq, band)
        })
        .await
    }

    /// All-market all-time order stats — populates
    /// `MarketResponse.orders_*_total` in one round-trip.
    #[tracing::instrument(skip_all)]
    pub async fn get_order_stats_by_market(
        &self,
    ) -> Result<HashMap<MarketId, crate::aggregates::OrderStats>, SequencerError> {
        self.read_query(|state| state.sequencer.analytics().all_market_order_stats())
            .await
    }

    /// Platform order stats `(all_time, last_24h)` for the activity hero.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_order_stats(
        &self,
        now_ms: u64,
    ) -> Result<(crate::aggregates::OrderStats, crate::aggregates::OrderStats), SequencerError>
    {
        self.read_query(move |state| state.sequencer.analytics().platform_order_stats(now_ms))
            .await
    }

    /// Platform welfare `(all_time, last_24h)` in nanos for the activity hero.
    #[tracing::instrument(skip_all)]
    pub async fn get_platform_welfare(&self, now_ms: u64) -> Result<(i64, i64), SequencerError> {
        self.read_query(move |state| state.sequencer.analytics().platform_welfare(now_ms))
            .await
    }

    /// Cached indicative snapshot for one market (C2). Returns a default
    /// `(None, None, 0, 0)` snapshot if the market hasn't been touched by
    /// the shadow-solver yet — e.g. on cold start or for markets with no
    /// resting orders.
    #[tracing::instrument(skip_all)]
    pub async fn get_indicative(
        &self,
        market_id: MarketId,
    ) -> Result<IndicativeSnapshot, SequencerError> {
        self.read_query(move |state| {
            state
                .indicative_cache
                .get(&market_id)
                .cloned()
                .unwrap_or_default()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::bridge::L1WithdrawalStatus;
    use crate::crypto::sign_cancel;
    use crate::market_info::ResolutionConfig;
    use crate::sequencer::SequencerConfig;
    use crate::system_event::SystemEvent;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use sybil_oracle::{AdminOracle, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_store_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sybil-actor-{prefix}-{}-{unique}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn make_test_sequencer() -> (BlockSequencer, AccountId) {
        make_test_sequencer_with_config(SequencerConfig::default())
    }

    fn make_test_sequencer_with_config(config: SequencerConfig) -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        let oracle = Arc::new(AdminOracle::new());
        (
            BlockSequencer::with_default_solver(accounts, markets, vec![], oracle, config),
            aid,
        )
    }

    #[test]
    fn mailbox_monitor_tracks_depth_without_underflow() {
        let monitor = MailboxMonitor::new("test_actor", 2, 4);
        assert_eq!(monitor.depth(), 0);

        monitor.queued();
        monitor.queued();
        assert_eq!(monitor.depth(), 2);

        monitor.started();
        assert_eq!(monitor.depth(), 1);

        monitor.started();
        monitor.started();
        assert_eq!(monitor.depth(), 0);

        monitor.queued();
        monitor.reset();
        assert_eq!(monitor.depth(), 0);
    }

    #[tokio::test]
    async fn test_spawn_and_produce_block() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
    }

    #[tokio::test]
    async fn produce_block_rpc_returns_error_when_paused() {
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn(seq);

        let first = handle.produce_block().await.unwrap();
        assert_eq!(first.canonical.header.height, 1);

        handle.pause_block_production().await.unwrap();
        let error = match handle.produce_block().await {
            Ok(block) => panic!(
                "expected paused error, got block height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(matches!(error, SequencerError::BlockProductionPaused));

        let latest = handle
            .get_latest_block()
            .await
            .unwrap()
            .expect("first block remains latest");
        assert_eq!(latest.canonical.header.height, 1);
    }

    #[tokio::test]
    async fn produce_block_rpc_returns_error_when_persistence_fails() {
        let path = temp_store_path("produce-persist-failure");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        store.inject_next_save_block_fault(crate::store::StoreFaultPoint::BeforeQmdbPersist);
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc_for_test(seq, store);

        let error = match handle.produce_block().await {
            Ok(block) => panic!(
                "expected persistence error, got block height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(
            matches!(error, SequencerError::Persistence(ref message) if message.contains("BeforeQmdbPersist")),
            "expected persistence error from injected fault, got {error:?}"
        );
        assert!(
            handle.get_latest_block().await.unwrap().is_none(),
            "failed persistence attempt must not publish a stale or prepared block"
        );

        let block = handle.produce_block().await.unwrap();
        assert_eq!(
            block.canonical.header.height, 1,
            "retry after failed persistence should commit the same next height"
        );
    }

    #[tokio::test]
    async fn test_submit_and_produce() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let handle = SequencerHandle::spawn(seq);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        handle.submit_order(sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
        assert!(block.canonical.header.order_count >= 1);
    }

    #[tokio::test]
    async fn ioc_expiry_uses_admit_height_after_latest_read_race() {
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let counterparty = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let market = markets.add_binary("IOC race");
        let sequencer = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            config,
        );
        let handle = SequencerHandle::spawn(sequencer);

        // Model the old API's separate latest-block RPC, then commit a block
        // before admission. Under the old path, height 1 would be carried on
        // the order and rejected because the next eligible batch is height 2.
        let stale_expiry = handle
            .get_latest_block()
            .await
            .unwrap()
            .map(|block| block.canonical.header.height)
            .unwrap_or(0)
            .saturating_add(1);
        let intervening = handle.produce_block().await.unwrap();
        assert_eq!(intervening.canonical.header.height, stale_expiry);

        let mut ioc = outcome_buy(&markets, 0, market, 0, 600_000_000, 5);
        ioc.expires_at_block = Some(stale_expiry);
        let ioc_order_id = handle
            .submit_ioc_order(OrderSubmission {
                account_id: buyer,
                orders: vec![ioc],
                mm_constraint: None,
            })
            .await
            .expect("admit-time IOC expiry must replace the stale API-style value")[0];

        handle
            .submit_order(OrderSubmission {
                account_id: counterparty,
                orders: vec![outcome_buy(&markets, 0, market, 1, 600_000_000, 5)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, stale_expiry + 1);
        assert!(
            block
                .canonical
                .fills
                .iter()
                .any(|fill| fill.order_id == ioc_order_id && fill.fill_qty.0 > 0),
            "IOC should participate in and match in its first admit-eligible batch"
        );
        assert!(handle
            .get_pending_orders(Some(buyer))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn per_account_submission_rate_limit_rejects_runaway_client() {
        let config = SequencerConfig {
            max_submissions_per_account_per_second: 1,
            submission_burst_per_account: 1,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 1_000,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = |qty| OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, qty)],
            mm_constraint: None,
        };

        handle.submit_order(sub(1)).await.unwrap();
        let err = handle.submit_order(sub(1)).await.unwrap_err();
        assert!(matches!(err, SequencerError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn global_submission_rate_limit_bounds_many_account_floods() {
        let config = SequencerConfig {
            max_global_submissions_per_second: 1,
            global_submission_burst: 1,
            max_submissions_per_account_per_second: 1_000,
            submission_burst_per_account: 1_000,
            ..SequencerConfig::default()
        };
        let mut accounts = AccountStore::new();
        let a = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let b = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            config,
        );
        let handle = SequencerHandle::spawn(seq);

        let sub = |account_id| OrderSubmission {
            account_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        handle.submit_order(sub(a)).await.unwrap();
        let err = handle.submit_order(sub(b)).await.unwrap_err();
        assert!(matches!(err, SequencerError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn open_order_cap_rejects_excess_resting_orders() {
        let config = SequencerConfig {
            max_open_orders_per_account: 1,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = |qty| OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, qty)],
            mm_constraint: None,
        };

        handle.submit_order(sub(1)).await.unwrap();
        let err = handle.submit_order(sub(1)).await.unwrap_err();
        assert!(matches!(
            err,
            SequencerError::TooManyOpenOrders { account_id, limit: 1 } if account_id == aid
        ));
    }

    #[tokio::test]
    async fn pending_bundle_cap_does_not_block_direct_orders() {
        let config = SequencerConfig {
            max_pending_bundles: 0,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let deferred = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&ms, 0, m0, 0, 500_000_000, 1),
                outcome_buy(&ms, 0, m0, 1, 500_000_000, 1),
            ],
            mm_constraint: None,
        };
        let err = handle.submit_order(deferred).await.unwrap_err();
        assert!(matches!(err, SequencerError::MempoolFull));

        let direct = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };
        handle.submit_order(direct).await.unwrap();
    }

    #[tokio::test]
    async fn max_orders_per_submission_bounds_request_amplification() {
        let config = SequencerConfig {
            max_orders_per_submission: 1,
            ..SequencerConfig::default()
        };
        let (seq, aid) = make_test_sequencer_with_config(config);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&ms, 0, m0, 0, 500_000_000, 1),
                outcome_buy(&ms, 0, m0, 1, 500_000_000, 1),
            ],
            mm_constraint: None,
        };
        let err = handle.submit_order(sub).await.unwrap_err();
        assert!(matches!(
            err,
            SequencerError::TooManyOrdersInSubmission { count: 2, limit: 1 }
        ));
    }

    #[tokio::test]
    async fn test_get_state_root() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let root = handle.get_state_root().await.unwrap();
        assert_ne!(root, [0u8; 32]); // non-empty accounts -> non-zero root
    }

    #[tokio::test]
    async fn test_get_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle.get_account(aid).await.unwrap();
        assert!(account.is_some());
        assert_eq!(account.unwrap().balance, 100 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_get_latest_block_none_initially() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_block_after_produce() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().canonical.header.height, 1);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        drop(handle);

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn stop_and_wait_drains_in_flight_tick() {
        let path = temp_store_path("stop-drain-in-flight");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc_for_test(seq, store.clone());

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        handle
            .hold_next_tick_for_test(SequencerTestTickHold {
                started: started_tx,
                release: release_rx,
            })
            .await
            .unwrap();

        handle.send_tick_for_test().await.unwrap();
        started_rx.await.unwrap();

        let stopper = tokio::spawn({
            let handle = handle.clone();
            async move { handle.stop_and_wait(TEST_ACTOR_TIMEOUT).await }
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            !stopper.is_finished(),
            "stop_and_wait returned before the in-flight Tick was released"
        );

        release_tx.send(()).unwrap();
        assert!(stopper.await.unwrap());

        let restored = store
            .load_state()
            .await
            .unwrap()
            .expect("in-flight Tick should persist a committed block");
        assert_eq!(restored.height, 1);
        assert!(matches!(
            handle.get_latest_block().await,
            Err(SequencerError::ActorGone)
        ));
    }

    #[tokio::test]
    async fn test_block_chain_via_actor() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block1 = handle.produce_block().await.unwrap();
        assert_eq!(block1.canonical.header.height, 1);
        assert_eq!(block1.canonical.header.parent_hash, [0u8; 32]); // genesis

        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block2.canonical.header.height, 2);
        let expected = crate::block::hash_header(&block1.canonical.header);
        assert_eq!(block2.canonical.header.parent_hash, expected);
    }

    #[tokio::test]
    async fn test_state_root_changes_after_fill() {
        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((m0, 0), 100);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        let root_before = handle.get_state_root().await.unwrap();

        let buy_sub = OrderSubmission {
            account_id: buyer,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 5)],
            mm_constraint: None,
        };
        handle.submit_order(buy_sub).await.unwrap();

        let sell_sub = OrderSubmission {
            account_id: seller,
            orders: vec![matching_engine::outcome_sell(
                &markets,
                0,
                m0,
                0,
                400_000_000,
                5,
            )],
            mm_constraint: None,
        };
        handle.submit_order(sell_sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();

        if block.analytics.orders_filled > 0 {
            let root_after = handle.get_state_root().await.unwrap();
            assert_ne!(root_before, root_after);
        }
    }

    #[tokio::test]
    async fn test_create_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 50 * NANOS_PER_DOLLAR as i64);

        let fetched = handle.get_account(account.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().balance, 50 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_fund_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 125 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_system_events_emitted_for_create_and_fund() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        handle
            .fund_account(account.id, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.system_events.len(), 2);

        match &block.canonical.system_events[0] {
            SystemEvent::CreateAccount {
                account_id,
                initial_balance,
            } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*initial_balance, 50 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected CreateAccount event, got {:?}", other),
        }

        match &block.canonical.system_events[1] {
            SystemEvent::Deposit { account_id, amount } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*amount, 25 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected Deposit event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn acknowledged_control_plane_writes_survive_restart_before_next_block() {
        let path = temp_store_path("control-plane-wal");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        baseline.produce_block(Vec::new(), 1);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let created = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey.clone()).await.unwrap();

        let plain_market = handle
            .create_market("plain wal market".to_string())
            .await
            .unwrap();
        let metadata = MarketMetadata {
            description: "metadata survives WAL replay".to_string(),
            category: "regression".to_string(),
            tags: vec!["wal".to_string()],
            resolution_criteria: "resolve by admin".to_string(),
            expiry_timestamp_ms: 9_999_999,
            created_at_ms: 123_456,
            resolution_config: Some(ResolutionConfig {
                template: "wal_template".to_string(),
            }),
            committed_metadata_digest: None,
        };
        let metadata_market = handle
            .create_market_with_metadata("metadata wal market".to_string(), metadata.clone())
            .await
            .unwrap();
        let group_market = handle
            .create_market("group wal market".to_string())
            .await
            .unwrap();
        let (group_id, group) = handle
            .create_market_group("wal group".to_string(), vec![group_market])
            .await
            .unwrap();
        assert_eq!(group_id, 0);
        assert_eq!(group.markets, vec![group_market]);
        let (extended_group, inserted) = handle
            .extend_market_group(group_id, metadata_market)
            .await
            .unwrap();
        assert!(inserted);
        assert_eq!(extended_group.markets, vec![group_market, metadata_market]);

        let feed_pubkey = FeedPubkey(vec![2u8; 33]);
        let feed_id = handle
            .register_feed(feed_pubkey.clone(), "wal_feed".to_string())
            .await
            .unwrap();
        let template = ResolutionTemplate {
            id: TemplateId("wal_template".to_string()),
            policy: ResolutionPolicy::Immediate { feed_id },
        };
        handle.install_template(template.clone()).await.unwrap();

        let resolved_market = plain_market;
        handle
            .resolve_market(resolved_market, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();

        let markets = handle.list_markets().await.unwrap();
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(
                &markets,
                0,
                MarketId::new(0),
                0,
                500_000_000,
                1,
            )],
            mm_constraint: None,
        };
        handle.submit_order(sub).await.unwrap();
        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let signed_cancel = sign_cancel(aid, pending[0].order_id, 1, genesis_hash, &signing_key);
        handle.cancel_signed_order(signed_cancel).await.unwrap();

        let assert_replayed = |restored_seq: &BlockSequencer| {
            assert!(
                restored_seq.accounts.get(created.id).is_some(),
                "created account was acknowledged but vanished after restore"
            );
            assert_eq!(
                restored_seq.accounts.get(aid).unwrap().balance,
                125 * NANOS_PER_DOLLAR as i64,
                "funding was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq.lookup_pubkey(&pubkey),
                Some(aid),
                "pubkey registration was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.markets().get(plain_market).is_some(),
                "created market was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq.market_metadata(metadata_market),
                Some(&metadata),
                "market metadata was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq
                    .market_groups()
                    .iter()
                    .any(|group| group.name == "wal group"
                        && group.markets == vec![group_market, metadata_market]),
                "market group extension was acknowledged but not replayed after restore"
            );
            assert!(
                matches!(
                    restored_seq.market_status(resolved_market),
                    MarketStatus::Resolved { .. }
                ),
                "market resolution was acknowledged but not replayed after restore"
            );
            assert_eq!(
                restored_seq
                    .feed_by_pubkey(&feed_pubkey)
                    .map(|feed| feed.id),
                Some(feed_id),
                "feed registration was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.template_exists("wal_template"),
                "template installation was acknowledged but not replayed after restore"
            );
            assert!(
                restored_seq.pending_orders_info(Some(aid)).is_empty(),
                "signed cancel was acknowledged but the order reappeared after restore"
            );
        };

        const EXPECTED_CONTROL_PLANE_COMMANDS: usize = 13;
        for restart_round in 0..3 {
            let restored = store.load_state().await.unwrap().unwrap();
            assert_eq!(
                restored.control_plane_log.len(),
                EXPECTED_CONTROL_PLANE_COMMANDS,
                "restart round {restart_round} should see every acknowledged control-plane command before commit"
            );
            assert!(
                restored.control_plane_log.iter().any(|command| matches!(
                    command,
                    ControlPlaneCommand::ExtendMarketGroup {
                        group_id,
                        market_id
                    } if *group_id == 0 && *market_id == metadata_market
                )),
                "restart round {restart_round} should replay the market group extension"
            );
            assert!(
                restored.control_plane_log.iter().any(|command| matches!(
                    command,
                    ControlPlaneCommand::AdvanceReplayNonce {
                        account_id,
                        nonce: 1
                    } if *account_id == aid
                )),
                "restart round {restart_round} should replay the signed cancel nonce"
            );
            assert_eq!(
                restored.admit_log.len(),
                1,
                "restart round {restart_round} should replay the direct admit before the cancel command"
            );
            let restored_seq =
                BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
            assert_replayed(&restored_seq);
            let created_history =
                restored_seq
                    .analytics()
                    .pending_account_history(created.id, None, Some("funding"));
            assert!(
                created_history
                    .iter()
                    .any(|event| matches!(event.kind, crate::aggregates::HistoryKind::Created)),
                "restart round {restart_round} should expose pending account creation history"
            );
            let funding_history =
                restored_seq
                    .analytics()
                    .pending_account_history(aid, None, Some("funding"));
            assert!(
                funding_history
                    .iter()
                    .any(|event| matches!(event.kind, crate::aggregates::HistoryKind::Deposit)),
                "restart round {restart_round} should expose pending deposit history"
            );

            let mut probe = restored_seq.clone();
            let probe_block = probe
                .produce_block(Vec::new(), 10_000 + restart_round)
                .block;
            assert!(
                probe_block
                    .system_events
                    .iter()
                    .any(|event| matches!(event, SystemEvent::CreateAccount { account_id, .. } if *account_id == created.id)),
                "restart round {restart_round} should stage the uncommitted account creation event"
            );
            assert!(
                probe_block
                    .system_events
                    .iter()
                    .any(|event| matches!(event, SystemEvent::Deposit { account_id, .. } if *account_id == aid)),
                "restart round {restart_round} should stage the uncommitted funding event"
            );
            assert!(
                probe_block.system_events.iter().any(|event| matches!(
                    event,
                    SystemEvent::MarketResolved { market_id, .. } if *market_id == resolved_market
                )),
                "restart round {restart_round} should stage the uncommitted resolution event"
            );
            assert!(
                probe_block.system_events.iter().any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
                "restart round {restart_round} should stage the uncommitted cancellation event"
            );
        }

        let committed = handle.produce_block().await.unwrap();
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::MarketResolved { market_id, .. } if *market_id == resolved_market
                )),
            "committed block should include the WAL-replayed market resolution"
        );
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
            "committed block should include the WAL-replayed cancellation"
        );

        for restart_round in 0..3 {
            let restored_after_commit = store.load_state().await.unwrap().unwrap();
            assert!(
                restored_after_commit.control_plane_log.is_empty(),
                "control-plane WAL should clear once a block commits the writes"
            );
            assert!(
                restored_after_commit.admit_log.is_empty(),
                "admit WAL should clear once a block commits the direct admit and cancel"
            );
            let restored_seq = BlockSequencer::restore(
                restored_after_commit,
                Arc::new(AdminOracle::new()),
                config.clone(),
            );
            assert_replayed(&restored_seq);

            let mut probe = restored_seq.clone();
            let probe_block = probe
                .produce_block(Vec::new(), 20_000 + restart_round)
                .block;
            assert!(
                probe_block.system_events.is_empty(),
                "restart round {restart_round} after commit must not duplicate control-plane system events"
            );
        }
    }

    #[tokio::test]
    async fn bridge_withdrawal_replays_after_control_plane_cancel_wal() {
        let path = temp_store_path("bridge-after-cancel-wal");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        baseline.register_pubkey(aid, pubkey).unwrap();
        baseline.produce_block(Vec::new(), 1);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let markets = handle.list_markets().await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(
                    &markets,
                    1,
                    MarketId::new(0),
                    0,
                    500_000_000,
                    100,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);

        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let signed_cancel = sign_cancel(aid, pending[0].order_id, 1, genesis_hash, &signing_key);
        handle.cancel_signed_order(signed_cancel).await.unwrap();

        let withdrawal = handle
            .create_bridge_withdrawal(BridgeWithdrawalRequest {
                account_id: aid,
                chain_id: 1,
                vault_address: [0x10; 20],
                recipient: [0x40; 20],
                token_address: [0x20; 20],
                amount_token_units: 80_000_000,
                expiry_height: 10,
            })
            .await
            .unwrap();
        assert_eq!(withdrawal.amount_nanos, 80 * NANOS_PER_DOLLAR);

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.admit_log.len(), 1);
        assert_eq!(restored.control_plane_log.len(), 2);
        assert!(restored.control_plane_log.iter().any(|command| matches!(
            command,
            ControlPlaneCommand::AdvanceReplayNonce {
                account_id,
                nonce: 1
            } if *account_id == aid
        )));
        assert_eq!(restored.pending_bridge_withdrawals.len(), 1);
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());

        assert!(
            restored_seq.pending_orders_info(Some(aid)).is_empty(),
            "cancel must replay before the bridge withdrawal validates"
        );
        assert_eq!(
            restored_seq.bridge_withdrawal(withdrawal.withdrawal_id),
            Some(&withdrawal)
        );
        assert_eq!(
            restored_seq.accounts.get(aid).unwrap().balance,
            20 * NANOS_PER_DOLLAR as i64
        );

        let committed = handle.produce_block().await.unwrap();
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::OrderCancelled { account_id, .. } if *account_id == aid
                )),
            "committed block should include the WAL-replayed cancellation"
        );
        assert!(
            committed
                .canonical
                .system_events
                .iter()
                .any(|event| matches!(
                    event,
                    SystemEvent::WithdrawalCreated { account_id, withdrawal: leaf, .. }
                        if *account_id == aid && leaf.withdrawal_id == withdrawal.withdrawal_id
                )),
            "committed block should include the WAL-replayed bridge withdrawal"
        );
    }

    #[tokio::test]
    async fn acknowledged_withdrawal_cancel_refund_survives_actor_crash_before_block() {
        let path = temp_store_path("bridge-l1-refund-wal");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        baseline.produce_block(Vec::new(), 1);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config);
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let withdrawal = handle
            .create_bridge_withdrawal(BridgeWithdrawalRequest {
                account_id: aid,
                chain_id: 1,
                vault_address: [0x10; 20],
                recipient: [0x40; 20],
                token_address: [0x20; 20],
                amount_token_units: 10_000_000,
                expiry_height: 100,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().balance,
            90 * NANOS_PER_DOLLAR as i64
        );

        let cancelled = BridgeWithdrawalL1Event {
            nullifier: withdrawal.nullifier,
            status: L1WithdrawalStatus::Cancelled,
            event_at_unix: 1_700_000_005,
            executable_at_unix: None,
            tx_hash: Some([0xAB; 32]),
            l1_block_height: 5,
        };
        let refunded = handle
            .apply_bridge_withdrawal_l1_event(cancelled.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(refunded.l1_status, L1WithdrawalStatus::Refunded);
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().balance,
            100 * NANOS_PER_DOLLAR as i64
        );
        let pre_crash = store.load_state().await.unwrap().unwrap();
        assert_eq!(pre_crash.pending_bridge_l1_inputs.len(), 1);

        handle.crash_actor_for_test().await.unwrap();
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().balance,
            100 * NANOS_PER_DOLLAR as i64,
            "WAL replay must restore the refund exactly once"
        );
        assert_eq!(
            handle
                .get_bridge_state()
                .await
                .unwrap()
                .withdrawals
                .get(&withdrawal.withdrawal_id)
                .unwrap()
                .l1_status,
            L1WithdrawalStatus::Refunded
        );

        let duplicate = handle
            .apply_bridge_withdrawal_l1_event(cancelled)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(duplicate.l1_status, L1WithdrawalStatus::Refunded);
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().balance,
            100 * NANOS_PER_DOLLAR as i64,
            "duplicate cancellation after restore must not double-credit"
        );

        let committed = handle.produce_block().await.unwrap();
        assert_eq!(
            committed
                .canonical
                .system_events
                .iter()
                .filter(|event| matches!(event, SystemEvent::WithdrawalRefunded { withdrawal_id, .. } if *withdrawal_id == withdrawal.withdrawal_id))
                .count(),
            1
        );
        let after_commit = store.load_state().await.unwrap().unwrap();
        assert!(after_commit.pending_bridge_l1_inputs.is_empty());
        assert!(after_commit.bridge_state.withdrawals.is_empty());
        assert_eq!(
            after_commit.accounts.get(aid).unwrap().balance,
            100 * NANOS_PER_DOLLAR as i64
        );
        assert!(handle.stop_and_wait(Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn test_fund_nonexistent_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle.fund_account(AccountId(999), 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_pubkey_and_signed_order() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());

        handle.register_pubkey(aid, pubkey).await.unwrap();
        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let signed = crate::crypto::sign_order(&order, 1, genesis_hash, &signing_key);
        handle.submit_signed_order(signed).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert!(block.canonical.header.order_count >= 1);
    }

    #[tokio::test]
    async fn test_signed_order_replay_across_fresh_genesis_rejected() {
        let (mut seq_a, _) = make_test_sequencer();
        let (mut seq_b, aid_b) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        seq_b.register_pubkey(aid_b, pubkey).unwrap();

        seq_a.produce_block(Vec::new(), 1_000);
        seq_b.produce_block(Vec::new(), 2_000);
        let genesis_a = seq_a.genesis_hash().unwrap();
        let genesis_b = seq_b.genesis_hash().unwrap();
        assert_ne!(genesis_a, genesis_b);

        let handle_b = SequencerHandle::spawn(seq_b);
        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let err = handle_b
            .submit_signed_order(crate::crypto::sign_order(
                &order,
                1,
                genesis_a,
                &signing_key,
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, SequencerError::InvalidSignature));
    }

    #[tokio::test]
    async fn test_signed_key_revocation_replay_across_fresh_genesis_rejected() {
        // SYB-231: a revocation signed under genesis A must be rejected by a
        // fresh store with genesis B, mirroring the order-replay guard.
        let (mut seq_a, _) = make_test_sequencer();
        let (mut seq_b, aid_b) = make_test_sequencer();

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        let target_bytes = pubkey.compressed_bytes();
        seq_b.register_pubkey(aid_b, pubkey).unwrap();

        seq_a.produce_block(Vec::new(), 1_000);
        seq_b.produce_block(Vec::new(), 2_000);
        let genesis_a = seq_a.genesis_hash().unwrap();
        let genesis_b = seq_b.genesis_hash().unwrap();
        assert_ne!(genesis_a, genesis_b);

        let handle_b = SequencerHandle::spawn(seq_b);
        // Revocation signed under genesis A replayed against genesis-B store.
        let signed =
            crate::crypto::sign_key_revocation(aid_b, target_bytes, 1, genesis_a, &signing_key);
        let err = handle_b
            .revoke_signing_key_signed(signed)
            .await
            .unwrap_err();
        assert!(matches!(err, SequencerError::InvalidSignature));
    }

    #[tokio::test]
    async fn test_signed_order_replay_rejected_and_nonce_gap_allowed() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();
        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        handle
            .submit_signed_order(crate::crypto::sign_order(
                &order,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap();

        let replay_error = handle
            .submit_signed_order(crate::crypto::sign_order(
                &order,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap_err();
        assert!(matches!(
            replay_error,
            SequencerError::ReplayNonceStale {
                account_id,
                nonce: 1,
                last_nonce: 1
            } if account_id == aid
        ));

        handle
            .submit_signed_order(crate::crypto::sign_order(
                &order,
                10,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_cancel_signed_order_by_owner() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);

        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let cancel =
            crate::crypto::sign_cancel(aid, pending[0].order_id, 2, genesis_hash, &signing_key);
        handle.cancel_signed_order(cancel).await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn failed_cancel_validation_does_not_advance_replay_nonce() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);
        let order_id = pending[0].order_id;
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();

        let error = handle
            .cancel_signed_order(crate::crypto::sign_cancel(
                aid,
                order_id + 1,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap_err();
        assert!(matches!(error, SequencerError::OrderNotFound));
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().last_nonce,
            0
        );

        handle
            .cancel_signed_order(crate::crypto::sign_cancel(
                aid,
                order_id,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .expect("the same nonce should still authorize a valid cancel");

        assert!(handle
            .get_pending_orders(Some(aid))
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().last_nonce,
            1
        );
    }

    #[tokio::test]
    async fn test_signed_cancel_replay_is_rejected_as_order_not_found() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);
        let order_id = pending[0].order_id;
        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();

        handle
            .cancel_signed_order(crate::crypto::sign_cancel(
                aid,
                order_id,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap();
        let replay_error = handle
            .cancel_signed_order(crate::crypto::sign_cancel(
                aid,
                order_id,
                1,
                genesis_hash,
                &signing_key,
            ))
            .await
            .unwrap_err();
        // The applied cancel removed its target, so the replay fails order
        // validation (SYB-263: cancel validates before the nonce is consulted)
        // and the nonce is left untouched.
        assert!(matches!(replay_error, SequencerError::OrderNotFound));
        assert_eq!(
            handle.get_account(aid).await.unwrap().unwrap().last_nonce,
            1
        );
    }

    #[tokio::test]
    async fn test_cancel_signed_order_rejects_wrong_account_claim() {
        let (seq, aid) = make_test_sequencer();
        let other = AccountId(aid.0 + 1);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let cancel =
            crate::crypto::sign_cancel(other, pending[0].order_id, 2, genesis_hash, &signing_key);
        let error = handle.cancel_signed_order(cancel).await.unwrap_err();
        assert!(matches!(error, SequencerError::SignerAccountMismatch));
    }

    #[tokio::test]
    async fn test_register_pubkey_duplicate() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());

        handle.register_pubkey(aid, pubkey.clone()).await.unwrap();
        let result = handle.register_pubkey(aid, pubkey).await;
        assert!(matches!(
            result,
            Err(SequencerError::AccountAlreadyRegistered)
        ));
    }

    #[tokio::test]
    async fn second_first_key_bootstrap_is_rejected_before_wal_append() {
        let path = temp_store_path("first-key-bootstrap");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut sequencer, account_id) = make_test_sequencer_with_config(config.clone());
        sequencer.produce_block(Vec::new(), 1);
        store.save_block(sequencer.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let sequencer =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));

        let first_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let racing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );

        handle
            .register_pubkey(account_id, PublicKey(*first_key.verifying_key()))
            .await
            .unwrap();
        let error = handle
            .register_pubkey(account_id, PublicKey(*racing_key.verifying_key()))
            .await
            .unwrap_err();
        assert!(matches!(error, SequencerError::AccountAlreadyRegistered));

        let pending_restore = store.load_state().await.unwrap().unwrap();
        assert_eq!(
            pending_restore.control_plane_log.len(),
            1,
            "the rejected racing bootstrap must never enter the control-plane WAL"
        );
        let replayed =
            BlockSequencer::restore(pending_restore, Arc::new(AdminOracle::new()), config);
        let keys = replayed.signing_keys_for_account(account_id);
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0].0,
            first_key.verifying_key().to_sec1_point(true).as_bytes()
        );

        drop(handle);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn onboarding_bootstrap_then_signed_key_allows_no_later_bootstrap() {
        let (sequencer, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(sequencer);
        let account = handle.create_account(0).await.unwrap();

        let primary =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        handle
            .register_pubkey(account.id, PublicKey(*primary.verifying_key()))
            .await
            .expect("first-key bootstrap should succeed");

        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let second = <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
        let signed = crate::crypto::sign_key_registration(
            account.id,
            PublicKey(*second.verifying_key()),
            AccountAuthScheme::RawP256,
            Some("backup".to_string()),
            crate::crypto::KeyScope::Custom,
            1,
            genesis_hash,
            &primary,
        );
        handle
            .register_key_signed(signed)
            .await
            .expect("an existing key should authorize a second key");

        let late_bootstrap =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let error = handle
            .register_pubkey(account.id, PublicKey(*late_bootstrap.verifying_key()))
            .await
            .unwrap_err();
        assert!(matches!(error, SequencerError::AccountAlreadyRegistered));
        assert_eq!(
            handle
                .signing_keys_for_account(account.id)
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn test_list_and_create_markets() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 1);

        let new_id = handle
            .create_market("New Market".to_string())
            .await
            .unwrap();
        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 2);
        assert!(markets.get(new_id).is_some());
    }

    #[tokio::test]
    async fn test_create_and_list_market_groups() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m1 = handle.create_market("A wins".to_string()).await.unwrap();
        let m2 = handle.create_market("B wins".to_string()).await.unwrap();

        let (group_id, group) = handle
            .create_market_group("Election".to_string(), vec![m1, m2])
            .await
            .unwrap();
        assert_eq!(group_id, 0);
        assert_eq!(group.name, "Election");
        assert_eq!(group.markets.len(), 2);

        let groups = handle.list_market_groups().await.unwrap();
        assert_eq!(groups.len(), 1);
    }

    #[tokio::test]
    async fn test_resolve_market() {
        let (seq, _aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m0 = MarketId::new(0);
        let record = handle
            .resolve_market(m0, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();
        assert_eq!(record.payout_nanos, Nanos(NANOS_PER_DOLLAR));
        assert_eq!(record.market_id, m0);
    }

    #[tokio::test]
    async fn test_system_event_emitted_for_market_resolution() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts.get_mut(aid).unwrap().positions.insert((m0, 0), 10);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        handle
            .resolve_market(m0, Nanos(NANOS_PER_DOLLAR))
            .await
            .unwrap();
        let block = handle.produce_block().await.unwrap();

        assert_eq!(block.canonical.system_events.len(), 1);
        match &block.canonical.system_events[0] {
            SystemEvent::MarketResolved {
                market_id,
                payout_nanos,
                affected_accounts,
            } => {
                assert_eq!(*market_id, m0);
                assert_eq!(*payout_nanos, Nanos(NANOS_PER_DOLLAR));
                assert_eq!(affected_accounts, &vec![aid]);
            }
            other => panic!("expected MarketResolved event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_market() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle
            .resolve_market(MarketId::new(999), Nanos(NANOS_PER_DOLLAR))
            .await;
        assert!(matches!(result, Err(SequencerError::MarketNotFound)));
    }

    #[tokio::test]
    async fn test_get_block_by_height() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();

        let block = handle.get_block(1).await.unwrap();
        assert_eq!(block.canonical.header.height, 1);

        let block = handle.get_block(2).await.unwrap();
        assert_eq!(block.canonical.header.height, 2);

        let result = handle.get_block(99).await;
        assert!(matches!(result, Err(SequencerError::BlockNotFound)));
    }

    #[tokio::test]
    async fn store_backed_get_block_survives_ring_eviction_and_restart() {
        let path = temp_store_path("block-history");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_history_capacity: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let block1 = handle.produce_block().await.unwrap();
        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block1.canonical.header.height, 1);
        assert_eq!(block2.canonical.header.height, 2);

        let recent = handle.get_recent_blocks(10).await.unwrap();
        assert_eq!(recent.len(), 1, "hot ring should evict block 1");
        assert_eq!(recent[0].canonical.header.height, 2);

        let block_page = handle.get_block_page(None, 10).await.unwrap();
        assert_eq!(
            block_page
                .iter()
                .map(|block| block.canonical.header.height)
                .collect::<Vec<_>>(),
            vec![2, 1],
            "store-backed page should include blocks outside the hot ring"
        );
        let older_page = handle.get_block_page(Some(2), 10).await.unwrap();
        assert_eq!(older_page.len(), 1);
        assert_eq!(older_page[0].canonical.header.height, 1);

        let evicted = handle.get_block(1).await.unwrap();
        assert_eq!(evicted.canonical.header.height, 1);
        assert_eq!(
            evicted.canonical.header.state_root,
            block1.canonical.header.state_root
        );

        drop(handle);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let reader = SequencerHandle::spawn_with_store_arc(restored_seq, Some(store.clone()));

        let restored_latest = reader
            .get_latest_block()
            .await
            .unwrap()
            .expect("latest block should load from durable store after restore");
        assert_eq!(restored_latest.canonical.header.height, 2);

        let stored_block1 = reader.get_block(1).await.unwrap();
        assert_eq!(stored_block1.canonical.header.height, 1);
        assert_eq!(
            stored_block1.canonical.header.state_root,
            block1.canonical.header.state_root
        );
        let stored_block2 = reader.get_block(2).await.unwrap();
        assert_eq!(stored_block2.canonical.header.height, 2);
        assert_eq!(
            stored_block2.canonical.header.state_root,
            block2.canonical.header.state_root
        );
        let restored_page = reader.get_block_page(None, 10).await.unwrap();
        assert_eq!(
            restored_page
                .iter()
                .map(|block| block.canonical.header.height)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[tokio::test]
    async fn store_backed_history_pruning_runs_after_block_commit() {
        let path = temp_store_path("block-history-retention");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_history_capacity: 1,
            block_history_retention_blocks: 1,
            history_prune_interval_blocks: 1,
            history_prune_max_rows: 10,
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();
        let block3 = handle.produce_block().await.unwrap();
        assert_eq!(block3.canonical.header.height, 3);

        let meta = store.history_retention_meta().unwrap();
        assert_eq!(meta.blocks_full_min_height, Some(3));
        assert_eq!(meta.last_history_prune_height, Some(3));
        assert!(store.load_block(1).await.unwrap().is_none());
        assert!(store.load_block(2).await.unwrap().is_none());
        assert_eq!(
            store
                .load_block(3)
                .await
                .unwrap()
                .unwrap()
                .canonical
                .header
                .height,
            3
        );

        let pruned = match handle.get_block(1).await {
            Ok(block) => panic!(
                "expected block 1 to be pruned, got height {}",
                block.canonical.header.height
            ),
            Err(error) => error,
        };
        assert!(matches!(
            pruned,
            SequencerError::BlockPruned {
                requested_height: 1,
                retention_min_height: 3,
            }
        ));
        assert_eq!(
            handle.get_block(3).await.unwrap().canonical.header.height,
            3
        );
    }

    #[tokio::test]
    async fn store_backed_price_history_survives_cache_eviction_and_restart() {
        let path = temp_store_path("price-history");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            max_price_history_points_per_market: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };

        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Price history");
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((market_id, 0), 10);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            config.clone(),
        );
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        handle
            .submit_order(OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 1, market_id, 0, 600_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle
            .submit_order(OrderSubmission {
                account_id: seller,
                orders: vec![matching_engine::outcome_sell(
                    &markets,
                    2,
                    market_id,
                    0,
                    400_000_000,
                    1,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let block1 = handle.produce_block().await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 3, market_id, 0, 700_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle
            .submit_order(OrderSubmission {
                account_id: seller,
                orders: vec![matching_engine::outcome_sell(
                    &markets,
                    4,
                    market_id,
                    0,
                    300_000_000,
                    1,
                )],
                mm_constraint: None,
            })
            .await
            .unwrap();
        let block2 = handle.produce_block().await.unwrap();

        let page = handle
            .get_price_history(market_id, None, None, None, 2)
            .await
            .unwrap();
        assert_eq!(page.next_before_height, None);
        let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
        assert_eq!(
            heights,
            vec![
                block1.canonical.header.height,
                block2.canonical.header.height
            ],
            "store-backed reads should include points older than the hot price cache"
        );
        assert!(page.points.iter().all(|point| point.volume_nanos > 0));

        let newest_page = handle
            .get_price_history(market_id, None, None, None, 1)
            .await
            .unwrap();
        assert_eq!(newest_page.next_before_height, Some(2));
        assert_eq!(newest_page.points.len(), 1);
        assert_eq!(newest_page.points[0].height, 2);
        let older_page = handle
            .get_price_history(market_id, None, None, newest_page.next_before_height, 1)
            .await
            .unwrap();
        assert_eq!(older_page.next_before_height, None);
        assert_eq!(older_page.points.len(), 1);
        assert_eq!(older_page.points[0].height, 1);
        let candle_page = handle
            .get_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert!(!candle_page.candles.is_empty());
        assert_eq!(
            candle_page
                .candles
                .iter()
                .map(|candle| candle.point_count)
                .sum::<u64>(),
            2,
            "candles should aggregate the two committed raw price points"
        );

        drop(handle);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let reader = SequencerHandle::spawn_with_store_arc(restored_seq, Some(store.clone()));
        let restored_page = reader
            .get_price_history(market_id, None, None, None, 2)
            .await
            .unwrap();
        let restored_heights: Vec<_> = restored_page
            .points
            .iter()
            .map(|point| point.height)
            .collect();
        assert_eq!(restored_heights, heights);
        let restored_candles = reader
            .get_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(restored_candles.candles, candle_page.candles);
    }

    #[tokio::test]
    async fn store_backed_direct_admit_read_model_rows_survive_empty_block() {
        let path = temp_store_path("direct-admit-read-model");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            max_equity_points_per_account: 0,
            max_history_events_per_account: 0,
            ..SequencerConfig::default()
        };

        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        baseline.produce_block(Vec::new(), 1_000);
        store.save_block(baseline.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let markets = handle.list_markets().await.unwrap();
        let market_id = MarketId::new(0);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&markets, 0, market_id, 0, 600_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.canonical.header.height, 2);
        assert_eq!(
            block.canonical.header.fill_count, 0,
            "the read-model rows must persist even when the admitted order does not cross"
        );
        assert!(
            block.canonical.system_events.is_empty(),
            "the regression targets empty non-system blocks"
        );

        let events = store
            .account_events(aid, 10, None, Some("trades".into()))
            .unwrap();
        let placed_count = events
            .iter()
            .filter(|event| matches!(event.kind, crate::aggregates::HistoryKind::Placed))
            .count();
        assert_eq!(
            placed_count, 1,
            "direct-admit Placed history must be durable exactly once even with no in-memory fallback"
        );

        let equity = store.equity_series(aid, 0).unwrap();
        assert!(
            equity
                .iter()
                .any(|point| point.height == block.canonical.header.height),
            "equity point from the empty-fill block must be durable"
        );

        handle.produce_block().await.unwrap();
        let events_after_next_block = store
            .account_events(aid, 10, None, Some("trades".into()))
            .unwrap();
        let placed_count_after_next_block = events_after_next_block
            .iter()
            .filter(|event| matches!(event.kind, crate::aggregates::HistoryKind::Placed))
            .count();
        assert_eq!(
            placed_count_after_next_block, 1,
            "later persisted blocks must not re-flush already-cleared Placed history"
        );
    }

    #[tokio::test]
    async fn test_subscribe_blocks() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let mut rx = handle.subscribe_blocks().await.unwrap();

        handle.produce_block().await.unwrap();

        let block = rx.recv().await.unwrap();
        assert_eq!(block.canonical.header.height, 1);
    }

    // C2 — indicative scheduler ---------------------------------------------

    #[test]
    fn indicative_solve_gate_prevents_stacked_solves() {
        let mut gate = IndicativeSolveGate::default();

        assert!(gate.try_start(), "first tick should start a solve");
        assert!(
            !gate.try_start(),
            "second tick must not start while solve is in flight"
        );

        gate.finish();
        assert!(gate.try_start(), "next tick may start after update arrives");
    }

    struct PanicOnceSolver {
        calls: Arc<AtomicUsize>,
    }

    impl matching_solver::Solver for PanicOnceSolver {
        fn solve(&self, _problem: &Problem) -> matching_solver::PipelineResult {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("injected indicative solver panic");
            }
            matching_solver::PipelineResult::empty()
        }

        fn name(&self) -> &str {
            "panic-once"
        }
    }

    #[tokio::test]
    async fn get_indicative_returns_default_when_uncached() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);
        let mid = MarketId::new(99);
        let snap = handle.get_indicative(mid).await.unwrap();
        assert!(snap.yes_price_nanos.is_none());
        assert!(snap.no_price_nanos.is_none());
        assert_eq!(snap.volume_nanos, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indicative_solver_panic_releases_gate_for_next_tick() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let seq = BlockSequencer::new(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            Arc::new(PanicOnceSolver {
                calls: Arc::clone(&calls),
            }),
            config,
        );
        let handle = SequencerHandle::spawn(seq);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut released_and_reran = false;
        while Instant::now() < deadline {
            if calls.load(Ordering::SeqCst) >= 2 {
                let snap = handle.get_indicative(m0).await.unwrap();
                if snap.computed_at_ms > 0 {
                    released_and_reran = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(
            released_and_reran,
            "indicative solve gate did not release after panic; calls={}",
            calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indicative_timer_populates_cache_after_one_tick() {
        // With at least one resting order on a market, the 750ms
        // indicative ticker should populate the cache for that market
        // within one ticker cycle. We verify by checking `computed_at_ms`
        // moves off the default `0`.
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();

        // First indicative tick fires at +750ms; the solve + cache-write
        // round-trip needs a little extra. 1500ms gives generous slack
        // on a CI box.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let snap = handle.get_indicative(m0).await.unwrap();
        assert!(
            snap.computed_at_ms > 0,
            "indicative tick should have written a snapshot by now"
        );
    }
}
