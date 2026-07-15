use super::*;

/// Per-order self-trade prevention (STP) for market groups.
///
/// Tracks buy-side outcome coverage per account across a batch. When an order
/// would complete coverage of all N outcomes in a group (enabling minting
/// self-trade), that specific order is rejected. Earlier orders are kept.
///
/// Applied to ALL accounts, not just MMs — same principle as traditional
/// exchange STP (CME, Nasdaq, etc.) but adapted for batch auctions.
///
/// Coverage rules:
/// - BuyYes on market_i → covers outcome i
/// - BuyNo on market_i → covers all outcomes EXCEPT i (in the group)
/// - SellYes/SellNo → does NOT contribute (reduces exposure)
pub(super) struct GroupCoverageTracker {
    /// market_id → (group_index, group_size)
    market_to_group: HashMap<MarketId, (usize, usize)>,
    /// (account_id, group_index) → set of covered outcome market_ids
    coverage: HashMap<(AccountId, usize), HashSet<MarketId>>,
    /// group_index → list of market_ids in the group
    group_markets: Vec<Vec<MarketId>>,
}

impl GroupCoverageTracker {
    pub(super) fn new(market_groups: &[MarketGroup]) -> Self {
        let mut market_to_group = HashMap::new();
        let mut group_markets = Vec::with_capacity(market_groups.len());
        for (gi, group) in market_groups.iter().enumerate() {
            let markets: Vec<MarketId> = group.markets.clone();
            let n = markets.len();
            for &mid in &markets {
                market_to_group.insert(mid, (gi, n));
            }
            group_markets.push(markets);
        }
        Self {
            market_to_group,
            coverage: HashMap::new(),
            group_markets,
        }
    }

    /// Check if accepting this order would complete a group set for the account.
    /// Returns true if the order should be REJECTED (would complete self-trade).
    pub(super) fn would_complete_set(&self, account_id: AccountId, order: &Order) -> bool {
        if order.num_markets != 1 || order.num_states != 2 {
            return false;
        }
        let market = order.markets[0];
        let Some(&(gi, n)) = self.market_to_group.get(&market) else {
            return false;
        };

        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);

        // Compute what this order would add to coverage
        let mut new_coverage: HashSet<MarketId> = HashSet::new();
        if yes_pay > 0 && no_pay == 0 {
            new_coverage.insert(market);
        } else if yes_pay == 0 && no_pay > 0 {
            for &gm in &self.group_markets[gi] {
                if gm != market {
                    new_coverage.insert(gm);
                }
            }
        } else {
            return false; // Sell or mixed — not a coverage concern
        }

        let key = (account_id, gi);
        let existing = self.coverage.get(&key);
        let total = match existing {
            Some(set) => set.union(&new_coverage).count(),
            None => new_coverage.len(),
        };

        total >= n
    }

    /// Record that this order was accepted — update coverage for the account.
    pub(super) fn record(&mut self, account_id: AccountId, order: &Order) {
        if order.num_markets != 1 || order.num_states != 2 {
            return;
        }
        let market = order.markets[0];
        let Some(&(gi, _)) = self.market_to_group.get(&market) else {
            return;
        };

        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);
        let key = (account_id, gi);
        let set = self.coverage.entry(key).or_default();

        if yes_pay > 0 && no_pay == 0 {
            set.insert(market);
        } else if yes_pay == 0 && no_pay > 0 {
            for &gm in &self.group_markets[gi] {
                if gm != market {
                    set.insert(gm);
                }
            }
        }
    }
}

impl BlockSequencer {
    pub fn try_admit_direct(&mut self, submission: OrderSubmission, now_ms: u64) -> AdmitOutcome {
        self.try_admit_direct_with_ioc(submission, now_ms, false)
    }

    /// Admit an IOC submission using the currently committed height as the
    /// single source of truth. The actor calls this while processing the
    /// submission, so a block cannot commit between deriving the expiry and
    /// admitting (or deferring) the order.
    pub fn try_admit_ioc(&mut self, submission: OrderSubmission, now_ms: u64) -> AdmitOutcome {
        self.try_admit_direct_with_ioc(submission, now_ms, true)
    }

    fn try_admit_direct_with_ioc(
        &mut self,
        mut submission: OrderSubmission,
        now_ms: u64,
        is_ioc: bool,
    ) -> AdmitOutcome {
        if is_ioc {
            // IOC means the first batch eligible after this atomic admission.
            // Store the concrete height on the Order so canonical order and
            // witness encodings remain unchanged.
            let expires_at_block = self.height.saturating_add(1);
            for order in &mut submission.orders {
                order.expires_at_block = Some(expires_at_block);
            }
        }

        for order in &submission.orders {
            let shape = if submission.mm_constraint.is_some() {
                validate_order_shape(order)
            } else {
                crate::validation::validate_resting_order_shape(
                    order,
                    self.config.min_resting_order_notional_nanos,
                )
            };
            if let Err(reason) = shape {
                return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                    order_id: 0,
                    account_id: submission.account_id,
                    reason,
                }));
            }
            for market_id in order.active_markets() {
                if self.markets.get(market_id).is_none() {
                    return AdmitOutcome::Rejected(SequencerError::MarketNotFound);
                }
                let status = self.market_status(market_id);
                if !status.is_tradeable() {
                    return AdmitOutcome::Rejected(SequencerError::InvalidMarketState(format!(
                        "market {} is {}",
                        market_id.0,
                        status.as_str()
                    )));
                }
            }
        }

        let eligible = submission.mm_constraint.is_none()
            && submission.orders.len() == 1
            && submission.orders[0].num_markets == 1;
        let order_ids = self.assign_submission_order_ids(&mut submission);
        if !eligible {
            return AdmitOutcome::Deferred {
                order_ids,
                submission,
            };
        }

        let account_id = submission.account_id;
        let Some(account) = self.accounts.get(account_id) else {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        };

        let order = submission.orders.into_iter().next().expect("len == 1");
        let order_id = order.id;
        let next_batch_height = self.height.saturating_add(1);
        let expires_at_block = order.effective_expires_at_block(self.height, self.order_book.ttl());
        if next_batch_height > expires_at_block {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason: RejectionReason::Expired {
                    current_block: next_batch_height,
                    expires_at_block,
                },
            }));
        }

        let mut stp = GroupCoverageTracker::new(&self.market_groups);
        self.seed_group_coverage_for_account(&mut stp, account_id);
        if stp.would_complete_set(account_id, &order) {
            return AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason: RejectionReason::CompleteSetFormation,
            }));
        }

        match self
            .order_book
            .accept(order, account_id, account, self.height, now_ms)
        {
            Ok(accepted) => AdmitOutcome::Admitted {
                order_id: accepted.order.id,
                resting_order: accepted.resting_order,
            },
            Err(reason) => AdmitOutcome::Rejected(SequencerError::Rejected(Rejection {
                order_id,
                account_id,
                reason,
            })),
        }
    }

    /// Assign durable, globally monotonic IDs before either direct admission or
    /// deferred persistence so the submission acknowledgement can expose them.
    fn assign_submission_order_ids(&mut self, submission: &mut OrderSubmission) -> Vec<u64> {
        let order_ids: Vec<u64> = submission
            .orders
            .iter_mut()
            .map(|order| {
                let order_id = self.next_order_id;
                self.next_order_id = self.next_order_id.saturating_add(1);
                order.id = order_id;
                order_id
            })
            .collect();

        if let Some(mm_constraint) = submission.mm_constraint.take() {
            let mut remapped = MmConstraint::new(mm_constraint.mm_id, mm_constraint.max_capital);
            for (submission_index, old_id) in mm_constraint.order_ids.iter().enumerate() {
                if let (Some(&order_id), Some(&side)) = (
                    order_ids.get(submission_index),
                    mm_constraint.order_sides.get(old_id),
                ) {
                    remapped.add_order(order_id, side);
                }
            }
            submission.mm_constraint = Some(remapped);
        }

        order_ids
    }

    /// Seed an STP tracker with every resting/pending-bundle order belonging
    /// to `account_id`. Used at admit time so a single order can't complete a
    /// coverage set against the account's prior-block resting orders or against
    /// bundles still staged in `pending_bundles`.
    pub(super) fn seed_group_coverage_for_account(
        &self,
        stp: &mut GroupCoverageTracker,
        account_id: AccountId,
    ) {
        for (order, aid) in self.order_book.resting_orders() {
            if aid == account_id {
                stp.record(aid, order);
            }
        }
        for bundle in &self.pending_bundles {
            if bundle.account_id == account_id {
                for order in &bundle.orders {
                    stp.record(account_id, order);
                }
            }
        }
    }

    /// Seed an STP tracker with every account's resting coverage. Used inside
    /// `prepare_block` before the submission loop so cross-block coverage
    /// participates in the same check the loop applies to fresh orders.
    pub(super) fn seed_group_coverage_from_all_resting(&self, stp: &mut GroupCoverageTracker) {
        for (order, aid) in self.order_book.resting_orders() {
            stp.record(aid, order);
        }
    }

    /// Get pending orders, optionally filtered by account.
    pub fn pending_orders_info(
        &self,
        account_id_filter: Option<AccountId>,
    ) -> Vec<PendingOrderInfo> {
        self.order_book
            .resting_orders_full()
            .filter(|(_, aid, _, _, _, _)| account_id_filter.is_none_or(|filter| *aid == filter))
            .map(
                |(order, aid, created_at, expires_at_block, original_max_fill, created_at_ms)| {
                    PendingOrderInfo::from_resting(
                        order,
                        aid,
                        created_at,
                        expires_at_block,
                        original_max_fill,
                        created_at_ms,
                    )
                },
            )
            .collect()
    }

    /// Get pending orders for a specific market.
    pub fn market_orderbook(&self, market_id: MarketId) -> Vec<PendingOrderInfo> {
        self.order_book
            .resting_orders_full()
            .filter(|(order, _, _, _, _, _)| order.active_markets().any(|m| m == market_id))
            .map(
                |(order, aid, created_at, expires_at_block, original_max_fill, created_at_ms)| {
                    PendingOrderInfo::from_resting(
                        order,
                        aid,
                        created_at,
                        expires_at_block,
                        original_max_fill,
                        created_at_ms,
                    )
                },
            )
            .collect()
    }

    /// Cancel a resting order owned by `account_id`.
    ///
    /// On success, stages a `SystemEvent::OrderCancelled` so the next block
    /// commits an on-chain cancellation record (D1). The active markets and
    /// categorical direction come from the resting order returned by
    /// `OrderBook.cancel` (B5's widened return type); `remaining_quantity`
    /// is the unfilled `max_fill` at cancel time.
    pub fn cancel_pending_order(
        &mut self,
        account_id: AccountId,
        order_id: u64,
    ) -> Result<(), SequencerError> {
        self.cancel_pending_order_at(account_id, order_id, current_timestamp_ms())
    }

    pub fn cancel_pending_order_at(
        &mut self,
        account_id: AccountId,
        order_id: u64,
        timestamp_ms: u64,
    ) -> Result<(), SequencerError> {
        let ro = self
            .order_book
            .cancel(account_id, order_id)
            .map_err(cancel_error_to_sequencer_error)?;
        self.capture_system_account_baseline(account_id);
        let market_ids: Vec<MarketId> = ro.order.active_markets().collect();
        let primary_market = market_ids.first().copied().unwrap_or(MarketId::NONE);
        let side = derive_order_direction(&ro.order, primary_market);
        self.pending_system_events
            .push(SystemEvent::OrderCancelled {
                account_id,
                order_id,
                market_ids,
                side,
                remaining_quantity: ro.order.max_fill.0,
            });
        self.analytics.record_order_history(
            account_id,
            crate::aggregates::HistoryKind::Cancelled,
            self.height,
            timestamp_ms,
            &ro.order,
            OrderHistoryOptions::default(),
        );
        Ok(())
    }

    /// Check whether [`Self::cancel_pending_order_at`] would accept a cancel
    /// without mutating sequencer state.
    ///
    /// `timestamp_ms` is accepted to keep the preflight boundary identical to
    /// the apply boundary. It currently affects only cancellation history after
    /// validation succeeds.
    pub fn can_cancel_pending_order(
        &self,
        account_id: AccountId,
        order_id: u64,
        timestamp_ms: u64,
    ) -> Result<(), SequencerError> {
        let _ = timestamp_ms;
        self.order_book
            .can_cancel(account_id, order_id)
            .map_err(cancel_error_to_sequencer_error)
    }
}

fn cancel_error_to_sequencer_error(error: crate::order_book::CancelError) -> SequencerError {
    match error {
        crate::order_book::CancelError::NotFound => SequencerError::OrderNotFound,
        crate::order_book::CancelError::WrongOwner => SequencerError::OrderOwnershipMismatch,
        crate::order_book::CancelError::Reservation(error) => error.into(),
    }
}
