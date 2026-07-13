use alloy::rpc::types::Log as EthLog;
use sybil_api_types::request::{BridgeWithdrawalL1Status, SubmitL1WithdrawalEventRequest};
use sybil_l1_protocol::{
    L1Log, WithdrawalEvent, parse_deposit_received_log, parse_withdrawal_event_log,
};

use super::{IndexerError, Result};

#[derive(Clone, Debug)]
pub(super) struct IndexedDeposit {
    pub(super) log: EthLog,
    pub(super) event: sybil_l1_protocol::DepositReceived,
}

#[derive(Clone, Debug)]
pub(super) struct IndexedWithdrawalEvent {
    pub(super) log: EthLog,
    event: WithdrawalEvent,
}

pub(super) fn indexed_deposit_from_log(log: EthLog) -> Result<IndexedDeposit> {
    let l1_log = L1Log {
        address: log.address().into(),
        topics: log.topics().iter().copied().map(Into::into).collect(),
        data: log.data().data.to_vec(),
    };
    let event = parse_deposit_received_log(&l1_log)?;
    Ok(IndexedDeposit { log, event })
}

pub(super) fn indexed_withdrawal_event_from_log(log: EthLog) -> Result<IndexedWithdrawalEvent> {
    let l1_log = L1Log {
        address: log.address().into(),
        topics: log.topics().iter().copied().map(Into::into).collect(),
        data: log.data().data.to_vec(),
    };
    let event = parse_withdrawal_event_log(&l1_log)?;
    Ok(IndexedWithdrawalEvent { log, event })
}

pub(super) fn sort_deposits(deposits: &mut [IndexedDeposit]) {
    deposits.sort_by_key(|deposit| {
        (
            deposit.log.block_number.unwrap_or(u64::MAX),
            deposit.log.log_index.unwrap_or(u64::MAX),
        )
    });
}

pub(super) fn sort_withdrawal_events(events: &mut [IndexedWithdrawalEvent]) {
    events.sort_by_key(|event| {
        (
            event.log.block_number.unwrap_or(u64::MAX),
            event.log.log_index.unwrap_or(u64::MAX),
        )
    });
}

pub(super) fn withdrawal_event_request(
    event: &IndexedWithdrawalEvent,
) -> Result<SubmitL1WithdrawalEventRequest> {
    let (nullifier, status, event_at_unix, executable_at_unix) = match &event.event {
        WithdrawalEvent::Queued(queued) => (
            queued.nullifier,
            BridgeWithdrawalL1Status::Queued,
            queued.requested_at_unix,
            Some(queued.executable_at_unix),
        ),
        WithdrawalEvent::Finalized(finalized) => (
            finalized.nullifier,
            BridgeWithdrawalL1Status::Finalized,
            finalized.finalized_at_unix,
            Some(finalized.executable_at_unix),
        ),
        WithdrawalEvent::Cancelled(cancelled) => (
            cancelled.nullifier,
            BridgeWithdrawalL1Status::Cancelled,
            cancelled.cancelled_at_unix,
            Some(cancelled.executable_at_unix),
        ),
    };
    let l1_block_height = event
        .log
        .block_number
        .ok_or(IndexerError::MissingRpcResult)?;
    Ok(SubmitL1WithdrawalEventRequest {
        nullifier_hex: hex::encode(nullifier),
        status,
        event_at_unix,
        executable_at_unix,
        tx_hash_hex: event.transaction_hash_hex(),
        l1_block_height,
    })
}

impl IndexedWithdrawalEvent {
    fn transaction_hash_hex(&self) -> Option<String> {
        self.log.transaction_hash.map(hex::encode)
    }
}
