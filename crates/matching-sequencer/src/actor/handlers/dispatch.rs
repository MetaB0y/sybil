use super::super::*;

#[ractor::async_trait]
impl Actor for SequencerActor {
    type Msg = SequencerMsg;
    type State = SequencerActorState;
    type Arguments = SequencerActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let global_submission_limiter = rate_limiter(
            args.sequencer.config.max_global_submissions_per_second,
            args.sequencer.config.global_submission_burst,
        );
        Ok(SequencerActorState {
            sequencer: args.sequencer,
            latest_block: None,
            recent_blocks: VecDeque::new(),
            block_broadcast: args.block_broadcast,
            pause_count: 0,
            halted_error: None,
            store: args.store,
            global_submission_limiter,
            account_submission_limiters: HashMap::new(),
            mailbox_monitor: args.mailbox_monitor,
            indicative_cache: HashMap::new(),
            indicative_solve_gate: IndicativeSolveGate::default(),
            background_tasks: TaskTracker::new(),
            background_cancel: CancellationToken::new(),
            #[cfg(test)]
            next_tick_hold: None,
        })
    }

    async fn post_start(
        &self,
        myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let actor = myself.clone();
        let block_interval = state.sequencer.config.block_interval;
        let mailbox_monitor = state.mailbox_monitor.clone();
        let cancel = state.background_cancel.child_token();
        state.background_tasks.spawn(async move {
            let mut ticker = interval_at(Instant::now() + block_interval, block_interval);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = ticker.tick() => {}
                }
                mailbox_monitor.queued();
                if actor.send_message(SequencerMsg::Tick).is_err() {
                    mailbox_monitor.send_failed();
                    break;
                }
            }
        });

        // Indicative scheduler (C2). Separate timer task, NOT an idle
        // branch in on_tick — block production and indicative refresh are
        // decoupled. Cadence chosen well under one block period so the
        // open-batch snapshot refreshes mid-batch.
        let actor_indicative = myself.clone();
        let mailbox_indicative = state.mailbox_monitor.clone();
        let indicative_interval = std::time::Duration::from_millis(750);
        let indicative_cancel = state.background_cancel.child_token();
        state.background_tasks.spawn(async move {
            let mut ticker = interval_at(Instant::now() + indicative_interval, indicative_interval);
            loop {
                tokio::select! {
                    _ = indicative_cancel.cancelled() => return,
                    _ = ticker.tick() => {}
                }
                mailbox_indicative.queued();
                if actor_indicative
                    .send_message(SequencerMsg::IndicativeTick)
                    .is_err()
                {
                    mailbox_indicative.send_failed();
                    break;
                }
            }
        });
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.background_cancel.cancel();
        state.background_tasks.close();
        state.background_tasks.wait().await;
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.mailbox_monitor.started();
        match message {
            SequencerMsg::Tick => {
                let _ = state.on_tick().await?;
            }
            #[cfg(test)]
            SequencerMsg::TestCrashOnNextBlock(crashpoint) => {
                let _ = state.on_tick_inner(Some(crashpoint)).await?;
            }
            #[cfg(test)]
            SequencerMsg::TestHoldNextTick(hold, reply) => {
                state.next_tick_hold = Some(hold);
                let _ = reply.send(());
            }
            SequencerMsg::IndicativeTick => {
                state.on_indicative_tick(myself.clone());
            }
            SequencerMsg::IndicativeUpdate {
                target_height,
                snapshots,
            } => {
                if state.sequencer.height().saturating_add(1) == target_height {
                    state.indicative_cache = snapshots;
                }
                state.indicative_solve_gate.finish();
            }
            SequencerMsg::IndicativeSolveFailed { solver, error } => {
                metrics::counter!(
                    "sybil_indicative_solve_failures_total",
                    "solver" => solver.clone(),
                    "reason" => "panic"
                )
                .increment(1);
                tracing::error!(
                    solver = %solver,
                    error = %error,
                    "indicative shadow solve failed; releasing gate"
                );
                state.indicative_solve_gate.finish();
            }
            SequencerMsg::Query(query) => {
                query.execute(state);
            }
            SequencerMsg::SubmitOrder(submission, reply) => {
                let order_count = submission.orders.len();
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.admit_or_defer(submission, false).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("unsigned", order_count, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitIocOrder(submission, reply) => {
                let order_count = submission.orders.len();
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.admit_or_defer(submission, true).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("unsigned", order_count, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitActorEpoch(epoch, reply) => {
                let order_count = epoch.submission.orders.len();
                let result = state.handle_actor_epoch(epoch).await;
                state.record_submission_metrics("actor_epoch", order_count, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitSignedOrder(signed, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_signed_order(signed).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("signed", 1, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitAuthenticatedOrder(authenticated, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_authenticated_order(authenticated).await,
                    Err(err) => Err(err),
                };
                state.record_submission_metrics("signed", 1, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::CancelSignedOrder(signed, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_signed_cancel(signed).await,
                    Err(err) => Err(err),
                };
                state.record_cancel_metrics("signed", &result);
                let _ = reply.send(result);
            }
            SequencerMsg::CancelAuthenticatedOrder(authenticated, reply) => {
                let result = match state.check_global_submission_rate() {
                    Ok(()) => state.handle_authenticated_cancel(authenticated).await,
                    Err(err) => Err(err),
                };
                state.record_cancel_metrics("signed", &result);
                let _ = reply.send(result);
            }
            SequencerMsg::GetStateProof(leaf_key, reply) => {
                let _ = reply.send(state.handle_state_proof(leaf_key).await);
            }
            SequencerMsg::ProduceBlock(reply) => {
                let result = match state.on_tick().await? {
                    BlockTickOutcome::Produced(block) => Ok(*block),
                    BlockTickOutcome::Paused => Err(SequencerError::BlockProductionPaused),
                    BlockTickOutcome::Halted(error) | BlockTickOutcome::PersistFailed(error) => {
                        Err(error)
                    }
                };
                let _ = reply.send(result);
            }
            SequencerMsg::CreateAccount(initial_balance, reply) => {
                let _ = reply.send(state.handle_create_account(initial_balance).await);
            }
            SequencerMsg::CreateAccountWithInitialKey(initial_balance, pubkey, meta, reply) => {
                let _ = reply.send(
                    state
                        .handle_create_account_with_initial_key(initial_balance, pubkey, meta)
                        .await,
                );
            }
            SequencerMsg::FundAccount(account_id, amount, reply) => {
                let _ = reply.send(state.handle_fund_account(account_id, amount).await);
            }
            SequencerMsg::SubmitL1Deposit(deposit, reply) => {
                let _ = reply.send(state.handle_l1_deposit(deposit).await);
            }
            SequencerMsg::CreateBridgeWithdrawal(request, reply) => {
                let _ = reply.send(state.handle_bridge_withdrawal(request).await);
            }
            SequencerMsg::CreateSignedBridgeWithdrawal(signed, reply) => {
                let _ = reply.send(state.handle_signed_bridge_withdrawal(signed).await);
            }
            SequencerMsg::CreateAuthenticatedBridgeWithdrawal(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_bridge_withdrawal(authenticated)
                        .await,
                );
            }
            SequencerMsg::ApplyBridgeWithdrawalL1Event(event, reply) => {
                let _ = reply.send(state.handle_bridge_withdrawal_l1_event(event).await);
            }
            SequencerMsg::ObserveBridgeL1Height(height, reply) => {
                let _ = reply.send(state.handle_observe_bridge_l1_height(height).await);
            }
            SequencerMsg::RegisterPubkey(account_id, pubkey, auth_scheme, reply) => {
                let _ = reply.send(
                    state
                        .handle_register_pubkey(account_id, pubkey, auth_scheme)
                        .await,
                );
            }
            SequencerMsg::RegisterPubkeyWithMeta(account_id, pubkey, meta, reply) => {
                let _ = reply.send(
                    state
                        .handle_register_pubkey_with_meta(account_id, pubkey, meta)
                        .await,
                );
            }
            SequencerMsg::RegisterKeySigned(signed, reply) => {
                let _ = reply.send(state.handle_signed_key_registration(signed).await);
            }
            SequencerMsg::RegisterKeyAuthenticated(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_key_registration(authenticated)
                        .await,
                );
            }
            SequencerMsg::SetProfileSigned(signed, reply) => {
                let _ = reply.send(state.handle_signed_profile_update(signed).await);
            }
            SequencerMsg::SetProfileAuthenticated(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_profile_update(authenticated)
                        .await,
                );
            }
            SequencerMsg::RevokeSigningKeySigned(signed, reply) => {
                let _ = reply.send(state.handle_signed_key_revocation(signed).await);
            }
            SequencerMsg::RevokeSigningKeyAuthenticated(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_key_revocation(authenticated)
                        .await,
                );
            }
            SequencerMsg::CreateApiKeySigned(signed, reply) => {
                let _ = reply.send(state.handle_signed_api_key_create(signed).await);
            }
            SequencerMsg::CreateApiKeyAuthenticated(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_api_key_create(authenticated)
                        .await,
                );
            }
            SequencerMsg::RevokeApiKeySigned(signed, reply) => {
                let _ = reply.send(state.handle_signed_api_key_revoke(signed).await);
            }
            SequencerMsg::RevokeApiKeyAuthenticated(authenticated, reply) => {
                let _ = reply.send(
                    state
                        .handle_authenticated_api_key_revoke(authenticated)
                        .await,
                );
            }
            SequencerMsg::CreateMarket(name, reply) => {
                let _ = reply.send(state.handle_create_market(name).await);
            }
            SequencerMsg::CollateralizeCompleteSet(account_id, market_id, quantity, reply) => {
                let _ = reply.send(
                    state
                        .handle_collateralize_complete_set(account_id, market_id, quantity)
                        .await,
                );
            }
            SequencerMsg::RedeemCompleteSet(account_id, market_id, quantity, reply) => {
                let _ = reply.send(
                    state
                        .handle_redeem_complete_set(account_id, market_id, quantity)
                        .await,
                );
            }
            SequencerMsg::ApplyCompleteSetInventoryActions(account_id, actions, reply) => {
                let _ = reply.send(
                    state
                        .handle_complete_set_inventory_actions(account_id, actions)
                        .await,
                );
            }
            SequencerMsg::ActivateLiquidityUniverse(
                generation,
                policy_digest,
                market_ids,
                reply,
            ) => {
                let _ = reply.send(
                    state
                        .handle_activate_liquidity_universe(generation, policy_digest, market_ids)
                        .await,
                );
            }
            SequencerMsg::CreateMarketGroup(name, market_ids, reply) => {
                let _ = reply.send(state.handle_create_market_group(name, market_ids).await);
            }
            SequencerMsg::ExtendMarketGroup(group_id, market_id, reply) => {
                let _ = reply.send(state.handle_extend_market_group(group_id, market_id).await);
            }
            SequencerMsg::ResolveMarket(market_id, payout_nanos, reply) => {
                let _ = reply.send(state.handle_resolve_market(market_id, payout_nanos).await);
            }
            SequencerMsg::ResolveMarketAttested(market_id, signed, reply) => {
                let _ = reply.send(
                    state
                        .handle_resolve_market_attested(market_id, signed)
                        .await,
                );
            }
            SequencerMsg::RegisterFeed(pubkey, name, reply) => {
                let _ = reply.send(state.handle_register_feed(pubkey, name).await);
            }
            SequencerMsg::InstallTemplate(template, reply) => {
                let _ = reply.send(state.handle_install_template(template).await);
            }
            SequencerMsg::GetBlockPage(before_height, limit, reply) => {
                let limit = limit.min(MAX_BLOCK_REPLAY_QUERY_BLOCKS);
                let result = match &state.store {
                    Some(store) => store
                        .load_block_page(before_height, limit)
                        .await
                        .map_err(|error| SequencerError::Persistence(error.to_string())),
                    None => Ok(state
                        .recent_blocks
                        .iter()
                        .rev()
                        .filter(|block| {
                            before_height
                                .is_none_or(|before| block.canonical.header.height < before)
                        })
                        .take(limit)
                        .cloned()
                        .collect()),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetBlock(height, reply) => {
                let block = state
                    .recent_blocks
                    .iter()
                    .find(|b| b.canonical.header.height == height)
                    .cloned();
                let result = match block {
                    Some(block) => Ok(block),
                    None => match &state.store {
                        Some(store) => match store.load_block(height).await {
                            Ok(Some(block)) => Ok(block),
                            Ok(None) => match store.canonical_archive_meta() {
                                Ok(meta) => {
                                    if let Some(retention_min_height) = meta.oldest_retained_height
                                    {
                                        if height < retention_min_height {
                                            Err(SequencerError::BlockPruned {
                                                requested_height: height,
                                                retention_min_height,
                                            })
                                        } else {
                                            Err(SequencerError::BlockNotFound)
                                        }
                                    } else {
                                        Err(SequencerError::BlockNotFound)
                                    }
                                }
                                Err(error) => Err(SequencerError::Persistence(error.to_string())),
                            },
                            Err(error) => Err(SequencerError::Persistence(error.to_string())),
                        },
                        None => Err(SequencerError::BlockNotFound),
                    },
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetDaArtifact(height, reply) => {
                let result = match &state.store {
                    Some(store) => {
                        let oldest_retained_height = store
                            .canonical_archive_meta()
                            .map_err(|error| SequencerError::Persistence(error.to_string()))
                            .map(|meta| meta.oldest_retained_height);
                        match oldest_retained_height {
                            Ok(oldest_retained_height) => store
                                .load_da_artifact(height)
                                .await
                                .map(|artifact| DaArtifactLookup {
                                    artifact,
                                    oldest_retained_height,
                                })
                                .map_err(|error| SequencerError::Persistence(error.to_string())),
                            Err(error) => Err(error),
                        }
                    }
                    None => Ok(DaArtifactLookup {
                        artifact: None,
                        oldest_retained_height: None,
                    }),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::GetDaManifest(height, reply) => {
                let result = match &state.store {
                    Some(store) => {
                        let oldest_retained_height = store
                            .canonical_archive_meta()
                            .map_err(|error| SequencerError::Persistence(error.to_string()))
                            .map(|meta| meta.oldest_retained_height);
                        match oldest_retained_height {
                            Ok(oldest_retained_height) => store
                                .load_da_manifest(height)
                                .await
                                .map(|manifest| DaManifestLookup {
                                    manifest,
                                    oldest_retained_height,
                                })
                                .map_err(|error| SequencerError::Persistence(error.to_string())),
                            Err(error) => Err(error),
                        }
                    }
                    None => Ok(DaManifestLookup {
                        manifest: None,
                        oldest_retained_height: None,
                    }),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::CreateMarketWithMetadata(name, metadata, reply) => {
                let _ = reply.send(
                    state
                        .handle_create_market_with_metadata(name, metadata)
                        .await,
                );
            }
            SequencerMsg::ListAutoResolutionRecords(reply) => {
                let result = match &state.store {
                    Some(store) => store
                        .auto_resolution_records()
                        .map_err(|error| SequencerError::Persistence(error.to_string())),
                    None => Ok(Vec::new()),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::PutAutoResolutionRecord(record, reply) => {
                let result = match &state.store {
                    Some(store) => store
                        .put_auto_resolution_record(record)
                        .await
                        .map_err(|error| SequencerError::Persistence(error.to_string())),
                    None => Ok(()),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::PauseBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_add(1);
                let _ = reply.send(());
            }
            SequencerMsg::ResumeBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_sub(1);
                let _ = reply.send(());
            }
        }
        Ok(())
    }
}
