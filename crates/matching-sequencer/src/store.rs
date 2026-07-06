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

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use sybil_oracle::{
    AdminOracle, DataFeed, FeedPubkey, MarketStatus, ResolutionTemplate, SignedAttestation,
};
use sybil_verifier::BlockWitness;
use tracing::{debug, info, warn};

use crate::account::{AccountId, AccountStore};
use crate::account_storage::{
    AccountSnapshotSlot, AccountStateStore, CommittedAccountState, FencedAccountStorage,
    QmdbStateLeafExclusionProof, QmdbStateLeafProof, QmdbStateRoot, RecoveryAccountState,
};
use crate::aggregates::{
    CostBasisTrackerSnapshot, LiquidityTrackerSnapshot, OrderStatsTrackerSnapshot,
    TraderTrackerSnapshot, WelfareTrackerSnapshot,
};
use crate::block::{state_sidecar_snapshot_from_resting_orders, BlockHeader, SealedBlock};
use crate::bridge::{BridgeState, BridgeWithdrawalRequest, L1Deposit};
use crate::market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, PriceCandle, PriceCandlePage, PricePoint,
};
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::RestingOrder;
use crate::price_tracker::{PriceTrackerClearingHistorySnapshot, PriceTrackerVolumeSnapshot};

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

/// Resolution templates: template_id -> msgpack(ResolutionTemplate).
/// Built-in templates are reinstalled by the API on startup, but persisting
/// the registry keeps the sequencer snapshot self-contained and protects
/// operator-installed templates after the control-plane WAL is cleared.
const RESOLUTION_TEMPLATES: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resolution_templates");

/// Market statuses: market_id (u32) → msgpack(MarketStatus)
const MARKET_STATUSES: TableDefinition<u32, &[u8]> = TableDefinition::new("market_statuses");

/// Market groups: group_index (u32) → msgpack(MarketGroup)
const MARKET_GROUPS: TableDefinition<u32, &[u8]> = TableDefinition::new("market_groups");

/// Block headers: height (u64) → msgpack(BlockHeader)
const BLOCK_HEADERS: TableDefinition<u64, &[u8]> = TableDefinition::new("block_headers");

/// Block witnesses: height (u64) -> msgpack(BlockWitness).
/// Persisted for asynchronous witgen/prover workers. Historical qMDB slots are
/// not retained yet, so proof-job export currently targets the latest block.
const BLOCK_WITNESSES: TableDefinition<u64, &[u8]> = TableDefinition::new("block_witnesses");

/// Pubkey registry: compressed_point (33 bytes) → account_id (u64)
const PUBKEY_REGISTRY: TableDefinition<&[u8], u64> = TableDefinition::new("pubkey_registry");

/// Last clearing prices: market_id (u32) → msgpack(Vec<Nanos>)
const CLEARING_PRICES: TableDefinition<u32, &[u8]> = TableDefinition::new("clearing_prices");

/// Cumulative market volumes: market_id (u32) -> total traded volume in nanos.
const MARKET_VOLUMES: TableDefinition<u32, u64> = TableDefinition::new("market_volumes");

/// Scalar counters: name → value
const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

/// Historical-serving metadata: retained floors and maintenance cursors.
///
/// These rows describe durable history that is actually still present. They
/// are advanced only in the same transaction that deletes old rows.
const HISTORY_META: TableDefinition<&str, u64> = TableDefinition::new("history_meta");

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

/// Control-plane command WAL: monotonic seq (u64) -> msgpack(ControlPlaneCommand).
/// Protects acknowledged account, market, resolution, cancellation, feed, and
/// template mutations accepted after the last committed block. Cleared
/// atomically inside `save_block`.
const CONTROL_PLANE_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("control_plane_log");

/// Per-account fill history: account_id || block_height || order_id →
/// msgpack(AccountFillRecord). The byte key keeps records clustered by
/// account and ordered by block for efficient restoration and future scans.
const FILL_HISTORY: TableDefinition<&[u8], &[u8]> = TableDefinition::new("fill_history");

/// Per-account equity series. Key = account_id(8B BE) ++ height(8B BE); one
/// point per (account, block). Value = rmp-serde EquityPoint. Off-block.
const EQUITY_POINTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("equity_points");

/// Per-account history feed. Key = account_id(8B BE) ++ block_height(8B BE) ++
/// seq(8B BE). Value = rmp-serde StoredHistoryEvent. Off-block.
const HISTORY_EVENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("history_events");

/// L1 bridge sidecar state: consumed deposit cursor/root and withdrawal leaves.
const BRIDGE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("bridge_state");
const KEY_BRIDGE_STATE: &str = "state";

/// L1 deposits observed after the last committed block. They are replayed on
/// restart and cleared atomically once a block commits them into state.
const PENDING_L1_DEPOSITS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_l1_deposits");

/// Bridge withdrawals requested after the last committed block. They are
/// replayed on restart and cleared atomically once a block commits them.
const PENDING_BRIDGE_WITHDRAWALS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_bridge_withdrawals");

/// Trader tracker snapshot — one row keyed "snapshot" holding the full
/// `TraderTrackerSnapshot` payload. Off-block sidecar; missing table on
/// load yields `Default::default()` (cold start until activity accumulates).
const TRADER_TRACKER: TableDefinition<&str, &[u8]> = TableDefinition::new("trader_tracker");
const KEY_TRADER_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker volume extensions: platform running total +
/// rolling hourly buckets. Stored as a single blob keyed `"snapshot"`,
/// matching the pattern set by `TRADER_TRACKER`.
const PRICE_TRACKER_VOLUME: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_volume");
const KEY_PRICE_TRACKER_VOLUME_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker clearing-price history: per-market hourly
/// snapshot of the first-of-hour clearing price, used by
/// `price_n_hours_ago` (24h price-delta surfaces). Separate table from
/// `PRICE_TRACKER_VOLUME` so reverting B3 drops one table cleanly.
const PRICE_TRACKER_CLEARING_HISTORY: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_clearing_history");
const KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT: &str = "snapshot";

/// Durable raw mark-price points. Key =
/// market_id(4B BE) ++ block_height(8B BE).
const PRICE_POINTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("price_points");

/// Ordered retention index for raw mark-price points. Key =
/// block_height(8B BE) ++ market_id(4B BE). Value is unused.
const PRICE_POINTS_BY_HEIGHT: TableDefinition<&[u8], u64> =
    TableDefinition::new("price_points_by_height");

/// Downsampled committed-batch price candles. Key =
/// market_id(4B BE) ++ resolution_secs(4B BE) ++ bucket_start_ms(8B BE).
const PRICE_CANDLES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("price_candles");

/// Ordered retention index for price candles. Key =
/// resolution_secs(4B BE) ++ bucket_start_ms(8B BE) ++ market_id(4B BE).
/// Value is unused.
const PRICE_CANDLES_BY_RESOLUTION: TableDefinition<&[u8], u64> =
    TableDefinition::new("price_candles_by_resolution");

/// Off-block liquidity tracker: per-market ±band depth rings used by the
/// `liquidity_avg10` surface. Same single-blob shape as `TRADER_TRACKER`.
const LIQUIDITY_TRACKER: TableDefinition<&str, &[u8]> = TableDefinition::new("liquidity_tracker");
const KEY_LIQUIDITY_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block order stats tracker (B6): placed / matched / unmatched
/// counters per market + platform + hourly platform buckets. Single-blob
/// shape; missing table on load yields `OrderStatsTrackerSnapshot::default()`.
const ORDER_STATS_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("order_stats_tracker");
const KEY_ORDER_STATS_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block welfare tracker: cumulative platform welfare running total +
/// rolling hourly buckets for the 24h window. Single-blob shape; missing
/// table on load yields `WelfareTrackerSnapshot::default()`.
const WELFARE_TRACKER: TableDefinition<&str, &[u8]> = TableDefinition::new("welfare_tracker");
const KEY_WELFARE_TRACKER_SNAPSHOT: &str = "snapshot";

/// First-deposit timestamps per account (B8). Single blob keyed
/// "snapshot"; missing-row yields an empty map.
const FIRST_DEPOSIT_MS: TableDefinition<&str, &[u8]> = TableDefinition::new("first_deposit_ms");
const KEY_FIRST_DEPOSIT_MS_SNAPSHOT: &str = "snapshot";

/// All-time per-account fill counters (B8). The bounded fill window
/// lives in FILL_HISTORY; the counter survives trim and restart.
const FILL_TOTAL_COUNTS: TableDefinition<&str, &[u8]> = TableDefinition::new("fill_total_counts");
const KEY_FILL_TOTAL_COUNTS_SNAPSHOT: &str = "snapshot";

/// Off-block cost-basis tracker (C1): weighted-average entry price per
/// (account, market, outcome) + realized PnL per account. Single blob
/// keyed "snapshot"; missing row yields `CostBasisTrackerSnapshot::default()`
/// (cold start until activity accumulates).
const COST_BASIS_TRACKER: TableDefinition<&str, &[u8]> = TableDefinition::new("cost_basis_tracker");
const KEY_COST_BASIS_TRACKER_SNAPSHOT: &str = "snapshot";

// Counter keys
const KEY_STORE_LAYOUT_VERSION: &str = "store_layout_version";
const KEY_HEIGHT: &str = "height";
const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
const KEY_NEXT_MARKET_ID: &str = "next_market_id";
const KEY_NEXT_ORDER_ID: &str = "next_order_id";
const KEY_ACCOUNT_STATE_HEIGHT: &str = "account_state_height";
const KEY_ACCOUNT_STATE_SLOT: &str = "account_state_slot";
const KEY_HISTORY_EVENT_NEXT_SEQ: &str = "history_event_next_seq";
const KEY_BLOCKS_FULL_MIN_HEIGHT: &str = "blocks_full_min_height";
const KEY_PRICE_POINTS_MIN_HEIGHT: &str = "price_points_min_height";
const KEY_LAST_HISTORY_PRUNE_HEIGHT: &str = "last_history_prune_height";
const KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX: &str = "price_candles_min_bucket_ms:";

const STORE_LAYOUT_VERSION: u64 = 1;

fn fill_history_key(account_id: AccountId, record: &AccountFillRecord) -> [u8; 24] {
    let mut key = [0u8; 24];
    key[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    key[8..16].copy_from_slice(&record.block_height.to_be_bytes());
    key[16..24].copy_from_slice(&record.order_id.to_be_bytes());
    key
}

/// Inclusive `[lo, hi]` bounds covering every fill-history key for one account
/// (keys are `account_id || block_height || order_id`, big-endian, so a single
/// account is a contiguous range).
fn fill_history_account_bounds(account_id: AccountId) -> ([u8; 24], [u8; 24]) {
    let mut lo = [0u8; 24];
    lo[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    let mut hi = [0xffu8; 24];
    hi[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    (lo, hi)
}

fn equity_key(account_id: AccountId, height: u64) -> [u8; 16] {
    let mut k = [0u8; 16];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..].copy_from_slice(&height.to_be_bytes());
    k
}

fn history_event_key(account_id: AccountId, block_height: u64, seq: u64) -> [u8; 24] {
    let mut k = [0u8; 24];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..16].copy_from_slice(&block_height.to_be_bytes());
    k[16..].copy_from_slice(&seq.to_be_bytes());
    k
}

fn price_point_key(market_id: MarketId, height: u64) -> [u8; 12] {
    let mut key = [0u8; 12];
    key[..4].copy_from_slice(&market_id.0.to_be_bytes());
    key[4..].copy_from_slice(&height.to_be_bytes());
    key
}

fn price_point_parts_from_key(key: &[u8]) -> Option<(MarketId, u64)> {
    let market_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let height_bytes: [u8; 8] = key.get(4..12)?.try_into().ok()?;
    Some((
        MarketId(u32::from_be_bytes(market_bytes)),
        u64::from_be_bytes(height_bytes),
    ))
}

fn price_point_by_height_key(height: u64, market_id: MarketId) -> [u8; 12] {
    let mut key = [0u8; 12];
    key[..8].copy_from_slice(&height.to_be_bytes());
    key[8..].copy_from_slice(&market_id.0.to_be_bytes());
    key
}

fn price_point_by_height_parts_from_key(key: &[u8]) -> Option<(u64, MarketId)> {
    let height_bytes: [u8; 8] = key.get(..8)?.try_into().ok()?;
    let market_bytes: [u8; 4] = key.get(8..12)?.try_into().ok()?;
    Some((
        u64::from_be_bytes(height_bytes),
        MarketId(u32::from_be_bytes(market_bytes)),
    ))
}

fn price_point_market_bounds(market_id: MarketId) -> ([u8; 12], [u8; 12]) {
    (
        price_point_key(market_id, 0),
        price_point_key(market_id, u64::MAX),
    )
}

fn price_candle_key(market_id: MarketId, resolution_secs: u32, bucket_start_ms: u64) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..4].copy_from_slice(&market_id.0.to_be_bytes());
    key[4..8].copy_from_slice(&resolution_secs.to_be_bytes());
    key[8..].copy_from_slice(&bucket_start_ms.to_be_bytes());
    key
}

fn price_candle_parts_from_key(key: &[u8]) -> Option<(MarketId, u32, u64)> {
    let market_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let resolution_bytes: [u8; 4] = key.get(4..8)?.try_into().ok()?;
    let bucket_bytes: [u8; 8] = key.get(8..16)?.try_into().ok()?;
    Some((
        MarketId(u32::from_be_bytes(market_bytes)),
        u32::from_be_bytes(resolution_bytes),
        u64::from_be_bytes(bucket_bytes),
    ))
}

fn price_candle_market_resolution_bounds(
    market_id: MarketId,
    resolution_secs: u32,
) -> ([u8; 16], [u8; 16]) {
    (
        price_candle_key(market_id, resolution_secs, 0),
        price_candle_key(market_id, resolution_secs, u64::MAX),
    )
}

fn price_candle_by_resolution_key(
    resolution_secs: u32,
    bucket_start_ms: u64,
    market_id: MarketId,
) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..4].copy_from_slice(&resolution_secs.to_be_bytes());
    key[4..12].copy_from_slice(&bucket_start_ms.to_be_bytes());
    key[12..].copy_from_slice(&market_id.0.to_be_bytes());
    key
}

fn price_candle_by_resolution_parts_from_key(key: &[u8]) -> Option<(u32, u64, MarketId)> {
    let resolution_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let bucket_bytes: [u8; 8] = key.get(4..12)?.try_into().ok()?;
    let market_bytes: [u8; 4] = key.get(12..16)?.try_into().ok()?;
    Some((
        u32::from_be_bytes(resolution_bytes),
        u64::from_be_bytes(bucket_bytes),
        MarketId(u32::from_be_bytes(market_bytes)),
    ))
}

fn price_candle_resolution_bounds(resolution_secs: u32) -> ([u8; 16], [u8; 16]) {
    (
        price_candle_by_resolution_key(resolution_secs, 0, MarketId(0)),
        price_candle_by_resolution_key(resolution_secs, u64::MAX, MarketId(u32::MAX)),
    )
}

fn price_candles_min_bucket_key(resolution_secs: u32) -> String {
    format!("{KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX}{resolution_secs}")
}

fn parse_price_candles_min_bucket_key(key: &str) -> Option<u32> {
    key.strip_prefix(KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX)?
        .parse()
        .ok()
}

fn seq_from_history_event_key(key: &[u8]) -> Option<u64> {
    let seq_bytes: [u8; 8] = key.get(16..24)?.try_into().ok()?;
    Some(u64::from_be_bytes(seq_bytes))
}

fn account_id_from_fill_history_key(key: &[u8]) -> Option<AccountId> {
    let account_bytes: [u8; 8] = key.get(0..8)?.try_into().ok()?;
    Some(AccountId(u64::from_be_bytes(account_bytes)))
}

fn prune_historical_block_rows(db: &Database) -> Result<bool, StoreError> {
    let txn = db.begin_write()?;
    let Some(height) = ({
        let counters = txn.open_table(COUNTERS)?;
        let height = counters.get(KEY_HEIGHT)?.map(|value| value.value());
        height
    }) else {
        txn.commit()?;
        return Ok(false);
    };

    let mut pruned = false;
    {
        let mut headers = txn.open_table(BLOCK_HEADERS)?;
        headers.retain(|key, _| {
            let keep = key == height;
            pruned |= !keep;
            keep
        })?;
    }
    {
        let mut witnesses = txn.open_table(BLOCK_WITNESSES)?;
        witnesses.retain(|key, _| {
            let keep = key == height;
            pruned |= !keep;
            keep
        })?;
    }
    txn.commit()?;
    if pruned {
        info!(height, "pruned historical block rows from store");
    }
    Ok(pruned)
}

// TODO: Tier 2 tables (remaining)
// const MM_STATE: TableDefinition<u32, &[u8]> = TableDefinition::new("mm_state");

/// Full API replay block by height. Unlike `BLOCK_HEADERS`/`BLOCK_WITNESSES`,
/// this is historical serving data and is not pruned to latest-only.
const BLOCKS_FULL: TableDefinition<u64, &[u8]> = TableDefinition::new("blocks_full");

// TODO: Tier 3 tables (remaining)
// const PRICE_HISTORY: TableDefinition<u64, &[u8]> = TableDefinition::new("price_history");

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Persistent store for sequencer state. Wraps a redb database.
pub struct Store {
    db: Arc<Database>,
    account_state_store: Box<dyn AccountStateStore>,
    #[cfg(test)]
    fault_injection: Arc<Mutex<StoreFaultInjection>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreFaultPoint {
    BeforeQmdbPersist,
    AfterQmdbPersistBeforeRedbFence,
    BeforeRedbFenceCommit,
    AfterRedbFenceCommit,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct StoreFaultInjection {
    save_block_faults: VecDeque<StoreFaultPoint>,
}

#[cfg(test)]
fn pop_save_block_fault(
    fault_injection: &Arc<Mutex<StoreFaultInjection>>,
    point: StoreFaultPoint,
) -> Result<(), StoreError> {
    let mut faults = fault_injection
        .lock()
        .expect("store fault-injection lock poisoned");
    if faults.save_block_faults.front().copied() == Some(point) {
        faults.save_block_faults.pop_front();
        return Err(StoreError::InjectedFault(format!("{point:?}")));
    }
    Ok(())
}

/// Retention settings for durable history tables.
///
/// A value of 0 disables pruning for that stream. `prune_max_rows` bounds the
/// memory and write work of one maintenance pass; when it is exhausted,
/// metadata remains at the oldest row still present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryRetentionPolicy {
    pub block_history_retention_blocks: u64,
    pub raw_price_retention_blocks: u64,
    pub price_candle_resolutions_secs: Vec<u32>,
    pub price_candle_retention_secs: Vec<u64>,
    pub prune_interval_blocks: u64,
    pub prune_max_rows: usize,
}

impl HistoryRetentionPolicy {
    pub fn should_prune_at(&self, height: u64) -> bool {
        let prunes_price_candles = self
            .price_candle_resolutions_secs
            .iter()
            .zip(&self.price_candle_retention_secs)
            .any(|(&resolution_secs, &retention_secs)| resolution_secs > 0 && retention_secs > 0);
        height > 0
            && self.prune_interval_blocks > 0
            && self.prune_max_rows > 0
            && (self.block_history_retention_blocks > 0
                || self.raw_price_retention_blocks > 0
                || prunes_price_candles)
            && height.is_multiple_of(self.prune_interval_blocks)
    }

    fn blocks_full_floor(&self, head_height: u64) -> Option<u64> {
        retention_floor(head_height, self.block_history_retention_blocks)
    }

    fn price_points_floor(&self, head_height: u64) -> Option<u64> {
        retention_floor(head_height, self.raw_price_retention_blocks)
    }

    fn price_candle_cutoffs(&self, head_timestamp_ms: u64) -> BTreeMap<u32, u64> {
        self.price_candle_resolutions_secs
            .iter()
            .zip(&self.price_candle_retention_secs)
            .filter_map(|(&resolution_secs, &retention_secs)| {
                if resolution_secs == 0 || retention_secs == 0 {
                    return None;
                }
                let retention_ms = retention_secs.saturating_mul(1000);
                Some((
                    resolution_secs,
                    head_timestamp_ms.saturating_sub(retention_ms),
                ))
            })
            .collect()
    }
}

fn retention_floor(head_height: u64, retention_blocks: u64) -> Option<u64> {
    if head_height == 0 || retention_blocks == 0 {
        return None;
    }
    Some(
        head_height
            .saturating_sub(retention_blocks.saturating_sub(1))
            .max(1),
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryRetentionMeta {
    pub blocks_full_min_height: Option<u64>,
    pub price_points_min_height: Option<u64>,
    pub price_candles_min_bucket_ms: BTreeMap<u32, u64>,
    pub last_history_prune_height: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryPruneReport {
    pub blocks_full_pruned: usize,
    pub price_points_pruned: usize,
    pub price_candles_pruned: usize,
    pub meta: HistoryRetentionMeta,
}

fn read_history_retention_meta(db: &Database) -> Result<HistoryRetentionMeta, StoreError> {
    let txn = db.begin_read()?;
    let table = txn.open_table(HISTORY_META)?;
    let mut price_candles_min_bucket_ms = BTreeMap::new();
    for entry in table.iter()? {
        let (key, value) = entry?;
        if let Some(resolution_secs) = parse_price_candles_min_bucket_key(key.value()) {
            price_candles_min_bucket_ms.insert(resolution_secs, value.value());
        }
    }
    Ok(HistoryRetentionMeta {
        blocks_full_min_height: table
            .get(KEY_BLOCKS_FULL_MIN_HEIGHT)?
            .map(|value| value.value()),
        price_points_min_height: table
            .get(KEY_PRICE_POINTS_MIN_HEIGHT)?
            .map(|value| value.value()),
        price_candles_min_bucket_ms,
        last_history_prune_height: table
            .get(KEY_LAST_HISTORY_PRUNE_HEIGHT)?
            .map(|value| value.value()),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ControlPlaneCommand {
    CreateAccount {
        initial_balance: i64,
    },
    CreateAccountAt {
        initial_balance: i64,
        timestamp_ms: u64,
    },
    FundAccount {
        account_id: AccountId,
        amount: i64,
        timestamp_ms: u64,
    },
    RegisterPubkey {
        account_id: AccountId,
        compressed_pubkey: Vec<u8>,
    },
    AdvanceReplayNonce {
        account_id: AccountId,
        nonce: u64,
    },
    CreateMarket {
        name: String,
    },
    CreateMarketWithMetadata {
        name: String,
        metadata: MarketMetadata,
    },
    CreateMarketGroup {
        name: String,
        market_ids: Vec<MarketId>,
    },
    CancelPendingOrder {
        account_id: AccountId,
        order_id: u64,
        timestamp_ms: u64,
    },
    ResolveMarket {
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    },
    ResolveMarketAttested {
        market_id: MarketId,
        signed: SignedAttestation,
        timestamp_ms: u64,
    },
    RegisterFeed {
        pubkey: FeedPubkey,
        name: String,
        timestamp_ms: u64,
    },
    InstallTemplate {
        template: ResolutionTemplate,
    },
    ExtendMarketGroup {
        group_id: u64,
        market_id: MarketId,
    },
}

/// Store-restored analytics projections. These are grouped separately from
/// core sequencer state, but still loaded from the existing redb tables.
pub struct AnalyticsRestoredState {
    pub last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: HashMap<MarketId, u64>,
    pub account_fills: Vec<(AccountId, AccountFillRecord)>,
    pub trader_tracker: TraderTrackerSnapshot,
    pub price_tracker_volume: PriceTrackerVolumeSnapshot,
    pub price_tracker_clearing_history: PriceTrackerClearingHistorySnapshot,
    pub liquidity_tracker: LiquidityTrackerSnapshot,
    pub order_stats_tracker: OrderStatsTrackerSnapshot,
    pub welfare_tracker: WelfareTrackerSnapshot,
    pub first_deposit_ms: HashMap<AccountId, u64>,
    pub fill_total_counts: HashMap<AccountId, u64>,
    pub cost_basis_tracker: CostBasisTrackerSnapshot,
    pub history_event_next_seq: u64,
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
    pub resting_orders: Vec<RestingOrder>,
    /// All registered data feeds.
    pub data_feeds: Vec<DataFeed>,
    /// All installed resolution templates.
    pub resolution_templates: Vec<ResolutionTemplate>,
    /// Bundle / MM / multi-market submissions that were admitted after the
    /// last committed block. The actor replays these into its in-memory
    /// pending queue so nothing acknowledged with a 200 OK is dropped by a
    /// crash.
    pub pending_bundles: Vec<crate::sequencer::OrderSubmission>,
    /// Non-MM single-market admissions that went into the resting book
    /// after the last committed block. On restart these are re-inserted
    /// on top of `resting_orders` before the sequencer starts processing.
    pub admit_log: Vec<RestingOrder>,
    /// Acknowledged control-plane mutations accepted after the last committed
    /// block. Replayed after snapshot restore so those writes are not lost.
    pub control_plane_log: Vec<ControlPlaneCommand>,
    /// Derived analytics projections restored from redb.
    pub analytics: AnalyticsRestoredState,
    /// L1 bridge sidecar state restored from the last committed block.
    pub bridge_state: BridgeState,
    /// L1 deposits durably accepted after the last committed block.
    pub pending_l1_deposits: Vec<L1Deposit>,
    /// Bridge withdrawals durably accepted after the last committed block.
    pub pending_bridge_withdrawals: Vec<BridgeWithdrawalRequest>,
}

/// Borrowed analytics view needed to persist one block.
pub struct AnalyticsSnapshot<'a> {
    pub last_clearing_prices: &'a HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: &'a HashMap<MarketId, u64>,
    pub account_fills: Vec<(AccountId, AccountFillRecord)>,
    pub trader_tracker: TraderTrackerSnapshot,
    pub price_tracker_volume: PriceTrackerVolumeSnapshot,
    pub price_tracker_clearing_history: PriceTrackerClearingHistorySnapshot,
    pub liquidity_tracker: LiquidityTrackerSnapshot,
    pub order_stats_tracker: OrderStatsTrackerSnapshot,
    pub welfare_tracker: WelfareTrackerSnapshot,
    pub first_deposit_ms: HashMap<AccountId, u64>,
    pub fill_total_counts: HashMap<AccountId, u64>,
    pub cost_basis_tracker: CostBasisTrackerSnapshot,
    pub history_event_next_seq: u64,
    pub fill_history_delta: Vec<(AccountId, AccountFillRecord)>,
    pub price_points_delta: Vec<(MarketId, crate::market_info::PricePoint)>,
    pub equity_points_delta: Vec<(AccountId, crate::aggregates::EquityPoint)>,
    pub history_events_delta: Vec<crate::aggregates::StoredHistoryEvent>,
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
    pub analytics: AnalyticsSnapshot<'a>,
    pub price_candle_resolutions_secs: &'a [u32],
    /// Owned because the snapshot clones the live book — cheap for bounded sizes.
    pub resting_orders: Vec<RestingOrder>,
    pub bridge_state: &'a BridgeState,
}

struct RedbBlockCommit {
    height: u64,
    market_rows: Vec<(u32, Vec<u8>)>,
    market_meta_rows: Vec<(u32, Vec<u8>)>,
    market_status_rows: Vec<(u32, Vec<u8>)>,
    market_group_rows: Vec<(u32, Vec<u8>)>,
    header_bytes: Vec<u8>,
    history_block_bytes: Option<Vec<u8>>,
    witness_bytes: Option<Vec<u8>>,
    pubkey_rows: Vec<(Vec<u8>, u64)>,
    clearing_price_rows: Vec<(u32, Vec<u8>)>,
    market_volume_rows: Vec<(u32, u64)>,
    resting_orders_bytes: Vec<u8>,
    fill_history_rows: Vec<([u8; 24], Vec<u8>)>,
    equity_point_rows: Vec<([u8; 16], Vec<u8>)>,
    history_event_rows: Vec<([u8; 24], Vec<u8>)>,
    history_event_next_seq: u64,
    price_point_rows: Vec<RedbPricePointRow>,
    price_candle_resolutions_secs: Vec<u32>,
    data_feed_rows: Vec<(u64, Vec<u8>)>,
    resolution_template_rows: Vec<(String, Vec<u8>)>,
    bridge_state_bytes: Vec<u8>,
    trader_tracker_bytes: Vec<u8>,
    price_tracker_volume_bytes: Vec<u8>,
    price_tracker_clearing_history_bytes: Vec<u8>,
    liquidity_tracker_bytes: Vec<u8>,
    order_stats_tracker_bytes: Vec<u8>,
    welfare_tracker_bytes: Vec<u8>,
    first_deposit_ms_bytes: Vec<u8>,
    fill_total_counts_bytes: Vec<u8>,
    cost_basis_tracker_bytes: Vec<u8>,
    counters: PersistedCoreCounters,
}

struct RedbPricePointRow {
    market_id: MarketId,
    point: PricePoint,
    key: [u8; 12],
    bytes: Vec<u8>,
}

fn build_redb_block_commit(
    snapshot: &SequencerSnapshot<'_>,
    witness: Option<&BlockWitness>,
    history_block: Option<&SealedBlock>,
    next_slot: AccountSnapshotSlot,
) -> Result<RedbBlockCommit, StoreError> {
    let mut market_rows = Vec::new();
    for (id, market) in snapshot.markets.iter_with_ids() {
        market_rows.push((id.0, rmp_serde::to_vec(market)?));
    }

    let mut market_status_rows = Vec::new();
    for (&market_id, status) in snapshot.lifecycle.market_statuses() {
        market_status_rows.push((market_id.0, rmp_serde::to_vec(status)?));
    }

    let mut market_meta_rows = Vec::new();
    for (id, _) in snapshot.markets.iter_with_ids() {
        if let Some(meta) = snapshot.lifecycle.market_metadata(*id) {
            market_meta_rows.push((id.0, rmp_serde::to_vec(meta)?));
        }
    }

    let mut market_group_rows = Vec::new();
    for (i, group) in snapshot.market_groups.iter().enumerate() {
        market_group_rows.push((i as u32, rmp_serde::to_vec(group)?));
    }

    let witness_bytes = witness.map(rmp_serde::to_vec).transpose()?;
    let history_block_bytes = history_block.map(rmp_serde::to_vec).transpose()?;

    let pubkey_rows = snapshot
        .pubkey_registry
        .iter()
        .map(|(pubkey, account_id)| (pubkey.compressed_bytes().to_vec(), account_id.0))
        .collect();

    let mut clearing_price_rows = Vec::new();
    for (&market_id, prices) in snapshot.analytics.last_clearing_prices {
        clearing_price_rows.push((market_id.0, rmp_serde::to_vec(prices)?));
    }

    let market_volume_rows = snapshot
        .analytics
        .market_volumes
        .iter()
        .map(|(&market_id, &volume)| (market_id.0, volume))
        .collect();

    let mut fill_history_rows = Vec::new();
    for (account_id, record) in &snapshot.analytics.account_fills {
        fill_history_rows.push((
            fill_history_key(*account_id, record),
            rmp_serde::to_vec(record)?,
        ));
    }
    for (account_id, record) in &snapshot.analytics.fill_history_delta {
        fill_history_rows.push((
            fill_history_key(*account_id, record),
            rmp_serde::to_vec(record)?,
        ));
    }

    let mut equity_point_rows = Vec::new();
    for (aid, point) in &snapshot.analytics.equity_points_delta {
        equity_point_rows.push((equity_key(*aid, point.height), rmp_serde::to_vec(point)?));
    }

    let mut history_event_rows = Vec::new();
    for event in &snapshot.analytics.history_events_delta {
        history_event_rows.push((
            history_event_key(AccountId(event.account_id), event.block_height, event.seq),
            rmp_serde::to_vec(event)?,
        ));
    }

    let mut price_point_rows = Vec::new();
    for (market_id, point) in &snapshot.analytics.price_points_delta {
        price_point_rows.push(RedbPricePointRow {
            market_id: *market_id,
            point: point.clone(),
            key: price_point_key(*market_id, point.height),
            bytes: rmp_serde::to_vec(point)?,
        });
    }

    let mut data_feed_rows = Vec::new();
    for feed in snapshot.lifecycle.feeds().iter() {
        data_feed_rows.push((feed.id.0, rmp_serde::to_vec(feed)?));
    }

    let mut resolution_template_rows = Vec::new();
    for (template_id, template) in snapshot.lifecycle.templates().iter() {
        resolution_template_rows.push((template_id.0.clone(), rmp_serde::to_vec(template)?));
    }

    let mut first_deposit_entries: Vec<(AccountId, u64)> = snapshot
        .analytics
        .first_deposit_ms
        .iter()
        .map(|(&aid, &ts)| (aid, ts))
        .collect();
    first_deposit_entries.sort_by_key(|(aid, _)| aid.0);

    let mut fill_total_entries: Vec<(AccountId, u64)> = snapshot
        .analytics
        .fill_total_counts
        .iter()
        .map(|(&aid, &n)| (aid, n))
        .collect();
    fill_total_entries.sort_by_key(|(aid, _)| aid.0);

    Ok(RedbBlockCommit {
        height: snapshot.header.height,
        market_rows,
        market_meta_rows,
        market_status_rows,
        market_group_rows,
        header_bytes: rmp_serde::to_vec(snapshot.header)?,
        history_block_bytes,
        witness_bytes,
        pubkey_rows,
        clearing_price_rows,
        market_volume_rows,
        resting_orders_bytes: rmp_serde::to_vec(&snapshot.resting_orders)?,
        fill_history_rows,
        equity_point_rows,
        history_event_rows,
        history_event_next_seq: snapshot.analytics.history_event_next_seq,
        price_point_rows,
        price_candle_resolutions_secs: snapshot.price_candle_resolutions_secs.to_vec(),
        data_feed_rows,
        resolution_template_rows,
        bridge_state_bytes: rmp_serde::to_vec(snapshot.bridge_state)?,
        trader_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.trader_tracker)?,
        price_tracker_volume_bytes: rmp_serde::to_vec(&snapshot.analytics.price_tracker_volume)?,
        price_tracker_clearing_history_bytes: rmp_serde::to_vec(
            &snapshot.analytics.price_tracker_clearing_history,
        )?,
        liquidity_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.liquidity_tracker)?,
        order_stats_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.order_stats_tracker)?,
        welfare_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.welfare_tracker)?,
        first_deposit_ms_bytes: rmp_serde::to_vec(&first_deposit_entries)?,
        fill_total_counts_bytes: rmp_serde::to_vec(&fill_total_entries)?,
        cost_basis_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.cost_basis_tracker)?,
        counters: PersistedCoreCounters {
            height: snapshot.header.height,
            next_account_id: snapshot.accounts.next_id(),
            next_market_id: snapshot.markets.next_id() as u64,
            next_order_id: snapshot.next_order_id,
            account_state_fence: AccountStateFence {
                height: snapshot.header.height,
                slot: next_slot,
            },
        },
    })
}

#[cfg(test)]
fn write_redb_block_commit(
    db: &Database,
    commit: RedbBlockCommit,
    fault_injection: Arc<Mutex<StoreFaultInjection>>,
) -> Result<(), StoreError> {
    write_redb_block_commit_inner(db, commit, || {
        pop_save_block_fault(&fault_injection, StoreFaultPoint::BeforeRedbFenceCommit)
    })
}

#[cfg(not(test))]
fn write_redb_block_commit(db: &Database, commit: RedbBlockCommit) -> Result<(), StoreError> {
    write_redb_block_commit_inner(db, commit, || Ok(()))
}

fn write_redb_block_commit_inner<F>(
    db: &Database,
    commit: RedbBlockCommit,
    before_commit: F,
) -> Result<(), StoreError>
where
    F: FnOnce() -> Result<(), StoreError>,
{
    let txn = db.begin_write()?;

    {
        let mut table = txn.open_table(MARKETS)?;
        for (id, bytes) in &commit.market_rows {
            table.insert(*id, bytes.as_slice())?;
        }
    }

    {
        let mut meta_table = txn.open_table(MARKET_META)?;
        let mut status_table = txn.open_table(MARKET_STATUSES)?;
        for (market_id, bytes) in &commit.market_status_rows {
            status_table.insert(*market_id, bytes.as_slice())?;
        }
        for (market_id, bytes) in &commit.market_meta_rows {
            meta_table.insert(*market_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(MARKET_GROUPS)?;
        table.retain(|_, _| false)?;
        for (index, bytes) in &commit.market_group_rows {
            table.insert(*index, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(BLOCK_HEADERS)?;
        table.retain(|height, _| height == commit.height)?;
        table.insert(commit.height, commit.header_bytes.as_slice())?;
    }

    if let Some(bytes) = &commit.history_block_bytes {
        let mut table = txn.open_table(BLOCKS_FULL)?;
        table.insert(commit.height, bytes.as_slice())?;
    }

    {
        let mut table = txn.open_table(BLOCK_WITNESSES)?;
        table.retain(|height, _| height == commit.height)?;
        if let Some(bytes) = &commit.witness_bytes {
            table.insert(commit.height, bytes.as_slice())?;
        } else {
            table.remove(commit.height)?;
        }
    }

    {
        let mut table = txn.open_table(PUBKEY_REGISTRY)?;
        for (pubkey, account_id) in &commit.pubkey_rows {
            table.insert(pubkey.as_slice(), *account_id)?;
        }
    }

    {
        let mut table = txn.open_table(CLEARING_PRICES)?;
        for (market_id, bytes) in &commit.clearing_price_rows {
            table.insert(*market_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(MARKET_VOLUMES)?;
        for (market_id, volume) in &commit.market_volume_rows {
            table.insert(*market_id, *volume)?;
        }
    }

    {
        let mut table = txn.open_table(RESTING_ORDERS)?;
        table.insert(
            KEY_RESTING_ORDERS_SNAPSHOT,
            commit.resting_orders_bytes.as_slice(),
        )?;
    }

    {
        let mut table = txn.open_table(FILL_HISTORY)?;
        for (key, bytes) in &commit.fill_history_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(EQUITY_POINTS)?;
        for (key, bytes) in &commit.equity_point_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(HISTORY_EVENTS)?;
        for (key, bytes) in &commit.history_event_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }
    {
        let mut counters = txn.open_table(COUNTERS)?;
        counters.insert(KEY_HISTORY_EVENT_NEXT_SEQ, commit.history_event_next_seq)?;
    }

    {
        let mut table = txn.open_table(PRICE_POINTS)?;
        let mut by_height = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        for row in &commit.price_point_rows {
            table.insert(row.key.as_slice(), row.bytes.as_slice())?;
            by_height.insert(
                price_point_by_height_key(row.point.height, row.market_id).as_slice(),
                0,
            )?;
        }
    }

    if !commit.price_point_rows.is_empty() && !commit.price_candle_resolutions_secs.is_empty() {
        let mut candles = txn.open_table(PRICE_CANDLES)?;
        let mut candles_by_resolution = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        for row in &commit.price_point_rows {
            for &resolution_secs in &commit.price_candle_resolutions_secs {
                if resolution_secs == 0 {
                    continue;
                }
                let mut candle = PriceCandle::from_point(resolution_secs, &row.point);
                let key = price_candle_key(row.market_id, resolution_secs, candle.bucket_start_ms);
                let index_key = price_candle_by_resolution_key(
                    resolution_secs,
                    candle.bucket_start_ms,
                    row.market_id,
                );
                if let Some(existing) = {
                    candles
                        .get(key.as_slice())?
                        .map(|value| rmp_serde::from_slice(value.value()))
                        .transpose()?
                } {
                    candle = existing;
                    candle.merge_point(&row.point);
                }
                let bytes = rmp_serde::to_vec(&candle)?;
                candles.insert(key.as_slice(), bytes.as_slice())?;
                candles_by_resolution.insert(index_key.as_slice(), 0)?;
            }
        }
    }

    {
        let mut table = txn.open_table(DATA_FEEDS)?;
        for (feed_id, bytes) in &commit.data_feed_rows {
            table.insert(*feed_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(RESOLUTION_TEMPLATES)?;
        table.retain(|_, _| false)?;
        for (template_id, bytes) in &commit.resolution_template_rows {
            table.insert(template_id.as_str(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(BRIDGE_STATE)?;
        table.insert(KEY_BRIDGE_STATE, commit.bridge_state_bytes.as_slice())?;
    }

    {
        let mut table = txn.open_table(PENDING_BUNDLES)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(ADMIT_LOG)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(CONTROL_PLANE_LOG)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(PENDING_L1_DEPOSITS)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
        table.retain(|_, _| false)?;
    }

    {
        let mut table = txn.open_table(TRADER_TRACKER)?;
        table.insert(
            KEY_TRADER_TRACKER_SNAPSHOT,
            commit.trader_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(PRICE_TRACKER_VOLUME)?;
        table.insert(
            KEY_PRICE_TRACKER_VOLUME_SNAPSHOT,
            commit.price_tracker_volume_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
        table.insert(
            KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT,
            commit.price_tracker_clearing_history_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(LIQUIDITY_TRACKER)?;
        table.insert(
            KEY_LIQUIDITY_TRACKER_SNAPSHOT,
            commit.liquidity_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(ORDER_STATS_TRACKER)?;
        table.insert(
            KEY_ORDER_STATS_TRACKER_SNAPSHOT,
            commit.order_stats_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(WELFARE_TRACKER)?;
        table.insert(
            KEY_WELFARE_TRACKER_SNAPSHOT,
            commit.welfare_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(FIRST_DEPOSIT_MS)?;
        table.insert(
            KEY_FIRST_DEPOSIT_MS_SNAPSHOT,
            commit.first_deposit_ms_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(FILL_TOTAL_COUNTS)?;
        table.insert(
            KEY_FILL_TOTAL_COUNTS_SNAPSHOT,
            commit.fill_total_counts_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(COST_BASIS_TRACKER)?;
        table.insert(
            KEY_COST_BASIS_TRACKER_SNAPSHOT,
            commit.cost_basis_tracker_bytes.as_slice(),
        )?;
    }

    {
        let mut table = txn.open_table(COUNTERS)?;
        write_core_counters(&mut table, commit.counters)?;
    }

    before_commit()?;
    txn.commit()?;
    Ok(())
}

fn prune_history_redb(
    db: &Database,
    head_height: u64,
    policy: HistoryRetentionPolicy,
    block_floor: Option<u64>,
    price_floor: Option<u64>,
    price_candle_cutoffs: BTreeMap<u32, u64>,
) -> Result<HistoryPruneReport, StoreError> {
    let txn = db.begin_write()?;
    let mut remaining = policy.prune_max_rows;
    let mut blocks_full_pruned = 0usize;
    let mut price_points_pruned = 0usize;
    let mut price_candles_pruned = 0usize;

    if let Some(floor) = block_floor {
        if remaining > 0 {
            let mut table = txn.open_table(BLOCKS_FULL)?;
            let mut iter = table.extract_from_if(0..floor, |_, _| true)?;
            while remaining > 0 {
                let Some(_) = iter.next().transpose()? else {
                    break;
                };
                blocks_full_pruned += 1;
                remaining -= 1;
            }
        }
    }

    if remaining > 0 {
        if let Some(floor) = price_floor {
            let lo = price_point_by_height_key(0, MarketId(0));
            let hi = price_point_by_height_key(floor, MarketId(0));
            let mut points = txn.open_table(PRICE_POINTS)?;
            let mut by_height = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
            let mut iter = by_height.extract_from_if(lo.as_slice()..hi.as_slice(), |_, _| true)?;
            while remaining > 0 {
                let Some((key, _)) = iter.next().transpose()? else {
                    break;
                };
                if let Some((height, market_id)) = price_point_by_height_parts_from_key(key.value())
                {
                    if points
                        .remove(price_point_key(market_id, height).as_slice())?
                        .is_some()
                    {
                        price_points_pruned += 1;
                    }
                } else {
                    warn!("invalid price point retention index key in store");
                }
                remaining -= 1;
            }
        }
    }

    if remaining > 0 {
        for (&resolution_secs, &cutoff_ms) in &price_candle_cutoffs {
            if remaining == 0 {
                break;
            }
            if cutoff_ms == 0 {
                continue;
            }
            let lo = price_candle_by_resolution_key(resolution_secs, 0, MarketId(0));
            let hi = price_candle_by_resolution_key(resolution_secs, cutoff_ms, MarketId(0));
            let mut candles = txn.open_table(PRICE_CANDLES)?;
            let mut by_resolution = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
            let mut iter =
                by_resolution.extract_from_if(lo.as_slice()..hi.as_slice(), |_, _| true)?;
            while remaining > 0 {
                let Some((key, _)) = iter.next().transpose()? else {
                    break;
                };
                if let Some((indexed_resolution, bucket_start_ms, market_id)) =
                    price_candle_by_resolution_parts_from_key(key.value())
                {
                    if candles
                        .remove(
                            price_candle_key(market_id, indexed_resolution, bucket_start_ms)
                                .as_slice(),
                        )?
                        .is_some()
                    {
                        price_candles_pruned += 1;
                    }
                } else {
                    warn!("invalid price candle retention index key in store");
                }
                remaining -= 1;
            }
        }
    }

    let blocks_full_min_height = if block_floor.is_some() {
        let table = txn.open_table(BLOCKS_FULL)?;
        let min_height = table
            .iter()?
            .next()
            .transpose()?
            .map(|(key, _)| key.value());
        min_height
    } else {
        None
    };
    let price_points_min_height = if price_floor.is_some() {
        let table = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        let min_height =
            table.iter()?.next().transpose()?.and_then(|(key, _)| {
                price_point_by_height_parts_from_key(key.value()).map(|(h, _)| h)
            });
        min_height
    } else {
        None
    };
    let mut price_candles_min_bucket_ms = BTreeMap::new();
    if !price_candle_cutoffs.is_empty() {
        let table = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        for &resolution_secs in price_candle_cutoffs.keys() {
            let (lo, hi) = price_candle_resolution_bounds(resolution_secs);
            if let Some((key, _)) = table
                .range(lo.as_slice()..=hi.as_slice())?
                .next()
                .transpose()?
            {
                if let Some((_, bucket_start_ms, _)) =
                    price_candle_by_resolution_parts_from_key(key.value())
                {
                    price_candles_min_bucket_ms.insert(resolution_secs, bucket_start_ms);
                }
            }
        }
    }

    {
        let mut meta = txn.open_table(HISTORY_META)?;
        if block_floor.is_some() {
            match blocks_full_min_height {
                Some(height) => {
                    meta.insert(KEY_BLOCKS_FULL_MIN_HEIGHT, height)?;
                }
                None => {
                    meta.remove(KEY_BLOCKS_FULL_MIN_HEIGHT)?;
                }
            }
        }
        if price_floor.is_some() {
            match price_points_min_height {
                Some(height) => {
                    meta.insert(KEY_PRICE_POINTS_MIN_HEIGHT, height)?;
                }
                None => {
                    meta.remove(KEY_PRICE_POINTS_MIN_HEIGHT)?;
                }
            }
        }
        for &resolution_secs in price_candle_cutoffs.keys() {
            let key = price_candles_min_bucket_key(resolution_secs);
            match price_candles_min_bucket_ms.get(&resolution_secs) {
                Some(bucket_start_ms) => {
                    meta.insert(key.as_str(), *bucket_start_ms)?;
                }
                None => {
                    meta.remove(key.as_str())?;
                }
            }
        }
        meta.insert(KEY_LAST_HISTORY_PRUNE_HEIGHT, head_height)?;
    }

    txn.commit()?;
    Ok(HistoryPruneReport {
        blocks_full_pruned,
        price_points_pruned,
        price_candles_pruned,
        meta: read_history_retention_meta(db)?,
    })
}

fn backfill_price_history_indexes(db: &Database) -> Result<(), StoreError> {
    let (price_points_len, price_points_index_len, price_candles_len, price_candles_index_len) = {
        let txn = db.begin_read()?;
        let price_points_len = txn.open_table(PRICE_POINTS)?.len()?;
        let price_points_index_len = txn.open_table(PRICE_POINTS_BY_HEIGHT)?.len()?;
        let price_candles_len = txn.open_table(PRICE_CANDLES)?.len()?;
        let price_candles_index_len = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?.len()?;
        (
            price_points_len,
            price_points_index_len,
            price_candles_len,
            price_candles_index_len,
        )
    };

    if price_points_index_len >= price_points_len && price_candles_index_len >= price_candles_len {
        return Ok(());
    }

    let txn = db.begin_write()?;
    let mut price_points_backfilled = 0u64;
    let mut price_candles_backfilled = 0u64;

    if price_points_index_len < price_points_len {
        let mut rows = Vec::new();
        {
            let table = txn.open_table(PRICE_POINTS)?;
            for entry in table.iter()? {
                let (key, _) = entry?;
                let Some((market_id, height)) = price_point_parts_from_key(key.value()) else {
                    warn!("invalid price point key in store; skipping index backfill");
                    continue;
                };
                rows.push(price_point_by_height_key(height, market_id));
            }
        }
        {
            let mut index = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
            for key in rows {
                if index.insert(key.as_slice(), 0)?.is_none() {
                    price_points_backfilled += 1;
                }
            }
        }
    }

    if price_candles_index_len < price_candles_len {
        let mut rows = Vec::new();
        {
            let table = txn.open_table(PRICE_CANDLES)?;
            for entry in table.iter()? {
                let (key, _) = entry?;
                let Some((market_id, resolution_secs, bucket_start_ms)) =
                    price_candle_parts_from_key(key.value())
                else {
                    warn!("invalid price candle key in store; skipping index backfill");
                    continue;
                };
                rows.push(price_candle_by_resolution_key(
                    resolution_secs,
                    bucket_start_ms,
                    market_id,
                ));
            }
        }
        {
            let mut index = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
            for key in rows {
                if index.insert(key.as_slice(), 0)?.is_none() {
                    price_candles_backfilled += 1;
                }
            }
        }
    }

    txn.commit()?;
    if price_points_backfilled > 0 || price_candles_backfilled > 0 {
        info!(
            price_points_backfilled,
            price_candles_backfilled, "backfilled price history retention indexes"
        );
    }
    Ok(())
}

impl Store {
    /// Open (or create) a store at the given path.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut db = Database::create(path)?;
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
        txn.open_table(BLOCKS_FULL)?;
        txn.open_table(BLOCK_WITNESSES)?;
        txn.open_table(PUBKEY_REGISTRY)?;
        txn.open_table(COUNTERS)?;
        txn.open_table(HISTORY_META)?;
        txn.open_table(CLEARING_PRICES)?;
        txn.open_table(MARKET_VOLUMES)?;
        txn.open_table(RESTING_ORDERS)?;
        txn.open_table(PENDING_BUNDLES)?;
        txn.open_table(ADMIT_LOG)?;
        txn.open_table(CONTROL_PLANE_LOG)?;
        txn.open_table(FILL_HISTORY)?;
        txn.open_table(EQUITY_POINTS)?;
        txn.open_table(HISTORY_EVENTS)?;
        txn.open_table(DATA_FEEDS)?;
        txn.open_table(RESOLUTION_TEMPLATES)?;
        txn.open_table(BRIDGE_STATE)?;
        txn.open_table(PENDING_L1_DEPOSITS)?;
        txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
        txn.open_table(TRADER_TRACKER)?;
        txn.open_table(PRICE_TRACKER_VOLUME)?;
        txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
        txn.open_table(PRICE_POINTS)?;
        txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        txn.open_table(PRICE_CANDLES)?;
        txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        txn.open_table(LIQUIDITY_TRACKER)?;
        txn.open_table(ORDER_STATS_TRACKER)?;
        txn.open_table(WELFARE_TRACKER)?;
        txn.open_table(FIRST_DEPOSIT_MS)?;
        txn.open_table(FILL_TOTAL_COUNTS)?;
        txn.open_table(COST_BASIS_TRACKER)?;
        txn.commit()?;

        initialize_or_validate_layout(&db)?;
        backfill_price_history_indexes(&db)?;
        if prune_historical_block_rows(&db)? {
            match db.compact() {
                Ok(true) => info!(?path, "compacted store after pruning historical block rows"),
                Ok(false) => debug!(?path, "store compaction found no reclaimable pages"),
                Err(error) => warn!(?path, %error, "store compaction failed after pruning"),
            }
        }

        let db = Arc::new(db);

        info!(?path, "store opened");
        Ok(Self {
            db,
            account_state_store,
            #[cfg(test)]
            fault_injection: Arc::new(Mutex::new(StoreFaultInjection::default())),
        })
    }

    #[cfg(test)]
    pub(crate) fn inject_next_save_block_fault(&self, point: StoreFaultPoint) {
        self.fault_injection
            .lock()
            .expect("store fault-injection lock poisoned")
            .save_block_faults
            .push_back(point);
    }

    #[cfg(test)]
    fn fail_save_block_at(&self, point: StoreFaultPoint) -> Result<(), StoreError> {
        pop_save_block_fault(&self.fault_injection, point)
    }

    async fn redb_write<R, F>(&self, write: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(Arc<Database>) -> Result<R, StoreError> + Send + 'static,
    {
        let db = Arc::clone(&self.db);
        // Redb begin_write/commit is synchronous and can fsync. The actor
        // awaits this task before making the corresponding state visible or
        // committing a prepared block, so the durable-before-visible and qMDB
        // fence ordering stays identical while the Tokio worker is not blocked.
        tokio::task::spawn_blocking(move || write(db))
            .await
            .map_err(|error| StoreError::BlockingTask(error.to_string()))?
    }

    #[cfg(test)]
    fn save_block_faults(&self) -> Arc<Mutex<StoreFaultInjection>> {
        Arc::clone(&self.fault_injection)
    }

    /// Save the sequencer state after a block. Single ACID transaction.
    pub async fn save_block(&self, snapshot: SequencerSnapshot<'_>) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, None, None).await
    }

    /// Save the sequencer state and its witness after a block.
    ///
    /// The witness is committed in the same redb transaction as the block
    /// metadata, so an asynchronous witgen process can later export a proof job
    /// for the latest committed block.
    pub async fn save_block_with_witness(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: &BlockWitness,
    ) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, Some(witness), None).await
    }

    /// Save sequencer state, witness, and the API replay block payload after
    /// a block. Actor commits use this path so historical reads have the same
    /// durability boundary as recovery state.
    pub async fn save_block_with_witness_and_history(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: &BlockWitness,
        block: &SealedBlock,
    ) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, Some(witness), Some(block))
            .await
    }

    async fn save_block_inner(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: Option<&BlockWitness>,
        history_block: Option<&SealedBlock>,
    ) -> Result<(), StoreError> {
        if let Some(witness) = witness {
            validate_witness_header(snapshot.header, witness)?;
        }

        let current_fence = read_account_state_fence(&self.db)?;
        let next_slot = current_fence
            .map(|fence| fence.slot.inactive())
            .unwrap_or(AccountSnapshotSlot::A);

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::BeforeQmdbPersist)?;

        // Persist the inactive qmdb slot first. It becomes committed only when the
        // redb transaction below flips the fence to point at it.
        let state_sidecar = state_sidecar_snapshot_from_resting_orders(
            snapshot.bridge_state,
            &snapshot.resting_orders,
            snapshot.markets,
            snapshot.market_groups,
            snapshot.lifecycle,
        );

        self.account_state_store
            .persist(CommittedAccountState {
                accounts: snapshot.accounts,
                state_sidecar: &state_sidecar,
                height: snapshot.header.height,
                next_account_id: snapshot.accounts.next_id(),
                slot: next_slot,
            })
            .await?;

        if witness.is_some() {
            let state_root = self.account_state_store.qmdb_state_root(next_slot).await?;
            if state_root.root != snapshot.header.state_root {
                metrics::counter!("sybil_store_qmdb_root_mismatch_total", "phase" => "commit")
                    .increment(1);
                return Err(StoreError::CorruptLayout(format!(
                    "typed qMDB root mismatch at height {} before commit: slot {:?} root={:?} header_root={:?}",
                    snapshot.header.height, state_root.slot, state_root.root, snapshot.header.state_root
                )));
            }
            metrics::counter!("sybil_store_commit_root_verified_total").increment(1);
        }

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::AfterQmdbPersistBeforeRedbFence)?;

        let commit = build_redb_block_commit(&snapshot, witness, history_block, next_slot)?;
        #[cfg(test)]
        let fault_injection = self.save_block_faults();
        self.redb_write(move |db| {
            #[cfg(test)]
            {
                write_redb_block_commit(&db, commit, fault_injection)
            }
            #[cfg(not(test))]
            {
                write_redb_block_commit(&db, commit)
            }
        })
        .await?;

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::AfterRedbFenceCommit)?;

        debug!(height = snapshot.header.height, "block persisted");
        Ok(())
    }

    pub async fn state_qmdb_root(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<QmdbStateRoot, StoreError> {
        self.account_state_store.qmdb_state_root(slot).await
    }

    pub async fn state_qmdb_leaves(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
        self.account_state_store.qmdb_state_leaves(slot).await
    }

    pub async fn state_qmdb_leaf_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafProof>, StoreError> {
        self.account_state_store
            .qmdb_state_leaf_proof(slot, leaf_key)
            .await
    }

    pub async fn state_qmdb_leaf_exclusion_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
        self.account_state_store
            .qmdb_state_leaf_exclusion_proof(slot, leaf_key)
            .await
    }

    pub async fn current_state_qmdb_root(&self) -> Result<Option<QmdbStateRoot>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_root(fence.slot).await.map(Some)
    }

    pub async fn current_state_qmdb_leaf_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafProof>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_leaf_proof(fence.slot, leaf_key).await
    }

    pub async fn current_state_qmdb_leaf_exclusion_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_leaf_exclusion_proof(fence.slot, leaf_key)
            .await
    }

    /// Load a persisted block witness by height.
    pub fn block_witness(&self, height: u64) -> Result<Option<BlockWitness>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCK_WITNESSES)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Load a historical API replay block by exact height.
    pub async fn load_block(&self, height: u64) -> Result<Option<SealedBlock>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCKS_FULL)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Load a newest-first page of historical API replay blocks. When
    /// `before_height` is present, only blocks with height strictly below that
    /// cursor are returned.
    pub async fn load_block_page(
        &self,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<SealedBlock>, StoreError> {
        if limit == 0 || before_height == Some(0) {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCKS_FULL)?;
        let mut blocks = Vec::new();
        match before_height {
            Some(before) => {
                for entry in table.range(0..before)?.rev() {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                    if blocks.len() >= limit {
                        break;
                    }
                }
            }
            None => {
                for entry in table.iter()?.rev() {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                    if blocks.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(blocks)
    }

    pub fn history_retention_meta(&self) -> Result<HistoryRetentionMeta, StoreError> {
        read_history_retention_meta(&self.db)
    }

    /// Delete old durable history rows under a bounded row budget.
    ///
    /// This is deliberately separate from block commit. If the budget is too
    /// small to reach the target floor, rows remain and the metadata floor
    /// stays at the oldest row still present.
    pub async fn prune_history(
        &self,
        head_height: u64,
        head_timestamp_ms: u64,
        policy: HistoryRetentionPolicy,
    ) -> Result<HistoryPruneReport, StoreError> {
        let block_floor = policy.blocks_full_floor(head_height);
        let price_floor = policy.price_points_floor(head_height);
        let price_candle_cutoffs = policy.price_candle_cutoffs(head_timestamp_ms);
        if policy.prune_max_rows == 0
            || (block_floor.is_none() && price_floor.is_none() && price_candle_cutoffs.is_empty())
        {
            return Ok(HistoryPruneReport {
                meta: self.history_retention_meta()?,
                ..HistoryPruneReport::default()
            });
        }

        self.redb_write(move |db| {
            prune_history_redb(
                &db,
                head_height,
                policy,
                block_floor,
                price_floor,
                price_candle_cutoffs,
            )
        })
        .await
    }

    /// Load raw mark-price points for one market. The scan is bounded in
    /// memory: if more than `limit` points match, the newest `limit` points
    /// are returned in chronological order with a `before_height` cursor for
    /// the next older page.
    pub async fn load_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<crate::market_info::PriceHistoryPage, StoreError> {
        let txn = self.db.begin_read()?;
        let retention_min_height = {
            let meta = txn.open_table(HISTORY_META)?;
            meta.get(KEY_PRICE_POINTS_MIN_HEIGHT)?
                .map(|value| value.value())
        };
        if limit == 0 {
            return Ok(crate::market_info::PriceHistoryPage {
                points: Vec::new(),
                next_before_height: None,
                retention_min_height,
            });
        }
        let table = txn.open_table(PRICE_POINTS)?;
        let (lo, hi) = price_point_market_bounds(market_id);
        let mut points = VecDeque::new();
        for entry in table.range(lo.as_slice()..=hi.as_slice())? {
            let (_, value) = entry?;
            let point: crate::market_info::PricePoint = rmp_serde::from_slice(value.value())?;
            if from_ms.is_some_and(|from| point.timestamp_ms < from)
                || to_ms.is_some_and(|to| point.timestamp_ms > to)
                || before_height.is_some_and(|before| point.height >= before)
            {
                continue;
            }
            if points.len() == limit.saturating_add(1) {
                points.pop_front();
            }
            points.push_back(point);
        }
        let mut points: Vec<_> = points.into_iter().collect();
        let next_before_height = if points.len() > limit {
            points.remove(0);
            points.first().map(|point| point.height)
        } else {
            None
        };
        Ok(crate::market_info::PriceHistoryPage {
            points,
            next_before_height,
            retention_min_height,
        })
    }

    /// Load downsampled price candles for one market/resolution. The newest
    /// matching candles are returned in chronological order, with
    /// `before_ms` cursoring to the next older page.
    pub async fn load_price_candles(
        &self,
        market_id: MarketId,
        resolution_secs: u32,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<PriceCandlePage, StoreError> {
        if resolution_secs == 0 {
            return Ok(PriceCandlePage {
                resolution_secs,
                candles: Vec::new(),
                next_before_ms: None,
                retention_min_bucket_ms: None,
            });
        }
        let txn = self.db.begin_read()?;
        let retention_min_bucket_ms = {
            let meta = txn.open_table(HISTORY_META)?;
            let key = price_candles_min_bucket_key(resolution_secs);
            meta.get(key.as_str())?.map(|value| value.value())
        };
        if limit == 0 {
            return Ok(PriceCandlePage {
                resolution_secs,
                candles: Vec::new(),
                next_before_ms: None,
                retention_min_bucket_ms,
            });
        }
        let table = txn.open_table(PRICE_CANDLES)?;
        let (lo, hi) = price_candle_market_resolution_bounds(market_id, resolution_secs);
        let mut candles = VecDeque::new();
        for entry in table.range(lo.as_slice()..=hi.as_slice())? {
            let (_, value) = entry?;
            let candle: PriceCandle = rmp_serde::from_slice(value.value())?;
            if from_ms.is_some_and(|from| candle.bucket_start_ms < from)
                || to_ms.is_some_and(|to| candle.bucket_start_ms > to)
                || before_ms.is_some_and(|before| candle.bucket_start_ms >= before)
            {
                continue;
            }
            if candles.len() == limit.saturating_add(1) {
                candles.pop_front();
            }
            candles.push_back(candle);
        }
        let mut candles: Vec<_> = candles.into_iter().collect();
        let next_before_ms = if candles.len() > limit {
            candles.remove(0);
            candles.first().map(|candle| candle.bucket_start_ms)
        } else {
            None
        };
        Ok(PriceCandlePage {
            resolution_secs,
            candles,
            next_before_ms,
            retention_min_bucket_ms,
        })
    }

    /// Load the latest committed block witness, if the store has one.
    pub fn latest_block_witness(&self) -> Result<Option<BlockWitness>, StoreError> {
        let txn = self.db.begin_read()?;
        let Some(metadata) = read_recovery_metadata(&txn)? else {
            return Ok(None);
        };
        let table = txn.open_table(BLOCK_WITNESSES)?;
        table
            .get(metadata.height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
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
        let market_groups: Vec<MarketGroup> = {
            let table = txn.open_table(MARKET_GROUPS)?;
            let mut groups = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let group: MarketGroup = rmp_serde::from_slice(value.value())?;
                groups.push((key.value(), group));
            }
            groups.sort_by_key(|(index, _)| *index);
            groups.into_iter().map(|(_, group)| group).collect()
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
        let latest_witness_exists = {
            let table = txn.open_table(BLOCK_WITNESSES)?;
            table.get(recovery_metadata.height)?.is_some()
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

        let resolution_templates: Vec<ResolutionTemplate> = {
            let table = txn.open_table(RESOLUTION_TEMPLATES)?;
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

        let control_plane_log: Vec<ControlPlaneCommand> = {
            let table = txn.open_table(CONTROL_PLANE_LOG)?;
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

        let bridge_state: BridgeState = {
            let table = txn.open_table(BRIDGE_STATE)?;
            match table.get(KEY_BRIDGE_STATE)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => BridgeState::default(),
            }
        };

        let pending_l1_deposits: Vec<L1Deposit> = {
            let table = txn.open_table(PENDING_L1_DEPOSITS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let pending_bridge_withdrawals: Vec<BridgeWithdrawalRequest> = {
            let table = txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        if latest_witness_exists {
            let Some(header) = last_header.as_ref() else {
                return Err(StoreError::CorruptLayout(format!(
                    "missing block header for witnessed height {}",
                    recovery_metadata.height
                )));
            };
            self.ensure_state_qmdb_root(
                recovery_metadata.account_state,
                &accounts,
                &markets,
                &market_groups,
                &market_statuses,
                &market_metadata,
                &resting_orders,
                &bridge_state,
                header,
            )
            .await?;
        }

        // Trader tracker snapshot. Missing row -> cold-start default; the
        // tracker repopulates as admissions arrive after restart.
        let trader_tracker: TraderTrackerSnapshot = {
            let table = txn.open_table(TRADER_TRACKER)?;
            match table.get(KEY_TRADER_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => TraderTrackerSnapshot::default(),
            }
        };

        // Price-tracker volume extensions. Same missing-row → default
        // semantics as the trader tracker; cold restarts start with empty
        // hourly buckets and a zero platform total.
        let price_tracker_volume: PriceTrackerVolumeSnapshot = {
            let table = txn.open_table(PRICE_TRACKER_VOLUME)?;
            match table.get(KEY_PRICE_TRACKER_VOLUME_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => PriceTrackerVolumeSnapshot::default(),
            }
        };

        // Price-tracker clearing-history slice. Missing-row → default.
        let price_tracker_clearing_history: PriceTrackerClearingHistorySnapshot = {
            let table = txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
            match table.get(KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => PriceTrackerClearingHistorySnapshot::default(),
            }
        };

        // Liquidity tracker snapshot. Missing-row → default.
        let liquidity_tracker: LiquidityTrackerSnapshot = {
            let table = txn.open_table(LIQUIDITY_TRACKER)?;
            match table.get(KEY_LIQUIDITY_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => LiquidityTrackerSnapshot::default(),
            }
        };

        // OrderStatsTracker snapshot. Missing-row → default.
        let order_stats_tracker: OrderStatsTrackerSnapshot = {
            let table = txn.open_table(ORDER_STATS_TRACKER)?;
            match table.get(KEY_ORDER_STATS_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => OrderStatsTrackerSnapshot::default(),
            }
        };

        // WelfareTracker snapshot. Missing-row → default.
        let welfare_tracker: WelfareTrackerSnapshot = {
            let table = txn.open_table(WELFARE_TRACKER)?;
            match table.get(KEY_WELFARE_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => WelfareTrackerSnapshot::default(),
            }
        };

        // First-deposit timestamps (B8). Missing-row → empty.
        let first_deposit_ms: HashMap<AccountId, u64> = {
            let table = txn.open_table(FIRST_DEPOSIT_MS)?;
            match table.get(KEY_FIRST_DEPOSIT_MS_SNAPSHOT)? {
                Some(value) => {
                    let entries: Vec<(AccountId, u64)> = rmp_serde::from_slice(value.value())?;
                    entries.into_iter().collect()
                }
                None => HashMap::new(),
            }
        };

        // All-time fill counters per account (B8). Missing-row → empty.
        let fill_total_counts: HashMap<AccountId, u64> = {
            let table = txn.open_table(FILL_TOTAL_COUNTS)?;
            match table.get(KEY_FILL_TOTAL_COUNTS_SNAPSHOT)? {
                Some(value) => {
                    let entries: Vec<(AccountId, u64)> = rmp_serde::from_slice(value.value())?;
                    entries.into_iter().collect()
                }
                None => HashMap::new(),
            }
        };

        // CostBasisTracker snapshot (C1). Missing-row → default (cold start).
        let cost_basis_tracker: CostBasisTrackerSnapshot = {
            let table = txn.open_table(COST_BASIS_TRACKER)?;
            match table.get(KEY_COST_BASIS_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => CostBasisTrackerSnapshot::default(),
            }
        };

        let history_event_next_seq = {
            let counters = txn.open_table(COUNTERS)?;
            counters
                .get(KEY_HISTORY_EVENT_NEXT_SEQ)?
                .map(|value| value.value())
        };
        let history_event_next_seq = match history_event_next_seq {
            Some(seq) => seq,
            None => {
                let table = txn.open_table(HISTORY_EVENTS)?;
                let mut max_seq = None;
                for entry in table.iter()? {
                    let (key, _) = entry?;
                    let Some(seq) = seq_from_history_event_key(key.value()) else {
                        warn!("invalid history event key in store, skipping for next_seq recovery");
                        continue;
                    };
                    max_seq = Some(max_seq.map_or(seq, |current: u64| current.max(seq)));
                }
                max_seq.map_or(0, |seq| seq.saturating_add(1))
            }
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
            control_plane_log = control_plane_log.len(),
            account_fills = account_fills.len(),
            data_feeds = data_feeds.len(),
            resolution_templates = resolution_templates.len(),
            bridge_deposit_cursor = bridge_state.deposit_cursor,
            pending_l1_deposits = pending_l1_deposits.len(),
            pending_bridge_withdrawals = pending_bridge_withdrawals.len(),
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
            resting_orders,
            data_feeds,
            resolution_templates,
            pending_bundles,
            admit_log,
            control_plane_log,
            analytics: AnalyticsRestoredState {
                last_clearing_prices,
                market_volumes,
                account_fills,
                trader_tracker,
                price_tracker_volume,
                price_tracker_clearing_history,
                liquidity_tracker,
                order_stats_tracker,
                welfare_tracker,
                first_deposit_ms,
                fill_total_counts,
                cost_basis_tracker,
                history_event_next_seq,
            },
            bridge_state,
            pending_l1_deposits,
            pending_bridge_withdrawals,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn ensure_state_qmdb_root(
        &self,
        account_state: RecoveryAccountState,
        accounts: &AccountStore,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
        market_statuses: &HashMap<MarketId, MarketStatus>,
        market_metadata: &HashMap<MarketId, MarketMetadata>,
        resting_orders: &[RestingOrder],
        bridge_state: &BridgeState,
        header: &BlockHeader,
    ) -> Result<(), StoreError> {
        let state_root = self
            .account_state_store
            .qmdb_state_root(account_state.slot)
            .await?;
        if state_root.root == header.state_root {
            return Ok(());
        }
        metrics::counter!("sybil_store_qmdb_root_mismatch_total", "phase" => "restore")
            .increment(1);

        warn!(
            height = account_state.height,
            slot = ?account_state.slot,
            root = ?state_root.root,
            header_root = ?header.state_root,
            "typed qMDB root mismatch during restore; rebuilding fenced state slot from redb snapshot"
        );

        let mut lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));
        for (&market_id, status) in market_statuses {
            lifecycle.set_market_status(market_id, status.clone());
        }
        for (&market_id, metadata) in market_metadata {
            lifecycle.set_market_metadata(market_id, metadata.clone());
        }
        let state_sidecar = state_sidecar_snapshot_from_resting_orders(
            bridge_state,
            resting_orders,
            markets,
            market_groups,
            &lifecycle,
        );

        self.account_state_store
            .persist(CommittedAccountState {
                accounts,
                state_sidecar: &state_sidecar,
                height: account_state.height,
                next_account_id: account_state.next_account_id,
                slot: account_state.slot,
            })
            .await?;

        let repaired_root = self
            .account_state_store
            .qmdb_state_root(account_state.slot)
            .await?;
        if repaired_root.root == header.state_root {
            metrics::counter!("sybil_store_qmdb_repair_total", "result" => "success").increment(1);
            warn!(
                height = account_state.height,
                slot = ?account_state.slot,
                "repaired typed qMDB state slot from redb snapshot"
            );
            return Ok(());
        }

        warn!(
            height = account_state.height,
            slot = ?repaired_root.slot,
            root = ?repaired_root.root,
            header_root = ?header.state_root,
            "typed qMDB root still differs from committed header after repair"
        );
        metrics::counter!("sybil_store_qmdb_repair_total", "result" => "failed").increment(1);
        Err(StoreError::CorruptLayout(format!(
            "typed qMDB root mismatch at height {}: fence slot {:?} root={:?} header_root={:?}",
            account_state.height, repaired_root.slot, repaired_root.root, header.state_root
        )))
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
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_BUNDLES, bytes))
            .await
    }

    /// Append one `RestingOrder` to the admit-log WAL.
    ///
    /// Called by the actor right after `try_admit_direct` inserts a non-MM
    /// admit into the live resting book; the 200 OK only returns once this
    /// row is committed to redb. Rows are cleared atomically by `save_block`
    /// once the admit is rolled into the next `RESTING_ORDERS` snapshot.
    pub async fn append_admit_log(&self, resting: &RestingOrder) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(resting)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, ADMIT_LOG, bytes))
            .await
    }

    pub async fn append_control_plane_command(
        &self,
        command: &ControlPlaneCommand,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(command)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, CONTROL_PLANE_LOG, bytes))
            .await
    }

    pub async fn append_pending_l1_deposit(&self, deposit: &L1Deposit) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(deposit)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_L1_DEPOSITS, bytes))
            .await
    }

    pub async fn append_pending_bridge_withdrawal(
        &self,
        request: &BridgeWithdrawalRequest,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(request)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_BRIDGE_WITHDRAWALS, bytes))
            .await
    }

    /// Append this block's equity points and history events as individual rows.
    /// Append-only; standalone version used by tests and as a fallback.
    pub fn append_offblock_rows(
        &self,
        equity: &[(AccountId, crate::aggregates::EquityPoint)],
        history: &[crate::aggregates::StoredHistoryEvent],
    ) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(EQUITY_POINTS)?;
            for (aid, p) in equity {
                let key = equity_key(*aid, p.height);
                let bytes = rmp_serde::to_vec(p)?;
                t.insert(key.as_slice(), bytes.as_slice())?;
            }
            let mut h = txn.open_table(HISTORY_EVENTS)?;
            for ev in history {
                let key = history_event_key(AccountId(ev.account_id), ev.block_height, ev.seq);
                let bytes = rmp_serde::to_vec(ev)?;
                h.insert(key.as_slice(), bytes.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// Equity points for an account, oldest-first (matches `EquityTracker::series`),
    /// keeping only points with `timestamp_ms >= since_ms`. Pass `since_ms == 0`
    /// for the full series. Points are keyed by height, so the timestamp range is
    /// applied while scanning rather than as a key bound.
    pub fn equity_series(
        &self,
        account_id: AccountId,
        since_ms: u64,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EQUITY_POINTS)?;
        let lo = equity_key(account_id, 0);
        let hi = equity_key(account_id, u64::MAX);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
            let (_k, v) = entry?;
            let point: crate::aggregates::EquityPoint = rmp_serde::from_slice(v.value())?;
            if point.timestamp_ms >= since_ms {
                out.push(point);
            }
        }
        Ok(out)
    }

    /// Newest-first page of an account's history, replicating
    /// `AccountEventLog::query` (cursor `before = (block_height, seq)`,
    /// `category` filter via `HistoryKind::category`).
    pub fn account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY_EVENTS)?;
        let lo = history_event_key(account_id, 0, 0);
        let hi = history_event_key(account_id, u64::MAX, u64::MAX);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
            let (_k, v) = entry?;
            let stored: crate::aggregates::StoredHistoryEvent = rmp_serde::from_slice(v.value())?;
            if let Some((b, s)) = before {
                // Keep only events strictly before the cursor; skip the rest.
                if (stored.block_height, stored.seq) >= (b, s) {
                    continue;
                }
            }
            if let Some(ref c) = category {
                if stored.kind.category() != c.as_str() {
                    continue;
                }
            }
            out.push(stored.into_event());
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Newest-first page of an account's fills from the durable store,
    /// replicating [`crate::fill_recorder::FillRecorder::account_fills`]: a fill
    /// matches `market_id_filter` if any of its `position_deltas` touches that
    /// market, then `offset`/`limit` page over the filtered, newest-first
    /// sequence.
    ///
    /// Reads the full persisted history, which outlives the bounded in-memory
    /// recorder — so `/v1/accounts/{id}/fills` stays populated even when the hot
    /// serving window is empty (e.g. prod retention caps). Stored keys sort
    /// ascending by `(block_height, order_id)`; we iterate in reverse to serve
    /// newest-first.
    pub fn account_fills(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(FILL_HISTORY)?;
        let (lo, hi) = fill_history_account_bounds(account_id);
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
            let (_k, v) = entry?;
            let record: AccountFillRecord = rmp_serde::from_slice(v.value())?;
            let matches = market_id_filter
                .is_none_or(|mid| record.position_deltas.iter().any(|(m, _, _)| *m == mid));
            if !matches {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            out.push(record);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Oldest-first durable page of fills strictly after `after`, ordered by
    /// the stable `(block_height, order_id)` cursor.
    pub fn account_fills_after(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        after: Option<AccountFillCursor>,
        limit: usize,
    ) -> Result<Vec<AccountFillRecord>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(FILL_HISTORY)?;
        let (lo, hi) = fill_history_account_bounds(account_id);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
            let (_k, v) = entry?;
            let record: AccountFillRecord = rmp_serde::from_slice(v.value())?;
            if after.is_some_and(|cursor| AccountFillCursor::from_record(&record) <= cursor) {
                continue;
            }
            let matches = market_id_filter
                .is_none_or(|mid| record.position_deltas.iter().any(|(m, _, _)| *m == mid));
            if !matches {
                continue;
            }
            out.push(record);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}

fn append_msgpack_row_bytes(
    db: &Database,
    table: TableDefinition<u64, &[u8]>,
    bytes: Vec<u8>,
) -> Result<(), StoreError> {
    let txn = db.begin_write()?;
    let next_seq = {
        let table = txn.open_table(table)?;
        let last_key = table
            .iter()?
            .next_back()
            .transpose()?
            .map(|(k, _)| k.value());
        last_key.map(|k| k + 1).unwrap_or(0)
    };
    {
        let mut table = txn.open_table(table)?;
        table.insert(next_seq, bytes.as_slice())?;
    }
    txn.commit()?;
    Ok(())
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
    #[error("blocking store task failed: {0}")]
    BlockingTask(String),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("qmdb: {0}")]
    Qmdb(String),
    #[error("block witness header does not match persisted block header")]
    WitnessHeaderMismatch,
    #[error("unsupported store layout: {0}")]
    UnsupportedLayout(String),
    #[error("corrupt store layout: {0}")]
    CorruptLayout(String),
    #[cfg(test)]
    #[error("injected store fault: {0}")]
    InjectedFault(String),
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

fn validate_witness_header(header: &BlockHeader, witness: &BlockWitness) -> Result<(), StoreError> {
    let witness_header = &witness.header;
    if witness_header.height != header.height
        || witness_header.parent_hash != header.parent_hash
        || witness_header.state_root != header.state_root
        || witness_header.events_root != header.events_root
        || witness_header.order_count != header.order_count
        || witness_header.fill_count != header.fill_count
        || witness_header.timestamp_ms != header.timestamp_ms
    {
        return Err(StoreError::WitnessHeaderMismatch);
    }
    Ok(())
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
            events_root: [height as u8; 32],
            order_count: 0,
            fill_count: 0,
            timestamp_ms: height * 1000,
        }
    }

    fn sample_witness(header: &BlockHeader) -> BlockWitness {
        BlockWitness {
            header: header.to_witness_header(),
            previous_header: None,
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: Vec::new(),
            l1_deposits: vec![],
            fills: Vec::new(),
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: Vec::new(),
            post_system_state: Vec::new(),
            post_state: Vec::new(),
            state_sidecar: sybil_verifier::StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        }
    }

    fn sample_sealed_block(header: &BlockHeader) -> SealedBlock {
        SealedBlock {
            canonical: crate::block::Block {
                header: header.clone(),
                order_ids: Vec::new(),
                system_events: Vec::new(),
                bridge: crate::bridge::BridgeBlockData::default(),
                fills: Vec::new(),
                clearing_prices: HashMap::new(),
                rejections: Vec::new(),
            },
            analytics: crate::block::BlockAnalytics::default(),
            derived_view_sidecar: crate::block::DerivedViewSidecar::default(),
        }
    }

    fn coherent_header_and_witness(
        height: u64,
        accounts: &AccountStore,
        markets: &MarketSet,
        lifecycle: &MarketLifecycle,
        bridge_state: &BridgeState,
    ) -> (BlockHeader, BlockWitness) {
        let canonical_accounts = crate::canonical_state::CanonicalState::from_accounts(accounts);
        let state_sidecar =
            state_sidecar_snapshot_from_resting_orders(bridge_state, &[], markets, &[], lifecycle);
        let state_root = sybil_verifier::block::compute_state_root_with_sidecar(
            canonical_accounts.as_snapshots(),
            &state_sidecar,
        );
        let header = BlockHeader {
            height,
            parent_hash: [(height - 1) as u8; 32],
            state_root,
            events_root: sybil_verifier::event_commitment::empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: height * 1000,
        };
        let mut witness = sample_witness(&header);
        witness.post_state = canonical_accounts.into_snapshots();
        witness.state_sidecar = state_sidecar;
        (header, witness)
    }

    /// Owns the empty defaults for `SequencerSnapshot` references so test code
    /// doesn't have to repeat the ceremony on every call site.
    struct TestEnv {
        empty_pk: HashMap<crate::crypto::PublicKey, AccountId>,
        empty_prices: HashMap<MarketId, Vec<Nanos>>,
        empty_volumes: HashMap<MarketId, u64>,
        bridge_state: BridgeState,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                empty_pk: HashMap::new(),
                empty_prices: HashMap::new(),
                empty_volumes: HashMap::new(),
                bridge_state: BridgeState::default(),
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
                analytics: AnalyticsSnapshot {
                    last_clearing_prices: &self.empty_prices,
                    market_volumes: market_volumes.unwrap_or(&self.empty_volumes),
                    account_fills: Vec::new(),
                    trader_tracker: Default::default(),
                    price_tracker_volume: Default::default(),
                    price_tracker_clearing_history: Default::default(),
                    liquidity_tracker: Default::default(),
                    order_stats_tracker: Default::default(),
                    welfare_tracker: Default::default(),
                    first_deposit_ms: HashMap::new(),
                    fill_total_counts: HashMap::new(),
                    cost_basis_tracker: Default::default(),
                    history_event_next_seq: 0,
                    fill_history_delta: Vec::new(),
                    equity_points_delta: Vec::new(),
                    price_points_delta: Vec::new(),
                    history_events_delta: Vec::new(),
                },
                price_candle_resolutions_secs: &[],
                bridge_state: &self.bridge_state,
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
                analytics: AnalyticsSnapshot {
                    last_clearing_prices: &self.empty_prices,
                    market_volumes: &self.empty_volumes,
                    account_fills,
                    trader_tracker: Default::default(),
                    price_tracker_volume: Default::default(),
                    price_tracker_clearing_history: Default::default(),
                    liquidity_tracker: Default::default(),
                    order_stats_tracker: Default::default(),
                    welfare_tracker: Default::default(),
                    first_deposit_ms: HashMap::new(),
                    fill_total_counts: HashMap::new(),
                    cost_basis_tracker: Default::default(),
                    history_event_next_seq: 0,
                    fill_history_delta: Vec::new(),
                    equity_points_delta: Vec::new(),
                    price_points_delta: Vec::new(),
                    history_events_delta: Vec::new(),
                },
                price_candle_resolutions_secs: &[],
                bridge_state: &self.bridge_state,
                resting_orders: Vec::new(),
            }
        }

        fn snapshot_with_price_points<'a>(
            &'a self,
            accounts: &'a AccountStore,
            markets: &'a MarketSet,
            lifecycle: &'a MarketLifecycle,
            header: &'a BlockHeader,
            price_points_delta: Vec<(MarketId, crate::market_info::PricePoint)>,
        ) -> SequencerSnapshot<'a> {
            SequencerSnapshot {
                accounts,
                markets,
                market_groups: &[],
                lifecycle,
                header,
                next_order_id: 1,
                pubkey_registry: &self.empty_pk,
                analytics: AnalyticsSnapshot {
                    last_clearing_prices: &self.empty_prices,
                    market_volumes: &self.empty_volumes,
                    account_fills: Vec::new(),
                    trader_tracker: Default::default(),
                    price_tracker_volume: Default::default(),
                    price_tracker_clearing_history: Default::default(),
                    liquidity_tracker: Default::default(),
                    order_stats_tracker: Default::default(),
                    welfare_tracker: Default::default(),
                    first_deposit_ms: HashMap::new(),
                    fill_total_counts: HashMap::new(),
                    cost_basis_tracker: Default::default(),
                    history_event_next_seq: 0,
                    fill_history_delta: Vec::new(),
                    equity_points_delta: Vec::new(),
                    price_points_delta,
                    history_events_delta: Vec::new(),
                },
                price_candle_resolutions_secs: &[60, 300, 3_600],
                bridge_state: &self.bridge_state,
                resting_orders: Vec::new(),
            }
        }

        fn snapshot_with_history_events<'a>(
            &'a self,
            accounts: &'a AccountStore,
            markets: &'a MarketSet,
            lifecycle: &'a MarketLifecycle,
            header: &'a BlockHeader,
            history_event_next_seq: u64,
            history_events_delta: Vec<crate::aggregates::StoredHistoryEvent>,
        ) -> SequencerSnapshot<'a> {
            SequencerSnapshot {
                accounts,
                markets,
                market_groups: &[],
                lifecycle,
                header,
                next_order_id: 1,
                pubkey_registry: &self.empty_pk,
                analytics: AnalyticsSnapshot {
                    last_clearing_prices: &self.empty_prices,
                    market_volumes: &self.empty_volumes,
                    account_fills: Vec::new(),
                    trader_tracker: Default::default(),
                    price_tracker_volume: Default::default(),
                    price_tracker_clearing_history: Default::default(),
                    liquidity_tracker: Default::default(),
                    order_stats_tracker: Default::default(),
                    welfare_tracker: Default::default(),
                    first_deposit_ms: HashMap::new(),
                    fill_total_counts: HashMap::new(),
                    cost_basis_tracker: Default::default(),
                    history_event_next_seq,
                    fill_history_delta: Vec::new(),
                    equity_points_delta: Vec::new(),
                    price_points_delta: Vec::new(),
                    history_events_delta,
                },
                price_candle_resolutions_secs: &[],
                bridge_state: &self.bridge_state,
                resting_orders: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn witnessed_qmdb_state_root_matches_header_after_slot_reuse() {
        let path = temp_db_path("store-qmdb-root-reuse");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let mut lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        for height in 1..=3 {
            for index in 0..40 {
                let id = markets.add_binary(format!("root regression {height}-{index}"));
                lifecycle.set_market_metadata(
                    id,
                    MarketMetadata {
                        description: format!("description {height}-{index}"),
                        category: "regression".to_string(),
                        tags: vec![format!("height-{height}"), format!("market-{index}")],
                        resolution_criteria: format!("criteria {height}-{index}"),
                        expiry_timestamp_ms: 1_800_000_000_000 + height * 1000 + index,
                        created_at_ms: 1_700_000_000_000 + height * 1000 + index,
                        resolution_config: Some(crate::market_info::ResolutionConfig {
                            template: "admin_immediate".to_string(),
                        }),
                    },
                );
            }

            let (header, witness) = coherent_header_and_witness(
                height,
                &accounts,
                &markets,
                &lifecycle,
                &env.bridge_state,
            );
            store
                .save_block_with_witness(
                    env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                    &witness,
                )
                .await
                .unwrap();

            let qmdb_root = store.current_state_qmdb_root().await.unwrap().unwrap();
            assert_eq!(
                qmdb_root.root, header.state_root,
                "persisted typed-state qMDB root diverged from the committed block header at height {height}"
            );
        }
    }

    #[tokio::test]
    async fn history_pruning_deletes_blocks_and_price_points_with_metadata() {
        let path = temp_db_path("store-history-retention");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("retention");
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        for height in 1..=5 {
            let (header, witness) = coherent_header_and_witness(
                height,
                &accounts,
                &markets,
                &lifecycle,
                &env.bridge_state,
            );
            let point = crate::market_info::PricePoint {
                height,
                timestamp_ms: header.timestamp_ms,
                yes_price: Nanos(500_000_000 + height),
                no_price: Nanos(500_000_000 - height),
                volume_nanos: height * 10,
            };
            let block = sample_sealed_block(&header);
            store
                .save_block_with_witness_and_history(
                    env.snapshot_with_price_points(
                        &accounts,
                        &markets,
                        &lifecycle,
                        &header,
                        vec![(market_id, point)],
                    ),
                    &witness,
                    &block,
                )
                .await
                .unwrap();
        }

        let report = store
            .prune_history(
                5,
                sample_header(5).timestamp_ms,
                HistoryRetentionPolicy {
                    block_history_retention_blocks: 3,
                    raw_price_retention_blocks: 3,
                    price_candle_resolutions_secs: Vec::new(),
                    price_candle_retention_secs: Vec::new(),
                    prune_interval_blocks: 1,
                    prune_max_rows: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(report.blocks_full_pruned, 2);
        assert_eq!(report.price_points_pruned, 2);
        assert_eq!(report.meta.blocks_full_min_height, Some(3));
        assert_eq!(report.meta.price_points_min_height, Some(3));
        assert_eq!(report.meta.last_history_prune_height, Some(5));
        assert!(store.load_block(1).await.unwrap().is_none());
        assert!(store.load_block(2).await.unwrap().is_none());
        assert_eq!(
            store
                .load_block(3)
                .await
                .unwrap()
                .unwrap()
                .canonical
                .header
                .height,
            3
        );

        let page = store
            .load_price_history(market_id, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(page.retention_min_height, Some(3));
        let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
        assert_eq!(heights, vec![3, 4, 5]);
    }

    #[tokio::test]
    async fn history_pruning_partial_budget_keeps_metadata_at_oldest_remaining_row() {
        let path = temp_db_path("store-history-retention-budget");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("retention budget");
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        for height in 1..=5 {
            let (header, witness) = coherent_header_and_witness(
                height,
                &accounts,
                &markets,
                &lifecycle,
                &env.bridge_state,
            );
            let point = crate::market_info::PricePoint {
                height,
                timestamp_ms: header.timestamp_ms,
                yes_price: Nanos(500_000_000),
                no_price: Nanos(500_000_000),
                volume_nanos: 1,
            };
            let block = sample_sealed_block(&header);
            store
                .save_block_with_witness_and_history(
                    env.snapshot_with_price_points(
                        &accounts,
                        &markets,
                        &lifecycle,
                        &header,
                        vec![(market_id, point)],
                    ),
                    &witness,
                    &block,
                )
                .await
                .unwrap();
        }

        let report = store
            .prune_history(
                5,
                sample_header(5).timestamp_ms,
                HistoryRetentionPolicy {
                    block_history_retention_blocks: 2,
                    raw_price_retention_blocks: 2,
                    price_candle_resolutions_secs: Vec::new(),
                    price_candle_retention_secs: Vec::new(),
                    prune_interval_blocks: 1,
                    prune_max_rows: 2,
                },
            )
            .await
            .unwrap();

        assert_eq!(report.blocks_full_pruned, 2);
        assert_eq!(report.price_points_pruned, 0);
        assert_eq!(
            report.meta.blocks_full_min_height,
            Some(3),
            "block floor must not jump to target 4 while block 3 remains"
        );
        assert_eq!(
            report.meta.price_points_min_height,
            Some(1),
            "price metadata must not advance when budget is exhausted first"
        );
        assert_eq!(report.meta.last_history_prune_height, Some(5));
        assert!(store.load_block(3).await.unwrap().is_some());

        let page = store
            .load_price_history(market_id, None, None, None, 10)
            .await
            .unwrap();
        let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
        assert_eq!(heights, vec![1, 2, 3, 4, 5]);
        assert_eq!(page.retention_min_height, Some(1));
    }

    #[tokio::test]
    async fn price_candle_pruning_deletes_by_resolution_with_metadata() {
        let path = temp_db_path("store-price-candle-retention");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("candle retention");
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        for (height, timestamp_ms) in [(1, 0), (2, 60_000), (3, 300_000), (4, 600_000)] {
            let mut header = sample_header(height);
            header.timestamp_ms = timestamp_ms;
            let point = crate::market_info::PricePoint {
                height,
                timestamp_ms,
                yes_price: Nanos(500_000_000 + height),
                no_price: Nanos(500_000_000 - height),
                volume_nanos: height,
            };
            store
                .save_block(env.snapshot_with_price_points(
                    &accounts,
                    &markets,
                    &lifecycle,
                    &header,
                    vec![(market_id, point)],
                ))
                .await
                .unwrap();
        }

        let report = store
            .prune_history(
                4,
                600_000,
                HistoryRetentionPolicy {
                    block_history_retention_blocks: 0,
                    raw_price_retention_blocks: 0,
                    price_candle_resolutions_secs: vec![60, 300],
                    price_candle_retention_secs: vec![300, 600],
                    prune_interval_blocks: 1,
                    prune_max_rows: 100,
                },
            )
            .await
            .unwrap();

        assert_eq!(report.blocks_full_pruned, 0);
        assert_eq!(report.price_points_pruned, 0);
        assert_eq!(report.price_candles_pruned, 2);
        assert_eq!(
            report.meta.price_candles_min_bucket_ms.get(&60),
            Some(&300_000)
        );
        assert_eq!(report.meta.price_candles_min_bucket_ms.get(&300), Some(&0));

        let one_minute = store
            .load_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(one_minute.retention_min_bucket_ms, Some(300_000));
        assert_eq!(
            one_minute
                .candles
                .iter()
                .map(|candle| candle.bucket_start_ms)
                .collect::<Vec<_>>(),
            vec![300_000, 600_000]
        );

        let five_minute = store
            .load_price_candles(market_id, 300, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(five_minute.retention_min_bucket_ms, Some(0));
        assert_eq!(
            five_minute
                .candles
                .iter()
                .map(|candle| candle.bucket_start_ms)
                .collect::<Vec<_>>(),
            vec![0, 300_000, 600_000]
        );
    }

    #[tokio::test]
    async fn price_candle_pruning_obeys_batch_limit_and_keeps_floor_actual() {
        let path = temp_db_path("store-price-candle-retention-budget");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("candle retention budget");
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        for (height, timestamp_ms) in [(1, 0), (2, 60_000), (3, 120_000)] {
            let mut header = sample_header(height);
            header.timestamp_ms = timestamp_ms;
            let point = crate::market_info::PricePoint {
                height,
                timestamp_ms,
                yes_price: Nanos(500_000_000),
                no_price: Nanos(500_000_000),
                volume_nanos: 1,
            };
            store
                .save_block(env.snapshot_with_price_points(
                    &accounts,
                    &markets,
                    &lifecycle,
                    &header,
                    vec![(market_id, point)],
                ))
                .await
                .unwrap();
        }

        let report = store
            .prune_history(
                3,
                180_000,
                HistoryRetentionPolicy {
                    block_history_retention_blocks: 0,
                    raw_price_retention_blocks: 0,
                    price_candle_resolutions_secs: vec![60],
                    price_candle_retention_secs: vec![60],
                    prune_interval_blocks: 1,
                    prune_max_rows: 1,
                },
            )
            .await
            .unwrap();

        assert_eq!(report.price_candles_pruned, 1);
        assert_eq!(
            report.meta.price_candles_min_bucket_ms.get(&60),
            Some(&60_000),
            "floor must remain at the oldest actual retained candle while the prune budget is exhausted"
        );

        let page = store
            .load_price_candles(market_id, 60, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(page.retention_min_bucket_ms, Some(60_000));
        assert_eq!(
            page.candles
                .iter()
                .map(|candle| candle.bucket_start_ms)
                .collect::<Vec<_>>(),
            vec![60_000, 120_000]
        );
    }

    #[tokio::test]
    async fn price_candles_merge_committed_points_without_empty_buckets() {
        let path = temp_db_path("store-price-candles");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("candles");
        let accounts = AccountStore::new();
        let env = TestEnv::new();

        let samples = [
            (1, 1_000, 500_000_000, 500_000_000, 10),
            (2, 20_000, 700_000_000, 300_000_000, 20),
            (3, 65_000, 600_000_000, 400_000_000, 30),
        ];
        for (height, timestamp_ms, yes_price, no_price, volume_nanos) in samples {
            let mut header = sample_header(height);
            header.timestamp_ms = timestamp_ms;
            let point = crate::market_info::PricePoint {
                height,
                timestamp_ms,
                yes_price: Nanos(yes_price),
                no_price: Nanos(no_price),
                volume_nanos,
            };
            store
                .save_block(env.snapshot_with_price_points(
                    &accounts,
                    &markets,
                    &lifecycle,
                    &header,
                    vec![(market_id, point)],
                ))
                .await
                .unwrap();
        }

        let page = store
            .load_price_candles(market_id, 60, Some(0), Some(180_000), None, 10)
            .await
            .unwrap();
        assert_eq!(page.resolution_secs, 60);
        assert_eq!(
            page.candles.len(),
            2,
            "no synthetic empty bucket should be stored"
        );

        let first = &page.candles[0];
        assert_eq!(first.bucket_start_ms, 0);
        assert_eq!(first.bucket_end_ms, 60_000);
        assert_eq!(first.first_height, 1);
        assert_eq!(first.last_height, 2);
        assert_eq!(first.open_yes_price, Nanos(500_000_000));
        assert_eq!(first.high_yes_price, Nanos(700_000_000));
        assert_eq!(first.low_yes_price, Nanos(500_000_000));
        assert_eq!(first.close_yes_price, Nanos(700_000_000));
        assert_eq!(first.open_no_price, Nanos(500_000_000));
        assert_eq!(first.high_no_price, Nanos(500_000_000));
        assert_eq!(first.low_no_price, Nanos(300_000_000));
        assert_eq!(first.close_no_price, Nanos(300_000_000));
        assert_eq!(first.volume_nanos, 30);
        assert_eq!(first.point_count, 2);

        let second = &page.candles[1];
        assert_eq!(second.bucket_start_ms, 60_000);
        assert_eq!(second.first_height, 3);
        assert_eq!(second.last_height, 3);
        assert_eq!(second.open_yes_price, Nanos(600_000_000));
        assert_eq!(second.close_yes_price, Nanos(600_000_000));
        assert_eq!(second.volume_nanos, 30);
        assert_eq!(second.point_count, 1);

        let newest = store
            .load_price_candles(market_id, 60, None, None, None, 1)
            .await
            .unwrap();
        assert_eq!(newest.next_before_ms, Some(60_000));
        assert_eq!(newest.candles[0].bucket_start_ms, 60_000);

        let older = store
            .load_price_candles(market_id, 60, None, None, newest.next_before_ms, 1)
            .await
            .unwrap();
        assert_eq!(older.next_before_ms, None);
        assert_eq!(older.candles[0].bucket_start_ms, 0);
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
    async fn test_store_recovery_treats_redb_fence_as_commit_point() {
        let path = temp_db_path("store-redb-fence-commit-point");
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();

        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(100);
        let (header_1, witness_1) =
            coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);

        let mut accounts_after_uncommitted_qmdb = accounts.clone();
        accounts_after_uncommitted_qmdb
            .get_mut(account_id)
            .unwrap()
            .balance = 200;
        let (header_2, witness_2) = coherent_header_and_witness(
            2,
            &accounts_after_uncommitted_qmdb,
            &markets,
            &lifecycle,
            &env.bridge_state,
        );

        {
            let store = Store::open(&path).unwrap();
            store
                .save_block_with_witness(
                    env.snapshot(&accounts, &markets, &lifecycle, &header_1, 1, None, vec![]),
                    &witness_1,
                )
                .await
                .unwrap();

            let committed_root = store.current_state_qmdb_root().await.unwrap().unwrap();
            assert_eq!(committed_root.slot, AccountSnapshotSlot::A);
            assert_eq!(committed_root.root, header_1.state_root);

            // Simulate a crash after the inactive qMDB slot was written but
            // before redb committed the fence flip for height 2.
            store
                .account_state_store
                .persist(CommittedAccountState {
                    accounts: &accounts_after_uncommitted_qmdb,
                    state_sidecar: &witness_2.state_sidecar,
                    height: header_2.height,
                    next_account_id: accounts_after_uncommitted_qmdb.next_id(),
                    slot: AccountSnapshotSlot::B,
                })
                .await
                .unwrap();

            let uncommitted_root = store.state_qmdb_root(AccountSnapshotSlot::B).await.unwrap();
            assert_eq!(uncommitted_root.root, header_2.state_root);
            let still_committed_root = store.current_state_qmdb_root().await.unwrap().unwrap();
            assert_eq!(still_committed_root.slot, AccountSnapshotSlot::A);
            assert_eq!(still_committed_root.root, header_1.state_root);
        }

        let reopened = Store::open(&path).unwrap();
        let restored = reopened.load_state().await.unwrap().unwrap();
        assert_eq!(restored.height, 1);
        assert_eq!(restored.accounts.get(account_id).unwrap().balance, 100);
        let restored_root = reopened.current_state_qmdb_root().await.unwrap().unwrap();
        assert_eq!(restored_root.slot, AccountSnapshotSlot::A);
        assert_eq!(restored_root.root, header_1.state_root);

        // Once save_block completes its redb transaction, the same qMDB slot is
        // authoritative after restart.
        reopened
            .save_block_with_witness(
                env.snapshot(
                    &accounts_after_uncommitted_qmdb,
                    &markets,
                    &lifecycle,
                    &header_2,
                    1,
                    None,
                    vec![],
                ),
                &witness_2,
            )
            .await
            .unwrap();
        drop(reopened);

        let reopened_after_commit = Store::open(&path).unwrap();
        let restored_after_commit = reopened_after_commit.load_state().await.unwrap().unwrap();
        assert_eq!(restored_after_commit.height, 2);
        assert_eq!(
            restored_after_commit
                .accounts
                .get(account_id)
                .unwrap()
                .balance,
            200
        );
        let committed_after_flip = reopened_after_commit
            .current_state_qmdb_root()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(committed_after_flip.slot, AccountSnapshotSlot::B);
        assert_eq!(committed_after_flip.root, header_2.state_root);
    }

    #[tokio::test]
    async fn test_store_restores_history_event_next_seq() {
        use crate::aggregates::{HistoryEvent, HistoryKind, StoredHistoryEvent};

        let path = temp_db_path("store-history-next-seq");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();

        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(100);
        let header = sample_header(1);
        let mut placed = HistoryEvent::new(
            account_id,
            HistoryKind::Placed,
            header.height,
            header.timestamp_ms,
        );
        placed.seq = 0;
        let mut filled = HistoryEvent::new(
            account_id,
            HistoryKind::Filled,
            header.height,
            header.timestamp_ms,
        );
        filled.seq = 1;
        let history_events_delta = vec![
            StoredHistoryEvent::from_event(&placed),
            StoredHistoryEvent::from_event(&filled),
        ];

        store
            .save_block(env.snapshot_with_history_events(
                &accounts,
                &markets,
                &lifecycle,
                &header,
                2,
                history_events_delta,
            ))
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.analytics.history_event_next_seq, 2);

        // Backward compatibility for stores created before the explicit counter:
        // derive the next cursor from existing history-event keys.
        let txn = store.db.begin_write().unwrap();
        {
            let mut counters = txn.open_table(COUNTERS).unwrap();
            counters.remove(KEY_HISTORY_EVENT_NEXT_SEQ).unwrap();
        }
        txn.commit().unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.analytics.history_event_next_seq, 2);
    }

    #[tokio::test]
    async fn save_block_with_witness_persists_latest_witness() {
        let path = temp_db_path("store-witness");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();
        let mut accounts = AccountStore::new();
        accounts.create_account(100);

        let (header, witness) =
            coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();

        let latest = store
            .latest_block_witness()
            .unwrap()
            .expect("latest witness persisted");
        let by_height = store
            .block_witness(header.height)
            .unwrap()
            .expect("height witness persisted");

        assert_eq!(latest.header.height, header.height);
        assert_eq!(latest.header.state_root, header.state_root);
        assert_eq!(by_height.header.height, header.height);
    }

    #[tokio::test]
    async fn save_block_with_witness_prunes_historical_witnesses() {
        let path = temp_db_path("store-witness-prune");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();
        let mut accounts = AccountStore::new();
        accounts.create_account(100);

        let (header1, witness1) =
            coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header1, 1, None, vec![]),
                &witness1,
            )
            .await
            .unwrap();

        let (header2, witness2) =
            coherent_header_and_witness(2, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header2, 1, None, vec![]),
                &witness2,
            )
            .await
            .unwrap();

        assert!(store.block_witness(header1.height).unwrap().is_none());
        let latest = store
            .latest_block_witness()
            .unwrap()
            .expect("latest witness retained");
        assert_eq!(latest.header.height, header2.height);
    }

    #[tokio::test]
    async fn save_block_with_witness_rejects_mismatched_header() {
        let path = temp_db_path("store-witness-mismatch");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();
        let accounts = AccountStore::new();

        let header = sample_header(1);
        let mut witness = sample_witness(&header);
        witness.header.height = 2;

        let error = store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, StoreError::WitnessHeaderMismatch));
    }

    #[tokio::test]
    async fn save_block_without_witness_clears_stale_witness_for_height() {
        let path = temp_db_path("store-witness-clear");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();
        let accounts = AccountStore::new();

        let (header, witness) =
            coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();
        assert!(store.latest_block_witness().unwrap().is_some());

        store
            .save_block(env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]))
            .await
            .unwrap();

        assert!(store.latest_block_witness().unwrap().is_none());
    }

    #[tokio::test]
    async fn test_store_restores_pending_bridge_wals() {
        let path = temp_db_path("store-bridge-wal");
        let store = Store::open(&path).unwrap();
        let oracle = Arc::new(AdminOracle::new());
        let lifecycle = MarketLifecycle::new(oracle);
        let markets = MarketSet::new();
        let env = TestEnv::new();

        let mut accounts = AccountStore::new();
        let account_id = accounts.create_account(0);
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

        let deposit = L1Deposit {
            deposit_id: 1,
            account_id,
            chain_id: 1,
            vault_address: [0x10; 20],
            token_address: [0x20; 20],
            sender: [0x30; 20],
            sybil_account_key: crate::bridge::account_key(account_id),
            amount_token_units: 10_000,
            deposit_root: [0x44; 32],
        };
        let withdrawal = BridgeWithdrawalRequest {
            account_id,
            chain_id: 1,
            vault_address: [0x10; 20],
            recipient: [0x40; 20],
            token_address: [0x20; 20],
            amount_token_units: 4_000,
            expiry_height: 10,
        };
        store.append_pending_l1_deposit(&deposit).await.unwrap();
        store
            .append_pending_bridge_withdrawal(&withdrawal)
            .await
            .unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        assert_eq!(restored.pending_l1_deposits, vec![deposit]);
        assert_eq!(restored.pending_bridge_withdrawals, vec![withdrawal]);
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
            restored.analytics.market_volumes.get(&market_id),
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
        book.accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
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
        book.accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
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
            fill_price: Nanos(600_000_000),
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
        assert_eq!(
            restored.analytics.account_fills,
            vec![(account_id, fill.clone())]
        );

        let seq = crate::sequencer::BlockSequencer::restore(
            restored,
            oracle,
            crate::sequencer::SequencerConfig::default(),
        );
        assert_eq!(
            seq.analytics()
                .account_fills(account_id, Some(market_id), 10, 0),
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
            !seq.analytics().account_fills(buyer, None, 10, 0).is_empty(),
            "sanity check: block should record buyer fills before persistence"
        );
        store.save_block(seq.snapshot()).await.unwrap();

        let restored = store.load_state().await.unwrap().unwrap();
        let restored_seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
        let fills = restored_seq
            .analytics()
            .account_fills(buyer, Some(market_id), 10, 0);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].fill_qty, 5);
        assert_eq!(fills[0].block_height, 1);
    }

    #[tokio::test]
    async fn test_store_account_fills_reads_full_persisted_history() {
        use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
        use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-account-fills-read");
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
        // Two blocks, each crossing one unit, so two distinct buyer fills persist.
        for height in 1..=2u64 {
            seq.produce_block(
                vec![
                    OrderSubmission {
                        account_id: buyer,
                        orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                        mm_constraint: None,
                    },
                    OrderSubmission {
                        account_id: seller,
                        orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                        mm_constraint: None,
                    },
                ],
                height * 1_000,
            );
            store.save_block(seq.snapshot()).await.unwrap();
        }

        // Reads straight from the durable store, independent of any in-memory recorder.
        let fills = store.account_fills(buyer, None, 10, 0).unwrap();
        assert_eq!(fills.len(), 2, "both persisted fills should be served");
        assert!(store.account_fills(buyer, None, 0, 0).unwrap().is_empty());
        // Newest-first: block 2 ahead of block 1.
        assert_eq!(fills[0].block_height, 2);
        assert_eq!(fills[1].block_height, 1);

        let forward = store
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
            .unwrap();
        assert_eq!(forward.len(), 2);
        assert!(store
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 0)
            .unwrap()
            .is_empty());
        assert_eq!(forward[0].block_height, 1);
        assert_eq!(forward[1].block_height, 2);
        let cursor = AccountFillCursor::from_record(&forward[0]);
        let after_first = store
            .account_fills_after(buyer, None, Some(cursor), 10)
            .unwrap();
        assert_eq!(after_first.len(), 1);
        assert_eq!(after_first[0].block_height, 2);

        // Market filter keeps fills that touch the traded market...
        assert_eq!(
            store
                .account_fills(buyer, Some(market_id), 10, 0)
                .unwrap()
                .len(),
            2
        );
        // ...and drops everything for a market the account never traded.
        let untraded = markets.add_binary("Untraded");
        assert!(store
            .account_fills(buyer, Some(untraded), 10, 0)
            .unwrap()
            .is_empty());

        // offset/limit page over the newest-first sequence.
        let page = store.account_fills(buyer, None, 1, 1).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].block_height, 1);

        // Unknown account => empty, no error.
        assert!(store
            .account_fills(AccountId(99), None, 10, 0)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_store_fill_cursor_pagination_survives_reopen() {
        use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
        use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-account-fills-cursor-reopen");
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
            oracle,
            SequencerConfig::default(),
        );
        for height in 1..=2u64 {
            let prepared = seq
                .prepare_block(
                    vec![
                        OrderSubmission {
                            account_id: buyer,
                            orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                            mm_constraint: None,
                        },
                        OrderSubmission {
                            account_id: seller,
                            orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                            mm_constraint: None,
                        },
                    ],
                    height * 1_000,
                )
                .unwrap();
            store
                .save_block(prepared.next_sequencer().snapshot())
                .await
                .unwrap();
            seq.commit_prepared_block(prepared).unwrap();
        }
        drop(store);

        let reopened = Store::open(&path).unwrap();
        let fills = reopened
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
            .unwrap();
        let heights: Vec<u64> = fills.iter().map(|fill| fill.block_height).collect();
        assert_eq!(heights, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_store_persists_fill_delta_when_hot_cap_is_zero() {
        use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
        use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-fill-cap-zero");
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

        let config = SequencerConfig {
            max_fill_history_per_account: 0,
            ..SequencerConfig::default()
        };
        let mut seq =
            BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], oracle, config);

        let prepared = seq
            .prepare_block(
                vec![
                    OrderSubmission {
                        account_id: buyer,
                        orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                        mm_constraint: None,
                    },
                    OrderSubmission {
                        account_id: seller,
                        orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                        mm_constraint: None,
                    },
                ],
                1_000,
            )
            .unwrap();

        assert!(prepared
            .next_sequencer()
            .analytics()
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
            .is_empty());

        store
            .save_block(prepared.next_sequencer().snapshot())
            .await
            .unwrap();
        seq.commit_prepared_block(prepared).unwrap();
        assert!(seq
            .analytics()
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
            .is_empty());

        let durable = store
            .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
            .unwrap();
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].block_height, 1);
        drop(store);

        let reopened = Store::open(&path).unwrap();
        let reopened_fills = reopened
            .account_fills_after(buyer, Some(market_id), Some(AccountFillCursor::MIN), 10)
            .unwrap();
        assert_eq!(reopened_fills.len(), 1);
        assert_eq!(reopened_fills[0].fill_qty, 1);
    }

    #[tokio::test]
    async fn test_store_reopens_after_committed_trade_and_restores_qmdb_state() {
        use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
        use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

        let path = temp_db_path("store-reopen-smoke");
        let oracle = Arc::new(AdminOracle::new());

        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Persistent restart");
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
        let production = seq.produce_block(
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
        assert_eq!(production.block.header.height, 1);
        assert!(!production.block.fills.is_empty());

        {
            let store = Store::open(&path).unwrap();
            store
                .save_block_with_witness(seq.snapshot(), &production.witness)
                .await
                .unwrap();
        }

        let reopened = Store::open(&path).unwrap();
        let qmdb_root = reopened
            .current_state_qmdb_root()
            .await
            .unwrap()
            .expect("committed qMDB state root exists after reopen");
        let reopened_leaves = reopened.state_qmdb_leaves(qmdb_root.slot).await.unwrap();
        let expected_leaves = sybil_verifier::block::state_root_leaves(
            &production.witness.post_state,
            &production.witness.state_sidecar,
        );
        assert_eq!(reopened_leaves, expected_leaves);
        assert_eq!(
            sybil_verifier::block::state_root_from_leaves(&reopened_leaves),
            production.block.header.state_root
        );
        assert_eq!(qmdb_root.root, production.block.header.state_root);

        let buyer_key = sybil_verifier::state_schema::account_leaf_key(buyer.0);
        let buyer_proof = reopened
            .current_state_qmdb_leaf_proof(&buyer_key)
            .await
            .unwrap()
            .expect("buyer account leaf proof exists after reopen");
        assert_eq!(buyer_proof.root, production.block.header.state_root);
        assert_eq!(buyer_proof.leaf_key, buyer_key);

        let restored = reopened.load_state().await.unwrap().unwrap();
        assert_eq!(restored.height, 1);
        assert_eq!(
            restored.accounts.get(buyer).unwrap().position(market_id, 0),
            5
        );
        assert_eq!(
            restored
                .accounts
                .get(seller)
                .unwrap()
                .position(market_id, 0),
            5
        );
        assert!(
            restored
                .analytics
                .market_volumes
                .get(&market_id)
                .copied()
                .unwrap_or(0)
                > 0
        );

        let restored_seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
        let fills = restored_seq
            .analytics()
            .account_fills(buyer, Some(market_id), 10, 0);
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
            .accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
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
        use sybil_oracle::{FeedId, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

        let path = temp_db_path("store-data-feeds");
        let store = Store::open(&path).unwrap();

        let oracle = Arc::new(AdminOracle::new());
        let mut lifecycle = MarketLifecycle::new(oracle);
        lifecycle.register_feed(FeedPubkey(vec![1u8; 33]), "admin".into(), 100);
        lifecycle.register_feed(FeedPubkey(vec![2u8; 33]), "polymarket_mirror".into(), 200);
        lifecycle.install_template(ResolutionTemplate {
            id: TemplateId("polymarket_mirror".to_string()),
            policy: ResolutionPolicy::Immediate { feed_id: FeedId(1) },
        });

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
        assert_eq!(restored.resolution_templates.len(), 1);
        assert_eq!(
            restored.resolution_templates[0].id,
            TemplateId("polymarket_mirror".to_string())
        );
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
        let mut constraint = MmConstraint::new(MmId(1), Nanos(5 * NANOS_PER_DOLLAR));
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

    #[test]
    fn equity_and_history_rows_roundtrip() {
        use crate::account::AccountId;
        use crate::aggregates::{EquityPoint, HistoryEvent, HistoryKind, StoredHistoryEvent};

        let path = temp_db_path("equity-history-roundtrip");
        let store = Store::open(&path).unwrap();
        let aid = AccountId(7);

        let pts = vec![
            EquityPoint {
                height: 1,
                timestamp_ms: 1_000,
                portfolio_value_nanos: 100,
                deposited_nanos: 100,
            },
            EquityPoint {
                height: 2,
                timestamp_ms: 2_000,
                portfolio_value_nanos: 150,
                deposited_nanos: 100,
            },
        ];
        let mut e1 = HistoryEvent::new(aid, HistoryKind::Placed, 1, 1_000);
        e1.seq = 0;
        let mut e2 = HistoryEvent::new(aid, HistoryKind::Filled, 2, 2_000);
        e2.seq = 1;
        let events: Vec<StoredHistoryEvent> = vec![
            StoredHistoryEvent::from_event(&e1),
            StoredHistoryEvent::from_event(&e2),
        ];

        store
            .append_offblock_rows(&pts.iter().map(|p| (aid, *p)).collect::<Vec<_>>(), &events)
            .unwrap();

        // Equity: oldest-first, all points.
        let got = store.equity_series(aid, 0).unwrap();
        assert_eq!(got, pts);

        // History: newest-first, filtered + paged like AccountEventLog::query.
        let all = store.account_events(aid, 10, None, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, HistoryKind::Filled); // newest first
        assert!(store.account_events(aid, 0, None, None).unwrap().is_empty());

        let trades = store
            .account_events(aid, 10, None, Some("trades".into()))
            .unwrap();
        assert_eq!(trades.len(), 2);

        // Cursor before (2, 1) excludes the Filled@(2,1) event.
        let page = store.account_events(aid, 10, Some((2, 1)), None).unwrap();
        assert!(page.iter().all(|e| !(e.block_height == 2 && e.seq == 1)));

        // Unknown account → empty.
        assert!(store.equity_series(AccountId(99), 0).unwrap().is_empty());
        assert!(store
            .account_events(AccountId(99), 10, None, None)
            .unwrap()
            .is_empty());
    }
}
