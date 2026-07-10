use super::*;

impl BlockSequencer {
    /// Idempotent under replay: terminal states absorb all later observations,
    /// so duplicate/crossed Cancelled and expiry observations cannot re-credit.
    pub fn apply_bridge_withdrawal_l1_event(
        &mut self,
        event: BridgeWithdrawalL1Event,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        let withdrawal_id = self
            .bridge
            .withdrawals
            .values()
            .find(|withdrawal| withdrawal.nullifier == event.nullifier)
            .map(|withdrawal| withdrawal.withdrawal_id);

        if let Some(withdrawal_id) = withdrawal_id {
            let transition = self
                .bridge
                .withdrawals
                .get_mut(&withdrawal_id)
                .expect("withdrawal id sourced from bridge map")
                .apply_l1_event(&event);
            self.apply_withdrawal_transition(withdrawal_id, transition)?;
        }

        // Apply the targeted event first: a Finalized/Cancelled event carried
        // by this block wins over expiry observed at the same scan point.
        // Even when the target was already pruned, its confirmed block height
        // still advances the shared L1 cursor and can expire other active
        // withdrawals.
        self.observe_bridge_l1_height(event.l1_block_height)?;

        Ok(withdrawal_id.map(|withdrawal_id| {
            self.bridge
                .withdrawals
                .get(&withdrawal_id)
                .expect("terminal withdrawals are pruned only at block production")
                .clone()
        }))
    }

    /// Advance the withdrawal clock from the L1 indexer's existing confirmed
    /// scan cursor and refund every newly expired active withdrawal in id order.
    pub fn observe_bridge_l1_height(
        &mut self,
        l1_height: u64,
    ) -> Result<Vec<WithdrawalLeaf>, SequencerError> {
        if l1_height <= self.bridge.observed_l1_height {
            return Ok(Vec::new());
        }
        self.bridge.observed_l1_height = l1_height;
        self.record_system_event(SystemEvent::L1BlockObserved { height: l1_height });

        let withdrawal_ids: Vec<u64> = self.bridge.withdrawals.keys().copied().collect();
        let mut refunded = Vec::new();
        for withdrawal_id in withdrawal_ids {
            let transition = self
                .bridge
                .withdrawals
                .get_mut(&withdrawal_id)
                .expect("withdrawal id sourced from bridge map")
                .observe_l1_height(l1_height);
            self.apply_withdrawal_transition(withdrawal_id, transition)?;
            if matches!(transition, crate::bridge::WithdrawalTransition::Refunded(_)) {
                refunded.push(
                    self.bridge
                        .withdrawals
                        .get(&withdrawal_id)
                        .expect("terminal withdrawals are pruned only at block production")
                        .clone(),
                );
            }
        }
        Ok(refunded)
    }

    fn apply_withdrawal_transition(
        &mut self,
        withdrawal_id: u64,
        transition: crate::bridge::WithdrawalTransition,
    ) -> Result<(), SequencerError> {
        let withdrawal = self
            .bridge
            .withdrawals
            .get(&withdrawal_id)
            .expect("transition withdrawal exists")
            .clone();
        let amount = i64::try_from(withdrawal.amount_nanos)
            .map_err(|_| SequencerError::Bridge(BridgeError::AmountOverflow.to_string()))?;

        match transition {
            crate::bridge::WithdrawalTransition::Unchanged
            | crate::bridge::WithdrawalTransition::Queued => {}
            crate::bridge::WithdrawalTransition::Finalized => {
                self.capture_system_account_baseline(withdrawal.account_id);
                self.record_system_event(SystemEvent::WithdrawalFinalized {
                    account_id: withdrawal.account_id,
                    withdrawal_id,
                    amount,
                });
            }
            crate::bridge::WithdrawalTransition::Refunded(reason) => {
                self.capture_system_account_baseline(withdrawal.account_id);
                let account = self.accounts.get_mut(withdrawal.account_id).ok_or({
                    SequencerError::Rejected(Rejection {
                        order_id: 0,
                        account_id: withdrawal.account_id,
                        reason: RejectionReason::AccountNotFound,
                    })
                })?;
                account.balance = account.balance.checked_add(amount).ok_or_else(|| {
                    SequencerError::Bridge(BridgeError::AmountOverflow.to_string())
                })?;
                self.record_system_event(SystemEvent::WithdrawalRefunded {
                    account_id: withdrawal.account_id,
                    withdrawal_id,
                    amount,
                    reason,
                });
            }
        }
        Ok(())
    }

    pub fn validate_l1_deposit(&self, deposit: &L1Deposit) -> Result<i64, SequencerError> {
        if self.accounts.get(deposit.account_id).is_none() {
            return Err(SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: deposit.account_id,
                reason: RejectionReason::AccountNotFound,
            }));
        }
        let expected_id = self.bridge.deposit_cursor.saturating_add(1);
        if deposit.deposit_id != expected_id {
            return Err(SequencerError::Bridge(
                BridgeError::NonSequentialDeposit {
                    expected: expected_id,
                    actual: deposit.deposit_id,
                }
                .to_string(),
            ));
        }
        if deposit.sybil_account_key != account_key(deposit.account_id) {
            return Err(SequencerError::Bridge(
                BridgeError::AccountKeyMismatch.to_string(),
            ));
        }
        let mut frontier = self.bridge.deposit_frontier;
        let expected_root = crate::bridge::append_deposit_frontier(
            &mut frontier,
            self.bridge.deposit_cursor,
            deposit,
        )
        .ok_or_else(|| SequencerError::Bridge("deposit frontier is at capacity".to_string()))?;
        if deposit.deposit_root != expected_root {
            return Err(SequencerError::Bridge(
                BridgeError::DepositRootMismatch {
                    expected: expected_root,
                    actual: deposit.deposit_root,
                }
                .to_string(),
            ));
        }
        amount_token_units_to_i64_nanos(deposit.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))
    }

    pub fn ingest_l1_deposit(&mut self, deposit: L1Deposit) -> Result<Account, SequencerError> {
        let amount = self.validate_l1_deposit(&deposit)?;
        let account_id = deposit.account_id;
        self.capture_system_account_baseline(account_id);
        let account = self.accounts.get_mut(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;

        account.balance += amount;
        account.total_deposited += amount;
        let updated = account.clone();
        let mut frontier = self.bridge.deposit_frontier;
        let expected_root = crate::bridge::append_deposit_frontier(
            &mut frontier,
            self.bridge.deposit_cursor,
            &deposit,
        )
        .ok_or_else(|| SequencerError::Bridge("deposit frontier is at capacity".to_string()))?;
        debug_assert_eq!(expected_root, deposit.deposit_root);
        self.bridge.deposit_cursor = deposit.deposit_id;
        self.bridge.deposit_root = deposit.deposit_root;
        self.bridge.deposit_frontier = frontier;
        self.record_system_event(SystemEvent::L1Deposit {
            account_id,
            amount,
            deposit,
        });
        self.note_first_deposit(account_id);
        Ok(updated)
    }

    pub fn validate_bridge_withdrawal(
        &self,
        request: &BridgeWithdrawalRequest,
    ) -> Result<i64, SequencerError> {
        let account = self.accounts.get(request.account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: request.account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        let amount = amount_token_units_to_i64_nanos(request.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))?;
        if request.expiry_height < self.bridge.observed_l1_height {
            return Err(SequencerError::Bridge(
                BridgeError::WithdrawalExpired {
                    expiry_height: request.expiry_height,
                    observed_l1_height: self.bridge.observed_l1_height,
                }
                .to_string(),
            ));
        }
        let available = account.balance - self.order_book.reserved_balance(request.account_id);
        if amount > available {
            return Err(SequencerError::Bridge(
                BridgeError::InsufficientAvailableBalance {
                    required: amount,
                    available,
                }
                .to_string(),
            ));
        }
        Ok(amount)
    }

    pub fn request_bridge_withdrawal(
        &mut self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        let amount_i64 = self.validate_bridge_withdrawal(&request)?;
        let amount_nanos = amount_token_units_to_nanos(request.amount_token_units)
            .map_err(|err| SequencerError::Bridge(err.to_string()))?;
        let withdrawal_id = self.bridge.next_withdrawal_id;
        let nullifier = crate::bridge::withdrawal_nullifier(
            request.chain_id,
            request.vault_address,
            withdrawal_id,
            request.account_id,
            request.recipient,
            request.token_address,
            request.amount_token_units,
        );
        let withdrawal = WithdrawalLeaf {
            withdrawal_id,
            account_id: request.account_id,
            recipient: request.recipient,
            token_address: request.token_address,
            amount_token_units: request.amount_token_units,
            amount_nanos,
            expiry_height: request.expiry_height,
            nullifier,
            created_at_height: self.height.saturating_add(1),
            l1_status: L1WithdrawalStatus::NotRequested,
            l1_requested_at_unix: None,
            l1_executable_at_unix: None,
            l1_finalized_at_unix: None,
            l1_cancelled_at_unix: None,
            l1_tx_hash: None,
        };

        self.capture_system_account_baseline(request.account_id);
        let account = self.accounts.get_mut(request.account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id: request.account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        account.balance -= amount_i64;
        self.bridge.next_withdrawal_id = withdrawal_id.saturating_add(1);
        self.bridge
            .withdrawals
            .insert(withdrawal_id, withdrawal.clone());
        self.record_system_event(SystemEvent::WithdrawalCreated {
            account_id: request.account_id,
            amount: amount_i64,
            withdrawal: withdrawal.clone(),
        });
        Ok(withdrawal)
    }
}
