//! Block-boundary persistence via redb plus a qmdb-backed account snapshot.
//!
//! Philosophy: snapshot core state after each block in a single ACID transaction.
//! On crash, we resume from the last committed block plus every mutation
//! accepted after it, replayed in exact actor order from the global
//! `ACKNOWLEDGED_WRITES` table. The in-progress solve is lost but its accepted
//! inputs are durable.
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
//! **Tier 3**: Serving artifacts.
//! - Product history leaves the commit fence through `PRODUCT_HISTORY_OUTBOX` and is
//!   projected by `sybil-history`; there are no query projections here.
//! - Canonical replay blocks and paired DA artifacts form a separate bounded
//!   local archive.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use matching_engine::{Market, MarketGroup, MarketId, MarketSet, Nanos};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, MarketStatus, OracleSource, ResolutionRecord, ResolutionTemplate,
    SignedAttestation,
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
#[cfg(test)]
use crate::bridge::BridgeL1Input;
use crate::bridge::{
    BridgeState, BridgeWithdrawalRequest, L1Deposit, L1WithdrawalStatus, WithdrawalLeaf,
};
use crate::market_info::{AccountFillRecord, MarketMetadata, ResolutionConfig};
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::{
    OrderBook, RestingOrder, reservation_snapshots_from_resting_orders,
    validate_restored_account_reservations, validate_restored_reservations,
};
use crate::price_tracker::{RollingPriceAnchorsSnapshot, RollingVolumeSnapshot};
use crate::sequencer::{BlockSequencer, SequencerConfig};

/// One exact portable proof-job payload from the durable sequencer outbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofJobOutboxEntry {
    pub height: u64,
    pub digest: [u8; 32],
    pub bytes: Vec<u8>,
    pub acknowledged: bool,
}

mod codec;
mod commit;
mod da;
mod fault;
mod history_outbox;
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
use self::retention::prune_historical_block_rows;
use self::tables::*;

pub use self::commit::{AnalyticsSnapshot, SequencerSnapshot, Store, StoreError};
pub use self::da::{
    DA_FILE_PROVIDER_REF_ENCODING, DA_FILE_PROVIDER_REF_KIND, DA_PAYLOAD_ENCODING, DA_PAYLOAD_KIND,
    DA_PROVIDER_REFS_ENCODING_BYTES, DaArtifact, DaArtifactIntegrityError, DaArtifactLookup,
    DaArtifactManifest, DaManifestLookup, DaProviderRef,
};
#[cfg(test)]
pub(crate) use self::fault::StoreFaultPoint;
pub use self::history_outbox::{ProductHistoryOutboxAck, ProductHistoryOutboxStats};
pub use self::import::WitnessImportSummary;
pub use self::restore::{AnalyticsRestoredState, RestoredState};
pub use self::retention::{
    AcknowledgedProofJobPruneReport, AcknowledgedProofJobRetentionPolicy, CanonicalArchiveMeta,
    CanonicalArchivePruneReport, CanonicalArchiveRetentionPolicy,
};
pub use self::wal::{
    ACKNOWLEDGED_WRITE_ENVELOPE_VERSION, AcknowledgedWrite, ControlPlaneCommand,
    SequencedAcknowledgedWrite,
};
