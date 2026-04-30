pub mod account;
mod account_storage;
pub mod actor;
pub mod agent;
pub mod block;
pub mod bridge;
pub mod canonical_state;
pub mod crypto;
pub mod digest;
pub mod error;
pub mod fill_recorder;
pub mod market_info;
pub mod market_lifecycle;
pub mod metrics;
pub mod order_book;
pub mod portfolio;
pub mod price_tracker;
mod qmdb_accounts;
mod qmdb_state;
pub mod scenario;
pub mod sequencer;
pub mod settlement;
pub mod simulation;
pub mod store;
pub mod system_event;
pub mod validation;
pub mod zk_witness;

pub use account::{Account, AccountId, AccountStore};
pub use account_storage::{
    AccountSnapshotSlot, QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts,
    QmdbStateLeafExclusionProof, QmdbStateLeafProof, QmdbStateOperationProofParts,
    QmdbStateRangeProofParts, QmdbStateRoot, QMDB_STATE_MAX_KEY_BYTES,
};
pub use actor::{
    MarketSearchResult, SequencerHandle, SequencerStateProof, SequencerStateProofKind,
};
pub use block::{Block, BlockProduction};
pub use bridge::{
    BridgeBlockData, BridgeState, BridgeWithdrawalRequest, EthAddress, L1Deposit, WithdrawalLeaf,
};
pub use crypto::{PublicKey, SignedCancel, SignedOrder};
pub use error::{Rejection, RejectionReason, SequencerError};
pub use market_info::{
    AccountFillRecord, MarketMetadata, MarketSearchQuery, MarketSortField, PricePoint,
    ResolutionConfig,
};
pub use portfolio::{PortfolioSummary, PositionValue};
pub use scenario::Scenario;
pub use sequencer::{
    BatchResult, BatchSequencer, BlockSequencer, OrderSubmission, PendingOrderInfo,
    SequencerConfig, DEFAULT_ORDER_TTL_BLOCKS,
};
pub use simulation::{SimulationResult, SimulationRunner};
pub use system_event::SystemEvent;
pub use zk_witness::build_state_transition_guest_input;

// Re-export oracle types needed by consumers (e.g. sybil-api)
pub use sybil_oracle::{AdminOracle, MarketStatus, Oracle, ResolutionRecord};
