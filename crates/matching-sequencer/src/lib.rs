pub mod account;
mod account_storage;
pub mod actor;
pub mod aggregates;
mod analytics;
pub mod block;
pub mod bridge;
pub mod canonical_state;
pub mod crypto;
pub mod digest;
pub mod error;
pub mod fill_recorder;
pub mod market_info;
pub mod market_lifecycle;
pub mod order_book;
pub mod portfolio;
pub mod price_tracker;
mod qmdb_accounts;
mod qmdb_state;
pub mod sequencer;
pub mod settlement;
pub mod store;
pub mod system_event;
pub mod validation;

#[cfg(test)]
mod crash_harness;

pub use account::{
    Account, AccountId, AccountProfile, AccountStore, ApiKeyRecord, MAX_API_KEY_LABEL_BYTES,
    MAX_API_KEYS_PER_ACCOUNT, MAX_SERIALIZED_ACCOUNT_BYTES, MAX_SIGNING_KEY_LABEL_BYTES,
};
pub use account_storage::{
    AccountSnapshotSlot, QMDB_STATE_MAX_KEY_BYTES, QmdbStateExclusionProofParts,
    QmdbStateKeyValueProofParts, QmdbStateLeafExclusionProof, QmdbStateLeafProof,
    QmdbStateOperationProofParts, QmdbStateRangeProofParts, QmdbStateRoot,
};
pub use actor::{
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_BLOCK_REPLAY_QUERY_BLOCKS,
    MAX_PRICE_HISTORY_QUERY_POINTS, MarketSearchResult, SequencerHandle,
    SequencerOperationalStatus, SequencerStateProof, SequencerStateProofKind,
};
pub use analytics::AnalyticsState;
pub use block::{
    AdmitTimingView, Block, BlockAnalytics, BlockProduction, DerivedViewProvenance,
    DerivedViewSidecar, RejectedOrderView, RemovedOrderExitReason, RemovedOrderPhase,
    RemovedOrderView, SealedBlock,
};
pub use bridge::{
    BridgeBlockData, BridgeState, BridgeWithdrawalL1Event, BridgeWithdrawalRequest,
    DepositDisposition, EthAddress, L1Deposit, L1WithdrawalStatus, WithdrawalLeaf,
    WithdrawalRefundReason,
};
pub use crypto::{
    AccountAuthScheme, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedBridgeWithdrawal, AuthenticatedCancel, AuthenticatedKeyRegistration,
    AuthenticatedKeyRevocation, AuthenticatedMmBundle, AuthenticatedMmBundleCancel,
    AuthenticatedMmBundleReplace, AuthenticatedOrder, AuthenticatedProfileUpdate, KeyScope,
    PublicKey, RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke, SignedBridgeWithdrawal,
    SignedCancel, SignedKeyRegistration, SignedKeyRevocation, SignedMmBundle, SignedMmBundleCancel,
    SignedMmBundleReplace, SignedOrder, SignedProfileUpdate, api_key_hash,
};
pub use error::{Rejection, RejectionReason, SequencerError};
pub use market_info::{
    AccountFillRecord, MarketMetadata, MarketSearchQuery, MarketSortField, PricePoint,
    ResolutionConfig,
};
pub use portfolio::{PortfolioSummary, PositionValue};
pub use sequencer::{
    BatchResult, BlockSequencer, DEFAULT_MIN_RESTING_ORDER_NOTIONAL_NANOS,
    DEFAULT_ORDER_TTL_BLOCKS, LeaderboardBase, LeaderboardRow, MAX_ACCOUNT_PROVISIONING_KEY_BYTES,
    MmBundleLifecycleReceipt, MmBundleLifecycleResult, OrderSubmission, PendingOrderInfo,
    SequencerConfig, ServiceAccountProvisioningReceipt, ServiceAccountProvisioningResult,
};
pub use store::{
    AcknowledgedProofJobPruneReport, AcknowledgedProofJobRetentionPolicy,
    DA_FILE_PROVIDER_REF_ENCODING, DA_FILE_PROVIDER_REF_KIND, DA_PAYLOAD_ENCODING, DA_PAYLOAD_KIND,
    DA_PROVIDER_REFS_ENCODING_BYTES, DaArtifact, DaArtifactIntegrityError, DaArtifactLookup,
    DaArtifactManifest, DaManifestLookup, DaProviderRef, ProofJobOutboxEntry,
};
pub use sybil_verifier::{ClientActionAuth, ClientActionWitness, KeyOpAuth, KeyRecord};
pub use system_event::SystemEvent;

// Re-export oracle types needed by consumers (e.g. sybil-api)
pub use sybil_oracle::{MarketStatus, ResolutionRecord};
