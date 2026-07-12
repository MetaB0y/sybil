//! Block-boundary persistence via redb plus a qmdb-backed account snapshot.
//!
//! Philosophy: snapshot core state after each block in a single ACID transaction.
//! On crash, we resume from the last committed block plus any bundle submissions
//! that were admitted after it (replayed from the `PENDING_BUNDLES` table). The
//! in-progress solve is lost but its inputs are durable.
//!
//! The account-state boundary is explicit:
//! - qmdb stores account snapshots and typed state roots
//! - redb stores metadata plus the commit fence that declares which qmdb slot
//!   is committed
//!
//! Recovery trusts the redb fence, never "latest qmdb state".
//!
//! Transaction boundary:
//! 1. Write the next account snapshot and typed state tree into the inactive qmdb slot
//! 2. Commit redb metadata and flip the authoritative fence to that slot
//!
//! There is intentionally no cross-db transaction or journal. The redb commit
//! is the only commit point. Anything written to qmdb without a matching redb
//! fence flip is treated as uncommitted and ignored during recovery.
//!
//! Recovery invariants:
//! - `store_layout_version` must exist and match this binary
//! - if `height` exists, `account_state_height` and `account_state_slot` must exist
//! - `height == account_state_height`
//! - the fenced account qmdb slot must contain matching `height` and `next_account_id`
//! - the fenced typed-state qmdb slot root must match the block header `state_root`
//!
//! Uses MessagePack (rmp-serde) for values: self-describing, binary-stable across
//! schema changes. Adding fields with `#[serde(default)]` is backward-compatible.
//!
//! # Persistence Tiers
//!
//! **Tier 1 (implemented)**: Core state — accounts, markets, groups, resolution
//! templates, block headers, counters, pubkeys, clearing prices, market volumes.
//! Sufficient for crash recovery.
//!
//! **Tier 2 (partial)**: Order state.
//! - Resting order book: implemented (see `RESTING_ORDERS` table).
//! - Mempool: intentionally not persisted (short-lived by design; clients resubmit).
//! - MM inventory / variance: TODO.
//!
//! **Tier 3 (partial)**: Derived views.
//! - Fill history: implemented (see `FILL_HISTORY` table).
//! - Price history: implemented for raw mark points (see `PRICE_POINTS` table).
//! - Block ring buffer: exact-height fallback implemented, list/replay policy TODO.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use matching_engine::{Market, MarketGroup, MarketId, MarketSet, Nanos};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use sybil_oracle::{
    AdminOracle, Challenge, ChallengeId, DataFeed, FeedId, FeedPubkey, MarketStatus, OracleSource,
    ProposalId, ResolutionProposal, ResolutionRecord, ResolutionTemplate, SignedAttestation,
};
use sybil_verifier::{
    AccountSnapshot, BlockWitness, DepositAccumulatorWitness, L1DepositWitness,
    MarketStatusSnapshot, OracleSourceSnapshot, StateSidecarSnapshot, WitnessBlockHeader,
};
use tracing::{debug, info, warn};

use crate::account::{Account, AccountId, AccountStore};
use crate::account_storage::{
    AccountSnapshotSlot, AccountStateStore, CommittedAccountState, FencedAccountStorage,
    QmdbStateLeafExclusionProof, QmdbStateLeafProof, QmdbStateRoot, RecoveryAccountState,
};
use crate::aggregates::{
    CostBasisTrackerSnapshot, LiquidityTrackerSnapshot, OrderStatsTrackerSnapshot,
    TraderTrackerSnapshot, WelfareTrackerSnapshot,
};
use crate::block::{BlockHeader, SealedBlock, state_sidecar_snapshot_from_resting_orders};
use crate::bridge::{
    BridgeL1Input, BridgeState, BridgeWithdrawalRequest, L1Deposit, L1WithdrawalStatus,
    WithdrawalLeaf,
};
use crate::market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, PriceCandle, PriceCandlePage, PricePoint,
    ResolutionConfig,
};
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::{
    OrderBook, RestingOrder, reservation_snapshots_from_resting_orders,
    validate_restored_account_reservations, validate_restored_reservations,
};
use crate::price_tracker::{PriceTrackerClearingHistorySnapshot, PriceTrackerVolumeSnapshot};
use crate::sequencer::{BlockSequencer, SequencerConfig};

mod auto_resolution;
mod codec;
mod commit;
mod da;
mod fault;
mod import;
mod restore;
mod retention;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;
mod views;
mod wal;

use self::codec::*;
#[cfg(test)]
use self::fault::{StoreFaultInjection, pop_save_block_fault};
use self::restore::{
    AccountStateFence, PersistedCoreCounters, initialize_or_validate_layout,
    read_account_state_fence, read_recovery_metadata, validate_witness_header, write_core_counters,
};
use self::retention::{backfill_history_indexes, prune_historical_block_rows};
use self::tables::*;

pub use self::auto_resolution::{AutoResolutionAction, AutoResolutionRecord};
pub use self::commit::{
    AnalyticsSnapshot, DurableHistoryRowCaps, SequencerSnapshot, Store, StoreError,
};
pub use self::da::{
    DA_FILE_PROVIDER_REF_ENCODING, DA_FILE_PROVIDER_REF_KIND, DA_PAYLOAD_ENCODING, DA_PAYLOAD_KIND,
    DA_PROVIDER_REFS_ENCODING_BYTES, DaArtifact, DaArtifactIntegrityError, DaArtifactLookup,
    DaArtifactManifest, DaManifestLookup, DaProviderRef,
};
#[cfg(test)]
pub(crate) use self::fault::StoreFaultPoint;
pub use self::import::WitnessImportSummary;
pub use self::restore::{AnalyticsRestoredState, RestoredState};
pub use self::retention::{
    AccountHistoryRetention, HistoryPruneReport, HistoryRetentionMeta, HistoryRetentionPolicy,
};
pub use self::wal::ControlPlaneCommand;
