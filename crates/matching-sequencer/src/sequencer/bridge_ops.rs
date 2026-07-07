use super::*;

impl BlockSequencer {
    pub fn apply_bridge_withdrawal_l1_event(
        &mut self,
        event: BridgeWithdrawalL1Event,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        let withdrawal = self
            .bridge
            .withdrawals
            .values_mut()
            .find(|withdrawal| withdrawal.nullifier == event.nullifier)
            .ok_or_else(|| {
                SequencerError::Bridge(
                    BridgeError::UnknownWithdrawalNullifier(event.nullifier).to_string(),
                )
            })?;

        withdrawal.l1_status = event.status;
        withdrawal.l1_executable_at_unix = event
            .executable_at_unix
            .or(withdrawal.l1_executable_at_unix);
        withdrawal.l1_tx_hash = event.tx_hash.or(withdrawal.l1_tx_hash);
        match event.status {
            L1WithdrawalStatus::NotRequested => {}
            L1WithdrawalStatus::Queued => {
                withdrawal.l1_requested_at_unix = Some(event.event_at_unix);
            }
            L1WithdrawalStatus::Finalized => {
                withdrawal.l1_finalized_at_unix = Some(event.event_at_unix);
            }
            L1WithdrawalStatus::Cancelled => {
                withdrawal.l1_cancelled_at_unix = Some(event.event_at_unix);
            }
        }

        Ok(withdrawal.clone())
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
        self.bridge.deposit_log.push(deposit.clone());
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
        let next_height = self.height.saturating_add(1);
        if request.expiry_height < next_height {
            return Err(SequencerError::Bridge(
                BridgeError::WithdrawalExpired {
                    expiry_height: request.expiry_height,
                    next_height,
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
