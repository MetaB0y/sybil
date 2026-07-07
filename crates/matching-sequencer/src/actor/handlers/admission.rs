use super::super::*;
use super::current_timestamp_ms;

impl SequencerActorState {
    pub(super) fn record_submission_metrics(
        &self,
        source: &'static str,
        order_count: usize,
        result: &Result<(), SequencerError>,
    ) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_submissions_total", "source" => source, "result" => outcome)
            .increment(1);
        metrics::counter!("sybil_orders_received_total", "source" => source, "result" => outcome)
            .increment(order_count as u64);
    }

    pub(super) fn record_cancel_metrics(
        &self,
        source: &'static str,
        result: &Result<(), SequencerError>,
    ) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_cancels_total", "source" => source, "result" => outcome)
            .increment(1);
    }

    pub(super) async fn handle_signed_order(
        &mut self,
        signed: SignedOrder,
    ) -> Result<(), SequencerError> {
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_order(&signed, genesis_hash)?;
        self.handle_authenticated_order(AuthenticatedOrder {
            order: signed.order,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_order(
        &mut self,
        authenticated: AuthenticatedOrder,
    ) -> Result<(), SequencerError> {
        let account_id = self
            .sequencer
            .lookup_pubkey(&authenticated.signer)
            .ok_or(SequencerError::UnknownSigner)?;
        self.accept_replay_nonce(account_id, authenticated.nonce)
            .await?;

        let submission = OrderSubmission {
            account_id,
            orders: vec![authenticated.order],
            mm_constraint: None,
        };

        self.admit_or_defer(submission).await
    }

    pub(super) async fn handle_signed_cancel(
        &mut self,
        signed: SignedCancel,
    ) -> Result<(), SequencerError> {
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_cancel(&signed, genesis_hash)?;
        self.handle_authenticated_cancel(AuthenticatedCancel {
            account_id: signed.account_id,
            order_id: signed.order_id,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_cancel(
        &mut self,
        authenticated: AuthenticatedCancel,
    ) -> Result<(), SequencerError> {
        let account_id = self
            .sequencer
            .lookup_pubkey(&authenticated.signer)
            .ok_or(SequencerError::UnknownSigner)?;

        if account_id != authenticated.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        self.accept_replay_nonce(account_id, authenticated.nonce)
            .await?;

        let timestamp_ms = current_timestamp_ms();
        let mut validation = self.sequencer.clone();
        validation.cancel_pending_order_at(
            authenticated.account_id,
            authenticated.order_id,
            timestamp_ms,
        )?;
        self.persist_control_plane(&ControlPlaneCommand::CancelPendingOrder {
            account_id: authenticated.account_id,
            order_id: authenticated.order_id,
            timestamp_ms,
        })
        .await?;
        self.sequencer.cancel_pending_order_at(
            authenticated.account_id,
            authenticated.order_id,
            timestamp_ms,
        )
    }

    /// Admit a submission: fast path if it fits straight into the resting
    /// book (single-market, non-MM, single order), otherwise buffer it on
    /// the sequencer's pending queue. Either way the submission is durably
    /// logged before this returns `Ok`, so a crash before the next block
    /// commit doesn't drop anything acknowledged with a 200 OK. Returns
    /// `Err` for synchronous rejections so the caller can surface them to
    /// the client.
    pub(super) async fn admit_or_defer(
        &mut self,
        submission: OrderSubmission,
    ) -> Result<(), SequencerError> {
        self.check_account_submission_limits(&submission)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match self.sequencer.try_admit_direct(submission, now_ms) {
            crate::sequencer::AdmitOutcome::Admitted {
                order_id,
                resting_order,
            } => {
                if let Some(store) = &self.store {
                    if let Err(err) = store.append_admit_log(&resting_order).await {
                        // Durability lost — rollback the in-memory admit so
                        // the 200 OK contract holds. If cancel somehow fails
                        // (shouldn't: we just pushed the order), log loudly
                        // and leave the order in-book as a degraded state.
                        if let Err(cancel_err) = self
                            .sequencer
                            .cancel_pending_order(resting_order.account_id, order_id)
                        {
                            tracing::error!(
                                error = %cancel_err,
                                order_id,
                                "admit-log persist failed and rollback could not cancel the order"
                            );
                        }
                        return Err(SequencerError::Persistence(err.to_string()));
                    }
                }
                Ok(())
            }
            crate::sequencer::AdmitOutcome::Deferred(sub) => {
                self.check_deferred_submission_limits(&sub)?;
                if let Some(store) = &self.store {
                    store
                        .append_pending_bundle(&sub)
                        .await
                        .map_err(|err| SequencerError::Persistence(err.to_string()))?;
                }
                self.sequencer.push_pending_bundle(sub);
                Ok(())
            }
            crate::sequencer::AdmitOutcome::Rejected(err) => Err(err),
        }
    }

    pub(super) fn check_global_submission_rate(&mut self) -> Result<(), SequencerError> {
        let now = Instant::now();
        self.global_submission_bucket
            .allow(now)
            .map_err(|retry_after_secs| {
                metrics::counter!(
                    "sybil_admission_limit_rejections_total",
                    "limit" => "global_rate"
                )
                .increment(1);
                SequencerError::RateLimited { retry_after_secs }
            })
    }

    pub(super) fn check_account_submission_limits(
        &mut self,
        submission: &OrderSubmission,
    ) -> Result<(), SequencerError> {
        let config = &self.sequencer.config;
        if self.sequencer.accounts.get(submission.account_id).is_none() {
            return Err(SequencerError::Rejected(crate::error::Rejection {
                order_id: 0,
                account_id: submission.account_id,
                reason: crate::error::RejectionReason::AccountNotFound,
            }));
        }

        let order_count = submission.orders.len();
        if order_count > config.max_orders_per_submission {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "orders_per_submission"
            )
            .increment(1);
            return Err(SequencerError::TooManyOrdersInSubmission {
                count: order_count,
                limit: config.max_orders_per_submission,
            });
        }

        let now = Instant::now();
        let bucket = self
            .account_submission_buckets
            .entry(submission.account_id)
            .or_insert_with(|| {
                TokenBucket::new(
                    config.max_submissions_per_account_per_second,
                    config.submission_burst_per_account,
                    now,
                )
            });
        bucket.allow(now).map_err(|retry_after_secs| {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "account_rate"
            )
            .increment(1);
            SequencerError::RateLimited { retry_after_secs }
        })?;

        if submission.mm_constraint.is_none() {
            let open_orders = self
                .sequencer
                .open_orders_for_account(submission.account_id);
            let staged_orders = self
                .sequencer
                .pending_non_mm_orders_for_account(submission.account_id);
            if open_orders + staged_orders + order_count > config.max_open_orders_per_account {
                metrics::counter!(
                    "sybil_admission_limit_rejections_total",
                    "limit" => "open_orders_per_account"
                )
                .increment(1);
                return Err(SequencerError::TooManyOpenOrders {
                    account_id: submission.account_id,
                    limit: config.max_open_orders_per_account,
                });
            }
        }

        Ok(())
    }

    pub(super) fn check_deferred_submission_limits(
        &self,
        submission: &OrderSubmission,
    ) -> Result<(), SequencerError> {
        let config = &self.sequencer.config;
        if self.sequencer.pending_bundles_len() >= config.max_pending_bundles {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "pending_bundles_total"
            )
            .increment(1);
            return Err(SequencerError::MempoolFull);
        }

        if self
            .sequencer
            .pending_bundles_for_account(submission.account_id)
            >= config.max_pending_bundles_per_account
        {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "pending_bundles_per_account"
            )
            .increment(1);
            return Err(SequencerError::TooManyPendingBundles {
                account_id: submission.account_id,
                limit: config.max_pending_bundles_per_account,
            });
        }

        Ok(())
    }
}
