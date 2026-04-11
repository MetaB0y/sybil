//! Block-boundary persistence via redb plus a qmdb-backed account snapshot.
//!
//! Philosophy: snapshot core state after each block in a single ACID transaction.
//! On crash, we resume from the last committed block. Anything in-flight (mempool,
//! current solve) is lost — clients resubmit within seconds.
//!
//! Account persistence is in transition: we dual-write accounts to qmdb and redb.
//! This gives us a real authenticated-state boundary to exercise now, while redb
//! remains the crash-recovery fallback until we finish removing the cross-store
//! consistency gap.
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

use crate::account::{Account, AccountId, AccountStore};
use crate::block::BlockHeader;
use crate::market_info::MarketMetadata;
use crate::market_lifecycle::MarketLifecycle;
use crate::qmdb_accounts::QmdbAccounts;

// ---------------------------------------------------------------------------
// Table definitions
// ---------------------------------------------------------------------------

/// Account state: account_id (u64) → msgpack(Account)
const ACCOUNTS: TableDefinition<u64, &[u8]> = TableDefinition::new("accounts");

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
const KEY_HEIGHT: &str = "height";
const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
const KEY_NEXT_MARKET_ID: &str = "next_market_id";
const KEY_NEXT_ORDER_ID: &str = "next_order_id";

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
    qmdb_accounts: QmdbAccounts,
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
        let qmdb_accounts = QmdbAccounts::open(&qmdb_path)?;

        // Ensure all tables exist (redb creates on first write, but this
        // makes the schema explicit).
        let txn = db.begin_write()?;
        txn.open_table(ACCOUNTS)?;
        txn.open_table(MARKETS)?;
        txn.open_table(MARKET_META)?;
        txn.open_table(MARKET_STATUSES)?;
        txn.open_table(MARKET_GROUPS)?;
        txn.open_table(BLOCK_HEADERS)?;
        txn.open_table(PUBKEY_REGISTRY)?;
        txn.open_table(COUNTERS)?;
        txn.open_table(CLEARING_PRICES)?;
        txn.commit()?;

        info!(?path, "store opened");
        Ok(Self { db, qmdb_accounts })
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
        // Persist accounts first. If this succeeds and redb fails, redb remains the
        // recovery fallback because we still dual-write accounts into redb below.
        self.qmdb_accounts
            .persist(accounts, header.height, accounts.next_id())
            .await?;

        let txn = self.db.begin_write()?;

        // Accounts (transition fallback while qmdb becomes authoritative)
        {
            let mut table = txn.open_table(ACCOUNTS)?;
            for (id, account) in accounts.iter() {
                let bytes = rmp_serde::to_vec(account)?;
                table.insert(id.0, bytes.as_slice())?;
            }
        }

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
            table.insert(KEY_HEIGHT, header.height)?;
            table.insert(KEY_NEXT_ACCOUNT_ID, accounts.next_id())?;
            table.insert(KEY_NEXT_MARKET_ID, markets.next_id() as u64)?;
            table.insert(KEY_NEXT_ORDER_ID, next_order_id)?;
        }

        txn.commit()?;
        debug!(height = header.height, "block persisted");
        Ok(())
    }

    /// Load state from the store. Returns None if the store is empty (fresh start).
    pub async fn load_state(&self) -> Result<Option<RestoredState>, StoreError> {
        let txn = self.db.begin_read()?;

        // Check if we have any data
        let counters = txn.open_table(COUNTERS)?;
        let height = match counters.get(KEY_HEIGHT)? {
            Some(v) => v.value(),
            None => return Ok(None), // Fresh store
        };
        let next_account_id = counters
            .get(KEY_NEXT_ACCOUNT_ID)?
            .map(|v| v.value())
            .unwrap_or(0);
        let next_market_id = counters
            .get(KEY_NEXT_MARKET_ID)?
            .map(|v| v.value())
            .unwrap_or(0) as u32;
        let next_order_id = counters
            .get(KEY_NEXT_ORDER_ID)?
            .map(|v| v.value())
            .unwrap_or(1);

        let redb_accounts = load_redb_accounts(&txn)?;
        let qmdb_accounts = self.qmdb_accounts.load().await?;
        let accounts_map = if qmdb_accounts.accounts.is_empty() {
            if !redb_accounts.is_empty() {
                warn!("qmdb account snapshot missing, falling back to redb accounts");
            }
            redb_accounts
        } else if qmdb_accounts.height == Some(height)
            && qmdb_accounts.next_account_id.unwrap_or(next_account_id) == next_account_id
        {
            qmdb_accounts.accounts
        } else {
            warn!(
                redb_height = height,
                qmdb_height = ?qmdb_accounts.height,
                redb_next_account_id = next_account_id,
                qmdb_next_account_id = ?qmdb_accounts.next_account_id,
                "qmdb account snapshot did not match redb metadata, falling back to redb accounts"
            );
            redb_accounts
        };
        let num_accounts = accounts_map.len();
        let accounts = AccountStore::restore(accounts_map, next_account_id);

        // Markets
        let markets = {
            let table = txn.open_table(MARKETS)?;
            let mut market_map = HashMap::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let market: matching_engine::Market = rmp_serde::from_slice(value.value())?;
                market_map.insert(market.id, market);
            }
            MarketSet::restore(market_map, next_market_id)
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
            match table.get(height)? {
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
            height,
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
            height,
            last_header,
            next_order_id,
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
}

fn load_redb_accounts(
    txn: &redb::ReadTransaction,
) -> Result<HashMap<AccountId, Account>, StoreError> {
    let mut accounts = HashMap::new();
    let table = txn.open_table(ACCOUNTS)?;
    for entry in table.iter()? {
        let (_, value) = entry?;
        let account: Account = rmp_serde::from_slice(value.value())?;
        accounts.insert(account.id, account);
    }
    Ok(accounts)
}
