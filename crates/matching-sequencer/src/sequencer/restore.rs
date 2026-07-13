use super::*;

impl BlockSequencer {
    pub fn new(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        oracle: Arc<dyn Oracle>,
        solver: Arc<dyn Solver>,
        config: SequencerConfig,
    ) -> Self {
        let order_book = OrderBook::new(config.order_ttl_blocks);
        let bridge = BridgeState::default();
        let lifecycle = crate::market_lifecycle::MarketLifecycle::new(oracle);
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

    /// Create with the production default solver: [`matching_solver::LpSolver`].
    ///
    /// This is the solver that actually settles blocks in production (reached
    /// via `sybil-api`'s startup and [`BlockSequencer::restore`]). It runs a
    /// plain single-pass LP: the HiGHS welfare LP plus one SLP re-solve to shade
    /// MM budgets (`LpConfig::default().max_mm_iterations == 1`). The choice is
    /// deliberate, and its known tradeoff is recorded here so it doesn't get
    /// "fixed" by accident:
    ///
    /// - A single SLP pass linearizes MM budget constraints at the *unbudgeted*
    ///   clearing prices, so it mis-sizes budgets that only bind after prices
    ///   move. `matching_solver::IterLpSolver` (EG μ-boosted fixed point) exists
    ///   precisely to close that gap — see
    ///   `iterative_lp_solver::tests::test_auglp_vs_lp_tight_budget_price_shift`,
    ///   which asserts IterLP reaches >10× the welfare of the single-pass LP on a
    ///   tight-budget, price-shifting instance.
    /// - We still ship `LpSolver` because it is the simplest low-latency,
    ///   conformance- and verifier-clean settlement path. IterLP now shares the
    ///   same integer projection/trim boundary and passes conformance, but its
    ///   damped multiplier update is a capped fixed-point heuristic without a
    ///   general convergence guarantee. The paper's log-welfare program is
    ///   represented directly by `ConicSolver` in QuasiFisher mode; that remains
    ///   a research/reference path pending a broader empirical and operational
    ///   evaluation. The non-LP solvers are otherwise exercised by
    ///   `matching-sim` and benches.
    ///
    /// Inject a different `Arc<dyn Solver>` via [`BlockSequencer::new`] to
    /// experiment without touching this default.
    pub fn with_default_solver(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        oracle: Arc<dyn Oracle>,
        config: SequencerConfig,
    ) -> Self {
        Self::new(
            accounts,
            markets,
            market_groups,
            oracle,
            Arc::new(matching_solver::LpSolver::new()),
            config,
        )
    }

    /// Restore from persisted state.
    pub fn restore(state: RestoredState, oracle: Arc<dyn Oracle>, config: SequencerConfig) -> Self {
        let solver: Arc<dyn Solver> = Arc::new(matching_solver::LpSolver::new());
        let control_plane_log = state.control_plane_log.clone();
        let mut lifecycle = MarketLifecycle::new(oracle);
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
        // Replay the admit-log WAL on top of the snapshot: every non-MM
        // admit since the last committed block is durable on its own row
        // and must be re-inserted before the sequencer starts taking new
        // traffic, so nothing acknowledged with a 200 OK is dropped by a
        // crash.
        for resting in state.admit_log {
            order_book.reinsert_for_replay(resting);
        }
        let next_order_id = order_book
            .resting_orders()
            .map(|(order, _)| order.id.saturating_add(1))
            .chain(
                state
                    .pending_bundles
                    .iter()
                    .flat_map(|submission| submission.orders.iter())
                    .map(|order| order.id.saturating_add(1)),
            )
            .fold(state.next_order_id, u64::max);
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
            pending_bundles: state.pending_bundles,
            config,
        };
        // Control-plane replay can create/fund accounts and cancel resting
        // orders. Bridge WAL validation depends on those effects, so it must
        // run before deposits/withdrawals are replayed. The separate WAL
        // tables still do not encode exact cross-subsystem event ordering; if
        // system-event interleaving becomes consensus-sensitive, collapse these
        // into one sequenced acknowledged-write log.
        for command in control_plane_log {
            if let Err(error) = restored.replay_control_plane_command(command) {
                metrics::counter!(
                    "sybil_restore_wal_rows_dropped_total",
                    "kind" => "control_plane"
                )
                .increment(1);
                tracing::warn!(
                    height = restored.height,
                    %error,
                    "dropping invalid control-plane WAL row during restore"
                );
            }
        }

        let expired_on_restore = restored
            .order_book
            .expire_committed_through(restored.height)
            .expect("restored reservation aggregates were validated before replay");
        if !expired_on_restore.is_empty() {
            metrics::counter!("sybil_restore_expired_resting_orders_total")
                .increment(expired_on_restore.len() as u64);
            debug!(
                height = restored.height,
                expired_orders = expired_on_restore.len(),
                "expired stale resting orders during restore"
            );
        }

        for deposit in state.pending_l1_deposits {
            let account_id = deposit.account_id;
            let deposit_id = deposit.deposit_id;
            if let Err(error) = restored.ingest_l1_deposit(deposit) {
                metrics::counter!(
                    "sybil_restore_wal_rows_dropped_total",
                    "kind" => "l1_deposit"
                )
                .increment(1);
                tracing::warn!(
                    height = restored.height,
                    deposit_id,
                    account_id = ?account_id,
                    %error,
                    "dropping invalid pending l1 deposit WAL row during restore"
                );
            }
        }
        for request in state.pending_bridge_withdrawals {
            let account_id = request.account_id;
            let amount_token_units = request.amount_token_units;
            let expiry_height = request.expiry_height;
            if let Err(error) = restored.request_bridge_withdrawal(request) {
                metrics::counter!(
                    "sybil_restore_wal_rows_dropped_total",
                    "kind" => "bridge_withdrawal"
                )
                .increment(1);
                tracing::warn!(
                    height = restored.height,
                    account_id = ?account_id,
                    amount_token_units,
                    expiry_height,
                    %error,
                    "dropping invalid pending bridge withdrawal WAL row during restore"
                );
            }
        }
        for input in state.pending_bridge_l1_inputs {
            let result = match input {
                crate::bridge::BridgeL1Input::WithdrawalEvent(event) => {
                    restored.apply_bridge_withdrawal_l1_event(event).map(|_| ())
                }
                crate::bridge::BridgeL1Input::ObservedHeight(height) => {
                    restored.observe_bridge_l1_height(height).map(|_| ())
                }
            };
            if let Err(error) = result {
                metrics::counter!(
                    "sybil_restore_wal_rows_dropped_total",
                    "kind" => "bridge_l1_input"
                )
                .increment(1);
                tracing::warn!(
                    height = restored.height,
                    %error,
                    "dropping invalid pending bridge L1 input WAL row during restore"
                );
            }
        }
        let account_ids: Vec<AccountId> = restored.accounts.iter().map(|(id, _)| *id).collect();
        restored.analytics.seed_equity_known(account_ids);
        restored.rebuild_api_key_index();
        restored
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
                self.create_market_with_metadata(name, metadata);
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
