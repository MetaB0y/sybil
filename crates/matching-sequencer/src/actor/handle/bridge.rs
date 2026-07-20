use crate::account::AccountId;
use crate::bridge::{
    BridgeState, BridgeWithdrawalL1Event, BridgeWithdrawalRequest, L1Deposit, WithdrawalLeaf,
};
use crate::crypto::{AuthenticatedBridgeWithdrawal, SignedBridgeWithdrawal};
use crate::error::SequencerError;

use super::super::SequencerMsg;
use super::SequencerHandle;

impl SequencerHandle {
    pub async fn submit_l1_deposit(
        &self,
        deposit: L1Deposit,
    ) -> Result<crate::bridge::DepositDisposition, SequencerError> {
        self.control_rpc(|reply| SequencerMsg::SubmitL1Deposit(deposit, reply))
            .await?
    }

    pub async fn create_bridge_withdrawal(
        &self,
        request: BridgeWithdrawalRequest,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateBridgeWithdrawal(request, reply))
            .await?
    }

    pub async fn create_signed_bridge_withdrawal(
        &self,
        signed: SignedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateSignedBridgeWithdrawal(signed, reply))
            .await?
    }

    pub async fn create_authenticated_bridge_withdrawal(
        &self,
        authenticated: AuthenticatedBridgeWithdrawal,
    ) -> Result<WithdrawalLeaf, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateAuthenticatedBridgeWithdrawal(authenticated, reply))
            .await?
    }

    pub async fn apply_bridge_withdrawal_l1_event(
        &self,
        event: BridgeWithdrawalL1Event,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        self.control_rpc(|reply| SequencerMsg::ApplyBridgeWithdrawalL1Event(event, reply))
            .await?
    }

    pub async fn observe_bridge_l1_height(
        &self,
        height: u64,
    ) -> Result<Vec<WithdrawalLeaf>, SequencerError> {
        self.control_rpc(|reply| SequencerMsg::ObserveBridgeL1Height(height, reply))
            .await?
    }

    pub async fn get_bridge_state(&self) -> Result<BridgeState, SequencerError> {
        self.read_query(|state| state.sequencer.bridge_state().clone())
            .await
    }

    pub async fn get_bridge_account_key(
        &self,
        account_id: AccountId,
    ) -> Result<Option<[u8; 32]>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_key(account_id))
            .await
    }

    pub async fn get_bridge_account_id_by_key(
        &self,
        key: [u8; 32],
    ) -> Result<Option<AccountId>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_account_id_by_key(key))
            .await
    }

    pub async fn get_bridge_withdrawal(
        &self,
        withdrawal_id: u64,
    ) -> Result<Option<WithdrawalLeaf>, SequencerError> {
        self.read_query(move |state| state.sequencer.bridge_withdrawal(withdrawal_id).cloned())
            .await
    }

    pub async fn get_default_bridge_withdrawal_expiry(&self) -> Result<u64, SequencerError> {
        self.read_query(|state| state.sequencer.default_bridge_withdrawal_expiry_height())
            .await
    }
}
