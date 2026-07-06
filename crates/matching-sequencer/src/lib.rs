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

pub use account::{Account, AccountId, AccountStore};
pub use account_storage::{
    AccountSnapshotSlot, QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts,
    QmdbStateLeafExclusionProof, QmdbStateLeafProof, QmdbStateOperationProofParts,
    QmdbStateRangeProofParts, QmdbStateRoot, QMDB_STATE_MAX_KEY_BYTES,
};
pub use actor::{
    MarketSearchResult, SequencerHandle, SequencerStateProof, SequencerStateProofKind,
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_PRICE_HISTORY_QUERY_POINTS,
};
pub use analytics::AnalyticsState;
pub use block::{
    AdmitTimingView, Block, BlockAnalytics, BlockProduction, DerivedViewProvenance,
    DerivedViewSidecar, RejectedOrderView, RemovedOrderExitReason, RemovedOrderPhase,
    RemovedOrderView, SealedBlock,
};
pub use bridge::{
    BridgeBlockData, BridgeState, BridgeWithdrawalRequest, EthAddress, L1Deposit, WithdrawalLeaf,
};
pub use crypto::{PublicKey, SignedBridgeWithdrawal, SignedCancel, SignedOrder};
pub use error::{Rejection, RejectionReason, SequencerError};
pub use market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, MarketSearchQuery, MarketSortField,
    PriceCandle, PriceCandlePage, PriceHistoryPage, PricePoint, ResolutionConfig,
};
pub use portfolio::{PortfolioSummary, PositionValue};
pub use sequencer::{
    AnalyticsMemoryStats, BatchResult, BatchSequencer, BlockSequencer, OrderSubmission,
    PendingOrderInfo, SequencerConfig, DEFAULT_ORDER_TTL_BLOCKS,
};
pub use system_event::SystemEvent;

// Re-export oracle types needed by consumers (e.g. sybil-api)
pub use sybil_oracle::{AdminOracle, MarketStatus, Oracle, ResolutionRecord};
