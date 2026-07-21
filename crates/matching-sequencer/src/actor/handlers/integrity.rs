use super::super::*;

impl SequencerActorState {
    /// Enforce the fail-stop boundary before dispatch reaches any canonical
    /// mutation handler. Keeping the classification exhaustive means adding a
    /// new actor message requires an explicit decision about halted behavior.
    pub(super) fn reject_canonical_write_if_halted(
        &self,
        message: SequencerMsg,
    ) -> Option<SequencerMsg> {
        if self.halted_error.is_none() {
            return Some(message);
        }

        match message {
            SequencerMsg::SubmitOrder(submission, reply)
            | SequencerMsg::SubmitIocOrder(submission, reply) => {
                let result = Err(SequencerError::IntegrityHalted);
                self.record_submission_metrics("unsigned", submission.orders.len(), &result);
                self.reject_integrity_halted("order", reply);
                None
            }
            SequencerMsg::SubmitSignedOrder(_, reply) => {
                let result = Err(SequencerError::IntegrityHalted);
                self.record_submission_metrics("signed", 1, &result);
                self.reject_integrity_halted("order", reply);
                None
            }
            SequencerMsg::SubmitAuthenticatedOrder(_, reply) => {
                let result = Err(SequencerError::IntegrityHalted);
                self.record_submission_metrics("signed", 1, &result);
                self.reject_integrity_halted("order", reply);
                None
            }
            SequencerMsg::SubmitSignedMmBundle(_, reply)
            | SequencerMsg::SubmitAuthenticatedMmBundle(_, reply)
            | SequencerMsg::ReplaceSignedMmBundle(_, reply)
            | SequencerMsg::ReplaceAuthenticatedMmBundle(_, reply) => {
                self.record_mm_lifecycle_metrics("write", 0, false);
                self.reject_integrity_halted("mm_bundle", reply);
                None
            }
            SequencerMsg::CancelSignedMmBundle(_, reply)
            | SequencerMsg::CancelAuthenticatedMmBundle(_, reply) => {
                self.record_mm_lifecycle_metrics("cancel", 0, false);
                self.reject_integrity_halted("mm_bundle_cancel", reply);
                None
            }
            SequencerMsg::CancelSignedOrder(_, reply) => {
                let result = Err(SequencerError::IntegrityHalted);
                self.record_cancel_metrics("signed", &result);
                self.reject_integrity_halted("cancel", reply);
                None
            }
            SequencerMsg::CancelAuthenticatedOrder(_, reply) => {
                let result = Err(SequencerError::IntegrityHalted);
                self.record_cancel_metrics("signed", &result);
                self.reject_integrity_halted("cancel", reply);
                None
            }
            SequencerMsg::CreateAccount(_, reply)
            | SequencerMsg::CreateAccountWithInitialKey(_, _, _, reply)
            | SequencerMsg::CreatePublicAccountWithInitialKey(_, _, _, _, reply)
            | SequencerMsg::FundAccount(_, _, reply)
            | SequencerMsg::SetProfileSigned(_, reply)
            | SequencerMsg::SetProfileAuthenticated(_, reply) => {
                self.reject_integrity_halted("account", reply);
                None
            }
            SequencerMsg::ProvisionServiceAccount(_, _, _, reply) => {
                self.reject_integrity_halted("account", reply);
                None
            }
            SequencerMsg::SubmitL1Deposit(_, reply) => {
                self.reject_integrity_halted("bridge", reply);
                None
            }
            SequencerMsg::CreateBridgeWithdrawal(_, reply)
            | SequencerMsg::CreateSignedBridgeWithdrawal(_, reply)
            | SequencerMsg::CreateAuthenticatedBridgeWithdrawal(_, reply) => {
                self.reject_integrity_halted("bridge", reply);
                None
            }
            SequencerMsg::ApplyBridgeWithdrawalL1Event(_, reply) => {
                self.reject_integrity_halted("bridge", reply);
                None
            }
            SequencerMsg::ObserveBridgeL1Height(_, reply) => {
                self.reject_integrity_halted("bridge", reply);
                None
            }
            SequencerMsg::RegisterPubkey(_, _, _, reply)
            | SequencerMsg::RegisterPubkeyWithMeta(_, _, _, reply)
            | SequencerMsg::RegisterKeySigned(_, reply)
            | SequencerMsg::RegisterKeyAuthenticated(_, reply)
            | SequencerMsg::RevokeSigningKeySigned(_, reply)
            | SequencerMsg::RevokeSigningKeyAuthenticated(_, reply)
            | SequencerMsg::RevokeApiKeySigned(_, reply)
            | SequencerMsg::RevokeApiKeyAuthenticated(_, reply) => {
                self.reject_integrity_halted("identity", reply);
                None
            }
            SequencerMsg::CreateApiKeySigned(_, reply)
            | SequencerMsg::CreateApiKeyAuthenticated(_, reply) => {
                self.reject_integrity_halted("identity", reply);
                None
            }
            SequencerMsg::CreateMarket(_, reply)
            | SequencerMsg::CreateMarketWithMetadata(_, _, reply) => {
                self.reject_integrity_halted("market", reply);
                None
            }
            SequencerMsg::UpdateMarketContent(_, _, _, reply) => {
                self.reject_integrity_halted("market", reply);
                None
            }
            SequencerMsg::CreateMarketGroup(_, _, _, reply) => {
                self.reject_integrity_halted("market", reply);
                None
            }
            SequencerMsg::ExtendMarketGroup(_, _, reply) => {
                self.reject_integrity_halted("market", reply);
                None
            }
            SequencerMsg::ResolveMarket(_, _, reply)
            | SequencerMsg::ResolveMarketAttested(_, _, reply) => {
                self.reject_integrity_halted("market", reply);
                None
            }
            SequencerMsg::RegisterFeed(_, _, reply) => {
                self.reject_integrity_halted("oracle", reply);
                None
            }
            SequencerMsg::InstallTemplate(_, reply) => {
                self.reject_integrity_halted("oracle", reply);
                None
            }
            SequencerMsg::ResumeBlockProduction(reply) => {
                self.reject_integrity_halted("resume", reply);
                None
            }

            // Read paths and incident-response controls stay live. Auto-
            // resolution records are a separate, immediately durable operator
            // review log; they do not enter canonical state or the WAL.
            SequencerMsg::Tick => Some(SequencerMsg::Tick),
            #[cfg(test)]
            SequencerMsg::TestCrashOnNextBlock(crashpoint) => {
                Some(SequencerMsg::TestCrashOnNextBlock(crashpoint))
            }
            #[cfg(test)]
            SequencerMsg::TestEnterIntegrityHalt(error, reply) => {
                Some(SequencerMsg::TestEnterIntegrityHalt(error, reply))
            }
            #[cfg(test)]
            SequencerMsg::TestHoldNextTick(hold, reply) => {
                Some(SequencerMsg::TestHoldNextTick(hold, reply))
            }
            SequencerMsg::GetStateProof(key, reply) => {
                Some(SequencerMsg::GetStateProof(key, reply))
            }
            SequencerMsg::ProduceBlock(reply) => Some(SequencerMsg::ProduceBlock(reply)),
            SequencerMsg::GetDaArtifact(height, reply) => {
                Some(SequencerMsg::GetDaArtifact(height, reply))
            }
            SequencerMsg::GetDaManifest(height, reply) => {
                Some(SequencerMsg::GetDaManifest(height, reply))
            }
            SequencerMsg::PauseBlockProduction(reply) => {
                Some(SequencerMsg::PauseBlockProduction(reply))
            }
            SequencerMsg::Query(query) => Some(SequencerMsg::Query(query)),
            SequencerMsg::IndicativeTick => Some(SequencerMsg::IndicativeTick),
            SequencerMsg::IndicativeUpdate(snapshots) => {
                Some(SequencerMsg::IndicativeUpdate(snapshots))
            }
            SequencerMsg::IndicativeSolveFailed { solver, error } => {
                Some(SequencerMsg::IndicativeSolveFailed { solver, error })
            }
        }
    }

    fn reject_integrity_halted<T>(
        &self,
        kind: &'static str,
        reply: RpcReplyPort<Result<T, SequencerError>>,
    ) {
        metrics::counter!(
            "sybil_integrity_halted_write_rejections_total",
            "kind" => kind
        )
        .increment(1);
        let _ = reply.send(Err(SequencerError::IntegrityHalted));
    }
}
