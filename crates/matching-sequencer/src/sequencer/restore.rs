use super::*;

#[derive(Debug, thiserror::Error)]
pub enum SequencerRestoreError {
    #[error("failed to expire stale committed resting orders during restore: {0}")]
    SnapshotExpiry(#[source] crate::order_book::ReservationError),
    #[error("failed to replay acknowledged write {sequence} ({kind}): {source}")]
    AcknowledgedWrite {
        sequence: u64,
        kind: &'static str,
        #[source]
        source: SequencerError,
    },
}

impl BlockSequencer {
    pub fn new(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        solver: Arc<dyn Solver>,
        config: SequencerConfig,
    ) -> Self {
        let order_book = OrderBook::new(config.order_ttl_blocks);
        let bridge = BridgeState::default();
        let lifecycle = crate::market_lifecycle::MarketLifecycle::new();
        let last_clearing_prices = HashMap::new();
        let committed_state_sidecar = state_sidecar_snapshot(
            &bridge,
            &order_book,
            &markets,
            &market_groups,
            &lifecycle,
            &last_clearing_prices,
        );
        let committed_deposit_frontier = bridge.deposit_frontier;
        Self {
            accounts,
            solver,
            next_order_id: 1,
            order_book,
            height: 0,
            markets,
            market_groups,
            last_header: None,
            genesis_hash: None,
            committed_state_sidecar,
            committed_deposit_frontier,
            analytics: AnalyticsState::new(&config),
            lifecycle,
            pubkey_registry: HashMap::new(),
            api_key_index: HashMap::new(),
            bridge,
            pending_system_events: Vec::new(),
            pending_system_account_baselines: HashMap::new(),
            pending_bundles: Vec::new(),
            config,
        }
    }

    /// Create with the production default solver:
    /// [`matching_solver::RetainedCashSolver`].
    ///
    /// This is the solver that actually settles blocks in production (reached
    /// via `sybil-api`'s startup and [`BlockSequencer::restore`]). It runs a
    /// generalized Frank--Wolfe on the paper's exact retained-cash objective.
    /// Each iteration calls the HiGHS matching oracle; termination uses a
    /// certified continuous objective gap rather than iterate stability.
    ///
    /// - `LpSolver` remains the low-latency risk-neutral baseline. Its SLP MM
    ///   budget rows are a capped fixed-point heuristic, so it is no longer the
    ///   default when shared capital can bind after prices move.
    /// - `ConicSolver` in QuasiFisher mode solves the same convex objective by
    ///   exponential cones and is retained as an independent reference.
    /// - A configured Frank--Wolfe cap can return a valid landed allocation
    ///   before meeting the requested certificate. Diagnostics preserve that
    ///   distinction; the verifier remains the settlement authority.
    ///
    /// Inject a different `Arc<dyn Solver>` via [`BlockSequencer::new`] to
    /// experiment without touching this default.
    pub fn with_default_solver(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        config: SequencerConfig,
    ) -> Self {
        Self::new(
            accounts,
            markets,
            market_groups,
            Arc::new(matching_solver::RetainedCashSolver::new()),
            config,
        )
    }

    /// Restore from persisted state, panicking rather than starting from a
    /// partial acknowledged-write prefix. Production startup uses
    /// [`Self::try_restore`] to surface the same fail-closed error cleanly.
    pub fn restore(state: RestoredState, config: SequencerConfig) -> Self {
        Self::try_restore(state, config)
            .expect("persisted sequencer state must replay without divergence")
    }

    /// Restore from the committed snapshot and replay the complete global
    /// acknowledged-write interval in exact actor acceptance order.
    pub fn try_restore(
        state: RestoredState,
        config: SequencerConfig,
    ) -> Result<Self, SequencerRestoreError> {
        let solver: Arc<dyn Solver> = Arc::new(matching_solver::RetainedCashSolver::new());
        let mut lifecycle = MarketLifecycle::new();
        for (market_id, status) in state.market_statuses {
            lifecycle.set_market_status(market_id, status);
        }
        for (market_id, meta) in state.market_metadata {
            lifecycle.set_market_metadata(market_id, meta);
        }
        for feed in state.data_feeds {
            lifecycle.restore_feed(feed);
        }
        for template in state.resolution_templates {
            lifecycle.install_template(template);
        }
        let committed_order_book =
            OrderBook::restore(state.resting_orders.clone(), config.order_ttl_blocks);
        let committed_state_sidecar = state_sidecar_snapshot(
            &state.bridge_state,
            &committed_order_book,
            &state.markets,
            &state.market_groups,
            &lifecycle,
            &state.analytics.last_clearing_prices,
        );
        let committed_deposit_frontier = state.bridge_state.deposit_frontier;
        let mut order_book = OrderBook::restore(state.resting_orders, config.order_ttl_blocks);
        // A committed snapshot should already have swept these orders. Keep
        // the recovery repair before all later acknowledged writes so those
        // writes observe the same post-block state the live actor observed.
        let expired_on_restore = order_book
            .expire_committed_through(state.height)
            .map_err(SequencerRestoreError::SnapshotExpiry)?;
        if !expired_on_restore.is_empty() {
            metrics::counter!("sybil_restore_expired_resting_orders_total")
                .increment(expired_on_restore.len() as u64);
            debug!(
                height = state.height,
                expired_orders = expired_on_restore.len(),
                "expired stale resting orders before acknowledged-write replay"
            );
        }
        let next_order_id = order_book
            .resting_orders()
            .map(|(order, _)| order.id.saturating_add(1))
            .fold(state.next_order_id, u64::max);
        let acknowledged_writes = state.acknowledged_writes;
        let mut restored = Self {
            accounts: state.accounts,
            solver,
            next_order_id,
            order_book,
            height: state.height,
            markets: state.markets,
            market_groups: state.market_groups,
            last_header: state.last_header,
            genesis_hash: Some(state.genesis_hash),
            committed_state_sidecar,
            committed_deposit_frontier,
            analytics: AnalyticsState::restore(state.analytics, &config),
            lifecycle,
            pubkey_registry: state.pubkey_registry,
            api_key_index: HashMap::new(),
            bridge: state.bridge_state,
            pending_system_events: Vec::new(),
            pending_system_account_baselines: HashMap::new(),
            pending_bundles: Vec::new(),
            config,
        };
        for entry in acknowledged_writes {
            let sequence = entry.sequence;
            let kind = entry.write.kind();
            if let Err(source) = restored.replay_acknowledged_write(entry.write) {
                metrics::counter!(
                    "sybil_restore_acknowledged_write_failures_total",
                    "kind" => kind
                )
                .increment(1);
                tracing::error!(
                    height = restored.height,
                    sequence,
                    kind,
                    error = %source,
                    "acknowledged-write replay diverged; refusing partial recovery"
                );
                return Err(SequencerRestoreError::AcknowledgedWrite {
                    sequence,
                    kind,
                    source,
                });
            }
        }
        let account_ids: Vec<AccountId> = restored.accounts.iter().map(|(id, _)| *id).collect();
        restored.analytics.seed_equity_known(account_ids);
        restored.rebuild_api_key_index();
        Ok(restored)
    }

    fn replay_acknowledged_write(
        &mut self,
        write: AcknowledgedWrite,
    ) -> Result<(), SequencerError> {
        match write {
            AcknowledgedWrite::DirectAdmit(resting) => {
                self.next_order_id = self.next_order_id.max(resting.order.id.saturating_add(1));
                self.order_book.reinsert_for_replay(resting)?;
                Ok(())
            }
            AcknowledgedWrite::DeferredBundle(submission) => {
                for order in &submission.orders {
                    self.next_order_id = self.next_order_id.max(order.id.saturating_add(1));
                }
                self.pending_bundles.push(submission);
                Ok(())
            }
            AcknowledgedWrite::AuthenticatedDirectAdmit {
                resting,
                nonce,
                authorization,
            } => {
                let account_id = resting.account_id;
                let order = resting.order.clone();
                self.next_order_id = self.next_order_id.max(order.id.saturating_add(1));
                self.order_book.reinsert_for_replay(resting)?;
                self.apply_client_action_authorized(sybil_verifier::ClientActionWitness::Order {
                    account_id: account_id.0,
                    order,
                    nonce,
                    authorization,
                })
            }
            AcknowledgedWrite::AuthenticatedDeferredBundle {
                submission,
                nonce,
                authorization,
            } => {
                for order in &submission.orders {
                    self.next_order_id = self.next_order_id.max(order.id.saturating_add(1));
                }
                let account_id = submission.account_id;
                let order = submission.orders.first().cloned().ok_or_else(|| {
                    SequencerError::Persistence(
                        "authenticated deferred submission has no order".to_string(),
                    )
                })?;
                self.pending_bundles.push(submission);
                self.apply_client_action_authorized(sybil_verifier::ClientActionWitness::Order {
                    account_id: account_id.0,
                    order,
                    nonce,
                    authorization,
                })
            }
            AcknowledgedWrite::AuthenticatedCancel {
                account_id,
                order_id,
                nonce,
                authorization,
                timestamp_ms,
            } => {
                self.apply_client_action_authorized(sybil_verifier::ClientActionWitness::Cancel {
                    account_id: account_id.0,
                    order_id,
                    nonce,
                    authorization,
                })?;
                self.cancel_pending_order_at(account_id, order_id, timestamp_ms)
            }
            AcknowledgedWrite::ControlPlane(command) => self.replay_control_plane_command(command),
            AcknowledgedWrite::L1Deposit(deposit) => self.ingest_l1_deposit(deposit).map(|_| ()),
            AcknowledgedWrite::BridgeWithdrawal(request) => {
                self.request_bridge_withdrawal(request).map(|_| ())
            }
            AcknowledgedWrite::BridgeL1Input(input) => match input {
                crate::bridge::BridgeL1Input::WithdrawalEvent(event) => {
                    self.apply_bridge_withdrawal_l1_event(event).map(|_| ())
                }
                crate::bridge::BridgeL1Input::ObservedHeight(height) => {
                    self.observe_bridge_l1_height(height).map(|_| ())
                }
            },
        }
    }

    /// Rebuild the derived active-API-key reverse index from persisted accounts
    /// (SYB-60). Called after restore; the index itself is never serialized.
    fn rebuild_api_key_index(&mut self) {
        self.api_key_index.clear();
        for (id, account) in self.accounts.iter() {
            for key in &account.api_keys {
                if key.is_active() {
                    self.api_key_index.insert(key.hash, *id);
                }
            }
        }
    }

    fn replay_control_plane_command(
        &mut self,
        command: ControlPlaneCommand,
    ) -> Result<(), SequencerError> {
        match command {
            ControlPlaneCommand::CreateAccount { initial_balance } => {
                self.create_account(initial_balance);
                Ok(())
            }
            ControlPlaneCommand::CreateAccountAt {
                initial_balance,
                timestamp_ms,
            } => {
                self.create_account_at(initial_balance, timestamp_ms);
                Ok(())
            }
            ControlPlaneCommand::FundAccount {
                account_id,
                amount,
                timestamp_ms,
            } => self
                .fund_account_at(account_id, amount, timestamp_ms)
                .map(|_| ()),
            ControlPlaneCommand::RegisterPubkey {
                account_id,
                compressed_pubkey,
                auth_scheme,
            } => {
                let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&compressed_pubkey)
                    .ok_or_else(|| {
                        SequencerError::Persistence(
                            "invalid pubkey in control-plane WAL".to_string(),
                        )
                    })?;
                self.register_first_pubkey_with_meta(
                    account_id,
                    pubkey,
                    crate::crypto::RegisteredPubkey::primary(account_id, auth_scheme),
                )
            }
            ControlPlaneCommand::AdvanceReplayNonce { account_id, nonce } => {
                self.advance_replay_nonce(account_id, nonce)
            }
            ControlPlaneCommand::CreateMarket { name } => {
                self.create_market(name);
                Ok(())
            }
            ControlPlaneCommand::CreateMarketWithMetadata { name, metadata } => {
                self.create_market_with_metadata(name, metadata)?;
                Ok(())
            }
            ControlPlaneCommand::CreateMarketGroup { name, market_ids } => {
                self.create_market_group(name, market_ids);
                Ok(())
            }
            ControlPlaneCommand::ExtendMarketGroup {
                group_id,
                market_id,
            } => self.extend_market_group(group_id, market_id).map(|_| ()),
            ControlPlaneCommand::CancelPendingOrder {
                account_id,
                order_id,
                timestamp_ms,
            } => self.cancel_pending_order_at(account_id, order_id, timestamp_ms),
            ControlPlaneCommand::ResolveMarket {
                market_id,
                payout_nanos,
                timestamp_ms,
            } => self
                .resolve_market(market_id, payout_nanos, timestamp_ms)
                .map(|_| ()),
            ControlPlaneCommand::ResolveMarketAttested {
                market_id,
                signed,
                timestamp_ms,
            } => self
                .resolve_market_attested(market_id, &signed, timestamp_ms)
                .map(|_| ()),
            ControlPlaneCommand::RegisterFeed {
                pubkey,
                name,
                timestamp_ms,
            } => {
                self.register_feed(pubkey, name, timestamp_ms);
                Ok(())
            }
            ControlPlaneCommand::InstallTemplate { template } => {
                self.install_template(template);
                Ok(())
            }
            ControlPlaneCommand::RegisterPubkeyWithMeta {
                account_id,
                compressed_pubkey,
                auth_scheme,
                label,
                scope,
                created_at_ms,
            } => {
                let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&compressed_pubkey)
                    .ok_or_else(|| {
                        SequencerError::Persistence(
                            "invalid pubkey in control-plane WAL".to_string(),
                        )
                    })?;
                self.register_first_pubkey_with_meta(
                    account_id,
                    pubkey,
                    crate::crypto::RegisteredPubkey {
                        account_id,
                        auth_scheme,
                        label,
                        scope,
                        created_at_ms,
                    },
                )
            }
            ControlPlaneCommand::RegisterPubkeyAuthorized {
                account_id,
                compressed_pubkey,
                auth_scheme,
                label,
                scope,
                created_at_ms,
                authorization,
            } => {
                let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&compressed_pubkey)
                    .ok_or_else(|| {
                        SequencerError::Persistence(
                            "invalid pubkey in control-plane WAL".to_string(),
                        )
                    })?;
                self.register_pubkey_with_meta_authorized(
                    account_id,
                    pubkey,
                    crate::crypto::RegisteredPubkey {
                        account_id,
                        auth_scheme,
                        label,
                        scope,
                        created_at_ms,
                    },
                    authorization,
                )
            }
            ControlPlaneCommand::RevokeSigningKey {
                account_id,
                compressed_pubkey,
                authorization,
            } => {
                let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&compressed_pubkey)
                    .ok_or_else(|| {
                        SequencerError::Persistence(
                            "invalid pubkey in control-plane WAL".to_string(),
                        )
                    })?;
                self.revoke_signing_key(account_id, &pubkey, authorization)
            }
            ControlPlaneCommand::SetProfile {
                account_id,
                display_name,
                avatar_seed,
            } => self
                .set_profile(account_id, display_name, avatar_seed)
                .map(|_| ()),
            ControlPlaneCommand::CreateApiKey {
                account_id,
                token_hash,
                label,
                created_at_ms,
            } => self
                .create_api_key(account_id, token_hash, label, created_at_ms)
                .map(|_| ()),
            ControlPlaneCommand::RevokeApiKey {
                account_id,
                api_key_id,
                revoked_at_ms,
            } => self.revoke_api_key(account_id, api_key_id, revoked_at_ms),
            ControlPlaneCommand::CreateAccountWithInitialKey {
                initial_balance,
                timestamp_ms,
                compressed_pubkey,
                auth_scheme,
                label,
                scope,
                created_at_ms,
            } => {
                let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&compressed_pubkey)
                    .ok_or_else(|| {
                        SequencerError::Persistence(
                            "invalid pubkey in control-plane WAL".to_string(),
                        )
                    })?;
                let prepared = self.prepare_account_with_initial_key(
                    initial_balance,
                    timestamp_ms,
                    pubkey,
                    crate::crypto::RegisteredPubkey {
                        account_id: AccountId(self.accounts.next_id()),
                        auth_scheme,
                        label,
                        scope,
                        created_at_ms,
                    },
                )?;
                self.apply_prepared_account_with_initial_key(prepared);
                Ok(())
            }
        }
    }
}
