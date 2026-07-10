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

pub use account::{Account, AccountId, AccountProfile, AccountStore, ApiKeyRecord};
pub use account_storage::{
    AccountSnapshotSlot, QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts,
    QmdbStateLeafExclusionProof, QmdbStateLeafProof, QmdbStateOperationProofParts,
    QmdbStateRangeProofParts, QmdbStateRoot, QMDB_STATE_MAX_KEY_BYTES,
};
pub use actor::{
    MarketSearchResult, SequencerHandle, SequencerStateProof, SequencerStateProofKind,
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_BLOCK_HISTORY_QUERY_BLOCKS,
    MAX_PRICE_HISTORY_QUERY_POINTS,
};
pub use analytics::AnalyticsState;
pub use block::{
    AdmitTimingView, Block, BlockAnalytics, BlockProduction, DerivedViewProvenance,
    DerivedViewSidecar, RejectedOrderView, RemovedOrderExitReason, RemovedOrderPhase,
    RemovedOrderView, SealedBlock,
};
pub use bridge::{
    BridgeBlockData, BridgeState, BridgeWithdrawalL1Event, BridgeWithdrawalRequest, EthAddress,
    L1Deposit, L1WithdrawalStatus, WithdrawalLeaf, WithdrawalRefundReason,
};
pub use crypto::{
    api_key_hash, AccountAuthScheme, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedBridgeWithdrawal, AuthenticatedCancel, AuthenticatedKeyRegistration,
    AuthenticatedKeyRevocation, AuthenticatedOrder, AuthenticatedProfileUpdate, KeyScope,
    PublicKey, RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke, SignedBridgeWithdrawal,
    SignedCancel, SignedKeyRegistration, SignedKeyRevocation, SignedOrder, SignedProfileUpdate,
};
pub use error::{Rejection, RejectionReason, SequencerError};
pub use market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, MarketSearchQuery, MarketSortField,
    PriceCandle, PriceCandlePage, PriceHistoryPage, PricePoint, ResolutionConfig,
};
pub use portfolio::{PortfolioSummary, PositionValue};
pub use sequencer::{
    AnalyticsMemoryStats, BatchResult, BatchSequencer, BlockSequencer, LeaderboardBase,
    LeaderboardRow, OrderSubmission, PendingOrderInfo, SequencerConfig, DEFAULT_ORDER_TTL_BLOCKS,
};
pub use store::{
    AutoResolutionAction, AutoResolutionRecord, DaArtifact, DaArtifactIntegrityError,
    DaArtifactLookup, DaArtifactManifest, DaProviderRef, DA_FILE_PROVIDER_REF_ENCODING,
    DA_FILE_PROVIDER_REF_KIND, DA_PAYLOAD_ENCODING, DA_PAYLOAD_KIND,
    DA_PROVIDER_REFS_ENCODING_BYTES,
};
pub use system_event::SystemEvent;

// Re-export oracle types needed by consumers (e.g. sybil-api)
pub use sybil_oracle::{AdminOracle, MarketStatus, Oracle, ResolutionRecord};
