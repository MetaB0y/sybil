//! Block-boundary persistence via redb plus a qmdb-backed account snapshot.
//!
//! Philosophy: snapshot core state after each block in a single ACID transaction.
//! On crash, we resume from the last committed block. Anything in-flight (mempool,
//! current solve) is lost — clients resubmit within seconds.
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
//! counters, pubkeys. Sufficient for crash recovery.
//!
//! **Tier 2 (TODO)**: Order state — pending orders, mempool, MM inventory/variance.
//! Needed for seamless order continuity across restarts.
//!
//! **Tier 3 (TODO)**: Derived views — fill history, price history, block ring buffer.
//! Reconstructable from blocks but expensive to rebuild.

use std::collections::HashMap;
use std::path::Path;

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use redb::{Database, ReadableTable, TableDefinition};
use sybil_oracle::MarketStatus;
use tracing::{debug, info, warn};

use crate::account::{AccountId, AccountStore};
use crate::account_storage::{
    AccountSnapshotSlot, AccountStateStore, CommittedAccountState, FencedAccountStorage,
    RecoveryAccountState,
};
use crate::block::BlockHeader;
use crate::market_info::MarketMetadata;
use crate::market_lifecycle::MarketLifecycle;

// ---------------------------------------------------------------------------
// Table definitions
// ---------------------------------------------------------------------------

/// Markets: market_id (u32) → msgpack(Market)
const MARKETS: TableDefinition<u32, &[u8]> = TableDefinition::new("markets");

/// Market metadata: market_id (u32) → msgpack(MarketMetadata)
const MARKET_META: TableDefinition<u32, &[u8]> = TableDefinition::new("market_meta");

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

/// Scalar counters: name → value
const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

// Counter keys
const KEY_STORE_LAYOUT_VERSION: &str = "store_layout_version";
const KEY_HEIGHT: &str = "height";
const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
const KEY_NEXT_MARKET_ID: &str = "next_market_id";
const KEY_NEXT_ORDER_ID: &str = "next_order_id";
const KEY_ACCOUNT_STATE_HEIGHT: &str = "account_state_height";
const KEY_ACCOUNT_STATE_SLOT: &str = "account_state_slot";

const STORE_LAYOUT_VERSION: u64 = 1;

// TODO: Tier 2 tables
// const PENDING_ORDERS: TableDefinition<u64, &[u8]> = TableDefinition::new("pending_orders");
// const MM_STATE: TableDefinition<u32, &[u8]> = TableDefinition::new("mm_state");

// TODO: Tier 3 tables
// const FILL_HISTORY: TableDefinition<u64, &[u8]> = TableDefinition::new("fill_history");
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
        txn.commit()?;

        initialize_or_validate_layout(&db)?;

        info!(?path, "store opened");
        Ok(Self {
            db,
            account_state_store,
        })
    }

    /// Save the sequencer state after a block. Single ACID transaction.
    pub async fn save_block(
        &self,
        accounts: &AccountStore,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
        lifecycle: &MarketLifecycle,
        header: &BlockHeader,
        next_order_id: u64,
        pubkey_registry: &HashMap<crate::crypto::PublicKey, AccountId>,
        last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> Result<(), StoreError> {
        let current_fence = read_account_state_fence(&self.db)?;
        let next_slot = current_fence
            .map(|fence| fence.slot.inactive())
            .unwrap_or(AccountSnapshotSlot::A);

        // Persist the inactive qmdb slot first. It becomes committed only when the
        // redb transaction below flips the fence to point at it.
        self.account_state_store
            .persist(CommittedAccountState {
                accounts,
                height: header.height,
                next_account_id: accounts.next_id(),
                slot: next_slot,
            })
            .await?;

        let txn = self.db.begin_write()?;

        // Markets
        {
            let mut table = txn.open_table(MARKETS)?;
            for (id, market) in markets.iter_with_ids() {
                let bytes = rmp_serde::to_vec(market)?;
                table.insert(id.0, bytes.as_slice())?;
            }
        }

        // Market metadata + statuses
        {
            let mut meta_table = txn.open_table(MARKET_META)?;
            let mut status_table = txn.open_table(MARKET_STATUSES)?;
            for (&market_id, status) in lifecycle.market_statuses() {
                let bytes = rmp_serde::to_vec(status)?;
                status_table.insert(market_id.0, bytes.as_slice())?;
            }
            // Metadata for all markets we know about
            for (id, _) in markets.iter_with_ids() {
                if let Some(meta) = lifecycle.market_metadata(*id) {
                    let bytes = rmp_serde::to_vec(meta)?;
                    meta_table.insert(id.0, bytes.as_slice())?;
                }
            }
        }

        // Market groups
        {
            let mut table = txn.open_table(MARKET_GROUPS)?;
            // Clear old groups (groups can be added but not removed, so this is fine)
            // Actually, just overwrite by index
            for (i, group) in market_groups.iter().enumerate() {
                let bytes = rmp_serde::to_vec(group)?;
                table.insert(i as u32, bytes.as_slice())?;
            }
        }

        // Block header
        {
            let mut table = txn.open_table(BLOCK_HEADERS)?;
            let bytes = rmp_serde::to_vec(header)?;
            table.insert(header.height, bytes.as_slice())?;
        }

        // Pubkey registry
        {
            let mut table = txn.open_table(PUBKEY_REGISTRY)?;
            for (pubkey, account_id) in pubkey_registry {
                let point = pubkey.compressed_bytes();
                table.insert(point.as_slice(), account_id.0)?;
            }
        }

        // Clearing prices
        {
            let mut table = txn.open_table(CLEARING_PRICES)?;
            for (&market_id, prices) in last_clearing_prices {
                let bytes = rmp_serde::to_vec(prices)?;
                table.insert(market_id.0, bytes.as_slice())?;
            }
        }

        // Counters
        {
            let mut table = txn.open_table(COUNTERS)?;
            write_core_counters(
                &mut table,
                PersistedCoreCounters {
                    height: header.height,
                    next_account_id: accounts.next_id(),
                    next_market_id: markets.next_id() as u64,
                    next_order_id,
                    account_state_fence: AccountStateFence {
                        height: header.height,
                        slot: next_slot,
                    },
                },
            )?;
        }

        txn.commit()?;
        debug!(height = header.height, "block persisted");
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

        info!(
            height = recovery_metadata.height,
            accounts = num_accounts,
            markets = markets.len(),
            groups = market_groups.len(),
            clearing_prices = last_clearing_prices.len(),
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
        }))
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

    #[tokio::test]
    async fn test_store_restores_latest_committed_accounts() {
        let path = temp_db_path("store-restore");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();

        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(100);
        store
            .save_block(
                &accounts,
                &markets,
                &[],
                &lifecycle,
                &sample_header(1),
                1,
                &HashMap::new(),
                &HashMap::new(),
            )
            .await
            .unwrap();

        accounts.get_mut(account_id).unwrap().balance = 200;
        store
            .save_block(
                &accounts,
                &markets,
                &[],
                &lifecycle,
                &sample_header(2),
                1,
                &HashMap::new(),
                &HashMap::new(),
            )
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.height, 2);
        assert_eq!(restored.accounts.get(account_id).unwrap().balance, 200);
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
}
