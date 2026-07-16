use super::*;
use matching_engine::NANOS_PER_DOLLAR;

/// Per-order self-trade prevention (STP) within protocol market groups.
///
/// Tracks buy-side outcome coverage and limits per account across a batch.
/// Coverage alone is not a self-trade: non-crossing bids can span every
/// outcome without filling against one another. An order is rejected only
/// when the account's limits can fund a same-market binary complete set within
/// a group, or when it completes coverage that can fund the group mint.
///
/// Applied to ALL accounts, not just MMs — same principle as traditional
/// exchange STP (CME, Nasdaq, etc.) but adapted for batch auctions.
///
/// Crossing rules:
/// - BuyYes + BuyNo on one grouped binary market must sum to at least $1.
/// - BuyYes on every outcome in a group must sum to at least $1.
/// - SellYes/SellNo do not contribute; complete-set redemption is legitimate.
pub(super) struct GroupCoverageTracker {
    /// market_id → group_index
    market_to_group: HashMap<MarketId, usize>,
    /// (account_id, group_index) → highest accepted YES limit per outcome.
    group_yes_limits: HashMap<(AccountId, usize), GroupBidState>,
    /// Complementary buy limits for binary markets that belong to a group.
    binary_limits: HashMap<(AccountId, MarketId), BinaryBidState>,
    /// group_index → list of market_ids in the group
    group_markets: Vec<Vec<MarketId>>,
}

#[derive(Clone, Default)]
struct GroupBidState {
    yes_limits: HashMap<MarketId, u64>,
}

#[derive(Clone, Copy, Default)]
struct BinaryBidState {
    yes_limit: Option<u64>,
    no_limit: Option<u64>,
}

impl GroupCoverageTracker {
    pub(super) fn new(market_groups: &[MarketGroup]) -> Self {
        let mut market_to_group = HashMap::new();
        let mut group_markets = Vec::with_capacity(market_groups.len());
        for (gi, group) in market_groups.iter().enumerate() {
            let markets: Vec<MarketId> = group.markets.clone();
            for &mid in &markets {
                market_to_group.insert(mid, gi);
            }
            group_markets.push(markets);
        }
        Self {
            market_to_group,
            group_yes_limits: HashMap::new(),
            binary_limits: HashMap::new(),
            group_markets,
        }
    }

    /// Check whether accepting this order would create a price-crossing
    /// complete set for the account.
    pub(super) fn would_complete_set(&self, account_id: AccountId, order: &Order) -> bool {
        if order.num_markets != 1 || order.num_states != 2 {
            return false;
        }
        let market = order.markets[0];
        let Some(&gi) = self.market_to_group.get(&market) else {
            return false;
        };
        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);
        let mut binary = self
            .binary_limits
            .get(&(account_id, market))
            .copied()
            .unwrap_or_default();
        if yes_pay > 0 && no_pay == 0 {
            record_highest_optional_limit(&mut binary.yes_limit, order.limit_price.0);
        } else if yes_pay == 0 && no_pay > 0 {
            record_highest_optional_limit(&mut binary.no_limit, order.limit_price.0);
        } else {
            return false; // Sell or mixed — not a coverage concern
        }
        if binary_limits_cross(binary) {
            return true;
        }

        if yes_pay <= 0 || no_pay != 0 {
            return false;
        }

        let mut candidate = self
            .group_yes_limits
            .get(&(account_id, gi))
            .cloned()
            .unwrap_or_default();
        record_highest_limit(&mut candidate.yes_limits, market, order.limit_price.0);

        complete_set_limits_cross(&candidate, &self.group_markets[gi])
    }

    /// Record that this order was accepted — update coverage for the account.
    pub(super) fn record(&mut self, account_id: AccountId, order: &Order) {
        if order.num_markets != 1 || order.num_states != 2 {
            return;
        }
        let market = order.markets[0];
        let Some(&gi) = self.market_to_group.get(&market) else {
            return;
        };
        let (yes_pay, no_pay) = (order.payoffs[0], order.payoffs[1]);
        let binary = self.binary_limits.entry((account_id, market)).or_default();
        if yes_pay > 0 && no_pay == 0 {
            record_highest_optional_limit(&mut binary.yes_limit, order.limit_price.0);
        } else if yes_pay == 0 && no_pay > 0 {
            record_highest_optional_limit(&mut binary.no_limit, order.limit_price.0);
        } else {
            return;
        }

        if yes_pay > 0 && no_pay == 0 {
            let state = self.group_yes_limits.entry((account_id, gi)).or_default();
            record_highest_limit(&mut state.yes_limits, market, order.limit_price.0);
        }
    }
}

fn binary_limits_cross(state: BinaryBidState) -> bool {
    match (state.yes_limit, state.no_limit) {
        (Some(yes), Some(no)) => u128::from(yes) + u128::from(no) >= u128::from(NANOS_PER_DOLLAR),
        _ => false,
    }
}

fn record_highest_optional_limit(current: &mut Option<u64>, candidate: u64) {
    *current = Some(current.map_or(candidate, |limit| limit.max(candidate)));
}

fn record_highest_limit(limits: &mut HashMap<MarketId, u64>, market: MarketId, limit: u64) {
    limits
        .entry(market)
        .and_modify(|current| *current = (*current).max(limit))
        .or_insert(limit);
}

/// Return true when the accepted bids can fund a risk-free complete set.
///
/// Same-market binary crossing inside the group is handled before group
/// coverage. Group minting crosses when every group outcome has a YES bid and
/// their limits sum to at least $1. Other coverage combinations remain exposed
/// to at least one outcome and are not self-trades merely because their payoff
/// union spans the group.
fn complete_set_limits_cross(state: &GroupBidState, group_markets: &[MarketId]) -> bool {
    let Some(total) = group_markets.iter().try_fold(0_u128, |total, market| {
        state
            .yes_limits
            .get(market)
            .map(|limit| total + u128::from(*limit))
    }) else {
        return false;
    };
    total >= u128::from(NANOS_PER_DOLLAR)
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
                    return AdmitOutcome::Rejected(SequencerError::MarketNotFound { market_id });
                }
                let status = self.market_status(market_id);
                if !status.is_tradeable() {
                    return AdmitOutcome::Rejected(SequencerError::MarketNotTradeable {
                        market_id,
                        status: status.as_str().to_string(),
                    });
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
    /// price-crossing complete set against the account's prior-block resting
    /// orders or against bundles still staged in `pending_bundles`.
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
