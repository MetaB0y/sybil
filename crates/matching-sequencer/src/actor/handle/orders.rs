use crate::crypto::{
    AuthenticatedCancel, AuthenticatedMmBundle, AuthenticatedOrder, SignedCancel, SignedMmBundle,
    SignedOrder,
};
use crate::error::SequencerError;
use crate::sequencer::OrderSubmission;

use super::super::SequencerMsg;
use super::SequencerHandle;

impl SequencerHandle {
    pub async fn submit_order(
        &self,
        submission: OrderSubmission,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitOrder(submission, reply))
            .await?
    }

    /// Submit an unsigned IOC order whose concrete expiry is assigned by the
    /// sequencer actor from its committed height at admission time.
    pub async fn submit_ioc_order(
        &self,
        submission: OrderSubmission,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitIocOrder(submission, reply))
            .await?
    }

    pub async fn submit_signed_order(
        &self,
        signed: SignedOrder,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitSignedOrder(signed, reply))
            .await?
    }

    pub async fn submit_authenticated_order(
        &self,
        authenticated: AuthenticatedOrder,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitAuthenticatedOrder(authenticated, reply))
            .await?
    }

    pub async fn submit_signed_mm_bundle(
        &self,
        signed: SignedMmBundle,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitSignedMmBundle(signed, reply))
            .await?
    }

    pub async fn submit_authenticated_mm_bundle(
        &self,
        authenticated: AuthenticatedMmBundle,
    ) -> Result<Vec<u64>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitAuthenticatedMmBundle(authenticated, reply))
            .await?
    }

    pub async fn cancel_signed_order(&self, signed: SignedCancel) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelSignedOrder(signed, reply))
            .await?
    }

    pub async fn cancel_authenticated_order(
        &self,
        authenticated: AuthenticatedCancel,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelAuthenticatedOrder(authenticated, reply))
            .await?
    }
}
