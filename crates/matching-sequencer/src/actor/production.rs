use super::*;

pub(super) enum BlockTickOutcome {
    Produced(Box<SealedBlock>),
    Paused,
    Halted(SequencerError),
    PersistFailed(SequencerError),
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Extract per-market `IndicativeSnapshot` values from a shadow-solve
/// result. Markets that produced fills get the solver's discovered prices
/// and the fill notional; markets that are in the book but had no
/// matchable cross fall back to the last clearing price (volume = 0).
/// Markets absent from the book are not included in the map (lookup-miss
/// returns `IndicativeSnapshot::default()` on the read path).
fn build_indicative_snapshots(
    problem: &Problem,
    result: &matching_solver::PipelineResult,
    last_clearing: &HashMap<MarketId, Vec<Nanos>>,
    computed_at_ms: u64,
) -> HashMap<MarketId, IndicativeSnapshot> {
    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

    // Per-market notional volume from the shadow-solve's fills.
    let mut volume_by_market: HashMap<MarketId, u64> = HashMap::new();
    for fill in &result.result.fills {
        if fill.fill_qty == matching_engine::Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let notional = matching_engine::notional_nanos(fill.fill_price, fill.fill_qty);
        for m in order.active_markets() {
            let entry = volume_by_market.entry(m).or_insert(0);
            *entry = entry.saturating_add(notional.0);
        }
    }

    // Every market touched by a resting order in the speculative book.
    let mut markets_in_book: std::collections::HashSet<MarketId> = std::collections::HashSet::new();
    for order in &problem.orders {
        for m in order.active_markets() {
            markets_in_book.insert(m);
        }
    }

    let pd_prices = result.price_discovery.as_ref().map(|pd| &pd.prices);

    let mut snapshots: HashMap<MarketId, IndicativeSnapshot> = HashMap::new();
    for m in markets_in_book {
        let (yes, no) = if let Some(prices) = pd_prices.and_then(|p| p.get(&m)) {
            (prices.first().copied(), prices.get(1).copied())
        } else if let Some(prices) = last_clearing.get(&m) {
            (prices.first().copied(), prices.get(1).copied())
        } else {
            (None, None)
        };
        snapshots.insert(
            m,
            IndicativeSnapshot {
                yes_price_nanos: yes.map(|n| n.0),
                no_price_nanos: no.map(|n| n.0),
                volume_nanos: volume_by_market.get(&m).copied().unwrap_or(0),
                computed_at_ms,
            },
        );
    }
    snapshots
}

impl SequencerActorState {
    #[tracing::instrument(
        skip_all,
        fields(height = tracing::field::Empty, pending_bundles = tracing::field::Empty)
    )]
    pub(super) async fn on_tick(&mut self) -> Result<BlockTickOutcome, ActorProcessingErr> {
        self.on_tick_inner(None).await
    }

    #[tracing::instrument(
        skip_all,
        fields(height = tracing::field::Empty, pending_bundles = tracing::field::Empty)
    )]
    pub(super) async fn on_tick_inner(
        &mut self,
        test_crashpoint: Option<SequencerTestCrashpoint>,
    ) -> Result<BlockTickOutcome, ActorProcessingErr> {
        #[cfg(not(test))]
        let _ = test_crashpoint;

        if let Some(error) = &self.halted_error {
            tracing::error!(
                error = %error,
                "block production halted after invariant failure"
            );
            return Ok(BlockTickOutcome::Halted(error.clone()));
        }
        if self.pause_count > 0 {
            return Ok(BlockTickOutcome::Paused);
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let pending_bundles = self.sequencer.pending_bundles_len();
        tracing::Span::current().record("pending_bundles", pending_bundles);

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::BeforePrepare) {
            return Err(ActorProcessingErr::from(
                "injected crash before block prepare".to_string(),
            ));
        }

        let prepared = match self.sequencer.prepare_block(Vec::new(), timestamp_ms) {
            Ok(prepared) => prepared,
            Err(error) => {
                let outcome_error = error.clone();
                self.halt_after_invariant_failure(error);
                return Ok(BlockTickOutcome::Halted(outcome_error));
            }
        };
        tracing::Span::current().record("height", prepared.production().block.header.height);

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterPrepareBeforePersist) {
            return Err(ActorProcessingErr::from(
                "injected crash after block prepare before persist".to_string(),
            ));
        }

        #[cfg(test)]
        self.await_next_tick_hold_for_test().await;

        if let Err(error) = self.persist_block(&prepared).await {
            metrics::counter!("sybil_persistence_failures").increment(1);
            // The live sequencer still holds the pending bundles — the drain
            // happened on the clone. The next tick will retry, and the
            // The global acknowledged-write WAL was not cleared because
            // save_block's transaction rolled back atomically.
            tracing::error!(error = %error, "prepared block discarded before commit; pending bundles retained for retry");
            return Ok(BlockTickOutcome::PersistFailed(error));
        }

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterPersistBeforeCommit) {
            return Err(ActorProcessingErr::from(
                "injected crash after block persist before commit".to_string(),
            ));
        }

        let bp = match self.sequencer.commit_prepared_block(prepared) {
            Ok(bp) => bp,
            Err(error) => {
                let outcome_error = error.clone();
                self.halt_after_invariant_failure(error);
                return Ok(BlockTickOutcome::Halted(outcome_error));
            }
        };

        #[cfg(test)]
        if test_crashpoint == Some(SequencerTestCrashpoint::AfterCommit) {
            return Err(ActorProcessingErr::from(
                "injected crash after block commit".to_string(),
            ));
        }

        self.record_metrics(&bp, pending_bundles);
        let sealed = bp.sealed_block();
        self.push_to_history(sealed.clone());
        let _ = self.block_broadcast.send(sealed.clone());
        self.latest_block = Some(sealed.clone());
        Ok(BlockTickOutcome::Produced(Box::new(sealed)))
    }

    fn halt_after_invariant_failure(&mut self, error: SequencerError) {
        metrics::counter!("sybil_block_verification_failures").increment(1);
        // Verification failures mean the prepared state transition itself is
        // untrustworthy. Leave the live pre-block state intact and fail-stop
        // future ticks until an operator fixes the solver/state bug and
        // restarts from the last committed block. Persistence failures keep
        // the existing retry path above because they do not invalidate the
        // in-memory state transition.
        tracing::error!(
            error = %error,
            "prepared block discarded before commit; block production halted"
        );
        self.halted_error = Some(error);
    }

    #[cfg(test)]
    async fn await_next_tick_hold_for_test(&mut self) {
        if let Some(hold) = self.next_tick_hold.take() {
            let _ = hold.started.send(());
            let _ = hold.release.await;
        }
    }

    /// Indicative scheduler tick (C2). Builds a speculative `Problem` from
    /// the current resting book (Tier 1: no pending bundles, no MM flash),
    /// kicks off a `spawn_blocking` solve, and self-sends an
    /// `IndicativeUpdate` once the solver returns. An actor-local in-flight
    /// gate prevents the ~750ms timer from stacking LP jobs when one solve is
    /// slower than the cadence.
    pub(super) fn on_indicative_tick(&mut self, myself: ActorRef<SequencerMsg>) {
        if self.pause_count > 0 {
            return;
        }
        if !self.indicative_solve_gate.try_start() {
            return;
        }
        // Tier 1: single-market only. The LP solver asserts num_markets==1,
        // and multi-market orders sit in `pending_bundles`, not the book.
        let resting_orders: Vec<Order> = self
            .sequencer
            .order_book()
            .resting_orders()
            .filter(|(o, _)| o.num_markets == 1)
            .map(|(o, _)| o.clone())
            .collect();

        if resting_orders.is_empty() {
            // Nothing to solve. Leave the cache untouched so the last good
            // snapshot remains visible (FE displays staleness via
            // `computed_at_ms`).
            self.indicative_solve_gate.finish();
            return;
        }

        let mut problem = Problem::new("indicative");
        problem.markets = self.sequencer.markets().clone();
        problem.orders = resting_orders;
        problem.market_groups = self.sequencer.market_groups().to_vec();
        // mm_constraints intentionally empty (Tier 1 — no MM flash liquidity).

        let last_clearing = self.sequencer.analytics().last_clearing_prices().clone();
        let computed_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let target = myself.clone();
        let mailbox = self.mailbox_monitor.clone();

        let solver = self.sequencer.solver();
        let solver_name = solver.name().to_string();

        self.background_tasks.spawn(async move {
            let solve = tokio::task::spawn_blocking(move || {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    solver.solve(&problem)
                })) {
                    Ok(result) => {
                        let snapshots = build_indicative_snapshots(
                            &problem,
                            &result,
                            &last_clearing,
                            computed_at_ms,
                        );
                        SequencerMsg::IndicativeUpdate(snapshots)
                    }
                    Err(payload) => SequencerMsg::IndicativeSolveFailed {
                        solver: solver_name,
                        error: panic_payload_to_string(payload.as_ref()),
                    },
                }
            })
            .await;
            if let Ok(message) = solve {
                mailbox.queued();
                if target.send_message(message).is_err() {
                    mailbox.send_failed();
                }
            }
        });
    }

    async fn persist_block(&self, prepared: &PreparedBlock) -> Result<(), SequencerError> {
        if let Some(ref store) = self.store {
            let sealed = prepared.production().sealed_block();
            let height = sealed.canonical.header.height;
            // Commit clears pending off-block read-model rows after this returns.
            // Persist every prepared block so empty-fill batches can still durably
            // carry direct-admit history and equity deltas.
            store
                .save_block_with_witness_and_replay_block(
                    prepared.next_sequencer().snapshot(),
                    &prepared.production().witness,
                    &sealed,
                )
                .await
                .map_err(|error| SequencerError::Persistence(error.to_string()))?;

            let da_store = Arc::clone(store);
            let da_witness = prepared.production().witness.clone();
            let da_writes_in_flight = metrics::gauge!("sybil_da_artifact_writes_in_flight");
            da_writes_in_flight.increment(1.0);
            self.background_tasks.spawn(async move {
                let started = Instant::now();
                let artifact = DaArtifact::from_witness(&da_witness);
                let height = artifact.manifest.height;
                metrics::histogram!("sybil_witness_payload_bytes")
                    .record(artifact.manifest.payload_len as f64);
                match da_store.save_da_artifact(artifact).await {
                    Ok(true) => {
                        metrics::counter!("sybil_da_artifacts_persisted_total").increment(1);
                    }
                    Ok(false) => {
                        metrics::counter!("sybil_da_artifacts_skipped_total", "reason" => "below_retention_floor")
                            .increment(1);
                        tracing::warn!(
                            height,
                            "skipped DA artifact write below retained block-history floor"
                        );
                    }
                    Err(error) => {
                        metrics::counter!("sybil_da_artifact_persist_failures_total").increment(1);
                        tracing::warn!(height, %error, "DA artifact persistence failed after block commit");
                    }
                }
                metrics::histogram!("sybil_da_artifact_persist_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                da_writes_in_flight.decrement(1.0);
            });

            let canonical_policy = CanonicalArchiveRetentionPolicy {
                retention_blocks: self.sequencer.config.canonical_archive_retention_blocks,
                maintenance_interval_blocks: self
                    .sequencer
                    .config
                    .canonical_archive_maintenance_interval_blocks,
                max_rows_per_pass: self.sequencer.config.canonical_archive_max_rows_per_pass,
            };
            let proof_job_policy = AcknowledgedProofJobRetentionPolicy {
                retention_blocks: self
                    .sequencer
                    .config
                    .acknowledged_proof_job_retention_blocks,
                maintenance_interval_blocks: self
                    .sequencer
                    .config
                    .canonical_archive_maintenance_interval_blocks,
                max_rows_per_pass: self.sequencer.config.canonical_archive_max_rows_per_pass,
            };
            if canonical_policy.should_maintain_at(height)
                || proof_job_policy.should_maintain_at(height)
            {
                // Persistent artifact retention is maintenance, not part of
                // the commit fence. Never hold the single-writer actor while
                // it scans or deletes archive rows.
                let retention_store = Arc::clone(store);
                self.background_tasks.spawn(async move {
                    if canonical_policy.should_maintain_at(height) {
                        match retention_store
                            .prune_canonical_archive(height, canonical_policy)
                            .await
                        {
                            Ok(report) => {
                                metrics::counter!(
                                    "sybil_canonical_archive_pruned_rows_total",
                                    "stream" => "replay_blocks"
                                )
                                .increment(report.replay_blocks_pruned as u64);
                                metrics::counter!(
                                    "sybil_canonical_archive_pruned_rows_total",
                                    "stream" => "da_artifacts"
                                )
                                .increment(report.da_artifacts_pruned as u64);
                                if let Some(min_height) = report.meta.oldest_retained_height {
                                    metrics::gauge!("sybil_canonical_archive_oldest_height")
                                        .set(min_height as f64);
                                }
                            }
                            Err(error) => {
                                metrics::counter!(
                                    "sybil_canonical_archive_maintenance_failures_total"
                                )
                                .increment(1);
                                tracing::warn!(
                                    height,
                                    %error,
                                    "canonical archive maintenance failed"
                                );
                            }
                        }
                    }
                    if proof_job_policy.should_maintain_at(height) {
                        match retention_store
                            .prune_acknowledged_proof_jobs(height, proof_job_policy)
                            .await
                        {
                            Ok(report) => {
                                metrics::counter!("sybil_acknowledged_proof_jobs_pruned_total")
                                    .increment(report.jobs_pruned as u64);
                                if let Some(oldest) = report.oldest_retained_height {
                                    metrics::gauge!(
                                        "sybil_proof_job_outbox_oldest_retained_height"
                                    )
                                    .set(oldest as f64);
                                }
                            }
                            Err(error) => {
                                metrics::counter!(
                                    "sybil_acknowledged_proof_job_maintenance_failures_total"
                                )
                                .increment(1);
                                tracing::warn!(
                                    height,
                                    %error,
                                    "acknowledged proof-job maintenance failed"
                                );
                            }
                        }
                    }
                });
            }
        }
        Ok(())
    }

    fn record_metrics(&self, bp: &BlockProduction, pending_bundles_before: usize) {
        metrics::counter!("sybil_blocks_produced").increment(1);
        metrics::gauge!("sybil_block_height").set(bp.block.header.height as f64);
        metrics::histogram!("sybil_orders_per_block").record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_batch_orders_per_block")
            .record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_fresh_submissions_per_block")
            .record(bp.flow_metrics.fresh_submissions as f64);
        metrics::histogram!("sybil_fresh_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_received as f64);
        metrics::histogram!("sybil_carried_resting_orders_per_block")
            .record(bp.flow_metrics.carried_resting_orders as f64);
        metrics::histogram!("sybil_fresh_accepted_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_accepted as f64);
        metrics::histogram!("sybil_rejections_per_block")
            .record(bp.flow_metrics.rejected_orders as f64);
        metrics::histogram!("sybil_fills_per_block").record(bp.block.header.fill_count as f64);
        metrics::gauge!("sybil_welfare_nanos").set(bp.analytics.total_welfare as f64);
        metrics::gauge!("sybil_volume_nanos").set(bp.analytics.total_volume as f64);
        metrics::gauge!("sybil_pending_bundles").set(pending_bundles_before as f64);
        metrics::gauge!("sybil_pending_orders").set(bp.flow_metrics.pending_orders_after as f64);
        metrics::gauge!("sybil_state_accounts_total").set(bp.witness.post_state.len() as f64);
        metrics::histogram!("sybil_solve_time_seconds").record(bp.pipeline.total_time_secs);
        metrics::gauge!("sybil_recent_block_cache_len").set(self.recent_blocks.len() as f64);
        let recent_history = self.sequencer.analytics().recent_history_cache_counts();
        metrics::gauge!("sybil_recent_price_point_entries").set(recent_history.price_points as f64);
        metrics::gauge!("sybil_recent_fill_entries").set(recent_history.fills as f64);
        metrics::gauge!("sybil_recent_equity_point_entries")
            .set(recent_history.equity_points as f64);
        metrics::gauge!("sybil_recent_account_event_entries")
            .set(recent_history.account_events as f64);

        self.record_per_market_metrics(bp);
    }

    // Cardinality note: bounded by active markets this block (those with clearing
    // prices). Fine for MVP scale (tens of markets). Revisit top-N bucketing if
    // we ever exceed ~1000 concurrently active markets.
    fn record_per_market_metrics(&self, bp: &BlockProduction) {
        let order_to_market: HashMap<u64, MarketId> = bp
            .witness
            .orders
            .iter()
            .filter_map(|wo| wo.order.active_markets().next().map(|m| (wo.order.id, m)))
            .collect();

        let mut fills_per_market: HashMap<MarketId, u64> = HashMap::new();
        for fill in &bp.block.fills {
            if fill.fill_qty == matching_engine::Qty::ZERO {
                continue;
            }
            if let Some(&market_id) = order_to_market.get(&fill.order_id) {
                *fills_per_market.entry(market_id).or_default() += 1;
            }
        }
        for (market_id, count) in fills_per_market {
            metrics::counter!(
                "sybil_market_fills_total",
                "market_id" => market_id.0.to_string()
            )
            .increment(count);
        }

        let market_volumes = self.sequencer.analytics().market_volumes();
        for (market_id, prices) in &bp.block.clearing_prices {
            for (outcome, &price) in prices.iter().enumerate() {
                metrics::gauge!(
                    "sybil_market_clearing_price_nanos",
                    "market_id" => market_id.0.to_string(),
                    "outcome" => outcome.to_string()
                )
                .set(price.0 as f64);
            }
            if let Some(&volume) = market_volumes.get(market_id) {
                metrics::gauge!(
                    "sybil_market_volume_nanos",
                    "market_id" => market_id.0.to_string()
                )
                .set(volume as f64);
            }
        }
    }

    fn push_to_history(&mut self, block: SealedBlock) {
        if self.recent_blocks.len() >= self.sequencer.config.recent_block_cache_capacity {
            self.recent_blocks.pop_front();
        }
        self.recent_blocks.push_back(block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};

    fn mk_problem(orders: Vec<Order>, markets: MarketSet) -> Problem {
        let mut p = Problem::new("test");
        p.markets = markets;
        p.orders = orders;
        p
    }

    #[test]
    fn build_indicative_snapshots_empty_book_yields_empty_map() {
        let problem = mk_problem(Vec::new(), MarketSet::new());
        let result = matching_solver::PipelineResult::empty();
        let last_clearing = HashMap::new();
        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 1_000);
        assert!(snaps.is_empty());
    }

    #[test]
    fn build_indicative_snapshots_fallback_to_last_clearing() {
        // Book has an order on market m, but no fills (no cross). Snapshot
        // should fall back to the last clearing price for m.
        let mut markets = MarketSet::new();
        let m = markets.add_binary("m");
        let order = outcome_buy(&markets, 0, m, 0, NANOS_PER_DOLLAR / 2, 5);
        let problem = mk_problem(vec![order], markets);
        let result = matching_solver::PipelineResult::empty();
        let mut last_clearing = HashMap::new();
        last_clearing.insert(m, vec![Nanos(400_000_000), Nanos(600_000_000)]);

        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 12_345);

        let snap = snaps.get(&m).expect("market should be in the cache");
        assert_eq!(snap.yes_price_nanos, Some(400_000_000));
        assert_eq!(snap.no_price_nanos, Some(600_000_000));
        assert_eq!(snap.volume_nanos, 0);
        assert_eq!(snap.computed_at_ms, 12_345);
    }

    #[test]
    fn build_indicative_snapshots_fills_use_price_discovery_and_volume() {
        // Book has an order; result has a fill on it. Snapshot should
        // surface the price_discovery price and a non-zero volume.
        let mut markets = MarketSet::new();
        let m = markets.add_binary("m");
        let order = outcome_buy(
            &markets,
            1,
            m,
            0,
            NANOS_PER_DOLLAR / 2,
            matching_engine::shares_to_qty(5).0,
        );
        let order_id = order.id;
        let problem = mk_problem(vec![order], markets);

        let mut result = matching_solver::PipelineResult::empty();
        let mut fill = matching_engine::Fill::new(
            order_id,
            matching_engine::shares_to_qty(3),
            Nanos(400_000_000),
        );
        fill.account_id = 7;
        result.result.fills.push(fill);
        let mut pd = matching_solver::PriceDiscoveryResult::empty();
        pd.prices
            .insert(m, vec![Nanos(450_000_000), Nanos(550_000_000)]);
        result.price_discovery = Some(pd);

        // last_clearing has older values; PD should override.
        let mut last_clearing = HashMap::new();
        last_clearing.insert(m, vec![Nanos(100_000_000), Nanos(900_000_000)]);

        let snaps = build_indicative_snapshots(&problem, &result, &last_clearing, 0);
        let snap = snaps.get(&m).expect("market should be in cache");
        assert_eq!(snap.yes_price_nanos, Some(450_000_000));
        assert_eq!(snap.no_price_nanos, Some(550_000_000));
        // 3 qty * 400_000_000 = 1.2e9
        assert_eq!(snap.volume_nanos, 1_200_000_000);
    }
}
