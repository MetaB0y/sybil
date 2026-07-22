use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use matching_engine::{
    Fill, MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem,
    derive_order_direction,
};
use matching_solver::{PipelineResult, Solver};
use sybil_oracle::{MarketStatus, ResolutionRecord};
use sybil_verifier::{
    AccountSnapshot, BlockWitness, L1DepositWitness, SystemEventWitness, WitnessBlockHeader,
    WitnessOrder, WitnessRejection,
};
use tracing::{debug, error};

use crate::account::{Account, AccountId, AccountStore};
use crate::analytics::{AnalyticsState, OrderHistoryOptions};
use crate::block::{
    AdmitTimingView, Block, BlockAnalytics, BlockFlowMetrics, BlockHeader, BlockProduction,
    DerivedViewSidecar, RejectedOrderView, RemovedOrderExitReason, RemovedOrderPhase,
    RemovedOrderView, hash_header, state_sidecar_snapshot,
};
use crate::bridge::{
    BridgeBlockData, BridgeError, BridgeState, BridgeWithdrawalL1Event, BridgeWithdrawalRequest,
    DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS, DepositDisposition, L1Deposit, L1WithdrawalStatus,
    WithdrawalLeaf, account_key, amount_token_units_to_i64_nanos, amount_token_units_to_nanos,
};
use crate::canonical_state::{CanonicalState, snapshot_account};
use crate::error::{
    BlockInvariantFailure, Rejection, RejectionReason, SequencerError, VerifierFailure,
};
use crate::market_info::MarketMetadata;
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::{OrderBook, RestingExit, RestingRevalidationExit};
use crate::settlement;
use crate::store::{AcknowledgedWrite, ControlPlaneCommand, RestoredState, SequencerSnapshot};
use crate::system_event::SystemEvent;
use crate::validation::validate_order_shape;

mod accounts;
mod admission;
mod bridge_ops;
mod config;
mod markets;
mod production;
mod restore;
mod types;
mod views;

pub(crate) use self::accounts::PreparedServiceAccountProvisioning;
pub use self::accounts::{
    MAX_ACCOUNT_PROVISIONING_KEY_BYTES, ServiceAccountProvisioningReceipt,
    ServiceAccountProvisioningResult,
};
pub use self::config::{
    DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS, DEFAULT_ORDER_TTL_BLOCKS, SequencerConfig,
};
pub use self::restore::SequencerRestoreError;
pub use self::types::{
    AdmitOutcome, BatchResult, LeaderboardBase, LeaderboardRow, OrderSubmission, PendingOrderInfo,
    PreparedBlock, batch_result_from_block,
};

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Block-producing sequencer. Core sync layer.
///
/// Manages accounts, assigns order IDs, validates, solves, settles, and
/// produces blocks. The actor layer calls `produce_block()` on each timer tick.
/// Simulations can use this directly without the actor.
#[derive(Clone)]
pub struct BlockSequencer {
    pub accounts: AccountStore,
    /// Pluggable solver for matching optimization.
    solver: Arc<dyn Solver>,
    next_order_id: u64,
    /// Resting orders with tracked balance/position reservations.
    order_book: OrderBook,
    /// Current block height.
    height: u64,
    /// Markets available for trading.
    markets: MarketSet,
    /// Market groups (multi-outcome event constraints).
    market_groups: Vec<MarketGroup>,
    /// Last block header for hash chaining.
    last_header: Option<BlockHeader>,
    /// Hash of the first committed block header; scopes signed order/cancel bytes.
    genesis_hash: Option<[u8; 32]>,
    /// Non-account sidecar at the last committed header. Live state may include
    /// acknowledged writes for the next block; v3 pre-sidecar must not.
    committed_state_sidecar: sybil_verifier::StateSidecarSnapshot,
    /// Deposit frontier at the last committed header.
    committed_deposit_frontier: sybil_l1_protocol::DepositFrontier,
    /// In-process derived projections for API/UI surfaces. Updated
    /// synchronously by the sequencer, but kept separate from core matching,
    /// settlement, and witness state.
    analytics: AnalyticsState,
    /// Market lifecycle: statuses, resolution policies, feeds, and metadata.
    pub lifecycle: crate::market_lifecycle::MarketLifecycle,
    /// P256 public key to account mapping.
    pubkey_registry: HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>,
    /// Durable operator retry receipts. These are chain-local operational
    /// allocation state, not consensus inputs.
    service_account_receipts:
        HashMap<[u8; 32], crate::sequencer::accounts::ServiceAccountProvisioningReceipt>,
    /// Lifetime public grant allocations, independent from service accounts.
    public_accounts_allocated: u64,
    /// Derived, non-persisted reverse index of ACTIVE read API-key hashes to
    /// their owning account (SYB-60). Rebuilt from `accounts` on restore and
    /// maintained incrementally by create/revoke; lets the bearer extractor do
    /// an O(1) lookup without scanning every account. Revoked keys are removed
    /// from this index (but retained in `Account::api_keys` for audit).
    api_key_index: HashMap<[u8; 32], AccountId>,
    /// L1 bridge sidecar state: consumed deposits and normal withdrawal leaves.
    bridge: BridgeState,
    /// Administrative state changes that should be included in the next block.
    pending_system_events: Vec<SystemEvent>,
    /// Block-start baselines for accounts touched by pending system events.
    /// `None` means the account did not exist before the first system event.
    pending_system_account_baselines: HashMap<AccountId, Option<Account>>,
    /// Buffered submissions that couldn't be admitted into the resting book
    /// at submit time (MM-constrained, multi-order, multi-market). Drained
    /// by the clone inside `prepare_block` and consumed by the solver. The
    /// durable counterpart is a `DeferredBundle` row in the global
    /// acknowledged-write WAL so restart cannot drop it or reorder it against
    /// another subsystem.
    pending_bundles: Vec<OrderSubmission>,
    /// Runtime configuration for this sequencer and its surrounding actor.
    pub config: SequencerConfig,
}

impl BlockSequencer {
    fn capture_system_account_baseline(&mut self, account_id: AccountId) {
        if self
            .pending_system_account_baselines
            .contains_key(&account_id)
        {
            return;
        }
        self.pending_system_account_baselines
            .insert(account_id, self.accounts.get(account_id).cloned());
    }

    fn capture_missing_system_account(&mut self, account_id: AccountId) {
        self.pending_system_account_baselines
            .entry(account_id)
            .or_insert(None);
    }

    pub fn record_system_event(&mut self, event: SystemEvent) {
        self.pending_system_events.push(event);
    }

    /// Apply one already-verified ordinary client authorization. The account
    /// baseline is captured before the nonce changes so witness replay opens
    /// the exact prior cross-block nonce.
    pub fn apply_client_action_authorized(
        &mut self,
        action: sybil_verifier::ClientActionWitness,
    ) -> Result<(), SequencerError> {
        let (account_id, nonce) = match &action {
            sybil_verifier::ClientActionWitness::Order {
                account_id, nonce, ..
            }
            | sybil_verifier::ClientActionWitness::Cancel {
                account_id, nonce, ..
            }
            | sybil_verifier::ClientActionWitness::MmBundle {
                account_id, nonce, ..
            } => (AccountId(*account_id), *nonce),
        };
        self.capture_system_account_baseline(account_id);
        self.advance_replay_nonce(account_id, nonce)?;
        self.accounts
            .get_mut(account_id)
            .expect("nonce advance validated an existing account")
            .last_trading_nonce = nonce;
        self.record_system_event(SystemEvent::ClientActionAuthorized(action));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;
