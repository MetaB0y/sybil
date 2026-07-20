use super::*;

mod bridge;
mod identity;
mod orders;

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

    /// Spawn while allowing an out-of-actor delivery worker to retain a clone
    /// of the store. The shared handle may read/ack the product-history outbox directly;
    /// it must never mutate canonical exchange state outside the actor.
    pub fn spawn_with_shared_store(
        sequencer: BlockSequencer,
        store: Option<Arc<crate::store::Store>>,
    ) -> Self {
        Self::spawn_with_store_arc(sequencer, store)
    }

    fn spawn_with_store_arc(
        sequencer: BlockSequencer,
        store: Option<Arc<crate::store::Store>>,
    ) -> Self {
        let config = sequencer.config.clone();
        let (block_broadcast, _) = broadcast::channel(64);
        let recent_blocks = Arc::new(RwLock::new(VecDeque::new()));
        let mailbox_monitor = MailboxMonitor::new(
            SEQUENCER_ACTOR_METRIC_NAME,
            config.actor_queue_warn_depth,
            config.actor_queue_error_depth,
        );
        let inner = SequencerHandleInner {
            actor: Arc::new(RwLock::new(None)),
            block_broadcast: block_broadcast.clone(),
            recent_blocks: recent_blocks.clone(),
            store: store.clone(),
            mailbox_monitor,
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        };
        let supervisor_args = SequencerSupervisorArgs {
            config,
            store: store.clone(),
            handle: inner.clone(),
        };
        let (supervisor, _) =
            ractor::ActorRuntime::spawn_instant(None, SequencerSupervisor, supervisor_args)
                .expect("failed to spawn sequencer supervisor");
        let actor_args = SequencerActorArgs {
            sequencer,
            store,
            block_broadcast,
            recent_blocks,
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
            if let Some(actor) = actor
                && actor.get_id() != old_id
            {
                return Ok(());
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
    async fn enter_integrity_halt_for_test(
        &self,
        error: SequencerError,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::TestEnterIntegrityHalt(error, reply))
            .await
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

    /// Read committed chain identity and canonical-write availability from one
    /// actor snapshot.
    ///
    /// Readiness callers must not combine separate mailbox reads: the actor
    /// could become unavailable or integrity-halted between them and expose a
    /// chain identity without its current write status.
    pub async fn get_operational_status(
        &self,
    ) -> Result<SequencerOperationalStatus, SequencerError> {
        self.read_query(|state| {
            let height = state.sequencer.height();
            SequencerOperationalStatus {
                committed_height: (height > 0).then_some(height),
                genesis_hash: state.sequencer.genesis_hash(),
                integrity_halted: state.halted_error.is_some(),
            }
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
                state.sequencer.analytics().last_clearing_prices(),
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

    /// Number of non-system account ids allocated for this chain lifetime.
    /// Account ids are monotonic and never reused, so this is the durable stock
    /// counter used by the public onboarding ceiling.
    pub async fn account_stock(&self) -> Result<u64, SequencerError> {
        self.read_query(|state| state.sequencer.accounts.next_id())
            .await
    }

    /// Allocate an account and install its first signing key under one actor
    /// command and one durable control-plane WAL row. `meta.account_id` is
    /// replaced with the newly allocated id.
    pub async fn create_account_with_initial_key(
        &self,
        initial_balance: i64,
        pubkey: PublicKey,
        meta: RegisteredPubkey,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| {
            SequencerMsg::CreateAccountWithInitialKey(initial_balance, pubkey, meta, reply)
        })
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
        if let Some(block) = recent_block_at(
            &self
                .inner
                .recent_blocks
                .read()
                .expect("recent block cache lock poisoned"),
            height,
        ) {
            return Ok(block);
        }

        let Some(store) = &self.inner.store else {
            return Err(SequencerError::BlockNotFound);
        };
        match store.load_block(height).await {
            Ok(Some(block)) => Ok(block),
            Ok(None) => match store.canonical_archive_meta() {
                Ok(meta) => match meta.oldest_retained_height {
                    Some(retention_min_height) if height < retention_min_height => {
                        Err(SequencerError::BlockPruned {
                            requested_height: height,
                            retention_min_height,
                        })
                    }
                    _ => Err(SequencerError::BlockNotFound),
                },
                Err(error) => Err(SequencerError::Persistence(error.to_string())),
            },
            Err(error) => Err(SequencerError::Persistence(error.to_string())),
        }
    }

    pub async fn get_da_artifact(&self, height: u64) -> Result<DaArtifactLookup, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetDaArtifact(height, reply))
            .await?
    }

    pub async fn get_da_manifest(&self, height: u64) -> Result<DaManifestLookup, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetDaManifest(height, reply))
            .await?
    }

    /// Read the oldest unacknowledged portable proof job without blocking the
    /// actor on redb I/O. Returns `ProofUnavailable` for in-memory sequencers.
    pub async fn oldest_unacknowledged_proof_job(
        &self,
    ) -> Result<Option<crate::store::ProofJobOutboxEntry>, SequencerError> {
        let store = self
            .read_query(|state| state.store.clone())
            .await?
            .ok_or_else(|| {
                SequencerError::ProofUnavailable(
                    "proof-job outbox requires a persistent sequencer store".to_string(),
                )
            })?;
        tokio::task::spawn_blocking(move || store.oldest_unacknowledged_proof_job())
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .map_err(|error| SequencerError::Persistence(error.to_string()))
    }

    /// Acknowledge only the exact proof-job bytes made durable by the prover.
    pub async fn acknowledge_proof_job(
        &self,
        height: u64,
        digest: [u8; 32],
    ) -> Result<(), SequencerError> {
        let store = self
            .read_query(|state| state.store.clone())
            .await?
            .ok_or_else(|| {
                SequencerError::ProofUnavailable(
                    "proof-job outbox requires a persistent sequencer store".to_string(),
                )
            })?;
        store
            .acknowledge_proof_job(height, digest)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))
    }

    pub async fn get_recent_blocks(&self, n: usize) -> Result<Vec<SealedBlock>, SequencerError> {
        Ok(self
            .inner
            .recent_blocks
            .read()
            .expect("recent block cache lock poisoned")
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect())
    }

    pub async fn get_block_page(
        &self,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<SealedBlock>, SequencerError> {
        let limit = limit.min(MAX_BLOCK_REPLAY_QUERY_BLOCKS);
        match &self.inner.store {
            Some(store) => store
                .load_block_page(before_height, limit)
                .await
                .map_err(|error| SequencerError::Persistence(error.to_string())),
            None => Ok(self
                .inner
                .recent_blocks
                .read()
                .expect("recent block cache lock poisoned")
                .iter()
                .rev()
                .filter(|block| {
                    before_height.is_none_or(|before| block.canonical.header.height < before)
                })
                .take(limit)
                .cloned()
                .collect()),
        }
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

    /// Current, all-time leaderboard inputs from live state. Historical
    /// window baselines belong to the history service and must not be read by
    /// the sequencer actor.
    pub async fn leaderboard_bases(&self) -> Result<Vec<LeaderboardBase>, SequencerError> {
        self.read_query(|state| state.sequencer.leaderboard_bases())
            .await
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
        self.rpc(SequencerMsg::PauseBlockProduction).await?
    }

    pub async fn resume_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::ResumeBlockProduction).await?
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

fn recent_block_at(blocks: &VecDeque<SealedBlock>, height: u64) -> Option<SealedBlock> {
    let first_height = blocks.front()?.canonical.header.height;
    let index = usize::try_from(height.checked_sub(first_height)?).ok()?;
    let block = blocks.get(index)?;
    (block.canonical.header.height == height).then(|| block.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::bridge::L1WithdrawalStatus;
    use crate::crypto::{KeyScope, sign_cancel};
    use crate::market_info::ResolutionConfig;
    use crate::sequencer::SequencerConfig;
    use crate::store::AcknowledgedWrite;
    use crate::system_event::SystemEvent;
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::Duration;
    use sybil_oracle::{FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};
    use sybil_verifier::SystemEventWitness;

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
        make_test_sequencer_with_config(SequencerConfig {
            // Actor tests use minimum-unit orders to isolate mailbox/WAL
            // behavior. The admission floor is covered in sequencer tests.
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        })
    }

    fn make_test_sequencer_with_config(mut config: SequencerConfig) -> (BlockSequencer, AccountId) {
        // Callers customize actor limits, persistence, and timing. Their tiny
        // order fixtures are not admission-policy tests.
        config.min_resting_order_notional_nanos = 0;
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        (
            BlockSequencer::with_default_solver(accounts, markets, vec![], config),
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
            min_resting_order_notional_nanos: 0,
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
    async fn integrity_halt_rejects_writes_without_growing_live_or_durable_pending_state() {
        let config = SequencerConfig {
            block_interval: Duration::from_secs(3_600),
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };
        let (mut sequencer, account_id) = make_test_sequencer_with_config(config);
        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        sequencer
            .register_pubkey(account_id, PublicKey(*signing_key.verifying_key()))
            .unwrap();

        let path = temp_store_path("integrity-halt");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));
        let baseline = handle.produce_block().await.unwrap();
        let genesis_hash = crate::block::hash_header(&baseline.canonical.header);

        let mut markets = MarketSet::new();
        let market = markets.add_binary("Test");
        let existing_order = outcome_buy(&markets, 0, market, 0, 500_000_000, 1);
        let existing_order_id = handle
            .submit_order(OrderSubmission {
                account_id,
                orders: vec![existing_order],
                mm_constraint: None,
            })
            .await
            .unwrap()[0];

        let pending_before = handle.get_pending_orders(Some(account_id)).await.unwrap();
        let balance_before = handle
            .get_account(account_id)
            .await
            .unwrap()
            .unwrap()
            .balance;
        let durable_before = store.load_state().await.unwrap().unwrap();
        assert_eq!(durable_before.acknowledged_writes.len(), 1);
        let durable_sequences_before: Vec<_> = durable_before
            .acknowledged_writes
            .iter()
            .map(|entry| entry.sequence)
            .collect();

        handle
            .enter_integrity_halt_for_test(SequencerError::BlockInvariantFailure {
                height: 2,
                failures: vec![
                    crate::error::BlockInvariantFailure::PreparedStateRootMismatch {
                        block_state_root: [0; 32],
                        prepared_state_root: [1; 32],
                    },
                ],
            })
            .await
            .unwrap();

        let status = handle.get_operational_status().await.unwrap();
        assert_eq!(status.committed_height, Some(1));
        assert_eq!(status.genesis_hash, Some(genesis_hash));
        assert!(status.integrity_halted);

        let unsigned = OrderSubmission {
            account_id,
            orders: vec![outcome_buy(&markets, 0, market, 0, 500_000_000, 2)],
            mm_constraint: None,
        };
        assert!(matches!(
            handle.submit_order(unsigned.clone()).await,
            Err(SequencerError::IntegrityHalted)
        ));
        assert!(matches!(
            handle.submit_ioc_order(unsigned).await,
            Err(SequencerError::IntegrityHalted)
        ));

        let signed_order = crate::crypto::sign_order(
            &outcome_buy(&markets, 0, market, 0, 500_000_000, 3),
            1,
            genesis_hash,
            &signing_key,
        );
        assert!(matches!(
            handle.submit_signed_order(signed_order).await,
            Err(SequencerError::IntegrityHalted)
        ));

        let signed_cancel = crate::crypto::sign_cancel(
            account_id,
            existing_order_id,
            1,
            genesis_hash,
            &signing_key,
        );
        assert!(matches!(
            handle.cancel_signed_order(signed_cancel).await,
            Err(SequencerError::IntegrityHalted)
        ));
        assert!(matches!(
            handle.fund_account(account_id, 1).await,
            Err(SequencerError::IntegrityHalted)
        ));
        assert!(matches!(
            handle.create_market("must not persist".to_string()).await,
            Err(SequencerError::IntegrityHalted)
        ));

        // Read-only diagnostics and the incident pause remain available. A
        // resume cannot claim success while the stronger integrity halt wins.
        assert!(handle.get_state_root().await.is_ok());
        assert!(handle.get_latest_block().await.unwrap().is_some());
        handle.pause_block_production().await.unwrap();
        assert!(matches!(
            handle.resume_block_production().await,
            Err(SequencerError::IntegrityHalted)
        ));

        let pending_after = handle.get_pending_orders(Some(account_id)).await.unwrap();
        assert_eq!(
            pending_after
                .iter()
                .map(|order| order.order_id)
                .collect::<Vec<_>>(),
            pending_before
                .iter()
                .map(|order| order.order_id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            handle
                .get_account(account_id)
                .await
                .unwrap()
                .unwrap()
                .balance,
            balance_before
        );

        let durable_after = store.load_state().await.unwrap().unwrap();
        assert_eq!(durable_after.acknowledged_writes.len(), 1);
        assert_eq!(
            durable_after
                .acknowledged_writes
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            durable_sequences_before
        );
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
        let sequencer =
            BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config);
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
        assert!(
            handle
                .get_pending_orders(Some(buyer))
                .await
                .unwrap()
                .is_empty()
        );
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
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };
        let mut accounts = AccountStore::new();
        let a = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let b = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        let seq = BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config);
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
        assert_eq!(SequencerConfig::default().max_orders_per_submission, 512);
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

        let produced = handle.produce_block().await.unwrap();

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().canonical.header.height, 1);

        let status = handle.get_operational_status().await.unwrap();
        assert_eq!(status.committed_height, Some(1));
        assert_eq!(
            status.genesis_hash,
            Some(crate::block::hash_header(&produced.canonical.header))
        );
        assert!(!status.integrity_halted);
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

    #[tokio::test(start_paused = true)]
    async fn scheduled_ticks_do_not_stack_while_actor_is_busy() {
        let config = SequencerConfig {
            block_interval: Duration::from_millis(20),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn(seq);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        handle
            .hold_next_tick_for_test(SequencerTestTickHold {
                started: started_tx,
                release: release_rx,
            })
            .await
            .unwrap();

        started_rx.await.unwrap();
        // Virtual time advances exactly five block periods. Host contention
        // cannot stretch this wait past the independent 750ms indicative
        // scheduler and add an unrelated message to the shared depth gauge.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            handle.inner.mailbox_monitor.depth(),
            1,
            "timer should retain only one scheduled tick while the actor is busy"
        );

        release_tx.send(()).unwrap();
        assert!(handle.stop_and_wait(TEST_ACTOR_TIMEOUT).await);
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
                ..
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
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };
        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        let cancel_signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        baseline
            .register_pubkey(aid, PublicKey(*cancel_signing_key.verifying_key()))
            .unwrap();
        let genesis = baseline.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                baseline.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let created = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(
            handle.account_stock().await.unwrap(),
            created.id.0.saturating_add(1)
        );
        handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        handle
            .register_pubkey(created.id, pubkey.clone())
            .await
            .unwrap();

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
            creation_key: Some("native:wal-market".to_string()),
            committed_metadata_digest: None,
        };
        let metadata_market = handle
            .create_market_with_metadata("metadata wal market".to_string(), metadata.clone())
            .await
            .unwrap();
        let mut retry_metadata = metadata.clone();
        retry_metadata.created_at_ms += 1;
        assert_eq!(
            handle
                .create_market_with_metadata(
                    "metadata wal market".to_string(),
                    retry_metadata.clone(),
                )
                .await
                .unwrap(),
            metadata_market,
            "an exact keyed retry should not allocate a second market"
        );
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
        let signed_cancel = sign_cancel(
            aid,
            pending[0].order_id,
            1,
            genesis_hash,
            &cancel_signing_key,
        );
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
                Some(created.id),
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

        const EXPECTED_CONTROL_PLANE_COMMANDS: usize = 11;
        for restart_round in 0..3 {
            let restored = store.load_state().await.unwrap().unwrap();
            assert_eq!(
                restored
                    .acknowledged_writes
                    .iter()
                    .filter(|entry| matches!(entry.write, AcknowledgedWrite::ControlPlane(_)))
                    .count(),
                EXPECTED_CONTROL_PLANE_COMMANDS,
                "restart round {restart_round} should see every acknowledged control-plane command before commit"
            );
            assert!(
                restored.acknowledged_writes.iter().any(|entry| matches!(
                    &entry.write,
                    AcknowledgedWrite::ControlPlane(ControlPlaneCommand::ExtendMarketGroup {
                        group_id,
                        market_id
                    }) if *group_id == 0 && *market_id == metadata_market
                )),
                "restart round {restart_round} should replay the market group extension"
            );
            assert!(
                restored.acknowledged_writes.iter().any(|entry| matches!(
                    &entry.write,
                    AcknowledgedWrite::AuthenticatedCancel {
                        account_id,
                        nonce: 1,
                        ..
                    } if *account_id == aid
                )),
                "restart round {restart_round} should replay the atomic signed cancel"
            );
            assert_eq!(
                restored
                    .acknowledged_writes
                    .iter()
                    .filter(|entry| matches!(entry.write, AcknowledgedWrite::DirectAdmit(_)))
                    .count(),
                1,
                "restart round {restart_round} should replay the direct admit before the cancel command"
            );
            let restored_seq = BlockSequencer::restore(restored, config.clone());
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
            assert_eq!(
                probe
                    .create_market_with_metadata(
                        "metadata wal market".to_string(),
                        retry_metadata.clone(),
                    )
                    .unwrap(),
                metadata_market,
                "restart round {restart_round} should retain keyed creation identity"
            );
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
                restored_after_commit.acknowledged_writes.is_empty(),
                "control-plane WAL should clear once a block commits the writes"
            );
            assert!(
                restored_after_commit.acknowledged_writes.is_empty(),
                "admit WAL should clear once a block commits the direct admit and cancel"
            );
            let restored_seq = BlockSequencer::restore(restored_after_commit, config.clone());
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
        let genesis = baseline.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                baseline.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, config.clone());
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
        let sequences: Vec<u64> = restored
            .acknowledged_writes
            .iter()
            .map(|entry| entry.sequence)
            .collect();
        let kinds: Vec<&str> = restored
            .acknowledged_writes
            .iter()
            .map(|entry| entry.write.kind())
            .collect();
        assert_eq!(sequences, vec![0, 1, 2]);
        assert_eq!(
            kinds,
            vec!["direct_admit", "authenticated_cancel", "bridge_withdrawal"],
            "the durable sequence must match actor acknowledgement order exactly"
        );
        assert!(matches!(
            &restored.acknowledged_writes[1].write,
            AcknowledgedWrite::AuthenticatedCancel {
                account_id,
                order_id,
                nonce: 1,
                ..
            } if *account_id == aid && *order_id == pending[0].order_id
        ));
        assert_eq!(
            restored
                .acknowledged_writes
                .iter()
                .filter(|entry| matches!(entry.write, AcknowledgedWrite::DirectAdmit(_)))
                .count(),
            1
        );
        assert_eq!(
            restored
                .acknowledged_writes
                .iter()
                .filter(|entry| matches!(entry.write, AcknowledgedWrite::ControlPlane(_)))
                .count(),
            0
        );
        assert_eq!(
            restored
                .acknowledged_writes
                .iter()
                .filter(|entry| matches!(entry.write, AcknowledgedWrite::BridgeWithdrawal(_)))
                .count(),
            1
        );
        let restored_seq = BlockSequencer::restore(restored, config.clone());

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
        let genesis = baseline.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                baseline.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, config);
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
        assert_eq!(
            pre_crash
                .acknowledged_writes
                .iter()
                .filter(|entry| matches!(entry.write, AcknowledgedWrite::BridgeL1Input(_)))
                .count(),
            1
        );

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
        assert!(after_commit.acknowledged_writes.is_empty());
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
        let (mut seq, aid) = make_test_sequencer_with_config(SequencerConfig {
            block_interval: Duration::from_secs(3_600),
            ..SequencerConfig::default()
        });
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        seq.register_pubkey(aid, pubkey).unwrap();

        let path = temp_store_path("signed-order-proof");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let signed = crate::crypto::sign_order(&order, 1, genesis_hash, &signing_key);
        handle.submit_signed_order(signed).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert!(block.canonical.header.order_count >= 1);
        let witness = store
            .block_witness(block.canonical.header.height)
            .unwrap()
            .expect("persisted signed-order witness");
        assert!(witness.system_events.iter().any(|event| matches!(
            event,
            sybil_verifier::SystemEventWitness::ClientActionAuthorized(
                sybil_verifier::ClientActionWitness::Order {
                    account_id,
                    nonce: 1,
                    ..
                }
            ) if *account_id == aid.0
        )));
        assert!(sybil_verifier::verify_full(&witness, false).valid);
        let order_id = witness
            .orders
            .first()
            .expect("signed order in witnessed block")
            .order
            .id;

        let mut forged = witness;
        let authorization = forged
            .system_events
            .iter_mut()
            .find_map(|event| match event {
                sybil_verifier::SystemEventWitness::ClientActionAuthorized(
                    sybil_verifier::ClientActionWitness::Order { authorization, .. },
                ) => Some(authorization),
                _ => None,
            })
            .expect("signed order authorization event");
        if let sybil_verifier::ClientActionAuth::RawP256 { signature, .. } = authorization {
            signature[0] ^= 1;
        } else {
            panic!("raw signed order produced a non-raw authorization envelope");
        }
        assert!(!sybil_verifier::verify_full(&forged, false).valid);

        let cancel = crate::crypto::sign_cancel(aid, order_id, 2, genesis_hash, &signing_key);
        handle.cancel_signed_order(cancel).await.unwrap();
        let cancel_block = handle.produce_block().await.unwrap();
        let cancel_witness = store
            .block_witness(cancel_block.canonical.header.height)
            .unwrap()
            .expect("persisted signed-cancel witness");
        assert!(cancel_witness.system_events.iter().any(|event| matches!(
            event,
            sybil_verifier::SystemEventWitness::ClientActionAuthorized(
                sybil_verifier::ClientActionWitness::Cancel {
                    account_id,
                    order_id: witnessed_order_id,
                    nonce: 2,
                    ..
                }
            ) if *account_id == aid.0 && *witnessed_order_id == order_id
        )));

        let epoch_entries = store
            .proof_job_outbox_page(Some(block.canonical.header.height - 1), 2)
            .unwrap();
        assert_eq!(
            epoch_entries
                .iter()
                .map(|entry| entry.height)
                .collect::<Vec<_>>(),
            vec![
                block.canonical.header.height,
                cancel_block.canonical.header.height
            ]
        );
        let epoch_inputs = epoch_entries
            .into_iter()
            .map(|entry| {
                let job: sybil_proof_protocol::StateTransitionProofJob =
                    rmp_serde::from_slice(&entry.bytes).unwrap();
                sybil_proof_protocol::build_state_transition_guest_input(job).unwrap()
            })
            .collect::<Vec<_>>();
        let mut accumulator = sybil_proof_protocol::EpochTransitionAccumulator::new();
        for input in &epoch_inputs {
            accumulator.push(input).unwrap();
        }
        let epoch_public_inputs = accumulator.finish().unwrap();
        assert_eq!(
            sybil_proof_protocol::verify_epoch_transition_inputs(
                &epoch_public_inputs,
                &epoch_inputs
            ),
            Ok(sybil_proof_protocol::epoch_transition_public_input_hash(
                &epoch_public_inputs
            ))
        );
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

        let account = seq_b.accounts.get(aid_b).unwrap().clone();
        let target_key = sybil_verifier::KeyRecord {
            auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
            pubkey_sec1: target_bytes.try_into().unwrap(),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };

        let handle_b = SequencerHandle::spawn(seq_b);
        // Revocation signed under genesis A replayed against genesis-B store.
        let signed = crate::crypto::sign_key_revocation(
            aid_b,
            target_key,
            account.keys_digest,
            account.events_digest,
            genesis_a,
            &signing_key,
        );
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

        assert!(
            handle
                .get_pending_orders(Some(aid))
                .await
                .unwrap()
                .is_empty()
        );
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
    async fn atomic_account_onboarding_is_one_wal_command_and_recovers_with_key() {
        let path = temp_store_path("atomic-account-onboarding");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut sequencer, _) = make_test_sequencer_with_config(config.clone());
        let genesis = sequencer.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                sequencer.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let sequencer = BlockSequencer::restore(restored, config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));
        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(*signing_key.verifying_key());
        let created = handle
            .create_account_with_initial_key(
                17,
                pubkey.clone(),
                RegisteredPubkey {
                    account_id: AccountId(0),
                    auth_scheme: AccountAuthScheme::WebAuthn,
                    label: Some("primary passkey".to_string()),
                    scope: KeyScope::Primary,
                    created_at_ms: 123,
                },
            )
            .await
            .unwrap();

        let duplicate = handle
            .create_account_with_initial_key(
                99,
                pubkey.clone(),
                RegisteredPubkey::primary(AccountId(0), AccountAuthScheme::WebAuthn),
            )
            .await;
        assert!(matches!(
            duplicate,
            Err(SequencerError::AccountAlreadyRegistered)
        ));

        let pending = store.load_state().await.unwrap().unwrap();
        assert_eq!(pending.acknowledged_writes.len(), 1);
        assert!(matches!(
            &pending.acknowledged_writes[0].write,
            AcknowledgedWrite::ControlPlane(ControlPlaneCommand::CreateAccountWithInitialKey {
                initial_balance: 17,
                auth_scheme: AccountAuthScheme::WebAuthn,
                label: Some(label),
                scope: KeyScope::Primary,
                created_at_ms: 123,
                ..
            }) if label == "primary passkey"
        ));

        let mut replayed = BlockSequencer::restore(pending, config);
        assert_eq!(replayed.accounts.next_id(), created.id.0.saturating_add(1));
        let account = replayed.accounts.get(created.id).unwrap();
        assert_eq!(account.balance, 17);
        assert_eq!(account.total_deposited, 17);
        let keys = replayed.signing_keys_for_account(created.id);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, pubkey.compressed_bytes());
        assert_eq!(keys[0].1.label.as_deref(), Some("primary passkey"));

        let block = replayed.produce_block(Vec::new(), 2);
        assert!(matches!(
            block.block.system_events.as_slice(),
            [SystemEvent::CreateAccount {
                account_id,
                initial_balance: 17,
                initial_keys,
            }] if *account_id == created.id && initial_keys.len() == 1
        ));
        assert!(sybil_verifier::verify_full(&block.witness, false).valid);

        drop(handle);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn second_first_key_bootstrap_is_rejected_before_wal_append() {
        let path = temp_store_path("first-key-bootstrap");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        };
        let (mut sequencer, _) = make_test_sequencer_with_config(config.clone());
        let genesis = sequencer.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                sequencer.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let sequencer = BlockSequencer::restore(restored, config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));
        let account_id = handle.create_account(0).await.unwrap().id;

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
            pending_restore.acknowledged_writes.len(),
            2,
            "only CreateAccount + the accepted bootstrap may enter the control-plane WAL"
        );
        let replayed = BlockSequencer::restore(pending_restore, config);
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
    async fn oversized_signing_key_labels_are_rejected_before_wal_and_state_mutation() {
        let path = temp_store_path("signing-key-label-limit");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(600),
            ..SequencerConfig::default()
        };
        let (mut sequencer, _) = make_test_sequencer_with_config(config.clone());
        let genesis = sequencer.produce_block(Vec::new(), 1);
        store
            .save_block_with_witness_and_replay_block(
                sequencer.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();
        let restored = store.load_state().await.unwrap().unwrap();
        let sequencer = BlockSequencer::restore(restored, config);
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));
        let account = handle.create_account(0).await.unwrap();
        let primary =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let oversized = "x".repeat(crate::account::MAX_SIGNING_KEY_LABEL_BYTES + 1);

        let before = handle.get_account(account.id).await.unwrap().unwrap();
        let error = handle
            .register_pubkey_with_meta(
                account.id,
                PublicKey(*primary.verifying_key()),
                crate::crypto::RegisteredPubkey {
                    account_id: account.id,
                    auth_scheme: AccountAuthScheme::RawP256,
                    label: Some(oversized.clone()),
                    scope: crate::crypto::KeyScope::Primary,
                    created_at_ms: 1,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            SequencerError::SigningKeyLabelTooLong { .. }
        ));
        assert!(
            handle
                .signing_keys_for_account(account.id)
                .await
                .unwrap()
                .is_empty()
        );
        let after = handle.get_account(account.id).await.unwrap().unwrap();
        assert_eq!(after.last_nonce, before.last_nonce);
        assert_eq!(after.keys_digest, before.keys_digest);
        assert_eq!(after.events_digest, before.events_digest);
        assert_eq!(
            store
                .load_state()
                .await
                .unwrap()
                .unwrap()
                .acknowledged_writes
                .len(),
            1,
            "only account creation may enter the WAL"
        );

        handle
            .register_pubkey_with_meta(
                account.id,
                PublicKey(*primary.verifying_key()),
                crate::crypto::RegisteredPubkey {
                    account_id: account.id,
                    auth_scheme: AccountAuthScheme::RawP256,
                    label: Some("primary".to_string()),
                    scope: crate::crypto::KeyScope::Primary,
                    created_at_ms: 2,
                },
            )
            .await
            .unwrap();
        handle.produce_block().await.unwrap();
        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let candidate =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let before = handle.get_account(account.id).await.unwrap().unwrap();
        let signed = crate::crypto::sign_key_registration(
            account.id,
            PublicKey(*candidate.verifying_key()),
            AccountAuthScheme::RawP256,
            Some(oversized),
            crate::crypto::KeyScope::Agent,
            before.keys_digest,
            before.events_digest,
            genesis_hash,
            &primary,
        );
        let error = handle.register_key_signed(signed).await.unwrap_err();
        assert!(matches!(
            error,
            SequencerError::SigningKeyLabelTooLong { .. }
        ));
        assert_eq!(
            handle
                .signing_keys_for_account(account.id)
                .await
                .unwrap()
                .len(),
            1
        );
        let after = handle.get_account(account.id).await.unwrap().unwrap();
        assert_eq!(after.last_nonce, before.last_nonce);
        assert_eq!(after.keys_digest, before.keys_digest);
        assert_eq!(after.events_digest, before.events_digest);
        assert!(
            store
                .load_state()
                .await
                .unwrap()
                .unwrap()
                .acknowledged_writes
                .is_empty(),
            "rejected signed registration must not append a WAL row"
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
        let binding = handle.get_account(account.id).await.unwrap().unwrap();
        let signed = crate::crypto::sign_key_registration(
            account.id,
            PublicKey(*second.verifying_key()),
            AccountAuthScheme::RawP256,
            Some("backup".to_string()),
            crate::crypto::KeyScope::Custom,
            binding.keys_digest,
            binding.events_digest,
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
    async fn key_register_revoke_events_survive_blocks_and_store_restore() {
        let path = temp_store_path("witness-v6-key-ops");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(600),
            ..SequencerConfig::default()
        };
        let (sequencer, _) = make_test_sequencer_with_config(config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(sequencer, Some(store.clone()));
        handle.produce_block().await.unwrap();
        let account = handle.create_account(0).await.unwrap();

        let primary =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        handle
            .register_pubkey(account.id, PublicKey(*primary.verifying_key()))
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let genesis_hash = handle.get_genesis_hash().await.unwrap().unwrap();
        let agent = <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
        let binding = handle.get_account(account.id).await.unwrap().unwrap();
        let signed_register = crate::crypto::sign_key_registration(
            account.id,
            PublicKey(*agent.verifying_key()),
            AccountAuthScheme::RawP256,
            Some("restore-agent".to_string()),
            crate::crypto::KeyScope::Agent,
            binding.keys_digest,
            binding.events_digest,
            genesis_hash,
            &primary,
        );
        handle.register_key_signed(signed_register).await.unwrap();
        handle.produce_block().await.unwrap();
        let register_witness = store.latest_block_witness().unwrap().unwrap();
        assert!(matches!(
            register_witness.system_events.as_slice(),
            [SystemEventWitness::KeyRegistered { account_id, .. }] if *account_id == account.id.0
        ));
        assert!(sybil_verifier::verify_full(&register_witness, false).valid);

        let binding = handle.get_account(account.id).await.unwrap().unwrap();
        let primary_record = sybil_verifier::KeyRecord {
            auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
            pubkey_sec1: primary
                .verifying_key()
                .to_sec1_point(true)
                .as_bytes()
                .try_into()
                .unwrap(),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        let signed_revoke = crate::crypto::sign_key_revocation(
            account.id,
            primary_record,
            binding.keys_digest,
            binding.events_digest,
            genesis_hash,
            &agent,
        );
        handle
            .revoke_signing_key_signed(signed_revoke)
            .await
            .unwrap();
        handle.produce_block().await.unwrap();
        let revoke_witness = store.latest_block_witness().unwrap().unwrap();
        assert!(matches!(
            revoke_witness.system_events.as_slice(),
            [SystemEventWitness::KeyRevoked { account_id, .. }] if *account_id == account.id.0
        ));
        assert!(sybil_verifier::verify_full(&revoke_witness, false).valid);

        let restored = store.load_state().await.unwrap().unwrap();
        let mut restored_sequencer = BlockSequencer::restore(restored, config);
        let restored_keys = restored_sequencer.signing_keys_for_account(account.id);
        assert_eq!(restored_keys.len(), 1);
        assert_eq!(
            restored_keys[0].0,
            agent.verifying_key().to_sec1_point(true).as_bytes()
        );
        let child = restored_sequencer.produce_block(Vec::new(), 4_000);
        assert!(sybil_verifier::verify_full(&child.witness, false).valid);
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
        assert!(matches!(
            result,
            Err(SequencerError::MarketNotFound { market_id }) if market_id == MarketId::new(999)
        ));
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
    async fn recent_block_reads_remain_available_without_the_actor_mailbox() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);
        let committed = handle.produce_block().await.unwrap();

        handle
            .actor_ref()
            .await
            .unwrap()
            .kill_and_wait(Some(Duration::from_secs(5)))
            .await
            .unwrap();

        let replayed = handle
            .get_block(committed.canonical.header.height)
            .await
            .unwrap();
        assert_eq!(replayed.canonical.header, committed.canonical.header);
        assert_eq!(handle.get_recent_blocks(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn canonical_archive_survives_recent_cache_eviction_and_restart() {
        let path = temp_store_path("canonical-archive");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            recent_block_cache_capacity: 1,
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
        assert_eq!(recent.len(), 1, "recent cache should evict block 1");
        assert_eq!(recent[0].canonical.header.height, 2);

        let block_page = handle.get_block_page(None, 10).await.unwrap();
        assert_eq!(
            block_page
                .iter()
                .map(|block| block.canonical.header.height)
                .collect::<Vec<_>>(),
            vec![2, 1],
            "canonical archive should include blocks outside the recent cache"
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
        let restored_seq = BlockSequencer::restore(restored, config.clone());
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
    async fn canonical_archive_maintenance_runs_after_block_commit() {
        let path = temp_store_path("canonical-archive-retention");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            recent_block_cache_capacity: 1,
            canonical_archive_retention_blocks: 1,
            canonical_archive_maintenance_interval_blocks: 1,
            canonical_archive_max_rows_per_pass: 10,
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();
        let block3 = handle.produce_block().await.unwrap();
        assert_eq!(block3.canonical.header.height, 3);

        assert!(
            handle.stop_and_wait(TEST_ACTOR_TIMEOUT).await,
            "graceful shutdown should drain post-commit block/DA pruning"
        );
        let meta = store.canonical_archive_meta().unwrap();
        assert_eq!(meta.oldest_retained_height, Some(3));
        assert_eq!(meta.last_maintenance_height, Some(3));
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

        let restored = store.load_state().await.unwrap().unwrap();
        let reader = SequencerHandle::spawn_with_store_arc(
            BlockSequencer::restore(restored, config),
            Some(store.clone()),
        );
        let pruned = match reader.get_block(1).await {
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
            reader.get_block(3).await.unwrap().canonical.header.height,
            3
        );
    }

    #[tokio::test]
    async fn acknowledged_proof_job_maintenance_runs_after_block_commit() {
        let path = temp_store_path("acknowledged-proof-job-retention");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            acknowledged_proof_job_retention_blocks: 1,
            acknowledged_proof_job_maintenance_interval_blocks: 1,
            acknowledged_proof_job_max_rows_per_pass: 10,
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let block1 = handle.produce_block().await.unwrap();
        let job1 = store
            .proof_job_outbox_page(None, 10)
            .unwrap()
            .into_iter()
            .find(|entry| entry.height == block1.canonical.header.height)
            .expect("first block proof job");
        handle
            .acknowledge_proof_job(job1.height, job1.digest)
            .await
            .unwrap();

        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block2.canonical.header.height, 2);
        assert!(
            handle.stop_and_wait(TEST_ACTOR_TIMEOUT).await,
            "graceful shutdown should drain post-commit maintenance"
        );
        let remaining = store.proof_job_outbox_page(None, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].height, block2.canonical.header.height);
        assert!(!remaining[0].acknowledged);
    }

    #[tokio::test]
    async fn product_only_chain_keeps_recovery_and_replay_without_validity_artifacts() {
        let path = temp_store_path("product-only-artifacts");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        store.bind_validity_artifact_retention(false).unwrap();
        let config = SequencerConfig {
            retain_validity_artifacts: false,
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        };
        let (seq, _) = make_test_sequencer_with_config(config);
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));

        let block = handle.produce_block().await.unwrap();
        let height = block.canonical.header.height;
        assert!(
            handle.stop_and_wait(TEST_ACTOR_TIMEOUT).await,
            "graceful shutdown should drain post-commit work"
        );

        assert!(store.proof_job_outbox_page(None, 10).unwrap().is_empty());
        assert!(store.load_da_artifact(height).await.unwrap().is_none());
        assert_eq!(
            store
                .latest_block_witness()
                .unwrap()
                .expect("latest recovery witness remains available")
                .header
                .height,
            height
        );
        assert_eq!(
            store
                .load_block(height)
                .await
                .unwrap()
                .expect("canonical replay block remains available")
                .canonical
                .header
                .height,
            height
        );
        assert_eq!(store.load_state().await.unwrap().unwrap().height, height);
    }

    #[tokio::test]
    async fn committed_price_facts_are_written_to_the_durable_history_outbox() {
        let path = temp_store_path("price-history");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            // This test drives every block explicitly. Keep the autonomous
            // scheduler outside even heavily contended CI runs so it cannot
            // insert an unrelated fourth block while a debug build links or
            // executes slowly.
            block_interval: Duration::from_secs(60 * 60),
            min_resting_order_notional_nanos: 0,
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
        accounts
            .get_mut(buyer)
            .unwrap()
            .positions
            .insert((market_id, 1), 10);

        let seq =
            BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config.clone());
        let handle = SequencerHandle::spawn_with_store_arc(seq, Some(store.clone()));
        let baseline = handle.produce_block().await.unwrap();

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

        let batches = store.product_history_outbox_batches(10).unwrap();
        let heights: Vec<_> = batches.iter().map(|batch| batch.height).collect();
        assert_eq!(
            heights,
            vec![
                baseline.canonical.header.height,
                block1.canonical.header.height,
                block2.canonical.header.height
            ],
            "each fenced block commit must append exactly one history batch"
        );
        assert!(batches[0].prices.is_empty());
        assert!(batches.iter().skip(1).all(|batch| {
            batch.prices.len() == 1
                && batch.prices[0].market_id == market_id.0
                && batch.prices[0].volume_nanos > 0
        }));

        drop(handle);
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(store.product_history_outbox_batches(10).unwrap(), batches);

        let mut bad_second = batches[1].payload_hash;
        bad_second[0] ^= 1;
        let bad_acks = [
            crate::store::ProductHistoryOutboxAck {
                height: batches[0].height,
                payload_hash: batches[0].payload_hash,
            },
            crate::store::ProductHistoryOutboxAck {
                height: batches[1].height,
                payload_hash: bad_second,
            },
        ];
        assert!(
            store
                .acknowledge_product_history_batches(&bad_acks)
                .is_err()
        );
        assert_eq!(
            store.product_history_outbox_batches(10).unwrap(),
            batches,
            "a bad hash must roll back the complete acknowledgement transaction"
        );

        let acks: Vec<_> = batches
            .iter()
            .map(|batch| crate::store::ProductHistoryOutboxAck {
                height: batch.height,
                payload_hash: batch.payload_hash,
            })
            .collect();
        assert_eq!(
            store.acknowledge_product_history_batches(&acks).unwrap(),
            batches.len()
        );
        assert_eq!(store.product_history_outbox_len().unwrap(), 0);
        assert_eq!(store.acknowledge_product_history_batches(&acks).unwrap(), 0);
    }

    #[tokio::test]
    async fn direct_admit_facts_survive_an_empty_block_in_the_history_outbox() {
        let path = temp_store_path("direct-admit-read-model");
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let config = SequencerConfig {
            block_interval: Duration::from_secs(60),
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };

        let (mut baseline, aid) = make_test_sequencer_with_config(config.clone());
        let genesis = baseline.produce_block(Vec::new(), 1_000);
        store
            .save_block_with_witness_and_replay_block(
                baseline.snapshot(),
                &genesis.witness,
                &genesis.sealed_block(),
                true,
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let seq = BlockSequencer::restore(restored, config.clone());
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

        let batches = store.product_history_outbox_batches(10).unwrap();
        let committed = batches
            .iter()
            .find(|batch| batch.height == block.canonical.header.height)
            .expect("missing product-history batch for empty block");
        let placed_count = committed
            .events
            .iter()
            .filter(|event| {
                event.account_id == aid.0
                    && matches!(event.kind, sybil_history_types::AccountEventKind::Placed)
            })
            .count();
        assert_eq!(
            placed_count, 1,
            "direct-admit Placed history must be durable exactly once even with no in-memory fallback"
        );

        assert!(
            committed
                .equity
                .iter()
                .any(|point| point.account_id == aid.0
                    && point.height == block.canonical.header.height),
            "equity point from the empty-fill block must be durable"
        );

        handle.produce_block().await.unwrap();
        let placed_count_after_next_block = store
            .product_history_outbox_batches(10)
            .unwrap()
            .iter()
            .flat_map(|batch| &batch.events)
            .filter(|event| {
                event.account_id == aid.0
                    && matches!(event.kind, sybil_history_types::AccountEventKind::Placed)
            })
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
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };
        let seq = BlockSequencer::new(
            accounts,
            markets.clone(),
            vec![],
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

        // First indicative tick fires at +750ms. Poll instead of assuming the
        // solve and actor round-trip fit a fixed wall-clock sleep: the full
        // workspace suite can heavily contend on the persistence tests.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snap = handle.get_indicative(m0).await.unwrap();
            if snap.computed_at_ms > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "indicative tick should have written a snapshot by now"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
