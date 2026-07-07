use super::admission::GroupCoverageTracker;
use super::types::{FinalizedBlockState, SolvedBatch, WitnessArtifacts, WitnessAssemblyInput};
use super::*;

pub(crate) mod finalize;
mod solve;
pub(crate) mod witness;

use self::finalize::collect_account_invariant_failures;
use self::witness::{
    bridge_block_data, build_witness_phase_snapshots, convert_rejection_reason, verifier_failures,
};

fn log_block_invariant_failures(height: u64, failures: &[BlockInvariantFailure], fail_open: bool) {
    error!(
        height,
        failures = failures.len(),
        fail_open,
        "prepared block failed hard invariant verification"
    );
    for failure in failures {
        error!(height, failure = ?failure, "block invariant failure");
    }
}

fn block_invariant_error(height: u64, failures: Vec<BlockInvariantFailure>) -> SequencerError {
    SequencerError::BlockInvariantFailure { height, failures }
}

fn revalidation_removed_order_reason(exit: &RestingRevalidationExit) -> RemovedOrderExitReason {
    match exit {
        RestingRevalidationExit::MarketInactive => RemovedOrderExitReason::RevalidateMarketInactive,
        RestingRevalidationExit::AccountGone => RemovedOrderExitReason::RevalidateAccountGone,
        RestingRevalidationExit::AccountInsolvent => {
            RemovedOrderExitReason::RevalidateAccountInsolvent
        }
        RestingRevalidationExit::Rejected(RejectionReason::InsufficientBalance { .. }) => {
            RemovedOrderExitReason::RevalidateInsufficientBalance
        }
        RestingRevalidationExit::Rejected(RejectionReason::InsufficientPosition { .. }) => {
            RemovedOrderExitReason::RevalidateInsufficientPosition
        }
        RestingRevalidationExit::Rejected(_) => RemovedOrderExitReason::RevalidateRejected,
    }
}

fn resting_order_is_new_direct_admit(
    resting: &crate::order_book::RestingOrder,
    block_height: u64,
    previous_header: Option<&BlockHeader>,
) -> bool {
    match previous_header {
        Some(header) => {
            resting.created_at == header.height
                && block_height == header.height.saturating_add(1)
                && resting.created_at_ms != header.timestamp_ms
        }
        None => block_height == 1 && resting.created_at == 0,
    }
}

impl BlockSequencer {
    /// Prepare one block from the given submissions without mutating live sequencer state.
    ///
    /// Any submissions buffered on `self.pending_bundles` (from the admit
    /// path for MM / multi-market orders) are drained into the same solver
    /// input ahead of the caller-supplied batch. The drain happens on the
    /// clone, so if the prepared block is discarded the live sequencer
    /// still holds the bundles and the next tick retries them.
    ///
    /// The returned [`PreparedBlock`] can either be committed atomically or discarded.
    #[tracing::instrument(skip_all, fields(height))]
    pub fn prepare_block(
        &self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> Result<PreparedBlock, SequencerError> {
        let mut next_sequencer = self.clone();
        let mut all_submissions = std::mem::take(&mut next_sequencer.pending_bundles);
        all_submissions.extend(submissions);
        let production = next_sequencer.produce_block_in_place(all_submissions, timestamp_ms)?;
        let prepared = PreparedBlock {
            next_sequencer,
            production,
        };
        self.validate_prepared_block_for_commit(&prepared)?;
        Ok(prepared)
    }

    /// Buffer a submission that couldn't be admitted directly into the
    /// resting book. The bundle is added to `self.pending_bundles` and will
    /// be handed to the solver on the next `prepare_block` call.
    ///
    /// Persistence of this bundle is the caller's responsibility (usually
    /// the actor, via `Store::append_pending_bundle`) so durability decisions
    /// stay out of the sync core.
    pub fn push_pending_bundle(&mut self, submission: OrderSubmission) {
        self.pending_bundles.push(submission);
    }

    pub fn pending_bundles_len(&self) -> usize {
        self.pending_bundles.len()
    }

    #[cfg(test)]
    pub(crate) fn pending_bundles_for_test(&self) -> &[OrderSubmission] {
        &self.pending_bundles
    }

    #[tracing::instrument(
        skip_all,
        fields(height = prepared.production().block.header.height)
    )]
    pub fn commit_prepared_block(
        &mut self,
        prepared: PreparedBlock,
    ) -> Result<BlockProduction, SequencerError> {
        self.validate_prepared_block_for_commit(&prepared)?;
        let PreparedBlock {
            next_sequencer,
            production,
        } = prepared;
        *self = next_sequencer;
        self.analytics.clear_offblock_pending();
        Ok(production)
    }

    /// Core sync method: prepare + immediately commit a block in-memory.
    pub fn try_produce_block(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> Result<BlockProduction, SequencerError> {
        let prepared = self.prepare_block(submissions, timestamp_ms)?;
        self.commit_prepared_block(prepared)
    }

    /// Legacy convenience wrapper for simulation-style callers.
    ///
    /// Money-path integrations should prefer [`Self::try_produce_block`] or
    /// the prepare/persist/commit split so invariant failures surface as
    /// typed errors and discard the prepared clone.
    #[tracing::instrument(skip_all, fields(height))]
    pub fn produce_block(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> BlockProduction {
        self.try_produce_block(submissions, timestamp_ms)
            .expect("block production failed hard invariant verification")
    }

    fn validate_prepared_block_for_commit(
        &self,
        prepared: &PreparedBlock,
    ) -> Result<(), SequencerError> {
        let height = prepared.production.block.header.height;
        let mut failures = collect_account_invariant_failures(
            &prepared.next_sequencer.accounts,
            &prepared.next_sequencer.markets,
        );

        let prepared_state_root = crate::block::compute_complete_state_root(
            &prepared.next_sequencer.accounts,
            prepared.next_sequencer.bridge_state(),
            prepared.next_sequencer.order_book(),
            prepared.next_sequencer.markets(),
            prepared.next_sequencer.market_groups(),
            prepared.next_sequencer.market_lifecycle(),
        );
        let block_state_root = prepared.production.block.header.state_root;
        if prepared_state_root != block_state_root {
            failures.push(BlockInvariantFailure::PreparedStateRootMismatch {
                block_state_root,
                prepared_state_root,
            });
        }

        if self.config.debug_verify_full {
            let verification = sybil_verifier::verify_full(
                &prepared.production.witness,
                /* diagnostics */ false,
            );
            if !verification.valid {
                failures.push(BlockInvariantFailure::FullVerificationFailed {
                    violations: verifier_failures(&verification),
                });
            }
        }

        if failures.is_empty() {
            return Ok(());
        }

        log_block_invariant_failures(height, &failures, self.config.verification_fail_open);
        if self.config.verification_fail_open {
            error!(
                height,
                "verification_fail_open enabled; committing prepared block despite hard invariant failures"
            );
            Ok(())
        } else {
            Err(block_invariant_error(height, failures))
        }
    }

    fn produce_block_in_place(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> Result<BlockProduction, SequencerError> {
        self.height += 1;
        tracing::Span::current().record("height", self.height);
        let pre_state_sidecar = self.committed_state_sidecar.clone();
        let pre_deposit_frontier = self.committed_deposit_frontier;
        let system_events = std::mem::take(&mut self.pending_system_events);
        let system_account_baselines = std::mem::take(&mut self.pending_system_account_baselines);

        for event in &system_events {
            match event {
                SystemEvent::CreateAccount {
                    account_id,
                    initial_balance,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_create_account_event(
                            *initial_balance,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::Deposit { account_id, amount } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_deposit_event(*amount, self.height);
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::L1Deposit {
                    account_id,
                    amount,
                    deposit,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_l1_deposit_event(
                            deposit.deposit_id,
                            *amount,
                            &deposit.deposit_root,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::WithdrawalCreated {
                    account_id,
                    amount,
                    withdrawal,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_withdrawal_created_event(
                            withdrawal.withdrawal_id,
                            *amount,
                            &withdrawal.nullifier,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                } => {
                    let encoded = crate::digest::encode_resolution_event(
                        *market_id,
                        *payout_nanos,
                        self.height,
                    );
                    for account_id in affected_accounts {
                        if let Some(account) = self.accounts.get_mut(*account_id) {
                            account.events_digest =
                                crate::digest::update_digest(&account.events_digest, &encoded);
                        }
                    }
                }
                SystemEvent::OrderCancelled {
                    account_id,
                    order_id,
                    market_ids,
                    side,
                    remaining_quantity,
                } => {
                    if let Some(account) = self.accounts.get_mut(*account_id) {
                        let encoded = crate::digest::encode_order_cancelled_event(
                            *order_id,
                            market_ids,
                            *side,
                            *remaining_quantity,
                            self.height,
                        );
                        account.events_digest =
                            crate::digest::update_digest(&account.events_digest, &encoded);
                    }
                }
                SystemEvent::MarketGroupExtended { .. } => {}
            }
        }
        let bridge = bridge_block_data(&system_events, &self.bridge);

        let fresh_submissions = submissions.len();
        let fresh_orders_received: usize = submissions
            .iter()
            .map(|submission| submission.orders.len())
            .sum();

        let mut all_orders: Vec<Order> = Vec::new();
        let mut all_mm_constraints: Vec<MmConstraint> = Vec::new();
        let mut rejections: Vec<Rejection> = Vec::new();

        // Track witness data alongside normal processing
        let mut witness_orders: Vec<WitnessOrder> = Vec::new();
        let mut witness_rejections: Vec<WitnessRejection> = Vec::new();
        let mut mm_order_ids_set: HashSet<u64> = HashSet::new();

        // Collect tradeable market IDs (active markets that aren't in a non-tradeable state)
        let active_markets: HashSet<MarketId> = self
            .markets
            .iter()
            .filter(|m| self.market_status(m.id).is_tradeable())
            .map(|m| m.id)
            .collect();

        // Collect resolved market IDs for witness
        let resolved_markets: Vec<MarketId> = self
            .lifecycle
            .market_statuses()
            .iter()
            .filter(|(_, status)| matches!(status, MarketStatus::Resolved { .. }))
            .map(|(&id, _)| id)
            .collect();

        // Per-block placed/matched/unmatched per market — accumulated
        // alongside the OrderStatsTracker hooks below and stamped on the
        // Block at the end of this function. Cancels are NOT counted here.
        let mut block_orders_by_market: HashMap<MarketId, crate::aggregates::OrderStats> =
            HashMap::new();
        let mut derived_view_sidecar = DerivedViewSidecar::default();

        // ── Order Book: expire stale, remove orders for resolved markets ──
        let expired = self.order_book.expire(self.height);
        let revalidated = self.order_book.revalidate(&self.accounts, &active_markets);
        for ro in &expired {
            derived_view_sidecar
                .removed_orders
                .push(RemovedOrderView::from_resting_order(
                    ro,
                    RemovedOrderPhase::BlockStartExpire,
                    RemovedOrderExitReason::Expired,
                    None,
                ));
            for m in ro.order.active_markets() {
                let slot = block_orders_by_market.entry(m).or_default();
                if ro.has_been_matched {
                    slot.matched += 1;
                } else {
                    slot.unmatched += 1;
                }
            }
        }
        // Resting orders evicted by revalidation for a genuine per-order reason
        // (insufficient balance/position) surface as Rejected history events.
        // Market-inactive / account-gone / insolvent removals are sidecar-only.
        for (ro, exit) in &revalidated {
            derived_view_sidecar
                .removed_orders
                .push(RemovedOrderView::from_resting_order(
                    ro,
                    RemovedOrderPhase::BlockStartRevalidate,
                    revalidation_removed_order_reason(exit),
                    exit.rejection_reason().cloned(),
                ));
            for m in ro.order.active_markets() {
                let slot = block_orders_by_market.entry(m).or_default();
                if ro.has_been_matched {
                    slot.matched += 1;
                } else {
                    slot.unmatched += 1;
                }
            }
        }

        // Build batch-local account map from resting orders
        let mut order_account_map: HashMap<u64, AccountId> = HashMap::new();
        let resting_snapshot = self.order_book.snapshot();
        for ro in &resting_snapshot {
            let order = &ro.order;
            let account_id = ro.account_id;
            order_account_map.insert(order.id, account_id);
            witness_orders.push(WitnessOrder {
                order: order.clone(),
                account_id: account_id.0,
                is_mm: false,
            });
            all_orders.push(order.clone());
            derived_view_sidecar.admits.push(AdmitTimingView {
                order_id: order.id,
                account_id: account_id.0,
                admit_height: ro.created_at,
                admit_timestamp_ms: ro.created_at_ms,
                is_new: resting_order_is_new_direct_admit(
                    ro,
                    self.height,
                    self.last_header.as_ref(),
                ),
                is_mm: false,
            });
        }
        let carried_resting_orders = all_orders.len();

        // ── Process new submissions ──
        // Seed STP from existing resting orders across all accounts so
        // cross-block coverage participates in the same complete-set check
        // the loop applies to fresh orders.
        let mut stp = GroupCoverageTracker::new(&self.market_groups);
        self.seed_group_coverage_from_all_resting(&mut stp);

        for mut sub in submissions {
            let account_id = sub.account_id;

            let Some(account) = self.accounts.get(account_id) else {
                for order in &sub.orders {
                    witness_rejections.push(WitnessRejection {
                        order: order.clone(),
                        account_id: account_id.0,
                        reason: sybil_verifier::RejectionReason::AccountNotFound,
                    });
                    rejections.push(Rejection {
                        order_id: order.id,
                        account_id,
                        reason: RejectionReason::AccountNotFound,
                    });
                }
                continue;
            };

            let is_mm = sub.mm_constraint.is_some();

            // Cap MM budget to account balance — prevents cumulative overdraft.
            if let Some(ref mut mm_c) = sub.mm_constraint {
                if account.balance <= 0 {
                    continue;
                }
                mm_c.max_capital = mm_c.max_capital.min(Nanos(account.balance as u64));
            }

            let mut accepted_orders: Vec<Order> = Vec::new();
            let mut submission_idx_to_order_id: HashMap<usize, u64> = HashMap::new();

            for (sub_idx, mut order) in sub.orders.into_iter().enumerate() {
                let order_markets_active =
                    order.active_markets().all(|m| active_markets.contains(&m));
                if !order_markets_active {
                    continue;
                }

                let order_id = self.next_order_id;
                self.next_order_id += 1;
                order.id = order_id;

                if let Err(reason) = validate_order_shape(&order) {
                    witness_rejections.push(WitnessRejection {
                        order: order.clone(),
                        account_id: account_id.0,
                        reason: convert_rejection_reason(&reason),
                    });
                    rejections.push(Rejection {
                        order_id,
                        account_id,
                        reason,
                    });
                    continue;
                }

                let expires_at_block =
                    order.effective_expires_at_block(self.height, self.order_book.ttl());
                if self.height > expires_at_block {
                    witness_rejections.push(WitnessRejection {
                        order: order.clone(),
                        account_id: account_id.0,
                        reason: sybil_verifier::RejectionReason::Expired {
                            current_block: self.height,
                            expires_at_block,
                        },
                    });
                    rejections.push(Rejection {
                        order_id,
                        account_id,
                        reason: RejectionReason::Expired {
                            current_block: self.height,
                            expires_at_block,
                        },
                    });
                    continue;
                }

                if is_mm {
                    // MM orders: STP check, flash liquidity (skip balance validation)
                    if stp.would_complete_set(account_id, &order) {
                        witness_rejections.push(WitnessRejection {
                            order: order.clone(),
                            account_id: account_id.0,
                            reason: sybil_verifier::RejectionReason::CompleteSetFormation,
                        });
                        rejections.push(Rejection {
                            order_id,
                            account_id,
                            reason: RejectionReason::CompleteSetFormation,
                        });
                        continue;
                    }
                    stp.record(account_id, &order);
                    submission_idx_to_order_id.insert(sub_idx, order_id);
                    order_account_map.insert(order_id, account_id);
                    mm_order_ids_set.insert(order_id);
                    witness_orders.push(WitnessOrder {
                        order: order.clone(),
                        account_id: account_id.0,
                        is_mm: true,
                    });
                    derived_view_sidecar.admits.push(AdmitTimingView {
                        order_id,
                        account_id: account_id.0,
                        admit_height: self.height,
                        admit_timestamp_ms: timestamp_ms,
                        is_new: true,
                        is_mm: true,
                    });
                    accepted_orders.push(order);
                } else {
                    // Non-MM orders: validate + reserve via OrderBook
                    match self.order_book.accept(
                        order.clone(),
                        account_id,
                        account,
                        self.height,
                        timestamp_ms,
                    ) {
                        Ok(accepted) => {
                            if stp.would_complete_set(account_id, &accepted.order) {
                                // Undo the book acceptance — release reservations
                                // (settle with a "fully filled" phantom to release)
                                let phantom_fill =
                                    Fill::new(accepted.order.id, accepted.order.max_fill, Nanos(0));
                                let _stp_undo = self.order_book.settle(
                                    &[phantom_fill],
                                    &HashSet::new(),
                                    self.height,
                                );
                                witness_rejections.push(WitnessRejection {
                                    order: accepted.order.clone(),
                                    account_id: account_id.0,
                                    reason: sybil_verifier::RejectionReason::CompleteSetFormation,
                                });
                                derived_view_sidecar
                                    .rejection_history
                                    .push(RejectedOrderView {
                                        order_id: accepted.order.id,
                                        order: accepted.order.clone(),
                                        account_id: account_id.0,
                                        reason: RejectionReason::CompleteSetFormation,
                                    });
                                rejections.push(Rejection {
                                    order_id: accepted.order.id,
                                    account_id,
                                    reason: RejectionReason::CompleteSetFormation,
                                });
                                continue;
                            }
                            stp.record(account_id, &accepted.order);
                            order_account_map.insert(accepted.order.id, account_id);
                            witness_orders.push(WitnessOrder {
                                order: accepted.order.clone(),
                                account_id: account_id.0,
                                is_mm: false,
                            });
                            derived_view_sidecar.admits.push(AdmitTimingView {
                                order_id: accepted.order.id,
                                account_id: account_id.0,
                                admit_height: accepted.resting_order.created_at,
                                admit_timestamp_ms: accepted.resting_order.created_at_ms,
                                is_new: true,
                                is_mm: false,
                            });
                            accepted_orders.push(accepted.order);
                        }
                        Err(reason) => {
                            witness_rejections.push(WitnessRejection {
                                order: order.clone(),
                                account_id: account_id.0,
                                reason: convert_rejection_reason(&reason),
                            });
                            derived_view_sidecar
                                .rejection_history
                                .push(RejectedOrderView {
                                    order_id,
                                    order: order.clone(),
                                    account_id: account_id.0,
                                    reason: reason.clone(),
                                });
                            rejections.push(Rejection {
                                order_id,
                                account_id,
                                reason,
                            });
                        }
                    }
                }
            }

            // Rebuild MmConstraint with assigned IDs
            if let Some(mm_constraint) = sub.mm_constraint {
                let old_order_ids = &mm_constraint.order_ids;
                let old_sides = &mm_constraint.order_sides;

                let mut new_constraint =
                    MmConstraint::new(mm_constraint.mm_id, mm_constraint.max_capital);

                for (sub_idx, old_id) in old_order_ids.iter().enumerate() {
                    if let (Some(&new_id), Some(&side)) = (
                        submission_idx_to_order_id.get(&sub_idx),
                        old_sides.get(old_id),
                    ) {
                        new_constraint.add_order(new_id, side);
                    }
                }

                if new_constraint.num_orders() > 0 {
                    all_mm_constraints.push(new_constraint);
                }
            }

            all_orders.extend(accepted_orders);
        }

        let fresh_orders_accepted = all_orders.len().saturating_sub(carried_resting_orders);
        let rejected_orders = rejections.len();
        let order_ids: Vec<u64> = all_orders.iter().map(|o| o.id).collect();
        let orders_submitted = all_orders.len() + rejections.len();

        // `placed` means order live in this batch's solve, not merely admitted
        // for the first time. Count carried resting orders and MM flash orders
        // here, after all rejections/evictions have been filtered out.
        for order in &all_orders {
            let markets: Vec<MarketId> = order.active_markets().collect();
            for market in markets {
                block_orders_by_market.entry(market).or_default().placed += 1;
            }
        }

        // Debug: log order and rejection counts per block
        if !all_orders.is_empty() || !rejections.is_empty() {
            let mut buy_yes = 0u32;
            let mut sell_yes = 0u32;
            let mut buy_no = 0u32;
            let mut sell_no = 0u32;
            for o in &all_orders {
                if o.num_markets == 1 && o.num_states == 2 {
                    let is_buy = o.payoffs[0] > 0 || o.payoffs[1] > 0;
                    let is_yes = o.payoffs[0] != 0;
                    match (is_buy, is_yes) {
                        (true, true) => buy_yes += 1,
                        (true, false) => buy_no += 1,
                        (false, true) => sell_yes += 1,
                        (false, false) => sell_no += 1,
                    }
                }
            }
            debug!(
                accepted = all_orders.len(),
                rejected = rejections.len(),
                buy_yes,
                sell_yes,
                buy_no,
                sell_no,
                "block order summary"
            );
            for rej in &rejections {
                debug!(
                    order_id = rej.order_id,
                    account = rej.account_id.0,
                    reason = ?rej.reason,
                    "order rejected"
                );
            }
        }

        // Capture per-block placers from witness_orders BEFORE it gets
        // consumed by WitnessAssemblyInput below. MM orders are excluded so
        // by_market[m].placers tracks real participation (decision Q-table).
        let mut block_placers: HashSet<AccountId> = HashSet::new();
        let mut block_placers_by_market: HashMap<MarketId, HashSet<AccountId>> = HashMap::new();
        for wo in &witness_orders {
            if wo.is_mm {
                continue;
            }
            let aid = AccountId(wo.account_id);
            if aid == AccountId::MINT {
                continue;
            }
            block_placers.insert(aid);
            for m in wo.order.active_markets() {
                block_placers_by_market.entry(m).or_default().insert(aid);
            }
        }
        let unique_placers = block_placers.len() as u32;
        let placers_by_market: HashMap<MarketId, u32> = block_placers_by_market
            .into_iter()
            .map(|(m, s)| (m, s.len() as u32))
            .collect();

        // Build Problem
        let mut problem = Problem::new("block");
        problem.markets = self.markets.clone();
        problem.orders = all_orders;
        problem.mm_constraints = all_mm_constraints;
        problem.market_groups = self.market_groups.clone();

        // Phase 1: solve the batch and derive fill/cross-market pricing outputs.
        let SolvedBatch {
            pipeline_result,
            fills,
            clearing_prices,
            total_welfare: _solver_total_welfare,
            total_volume,
            orders_filled,
            welfare_by_market,
        } = self.solve_batch_phase(&problem, &order_account_map, &active_markets);

        let (pre_state, post_system_state) =
            build_witness_phase_snapshots(&self.accounts, &system_account_baselines);

        // Phase 2: apply fills, derive minting, and validate the finalized account state.
        let FinalizedBlockState {
            post_state,
            volume_by_market,
            mark_prices,
            minting_cost,
            mut invariant_failures,
        } = self.finalize_block_state_phase(&fills, &problem, &clearing_prices, timestamp_ms);

        let total_welfare =
            matching_engine::net_welfare(pipeline_result.result.gross_welfare, minting_cost);

        // Off-block cumulative + 24h platform welfare — accumulate this block's
        // authoritative `total_welfare` scalar (counts each fill once, unlike the
        // per-market `welfare_by_market` attribution). Runs once per finalized
        // block, alongside the volume accumulated inside `record_finalized_block`.
        self.analytics.record_welfare(total_welfare, timestamp_ms);

        // Update order book: release filled orders' reservations, adjust partial fills
        let post_solve_removed = self
            .order_book
            .settle(&fills, &mm_order_ids_set, self.height);
        for (ro, exit) in &post_solve_removed {
            derived_view_sidecar
                .removed_orders
                .push(RemovedOrderView::from_resting_order(
                    ro,
                    RemovedOrderPhase::PostSolve,
                    match exit {
                        RestingExit::Expired => RemovedOrderExitReason::Expired,
                        RestingExit::Settled if ro.has_been_matched => {
                            RemovedOrderExitReason::Filled
                        }
                        RestingExit::Settled => RemovedOrderExitReason::Settled,
                    },
                    None,
                ));
            for m in ro.order.active_markets() {
                let slot = block_orders_by_market.entry(m).or_default();
                if ro.has_been_matched {
                    slot.matched += 1;
                } else {
                    slot.unmatched += 1;
                }
            }
        }
        let pending_orders_after = self.order_book.len();

        // Off-block liquidity tracker — score the post-settle resting book
        // PLUS this batch's flash MM orders against each market's midprice.
        // MM orders never enter the book, so pull them from the solver input.
        let mm_orders: Vec<&Order> = problem
            .orders
            .iter()
            .filter(|o| mm_order_ids_set.contains(&o.id))
            .collect();
        self.analytics.record_liquidity(
            &self.order_book,
            &mm_orders,
            &mark_prices,
            self.config.liquidity_band_nanos,
        );

        // MM flash orders live exactly one block and never enter the book, so
        // the matched/unmatched exit hooks above never see them. Classify each
        // here from this block's fills — any fill (qty > 0) → matched, else →
        // unmatched — so an MM quote is counted like any one-shot
        // (immediate-or-cancel) limit order. Their `placed` / `placed_distinct`
        // were already counted at admission. MM orders are not in the book, so
        // this cannot double-count with the exit hooks.
        let mm_filled_qty: HashMap<u64, u64> =
            fills
                .iter()
                .filter(|f| f.fill_qty.0 > 0)
                .fold(HashMap::new(), |mut acc, f| {
                    *acc.entry(f.order_id).or_insert(0) += f.fill_qty.0;
                    acc
                });
        for o in &mm_orders {
            let matched = mm_filled_qty.get(&o.id).copied().unwrap_or(0) > 0;
            for m in o.active_markets() {
                let slot = block_orders_by_market.entry(m).or_default();
                if matched {
                    slot.matched += 1;
                } else {
                    slot.unmatched += 1;
                }
            }
        }

        // Off-block per-account equity series — sample accounts that traded
        // this block (the tracker also periodically sweeps all known accounts).
        let touched: std::collections::HashSet<AccountId> = fills
            .iter()
            .filter_map(|f| order_account_map.get(&f.order_id).copied())
            .collect();
        self.analytics.record_equity(
            &touched,
            &self.accounts,
            &mark_prices,
            self.height,
            timestamp_ms,
        );

        let previous_header = self
            .last_header
            .as_ref()
            .map(BlockHeader::to_witness_header);

        let WitnessArtifacts { header, witness } =
            self.assemble_witness_artifacts(WitnessAssemblyInput {
                post_state,
                order_count: orders_submitted as u32,
                timestamp_ms,
                previous_header,
                witness_orders,
                witness_rejections,
                system_events: &system_events,
                fills: &fills,
                clearing_prices: &clearing_prices,
                total_welfare,
                minting_cost,
                problem: &problem,
                pre_state,
                pre_state_sidecar,
                pre_deposit_frontier,
                post_system_state,
                resolved_markets,
            });

        self.last_header = Some(header.clone());
        if self.height == 1 {
            self.genesis_hash = Some(hash_header(&header));
        }
        self.committed_state_sidecar = witness.state_sidecar.clone();
        self.committed_deposit_frontier = self.bridge.deposit_frontier;

        debug!(
            orders_submitted,
            accepted = order_ids.len(),
            rejected = rejections.len(),
            fills = fills.len(),
            orders_filled,
            total_welfare,
            total_volume,
            "block produced"
        );

        let block = Block {
            header,
            order_ids,
            system_events,
            bridge,
            fills,
            clearing_prices,
            rejections,
        };
        let analytics = BlockAnalytics {
            total_welfare,
            total_volume,
            orders_filled,
            unique_placers,
            placers_by_market,
            volume_by_market,
            orders_by_market: block_orders_by_market,
            welfare_by_market,
        };
        let sealed_for_observe = crate::block::SealedBlock {
            canonical: block.clone(),
            analytics: analytics.clone(),
            derived_view_sidecar: derived_view_sidecar.clone(),
        };
        self.analytics
            .observe_block(&sealed_for_observe, &derived_view_sidecar, &witness);

        // Debug/prover-adjacent native full verification. Production keeps
        // this off; a separate prover node owns the full verifier path.
        if self.config.debug_verify_full {
            let verification = sybil_verifier::verify_full(&witness, /* diagnostics */ false);
            if !verification.valid {
                invariant_failures.push(BlockInvariantFailure::FullVerificationFailed {
                    violations: verifier_failures(&verification),
                });
            }
        }

        if !invariant_failures.is_empty() {
            log_block_invariant_failures(
                self.height,
                &invariant_failures,
                self.config.verification_fail_open,
            );
            if !self.config.verification_fail_open {
                return Err(block_invariant_error(self.height, invariant_failures));
            }
            error!(
                height = self.height,
                "verification_fail_open enabled; prepared block will be allowed despite hard invariant failures"
            );
        }

        Ok(BlockProduction {
            block,
            analytics,
            derived_view_sidecar,
            pipeline: pipeline_result,
            witness,
            flow_metrics: BlockFlowMetrics {
                fresh_submissions,
                fresh_orders_received,
                carried_resting_orders,
                fresh_orders_accepted,
                rejected_orders,
                pending_orders_after,
            },
        })
    }
}
