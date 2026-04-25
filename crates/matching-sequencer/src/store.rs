//! Block-boundary persistence via redb plus a qmdb-backed account snapshot.
//!
//! Philosophy: snapshot core state after each block in a single ACID transaction.
//! On crash, we resume from the last committed block plus any bundle submissions
//! that were admitted after it (replayed from the `PENDING_BUNDLES` table). The
//! in-progress solve is lost but its inputs are durable.
//!
//! The account-state boundary is explicit:
//! - qmdb stores account snapshots
//! - redb stores metadata plus the commit fence that declares which qmdb slot
//!   is committed
//!
//! Recovery trusts the redb fence, never "latest qmdb state".
//!
//! Transaction boundary:
//! 1. Write the next account snapshot into the inactive qmdb slot
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
//! - the fenced qmdb slot must contain matching `height` and `next_account_id`
//!
//! Uses MessagePack (rmp-serde) for values: self-describing, binary-stable across
//! schema changes. Adding fields with `#[serde(default)]` is backward-compatible.
//!
//! # Persistence Tiers
//!
//! **Tier 1 (implemented)**: Core state — accounts, markets, groups, block headers,
//! counters, pubkeys, clearing prices, market volumes. Sufficient for crash recovery.
//!
//! **Tier 2 (partial)**: Order state.
//! - Resting order book: implemented (see `RESTING_ORDERS` table).
//! - Mempool: intentionally not persisted (short-lived by design; clients resubmit).
//! - MM inventory / variance: TODO.
//!
//! **Tier 3 (partial)**: Derived views.
//! - Fill history: implemented (see `FILL_HISTORY` table).
//! - Price history and block ring buffer: TODO.

use std::collections::HashMap;
use std::path::Path;

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use sybil_oracle::{DataFeed, MarketStatus};
use tracing::{debug, info, warn};

use crate::account::{AccountId, AccountStore};
use crate::account_storage::{
    AccountSnapshotSlot, AccountStateStore, CommittedAccountState, FencedAccountStorage,
    RecoveryAccountState,
};
use crate::block::BlockHeader;
use crate::market_info::{AccountFillRecord, MarketMetadata};
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::RestingOrder;

// ---------------------------------------------------------------------------
// Table definitions
// ---------------------------------------------------------------------------

/// Markets: market_id (u32) → msgpack(Market)
const MARKETS: TableDefinition<u32, &[u8]> = TableDefinition::new("markets");

/// Market metadata: market_id (u32) → msgpack(MarketMetadata)
const MARKET_META: TableDefinition<u32, &[u8]> = TableDefinition::new("market_meta");

/// Data feeds: feed_id (u64) → msgpack(DataFeed). Holds every registered
/// off-chain signer identity allowed to produce resolution attestations.
/// The redb layout is additive (rmp-serde); no layout version bump needed.
const DATA_FEEDS: TableDefinition<u64, &[u8]> = TableDefinition::new("data_feeds");

/// Market statuses: market_id (u32) → msgpack(MarketStatus)
const MARKET_STATUSES: TableDefinition<u32, &[u8]> = TableDefinition::new("market_statuses");

/// Market groups: group_index (u32) → msgpack(MarketGroup)
const MARKET_GROUPS: TableDefinition<u32, &[u8]> = TableDefinition::new("market_groups");

/// Block headers: height (u64) → msgpack(BlockHeader)
const BLOCK_HEADERS: TableDefinition<u64, &[u8]> = TableDefinition::new("block_headers");

/// Pubkey registry: compressed_point (33 bytes) → account_id (u64)
const PUBKEY_REGISTRY: TableDefinition<&[u8], u64> = TableDefinition::new("pubkey_registry");

/// Last clearing prices: market_id (u32) → msgpack(Vec<Nanos>)
const CLEARING_PRICES: TableDefinition<u32, &[u8]> = TableDefinition::new("clearing_prices");

/// Cumulative market volumes: market_id (u32) -> total traded volume in nanos.
const MARKET_VOLUMES: TableDefinition<u32, u64> = TableDefinition::new("market_volumes");

/// Scalar counters: name → value
const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

/// Resting order book snapshot: single row keyed "snapshot" → msgpack(Vec<RestingOrder>).
/// Rewritten atomically each block.
const RESTING_ORDERS: TableDefinition<&str, &[u8]> = TableDefinition::new("resting_orders");

const KEY_RESTING_ORDERS_SNAPSHOT: &str = "snapshot";

/// Pending bundle submissions: monotonic seq (u64) → msgpack(OrderSubmission).
/// Append-only buffer for MM / multi-market / multi-order submissions that
/// must wait for the block-time solver path. Each admit appends one row.
/// Cleared atomically inside `save_block` when the bundles get consumed into
/// a committed block. On restart, the table is replayed into the actor's
/// in-memory pending queue so nothing submitted with a 200 OK is lost.
const PENDING_BUNDLES: TableDefinition<u64, &[u8]> = TableDefinition::new("pending_bundles");

/// Admit log: monotonic seq (u64) → msgpack(RestingOrder).
/// Append-only log of non-MM single-market admissions that entered the
/// resting book after the last committed block. Each admit appends one row
/// before the 200 OK returns, so a crash between admit and the next block
/// commit doesn't drop orders from `try_admit_direct`. Cleared atomically
/// inside `save_block` when those admissions become part of the next
/// `RESTING_ORDERS` snapshot; restart loads the snapshot and then replays
/// this table on top.
const ADMIT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("admit_log");

/// Per-account fill history: account_id || block_height || order_id →
/// msgpack(AccountFillRecord). The byte key keeps records clustered by
/// account and ordered by block for efficient restoration and future scans.
const FILL_HISTORY: TableDefinition<&[u8], &[u8]> = TableDefinition::new("fill_history");

// Counter keys
const KEY_STORE_LAYOUT_VERSION: &str = "store_layout_version";
const KEY_HEIGHT: &str = "height";
const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
const KEY_NEXT_MARKET_ID: &str = "next_market_id";
const KEY_NEXT_ORDER_ID: &str = "next_order_id";
const KEY_ACCOUNT_STATE_HEIGHT: &str = "account_state_height";
const KEY_ACCOUNT_STATE_SLOT: &str = "account_state_slot";

const STORE_LAYOUT_VERSION: u64 = 1;

fn fill_history_key(account_id: AccountId, record: &AccountFillRecord) -> [u8; 24] {
    let mut key = [0u8; 24];
    key[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    key[8..16].copy_from_slice(&record.block_height.to_be_bytes());
    key[16..24].copy_from_slice(&record.order_id.to_be_bytes());
    key
}

fn account_id_from_fill_history_key(key: &[u8]) -> Option<AccountId> {
    let account_bytes: [u8; 8] = key.get(0..8)?.try_into().ok()?;
    Some(AccountId(u64::from_be_bytes(account_bytes)))
}

// TODO: Tier 2 tables (remaining)
// const MM_STATE: TableDefinition<u32, &[u8]> = TableDefinition::new("mm_state");

// TODO: Tier 3 tables (remaining)
// const PRICE_HISTORY: TableDefinition<u64, &[u8]> = TableDefinition::new("price_history");
// const BLOCKS_FULL: TableDefinition<u64, &[u8]> = TableDefinition::new("blocks_full");

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Persistent store for sequencer state. Wraps a redb database.
pub struct Store {
    db: Database,
    account_state_store: Box<dyn AccountStateStore>,
}

/// State restored from the store on startup.
pub struct RestoredState {
    pub accounts: AccountStore,
    pub markets: MarketSet,
    pub market_groups: Vec<MarketGroup>,
    pub market_statuses: HashMap<MarketId, MarketStatus>,
    pub market_metadata: HashMap<MarketId, MarketMetadata>,
    pub height: u64,
    pub last_header: Option<BlockHeader>,
    pub next_order_id: u64,
    pub pubkey_registry: HashMap<crate::crypto::PublicKey, AccountId>,
    pub last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: HashMap<MarketId, u64>,
    pub resting_orders: Vec<RestingOrder>,
    /// All registered data feeds.
    pub data_feeds: Vec<DataFeed>,
    /// Bundle / MM / multi-market submissions that were admitted after the
    /// last committed block. The actor replays these into its in-memory
    /// pending queue so nothing acknowledged with a 200 OK is dropped by a
    /// crash.
    pub pending_bundles: Vec<crate::sequencer::OrderSubmission>,
    /// Non-MM single-market admissions that went into the resting book
    /// after the last committed block. On restart these are re-inserted
    /// on top of `resting_orders` before the sequencer starts processing.
    pub admit_log: Vec<RestingOrder>,
    /// Full fill history restored from redb.
    pub account_fills: Vec<(AccountId, AccountFillRecord)>,
}

/// Borrowed view of sequencer state needed to persist one block.
/// Constructed by `BlockSequencer::snapshot()` and consumed by `Store::save_block`.
pub struct SequencerSnapshot<'a> {
    pub accounts: &'a AccountStore,
    pub markets: &'a MarketSet,
    pub market_groups: &'a [MarketGroup],
    pub lifecycle: &'a MarketLifecycle,
    pub header: &'a BlockHeader,
    pub next_order_id: u64,
    pub pubkey_registry: &'a HashMap<crate::crypto::PublicKey, AccountId>,
    pub last_clearing_prices: &'a HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: &'a HashMap<MarketId, u64>,
    pub account_fills: Vec<(AccountId, AccountFillRecord)>,
    /// Owned because the snapshot clones the live book — cheap for bounded sizes.
    pub resting_orders: Vec<RestingOrder>,
}

impl Store {
    /// Open (or create) a store at the given path.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let db = Database::create(path)?;
        let qmdb_path = path.with_extension("qmdb");
        std::fs::create_dir_all(&qmdb_path)?;
        let account_state_store =
            Box::new(FencedAccountStorage::open(&qmdb_path)?) as Box<dyn AccountStateStore>;

        // Ensure all tables exist (redb creates on first write, but this
        // makes the schema explicit).
        let txn = db.begin_write()?;
        txn.open_table(MARKETS)?;
        txn.open_table(MARKET_META)?;
        txn.open_table(MARKET_STATUSES)?;
        txn.open_table(MARKET_GROUPS)?;
        txn.open_table(BLOCK_HEADERS)?;
        txn.open_table(PUBKEY_REGISTRY)?;
        txn.open_table(COUNTERS)?;
        txn.open_table(CLEARING_PRICES)?;
        txn.open_table(MARKET_VOLUMES)?;
        txn.open_table(RESTING_ORDERS)?;
        txn.open_table(PENDING_BUNDLES)?;
        txn.open_table(ADMIT_LOG)?;
        txn.open_table(FILL_HISTORY)?;
        txn.open_table(DATA_FEEDS)?;
        txn.commit()?;

        initialize_or_validate_layout(&db)?;

        info!(?path, "store opened");
        Ok(Self {
            db,
            account_state_store,
        })
    }

    /// Save the sequencer state after a block. Single ACID transaction.
    pub async fn save_block(&self, snapshot: SequencerSnapshot<'_>) -> Result<(), StoreError> {
        let current_fence = read_account_state_fence(&self.db)?;
        let next_slot = current_fence
            .map(|fence| fence.slot.inactive())
            .unwrap_or(AccountSnapshotSlot::A);

        // Persist the inactive qmdb slot first. It becomes committed only when the
        // redb transaction below flips the fence to point at it.
        self.account_state_store
            .persist(CommittedAccountState {
                accounts: snapshot.accounts,
                height: snapshot.header.height,
                next_account_id: snapshot.accounts.next_id(),
                slot: next_slot,
            })
            .await?;

        let txn = self.db.begin_write()?;

        // Markets
        {
            let mut table = txn.open_table(MARKETS)?;
            for (id, market) in snapshot.markets.iter_with_ids() {
                let bytes = rmp_serde::to_vec(market)?;
                table.insert(id.0, bytes.as_slice())?;
            }
        }

        // Market metadata + statuses
        {
            let mut meta_table = txn.open_table(MARKET_META)?;
            let mut status_table = txn.open_table(MARKET_STATUSES)?;
            for (&market_id, status) in snapshot.lifecycle.market_statuses() {
                let bytes = rmp_serde::to_vec(status)?;
                status_table.insert(market_id.0, bytes.as_slice())?;
            }
            for (id, _) in snapshot.markets.iter_with_ids() {
                if let Some(meta) = snapshot.lifecycle.market_metadata(*id) {
                    let bytes = rmp_serde::to_vec(meta)?;
                    meta_table.insert(id.0, bytes.as_slice())?;
                }
            }
        }

        // Market groups (groups can be added but not removed; overwrite by index)
        {
            let mut table = txn.open_table(MARKET_GROUPS)?;
            for (i, group) in snapshot.market_groups.iter().enumerate() {
                let bytes = rmp_serde::to_vec(group)?;
                table.insert(i as u32, bytes.as_slice())?;
            }
        }

        // Block header
        {
            let mut table = txn.open_table(BLOCK_HEADERS)?;
            let bytes = rmp_serde::to_vec(snapshot.header)?;
            table.insert(snapshot.header.height, bytes.as_slice())?;
        }

        // Pubkey registry
        {
            let mut table = txn.open_table(PUBKEY_REGISTRY)?;
            for (pubkey, account_id) in snapshot.pubkey_registry {
                let point = pubkey.compressed_bytes();
                table.insert(point.as_slice(), account_id.0)?;
            }
        }

        // Clearing prices
        {
            let mut table = txn.open_table(CLEARING_PRICES)?;
            for (&market_id, prices) in snapshot.last_clearing_prices {
                let bytes = rmp_serde::to_vec(prices)?;
                table.insert(market_id.0, bytes.as_slice())?;
            }
        }

        // Market volumes
        {
            let mut table = txn.open_table(MARKET_VOLUMES)?;
            for (&market_id, &volume) in snapshot.market_volumes {
                table.insert(market_id.0, volume)?;
            }
        }

        // Resting order book snapshot
        {
            let mut table = txn.open_table(RESTING_ORDERS)?;
            let bytes = rmp_serde::to_vec(&snapshot.resting_orders)?;
            table.insert(KEY_RESTING_ORDERS_SNAPSHOT, bytes.as_slice())?;
        }

        // Fill history. Records are cumulative; re-inserting the full
        // snapshot is idempotent because the key is account/block/order.
        {
            let mut table = txn.open_table(FILL_HISTORY)?;
            for (account_id, record) in &snapshot.account_fills {
                let key = fill_history_key(*account_id, record);
                let bytes = rmp_serde::to_vec(record)?;
                table.insert(key.as_slice(), bytes.as_slice())?;
            }
        }

        // Data feeds
        {
            let mut table = txn.open_table(DATA_FEEDS)?;
            for feed in snapshot.lifecycle.feeds().iter() {
                let bytes = rmp_serde::to_vec(feed)?;
                table.insert(feed.id.0, bytes.as_slice())?;
            }
        }

        // Clear the pending-bundles buffer: everything admitted up to this
        // block has now been consumed (or rejected and logged into the
        // block's witness), so the recovery replay set resets atomically
        // with the rest of the block commit.
        {
            let mut table = txn.open_table(PENDING_BUNDLES)?;
            table.retain(|_, _| false)?;
        }

        // Same story for the admit log: non-MM admits from the last cycle
        // are now encoded in the RESTING_ORDERS snapshot above, so drop
        // the incremental log atomically in this txn.
        {
            let mut table = txn.open_table(ADMIT_LOG)?;
            table.retain(|_, _| false)?;
        }

        // Counters
        {
            let mut table = txn.open_table(COUNTERS)?;
            write_core_counters(
                &mut table,
                PersistedCoreCounters {
                    height: snapshot.header.height,
                    next_account_id: snapshot.accounts.next_id(),
                    next_market_id: snapshot.markets.next_id() as u64,
                    next_order_id: snapshot.next_order_id,
                    account_state_fence: AccountStateFence {
                        height: snapshot.header.height,
                        slot: next_slot,
                    },
                },
            )?;
        }

        txn.commit()?;
        debug!(height = snapshot.header.height, "block persisted");
        Ok(())
    }

    /// Load state from the store. Returns None if the store is empty (fresh start).
    pub async fn load_state(&self) -> Result<Option<RestoredState>, StoreError> {
        let txn = self.db.begin_read()?;
        let Some(recovery_metadata) = read_recovery_metadata(&txn)? else {
            return Ok(None);
        };

        let accounts_map = self
            .account_state_store
            .recover(recovery_metadata.account_state)
            .await?;
        let num_accounts = accounts_map.len();
        let accounts = AccountStore::restore(accounts_map, recovery_metadata.next_account_id);

        // Markets
        let markets = {
            let table = txn.open_table(MARKETS)?;
            let mut market_map = HashMap::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let market: matching_engine::Market = rmp_serde::from_slice(value.value())?;
                market_map.insert(market.id, market);
            }
            MarketSet::restore(market_map, recovery_metadata.next_market_id)
        };

        // Market groups
        let market_groups = {
            let table = txn.open_table(MARKET_GROUPS)?;
            let mut groups = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let group: MarketGroup = rmp_serde::from_slice(value.value())?;
                groups.push(group);
            }
            groups
        };

        // Market statuses
        let market_statuses = {
            let table = txn.open_table(MARKET_STATUSES)?;
            let mut statuses = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let status: MarketStatus = rmp_serde::from_slice(value.value())?;
                statuses.insert(MarketId(key.value()), status);
            }
            statuses
        };

        // Market metadata
        let market_metadata = {
            let table = txn.open_table(MARKET_META)?;
            let mut meta = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let metadata: MarketMetadata = rmp_serde::from_slice(value.value())?;
                meta.insert(MarketId(key.value()), metadata);
            }
            meta
        };

        // Last block header
        let last_header = {
            let table = txn.open_table(BLOCK_HEADERS)?;
            match table.get(recovery_metadata.height)? {
                Some(value) => {
                    let header: BlockHeader = rmp_serde::from_slice(value.value())?;
                    Some(header)
                }
                None => None,
            }
        };

        // Pubkey registry
        let pubkey_registry = {
            let table = txn.open_table(PUBKEY_REGISTRY)?;
            let mut registry = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let bytes = key.value();
                if let Some(pubkey) = crate::crypto::PublicKey::from_compressed_bytes(bytes) {
                    registry.insert(pubkey, AccountId(value.value()));
                } else {
                    warn!("invalid pubkey in store, skipping");
                }
            }
            registry
        };

        // Clearing prices
        let last_clearing_prices = {
            let table = txn.open_table(CLEARING_PRICES)?;
            let mut prices = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let price_vec: Vec<Nanos> = rmp_serde::from_slice(value.value())?;
                prices.insert(MarketId(key.value()), price_vec);
            }
            prices
        };

        let market_volumes = {
            let table = txn.open_table(MARKET_VOLUMES)?;
            let mut volumes = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                volumes.insert(MarketId(key.value()), value.value());
            }
            volumes
        };

        let resting_orders: Vec<RestingOrder> = {
            let table = txn.open_table(RESTING_ORDERS)?;
            match table.get(KEY_RESTING_ORDERS_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => Vec::new(),
            }
        };

        let data_feeds: Vec<DataFeed> = {
            let table = txn.open_table(DATA_FEEDS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let pending_bundles: Vec<crate::sequencer::OrderSubmission> = {
            let table = txn.open_table(PENDING_BUNDLES)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let admit_log: Vec<RestingOrder> = {
            let table = txn.open_table(ADMIT_LOG)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let account_fills: Vec<(AccountId, AccountFillRecord)> = {
            let table = txn.open_table(FILL_HISTORY)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let Some(account_id) = account_id_from_fill_history_key(key.value()) else {
                    warn!("invalid fill history key in store, skipping");
                    continue;
                };
                out.push((account_id, rmp_serde::from_slice(value.value())?));
            }
            out
        };

        info!(
            height = recovery_metadata.height,
            accounts = num_accounts,
            markets = markets.len(),
            groups = market_groups.len(),
            clearing_prices = last_clearing_prices.len(),
            resting_orders = resting_orders.len(),
            pending_bundles = pending_bundles.len(),
            admit_log = admit_log.len(),
            account_fills = account_fills.len(),
            data_feeds = data_feeds.len(),
            "state restored from store"
        );

        Ok(Some(RestoredState {
            accounts,
            markets,
            market_groups,
            market_statuses,
            market_metadata,
            height: recovery_metadata.height,
            last_header,
            next_order_id: recovery_metadata.next_order_id,
            pubkey_registry,
            last_clearing_prices,
            market_volumes,
            resting_orders,
            data_feeds,
            pending_bundles,
            admit_log,
            account_fills,
        }))
    }

    /// Append one pending bundle submission to the durable recovery log.
    ///
    /// Called by the actor on every admit that routes to the in-memory
    /// pending queue (MM-constrained, multi-order, or multi-market orders).
    /// The row is cleared atomically inside `save_block` when the bundle is
    /// consumed into a committed block. The next-seq is derived from the
    /// current table max so restart-then-admit doesn't collide with the
    /// replayed rows that are still in memory.
    pub async fn append_pending_bundle(
        &self,
        submission: &crate::sequencer::OrderSubmission,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(submission)?;
        let txn = self.db.begin_write()?;
        let next_seq = {
            let table = txn.open_table(PENDING_BUNDLES)?;
            let last_key = table
                .iter()?
                .next_back()
                .transpose()?
                .map(|(k, _)| k.value());
            last_key.map(|k| k + 1).unwrap_or(0)
        };
        {
            let mut table = txn.open_table(PENDING_BUNDLES)?;
            table.insert(next_seq, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Append one `RestingOrder` to the admit-log WAL.
    ///
    /// Called by the actor right after `try_admit_direct` inserts a non-MM
    /// admit into the live resting book; the 200 OK only returns once this
    /// row is committed to redb. Rows are cleared atomically by `save_block`
    /// once the admit is rolled into the next `RESTING_ORDERS` snapshot.
    pub async fn append_admit_log(&self, resting: &RestingOrder) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(resting)?;
        let txn = self.db.begin_write()?;
        let next_seq = {
            let table = txn.open_table(ADMIT_LOG)?;
            let last_key = table
                .iter()?
                .next_back()
                .transpose()?
                .map(|(k, _)| k.value());
            last_key.map(|k| k + 1).unwrap_or(0)
        };
        {
            let mut table = txn.open_table(ADMIT_LOG)?;
            table.insert(next_seq, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("redb: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb database: {0}")]
    Database(#[from] redb::DatabaseError),
    #[error("redb transaction: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("redb table: {0}")]
    Table(#[from] redb::TableError),
    #[error("redb storage: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("redb commit: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("msgpack encode: {0}")]
    MsgpackEncode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode: {0}")]
    MsgpackDecode(#[from] rmp_serde::decode::Error),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("qmdb: {0}")]
    Qmdb(String),
    #[error("unsupported store layout: {0}")]
    UnsupportedLayout(String),
    #[error("corrupt store layout: {0}")]
    CorruptLayout(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AccountStateFence {
    height: u64,
    slot: AccountSnapshotSlot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PersistedCoreCounters {
    height: u64,
    next_account_id: u64,
    next_market_id: u64,
    next_order_id: u64,
    account_state_fence: AccountStateFence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecoveryMetadata {
    height: u64,
    next_account_id: u64,
    next_market_id: u32,
    next_order_id: u64,
    account_state: RecoveryAccountState,
}

fn initialize_or_validate_layout(db: &Database) -> Result<(), StoreError> {
    let txn = db.begin_read()?;
    let counters = txn.open_table(COUNTERS)?;
    match counters.get(KEY_STORE_LAYOUT_VERSION)? {
        Some(value) => {
            let version = value.value();
            if version != STORE_LAYOUT_VERSION {
                return Err(StoreError::UnsupportedLayout(format!(
                    "expected layout version {}, found {}",
                    STORE_LAYOUT_VERSION, version
                )));
            }
        }
        None => {
            let has_existing_state = counters.get(KEY_HEIGHT)?.is_some()
                || counters.get(KEY_ACCOUNT_STATE_HEIGHT)?.is_some();
            drop(counters);
            drop(txn);

            if has_existing_state {
                return Err(StoreError::UnsupportedLayout(
                    "legacy store layout detected; this account-state layout requires a fresh store"
                        .to_string(),
                ));
            }

            let txn = db.begin_write()?;
            let mut counters = txn.open_table(COUNTERS)?;
            counters.insert(KEY_STORE_LAYOUT_VERSION, STORE_LAYOUT_VERSION)?;
            drop(counters);
            txn.commit()?;
        }
    }
    Ok(())
}

fn read_account_state_fence(db: &Database) -> Result<Option<AccountStateFence>, StoreError> {
    let txn = db.begin_read()?;
    let counters = txn.open_table(COUNTERS)?;
    let Some(height) = counters
        .get(KEY_ACCOUNT_STATE_HEIGHT)?
        .map(|value| value.value())
    else {
        return Ok(None);
    };
    let slot = required_counter(&counters, KEY_ACCOUNT_STATE_SLOT)?;
    Ok(Some(AccountStateFence {
        height,
        slot: AccountSnapshotSlot::decode(slot)?,
    }))
}

fn read_recovery_metadata(
    txn: &redb::ReadTransaction,
) -> Result<Option<RecoveryMetadata>, StoreError> {
    let counters = txn.open_table(COUNTERS)?;
    let Some(height) = counters.get(KEY_HEIGHT)?.map(|value| value.value()) else {
        return Ok(None);
    };

    let next_account_id = counters
        .get(KEY_NEXT_ACCOUNT_ID)?
        .map(|value| value.value())
        .unwrap_or(0);
    let account_state_height = required_counter(&counters, KEY_ACCOUNT_STATE_HEIGHT)?;
    let account_state_slot =
        AccountSnapshotSlot::decode(required_counter(&counters, KEY_ACCOUNT_STATE_SLOT)?)?;

    if account_state_height != height {
        return Err(StoreError::CorruptLayout(format!(
            "metadata height mismatch: height={} account_state_height={}",
            height, account_state_height
        )));
    }

    Ok(Some(RecoveryMetadata {
        height,
        next_account_id,
        next_market_id: counters
            .get(KEY_NEXT_MARKET_ID)?
            .map(|value| value.value())
            .unwrap_or(0) as u32,
        next_order_id: counters
            .get(KEY_NEXT_ORDER_ID)?
            .map(|value| value.value())
            .unwrap_or(1),
        account_state: RecoveryAccountState {
            height: account_state_height,
            next_account_id,
            slot: account_state_slot,
        },
    }))
}

fn write_core_counters(
    counters: &mut redb::Table<&str, u64>,
    persisted: PersistedCoreCounters,
) -> Result<(), StoreError> {
    counters.insert(KEY_HEIGHT, persisted.height)?;
    counters.insert(KEY_NEXT_ACCOUNT_ID, persisted.next_account_id)?;
    counters.insert(KEY_NEXT_MARKET_ID, persisted.next_market_id)?;
    counters.insert(KEY_NEXT_ORDER_ID, persisted.next_order_id)?;
    counters.insert(
        KEY_ACCOUNT_STATE_HEIGHT,
        persisted.account_state_fence.height,
    )?;
    counters.insert(
        KEY_ACCOUNT_STATE_SLOT,
        persisted.account_state_fence.slot.encode(),
    )?;
    Ok(())
}

fn required_counter(
    counters: &redb::ReadOnlyTable<&str, u64>,
    key: &'static str,
) -> Result<u64, StoreError> {
    counters
        .get(key)?
        .map(|value| value.value())
        .ok_or_else(|| StoreError::CorruptLayout(format!("missing required counter `{key}`")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use matching_engine::MarketSet;
    use redb::{Database, TableDefinition};

    use super::*;
    use crate::account::AccountStore;
    use crate::block::BlockHeader;
    use crate::market_lifecycle::MarketLifecycle;
    use crate::AdminOracle;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sybil-{prefix}-{}-{unique}.redb",
            std::process::id()
        ))
    }

    fn sample_header(height: u64) -> BlockHeader {
        BlockHeader {
            height,
            parent_hash: [height as u8; 32],
            state_root: [height as u8; 32],
            order_count: 0,
            fill_count: 0,
            timestamp_ms: height * 1000,
        }
    }

    /// Owns the empty defaults for `SequencerSnapshot` references so test code
    /// doesn't have to repeat the ceremony on every call site.
    struct TestEnv {
        empty_pk: HashMap<crate::crypto::PublicKey, AccountId>,
        empty_prices: HashMap<MarketId, Vec<Nanos>>,
        empty_volumes: HashMap<MarketId, u64>,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                empty_pk: HashMap::new(),
                empty_prices: HashMap::new(),
                empty_volumes: HashMap::new(),
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn snapshot<'a>(
            &'a self,
            accounts: &'a AccountStore,
            markets: &'a MarketSet,
            lifecycle: &'a MarketLifecycle,
            header: &'a BlockHeader,
            next_order_id: u64,
            market_volumes: Option<&'a HashMap<MarketId, u64>>,
            resting_orders: Vec<RestingOrder>,
        ) -> SequencerSnapshot<'a> {
            SequencerSnapshot {
                accounts,
                markets,
                market_groups: &[],
                lifecycle,
                header,
                next_order_id,
                pubkey_registry: &self.empty_pk,
                last_clearing_prices: &self.empty_prices,
                market_volumes: market_volumes.unwrap_or(&self.empty_volumes),
                account_fills: Vec::new(),
                resting_orders,
            }
        }

        fn snapshot_with_fills<'a>(
            &'a self,
            accounts: &'a AccountStore,
            markets: &'a MarketSet,
            lifecycle: &'a MarketLifecycle,
            header: &'a BlockHeader,
            account_fills: Vec<(AccountId, AccountFillRecord)>,
        ) -> SequencerSnapshot<'a> {
            SequencerSnapshot {
                accounts,
                markets,
                market_groups: &[],
                lifecycle,
                header,
                next_order_id: 1,
                pubkey_registry: &self.empty_pk,
                last_clearing_prices: &self.empty_prices,
                market_volumes: &self.empty_volumes,
                account_fills,
                resting_orders: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn test_store_restores_latest_committed_accounts() {
        let path = temp_db_path("store-restore");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();

        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(100);
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                1,
                None,
                vec![],
            ))
            .await
            .unwrap();

        accounts.get_mut(account_id).unwrap().balance = 200;
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(2),
                1,
                None,
                vec![],
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.height, 2);
        assert_eq!(restored.accounts.get(account_id).unwrap().balance, 200);
    }

    #[tokio::test]
    async fn test_store_restores_market_volumes() {
        let path = temp_db_path("store-market-volumes");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Will it rain?");
        let env = TestEnv::new();
        let accounts = AccountStore::new();

        let volumes = HashMap::from([(market_id, 42_000_000_000u64)]);
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                1,
                Some(&volumes),
                vec![],
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(
            restored.market_volumes.get(&market_id),
            Some(&42_000_000_000)
        );
    }

    #[tokio::test]
    async fn test_store_restores_resting_orders() {
        use crate::order_book::OrderBook;
        use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-resting-orders");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let env = TestEnv::new();

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");

        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);

        let mut book = OrderBook::new(10);
        let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
        book.accept(order, aid, accounts.get(aid).unwrap(), 1)
            .unwrap();
        let expected_reserved = book.reserved_balance(aid);
        assert!(expected_reserved > 0);
        let snapshot = book.snapshot();
        assert_eq!(snapshot.len(), 1);

        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                2,
                None,
                snapshot,
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.resting_orders.len(), 1);
        assert_eq!(restored.resting_orders[0].account_id, aid);
        assert_eq!(restored.resting_orders[0].order.id, 1);
        assert_eq!(
            restored.resting_orders[0].reserved_balance,
            expected_reserved
        );
        assert_eq!(restored.resting_orders[0].created_at, 1);

        let rebuilt = OrderBook::restore(restored.resting_orders, 10);
        assert_eq!(rebuilt.reserved_balance(aid), expected_reserved);
        assert_eq!(rebuilt.len(), 1);
    }

    #[tokio::test]
    async fn test_store_clears_resting_orders_when_snapshot_empty() {
        use crate::order_book::OrderBook;
        use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-resting-orders-empty");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let env = TestEnv::new();

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);

        // Block 1: save with a resting order.
        let mut book = OrderBook::new(10);
        let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
        book.accept(order, aid, accounts.get(aid).unwrap(), 1)
            .unwrap();
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                2,
                None,
                book.snapshot(),
            ))
            .await
            .unwrap();

        // Block 2: save with an empty snapshot (order filled/cancelled/expired).
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(2),
                2,
                None,
                vec![],
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert!(restored.resting_orders.is_empty());
    }

    #[tokio::test]
    async fn test_store_roundtrips_account_fill_history() {
        let path = temp_db_path("store-fill-history");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle.clone());
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");
        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(100);
        let env = TestEnv::new();

        let fill = AccountFillRecord {
            order_id: 42,
            fill_qty: 7,
            fill_price: 600_000_000,
            block_height: 1,
            timestamp_ms: 1_000,
            position_deltas: vec![(market_id, 0, 7)],
        };

        store
            .save_block(env.snapshot_with_fills(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                vec![(account_id, fill.clone())],
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.account_fills, vec![(account_id, fill.clone())]);

        let seq = crate::sequencer::BlockSequencer::restore(
            restored,
            oracle,
            crate::sequencer::SequencerConfig::default(),
        );
        assert_eq!(
            seq.account_fills(account_id, Some(market_id), 10, 0),
            vec![fill]
        );
    }

    #[tokio::test]
    async fn test_store_persists_fill_recorder_snapshot_from_committed_block() {
        use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
        use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-fill-recorder-snapshot");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");
        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((market_id, 0), 10);

        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle.clone(),
            SequencerConfig::default(),
        );
        seq.produce_block(
            vec![
                OrderSubmission {
                    account_id: buyer,
                    orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 5)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: seller,
                    orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 5)],
                    mm_constraint: None,
                },
            ],
            1_000,
        );

        assert!(
            !seq.account_fills(buyer, None, 10, 0).is_empty(),
            "sanity check: block should record buyer fills before persistence"
        );
        store.save_block(seq.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
        let fills = restored_seq.account_fills(buyer, Some(market_id), 10, 0);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].fill_qty, 5);
        assert_eq!(fills[0].block_height, 1);
    }

    #[test]
    fn test_open_rejects_legacy_store_layout() {
        const TEST_COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

        let path = temp_db_path("legacy-layout");
        let db = Database::create(&path).unwrap();
        let txn = db.begin_write().unwrap();
        let mut counters = txn.open_table(TEST_COUNTERS).unwrap();
        counters.insert(KEY_HEIGHT, 1).unwrap();
        drop(counters);
        txn.commit().unwrap();
        drop(db);

        match Store::open(&path) {
            Ok(_) => panic!("expected legacy store layout to be rejected"),
            Err(StoreError::UnsupportedLayout(_)) => {}
            Err(error) => panic!("expected unsupported layout error, got {error:?}"),
        }
    }

    #[tokio::test]
    async fn test_store_roundtrips_admit_log_and_replays_on_restore() {
        use crate::order_book::OrderBook;
        use crate::sequencer::{BlockSequencer, SequencerConfig};
        use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-admit-log");
        let store = Store::open(&path).unwrap();

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");

        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle.clone());
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let env = TestEnv::new();

        // Baseline block with no admits, so load_state has a metadata row.
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                1,
                None,
                Vec::new(),
            ))
            .await
            .unwrap();

        // Simulate a non-MM admit: build what `OrderBook::accept` would
        // produce, then append to the WAL directly.
        let mut book = OrderBook::new(10);
        let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
        let accepted = book
            .accept(order, aid, accounts.get(aid).unwrap(), 1)
            .unwrap();
        store
            .append_admit_log(&accepted.resting_order)
            .await
            .unwrap();

        // Load + restore: the order must live again in the book, with its
        // reservation correctly accounted for.
        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.admit_log.len(), 1);
        assert!(restored.resting_orders.is_empty());

        let seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
        assert_eq!(
            seq.pending_orders_info(Some(aid)).len(),
            1,
            "replayed admit must be visible on the restored resting book"
        );

        // save_block clears the admit log atomically.
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(2),
                2,
                None,
                Vec::new(),
            ))
            .await
            .unwrap();

        let restored_after = store.load_state().await.unwrap().unwrap();
        assert!(restored_after.admit_log.is_empty());
    }

    #[tokio::test]
    async fn test_store_roundtrips_data_feeds() {
        use sybil_oracle::FeedPubkey;

        let path = temp_db_path("store-data-feeds");
        let store = Store::open(&path).unwrap();

        let oracle = Arc::new(AdminOracle::new());
        let mut lifecycle = MarketLifecycle::new(oracle);
        lifecycle.register_feed(FeedPubkey(vec![1u8; 33]), "admin".into(), 100);
        lifecycle.register_feed(FeedPubkey(vec![2u8; 33]), "polymarket_mirror".into(), 200);

        let markets = MarketSet::new();
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                1,
                None,
                Vec::new(),
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.data_feeds.len(), 2);
        let names: Vec<_> = restored.data_feeds.iter().map(|f| f.name.clone()).collect();
        assert!(names.contains(&"admin".to_string()));
        assert!(names.contains(&"polymarket_mirror".to_string()));
    }

    #[tokio::test]
    async fn test_store_roundtrips_pending_bundles() {
        use crate::sequencer::OrderSubmission;
        use matching_engine::{
            mm_constraint::{MmConstraint, MmId, MmSide},
            outcome_buy, MarketSet, NANOS_PER_DOLLAR,
        };

        let path = temp_db_path("store-pending-bundles");
        let store = Store::open(&path).unwrap();

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Test");

        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        // Commit a baseline block so `load_state()` has something to return.
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                1,
                None,
                Vec::new(),
            ))
            .await
            .unwrap();

        let order = outcome_buy(&markets, 7, market_id, 0, NANOS_PER_DOLLAR / 2, 3);
        let mut constraint = MmConstraint::new(MmId(1), 5 * NANOS_PER_DOLLAR);
        constraint.add_order(7, MmSide::BuyYes);
        let sub = OrderSubmission {
            account_id: AccountId(42),
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        store.append_pending_bundle(&sub).await.unwrap();
        store.append_pending_bundle(&sub).await.unwrap();

        let restored_before = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored_before.pending_bundles.len(), 2);
        assert_eq!(restored_before.pending_bundles[0].account_id, AccountId(42));

        // save_block must clear the pending table atomically with the commit.
        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(2),
                1,
                None,
                Vec::new(),
            ))
            .await
            .unwrap();

        let restored_after = store.load_state().await.unwrap().unwrap();
        assert!(restored_after.pending_bundles.is_empty());
    }
}
