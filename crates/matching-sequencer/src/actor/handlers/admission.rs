use super::super::*;
use super::current_timestamp_ms;

impl SequencerActorState {
    pub(super) fn record_submission_metrics(
        &self,
        source: &'static str,
        order_count: usize,
        result: &Result<Vec<u64>, SequencerError>,
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
    ) -> Result<Vec<u64>, SequencerError> {
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_order(&signed, genesis_hash)?;
        let authorization = raw_client_action_authorization(&signed.signer, &signed.signature);
        self.handle_authenticated_order(AuthenticatedOrder {
            order: signed.order,
            nonce: signed.nonce,
            authorization,
        })
        .await
    }

    pub(super) async fn handle_authenticated_order(
        &mut self,
        authenticated: AuthenticatedOrder,
    ) -> Result<Vec<u64>, SequencerError> {
        let signer = PublicKey::from_compressed_bytes(authenticated.authorization.signer_pubkey())
            .ok_or(SequencerError::UnknownSigner)?;
        let registered = self
            .sequencer
            .lookup_registered_pubkey(&signer)
            .ok_or(SequencerError::UnknownSigner)?;
        let expected_scheme = match authenticated.authorization.signer_auth_scheme() {
            0 => AccountAuthScheme::RawP256,
            1 => AccountAuthScheme::WebAuthn,
            _ => return Err(SequencerError::UnknownSigner),
        };
        if registered.auth_scheme != expected_scheme {
            return Err(SequencerError::UnknownSigner);
        }
        let account_id = registered.account_id;
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        let canonical =
            canonical_order_bytes(&authenticated.order, authenticated.nonce, genesis_hash);
        let signer_record = sybil_verifier::KeyRecord {
            auth_scheme: registered.auth_scheme.canonical_byte(),
            pubkey_sec1: signer
                .compressed_bytes()
                .try_into()
                .expect("compressed P-256 key is 33 bytes"),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        sybil_verifier::verify_keyop_auth(
            &authenticated.authorization,
            [&signer_record],
            &canonical,
        )
        .map_err(|_| SequencerError::InvalidSignature)?;
        self.sequencer
            .validate_replay_nonce(account_id, authenticated.nonce)?;

        let submission = OrderSubmission {
            account_id,
            orders: vec![authenticated.order],
            mm_constraint: None,
        };

        self.admit_or_defer_with_authorization(
            submission,
            false,
            Some((authenticated.nonce, authenticated.authorization)),
        )
        .await
    }

    pub(super) async fn handle_signed_mm_bundle(
        &mut self,
        signed: SignedMmBundle,
    ) -> Result<Vec<u64>, SequencerError> {
        let authorization = raw_client_action_authorization(&signed.signer, &signed.signature);
        self.handle_authenticated_mm_bundle(AuthenticatedMmBundle {
            account_id: signed.account_id,
            bundle_id: signed.bundle_id,
            revision: signed.revision,
            orders: signed.orders,
            order_sides: signed.order_sides,
            max_capital: signed.max_capital,
            nonce: signed.nonce,
            authorization,
        })
        .await
    }

    pub(super) async fn handle_authenticated_mm_bundle(
        &mut self,
        authenticated: AuthenticatedMmBundle,
    ) -> Result<Vec<u64>, SequencerError> {
        let order_count = authenticated.orders.len();
        let order_limit = self.sequencer.config.max_orders_per_submission;
        if order_count > order_limit {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "orders_per_submission"
            )
            .increment(1);
            return Err(SequencerError::TooManyOrdersInSubmission {
                count: order_count,
                limit: order_limit,
            });
        }
        let signer = PublicKey::from_compressed_bytes(authenticated.authorization.signer_pubkey())
            .ok_or(SequencerError::UnknownSigner)?;
        let registered = self
            .sequencer
            .lookup_registered_pubkey(&signer)
            .ok_or(SequencerError::UnknownSigner)?;
        let expected_scheme = match authenticated.authorization.signer_auth_scheme() {
            0 => AccountAuthScheme::RawP256,
            1 => AccountAuthScheme::WebAuthn,
            _ => return Err(SequencerError::UnknownSigner),
        };
        if registered.auth_scheme != expected_scheme {
            return Err(SequencerError::UnknownSigner);
        }
        if registered.account_id != authenticated.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        if authenticated.revision != 0 {
            return Err(SequencerError::InvalidMmBundle(
                "initial submission revision must be zero".to_string(),
            ));
        }
        if authenticated.orders.is_empty()
            || authenticated.orders.len() != authenticated.order_sides.len()
        {
            return Err(SequencerError::InvalidMmBundle(
                "orders and sides must be non-empty and have equal lengths".to_string(),
            ));
        }
        let target_block = self.sequencer.height().saturating_add(1);
        if authenticated
            .orders
            .iter()
            .any(|order| order.expires_at_block != Some(target_block))
        {
            return Err(SequencerError::InvalidMmBundle(format!(
                "IOC bundle must target next block {target_block}"
            )));
        }

        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        let canonical = canonical_mm_bundle_bytes(
            authenticated.account_id,
            authenticated.bundle_id,
            authenticated.revision,
            &authenticated.orders,
            &authenticated.order_sides,
            authenticated.max_capital,
            authenticated.nonce,
            genesis_hash,
        )?;
        let signer_record = sybil_verifier::KeyRecord {
            auth_scheme: registered.auth_scheme.canonical_byte(),
            pubkey_sec1: signer
                .compressed_bytes()
                .try_into()
                .expect("compressed P-256 key is 33 bytes"),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        sybil_verifier::verify_keyop_auth(
            &authenticated.authorization,
            [&signer_record],
            &canonical,
        )
        .map_err(|_| SequencerError::InvalidSignature)?;
        self.sequencer
            .validate_replay_nonce(authenticated.account_id, authenticated.nonce)?;

        let mut constraint = matching_engine::MmConstraint::new(
            matching_engine::MmId(authenticated.account_id.0),
            authenticated.max_capital,
        );
        for (index, side) in authenticated.order_sides.iter().copied().enumerate() {
            constraint.add_order(index as u64, side);
        }
        let submission = OrderSubmission {
            account_id: authenticated.account_id,
            orders: authenticated.orders,
            mm_constraint: Some(constraint),
        };
        self.check_account_submission_limits(&submission)?;
        let now_ms = current_timestamp_ms();
        let (order_ids, submission) = match self.sequencer.try_admit_ioc(submission, now_ms) {
            crate::sequencer::AdmitOutcome::Deferred {
                order_ids,
                submission,
            } => (order_ids, submission),
            crate::sequencer::AdmitOutcome::Rejected(error) => return Err(error),
            crate::sequencer::AdmitOutcome::Admitted { .. } => {
                return Err(SequencerError::InvalidMmBundle(
                    "MM bundle unexpectedly entered the resting book".to_string(),
                ));
            }
        };
        self.check_deferred_submission_limits(&submission)?;
        if let Some(store) = &self.store {
            store
                .append_authenticated_mm_bundle(
                    &submission,
                    authenticated.bundle_id,
                    authenticated.revision,
                    &authenticated.order_sides,
                    authenticated.max_capital,
                    authenticated.nonce,
                    &authenticated.authorization,
                )
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        let action = sybil_verifier::ClientActionWitness::MmBundle {
            account_id: authenticated.account_id.0,
            bundle_id: authenticated.bundle_id,
            revision: authenticated.revision,
            orders: submission.orders.clone(),
            order_sides: authenticated.order_sides,
            max_capital: authenticated.max_capital,
            nonce: authenticated.nonce,
            authorization: authenticated.authorization,
        };
        self.sequencer.push_pending_bundle(submission);
        self.sequencer
            .apply_client_action_authorized(action)
            .expect("authenticated MM bundle nonce was validated in the actor turn");
        Ok(order_ids)
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
        let authorization = raw_client_action_authorization(&signed.signer, &signed.signature);
        self.handle_authenticated_cancel(AuthenticatedCancel {
            account_id: signed.account_id,
            order_id: signed.order_id,
            nonce: signed.nonce,
            authorization,
        })
        .await
    }

    pub(super) async fn handle_authenticated_cancel(
        &mut self,
        authenticated: AuthenticatedCancel,
    ) -> Result<(), SequencerError> {
        let signer = PublicKey::from_compressed_bytes(authenticated.authorization.signer_pubkey())
            .ok_or(SequencerError::UnknownSigner)?;
        let registered = self
            .sequencer
            .lookup_registered_pubkey(&signer)
            .ok_or(SequencerError::UnknownSigner)?;
        let expected_scheme = match authenticated.authorization.signer_auth_scheme() {
            0 => AccountAuthScheme::RawP256,
            1 => AccountAuthScheme::WebAuthn,
            _ => return Err(SequencerError::UnknownSigner),
        };
        if registered.auth_scheme != expected_scheme {
            return Err(SequencerError::UnknownSigner);
        }
        let account_id = registered.account_id;

        if account_id != authenticated.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }

        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        let canonical = canonical_cancel_bytes(
            authenticated.account_id,
            authenticated.order_id,
            authenticated.nonce,
            genesis_hash,
        );
        let signer_record = sybil_verifier::KeyRecord {
            auth_scheme: registered.auth_scheme.canonical_byte(),
            pubkey_sec1: signer
                .compressed_bytes()
                .try_into()
                .expect("compressed P-256 key is 33 bytes"),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        sybil_verifier::verify_keyop_auth(
            &authenticated.authorization,
            [&signer_record],
            &canonical,
        )
        .map_err(|_| SequencerError::InvalidSignature)?;

        let timestamp_ms = current_timestamp_ms();
        self.sequencer.can_cancel_pending_order(
            authenticated.account_id,
            authenticated.order_id,
            timestamp_ms,
        )?;
        self.sequencer
            .validate_replay_nonce(account_id, authenticated.nonce)?;
        if let Some(store) = &self.store {
            store
                .append_authenticated_cancel(
                    authenticated.account_id,
                    authenticated.order_id,
                    authenticated.nonce,
                    &authenticated.authorization,
                    timestamp_ms,
                )
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer
            .apply_client_action_authorized(sybil_verifier::ClientActionWitness::Cancel {
                account_id: authenticated.account_id.0,
                order_id: authenticated.order_id,
                nonce: authenticated.nonce,
                authorization: authenticated.authorization,
            })
            .expect("authenticated cancel nonce was validated in the actor turn");
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
        is_ioc: bool,
    ) -> Result<Vec<u64>, SequencerError> {
        self.admit_or_defer_with_authorization(submission, is_ioc, None)
            .await
    }

    async fn admit_or_defer_with_authorization(
        &mut self,
        submission: OrderSubmission,
        is_ioc: bool,
        authorization: Option<(u64, sybil_verifier::ClientActionAuth)>,
    ) -> Result<Vec<u64>, SequencerError> {
        self.check_account_submission_limits(&submission)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let outcome = if is_ioc {
            self.sequencer.try_admit_ioc(submission, now_ms)
        } else {
            self.sequencer.try_admit_direct(submission, now_ms)
        };
        match outcome {
            crate::sequencer::AdmitOutcome::Admitted {
                order_id,
                resting_order,
            } => {
                let persist_result = if let Some(store) = &self.store {
                    match &authorization {
                        Some((nonce, envelope)) => {
                            store
                                .append_authenticated_direct_admit(&resting_order, *nonce, envelope)
                                .await
                        }
                        None => store.append_admit_log(&resting_order).await,
                    }
                } else {
                    Ok(())
                };
                if let Err(err) = persist_result {
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
                if let Some((nonce, envelope)) = authorization {
                    self.sequencer
                        .apply_client_action_authorized(
                            sybil_verifier::ClientActionWitness::Order {
                                account_id: resting_order.account_id.0,
                                order: resting_order.order,
                                nonce,
                                authorization: envelope,
                            },
                        )
                        .expect("authenticated order nonce was validated in the actor turn");
                }
                Ok(vec![order_id])
            }
            crate::sequencer::AdmitOutcome::Deferred {
                order_ids,
                submission,
            } => {
                self.check_deferred_submission_limits(&submission)?;
                if let Some(store) = &self.store {
                    match &authorization {
                        Some((nonce, envelope)) => {
                            store
                                .append_authenticated_deferred_bundle(&submission, *nonce, envelope)
                                .await
                        }
                        None => store.append_pending_bundle(&submission).await,
                    }
                    .map_err(|err| SequencerError::Persistence(err.to_string()))?;
                }
                let authenticated_order = authorization.as_ref().map(|(nonce, envelope)| {
                    sybil_verifier::ClientActionWitness::Order {
                        account_id: submission.account_id.0,
                        order: submission
                            .orders
                            .first()
                            .expect("authenticated submission contains one order")
                            .clone(),
                        nonce: *nonce,
                        authorization: envelope.clone(),
                    }
                });
                self.sequencer.push_pending_bundle(submission);
                if let Some(action) = authenticated_order {
                    self.sequencer
                        .apply_client_action_authorized(action)
                        .expect("authenticated order nonce was validated in the actor turn");
                }
                Ok(order_ids)
            }
            crate::sequencer::AdmitOutcome::Rejected(err) => Err(err),
        }
    }

    pub(super) fn check_global_submission_rate(&mut self) -> Result<(), SequencerError> {
        self.global_submission_limiter.try_wait().map_err(|_| {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "global_rate"
            )
            .increment(1);
            SequencerError::RateLimited {
                retry_after_secs: 1,
            }
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

        let limiter = self
            .account_submission_limiters
            .entry(submission.account_id)
            .or_insert_with(|| {
                rate_limiter(
                    config.max_submissions_per_account_per_second,
                    config.submission_burst_per_account,
                )
            });
        limiter.try_wait().map_err(|_| {
            metrics::counter!(
                "sybil_admission_limit_rejections_total",
                "limit" => "account_rate"
            )
            .increment(1);
            SequencerError::RateLimited {
                retry_after_secs: 1,
            }
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
