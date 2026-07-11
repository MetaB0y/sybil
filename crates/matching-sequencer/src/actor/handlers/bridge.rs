use super::super::*;

impl SequencerActorState {
    pub(super) async fn handle_l1_deposit(
        &mut self,
        deposit: L1Deposit,
    ) -> Result<crate::bridge::DepositDisposition, SequencerError> {
        self.sequencer.validate_l1_deposit(&deposit)?;
        if let Some(store) = &self.store {
            store
                .append_pending_l1_deposit(&deposit)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        let disposition = self.sequencer.ingest_l1_deposit(deposit)?;
        let label = match &disposition {
            crate::bridge::DepositDisposition::Credited(_) => "credited",
            crate::bridge::DepositDisposition::Quarantined { .. } => "quarantined",
        };
        metrics::counter!("sybil_l1_deposit_dispositions_total", "disposition" => label)
            .increment(1);
        Ok(disposition)
    }

    pub(super) async fn handle_bridge_withdrawal(
        &mut self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.sequencer.validate_bridge_withdrawal(&request)?;
        if let Some(store) = &self.store {
            store
                .append_pending_bridge_withdrawal(&request)
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer.request_bridge_withdrawal(request)
    }

    pub(super) async fn handle_bridge_withdrawal_l1_event(
        &mut self,
        event: BridgeWithdrawalL1Event,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        let mut preflight = self.sequencer.clone();
        preflight.apply_bridge_withdrawal_l1_event(event.clone())?;
        if let Some(store) = &self.store {
            store
                .append_pending_bridge_l1_input(&crate::bridge::BridgeL1Input::WithdrawalEvent(
                    event.clone(),
                ))
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer.apply_bridge_withdrawal_l1_event(event)
    }

    pub(super) async fn handle_observe_bridge_l1_height(
        &mut self,
        height: u64,
    ) -> Result<Vec<WithdrawalLeaf>, SequencerError> {
        if height <= self.sequencer.bridge_state().observed_l1_height {
            return Ok(Vec::new());
        }
        let mut preflight = self.sequencer.clone();
        preflight.observe_bridge_l1_height(height)?;
        if let Some(store) = &self.store {
            store
                .append_pending_bridge_l1_input(&crate::bridge::BridgeL1Input::ObservedHeight(
                    height,
                ))
                .await
                .map_err(|err| SequencerError::Persistence(err.to_string()))?;
        }
        self.sequencer.observe_bridge_l1_height(height)
    }

    pub(super) async fn handle_signed_bridge_withdrawal(
        &mut self,
        signed: SignedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        let genesis_hash = self
            .sequencer
            .genesis_hash()
            .ok_or(SequencerError::GenesisHashUnavailable)?;
        verify_signed_bridge_withdrawal(&signed, genesis_hash)?;
        self.handle_authenticated_bridge_withdrawal(AuthenticatedBridgeWithdrawal {
            request: signed.request,
            nonce: signed.nonce,
            signer: signed.signer,
        })
        .await
    }

    pub(super) async fn handle_authenticated_bridge_withdrawal(
        &mut self,
        authenticated: AuthenticatedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        let account_id = self
            .sequencer
            .lookup_pubkey(&authenticated.signer)
            .ok_or(SequencerError::UnknownSigner)?;
        if account_id != authenticated.request.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }
        self.accept_replay_nonce(account_id, authenticated.nonce)
            .await?;
        self.handle_bridge_withdrawal(authenticated.request).await
    }
}
