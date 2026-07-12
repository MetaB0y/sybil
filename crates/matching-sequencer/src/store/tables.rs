use super::*;

// ---------------------------------------------------------------------------
// Table definitions
// ---------------------------------------------------------------------------

/// Markets: market_id (u32) → msgpack(Market)
pub(super) const MARKETS: TableDefinition<u32, &[u8]> = TableDefinition::new("markets");

/// Market metadata: market_id (u32) → msgpack(MarketMetadata)
pub(super) const MARKET_META: TableDefinition<u32, &[u8]> = TableDefinition::new("market_meta");

/// Data feeds: feed_id (u64) → msgpack(DataFeed). Holds every registered
/// off-chain signer identity allowed to produce resolution attestations.
/// The redb layout is additive (rmp-serde); no layout version bump needed.
pub(super) const DATA_FEEDS: TableDefinition<u64, &[u8]> = TableDefinition::new("data_feeds");

/// Resolution templates: template_id -> msgpack(ResolutionTemplate).
/// Built-in templates are reinstalled by the API on startup, but persisting
/// the registry keeps the sequencer snapshot self-contained and protects
/// operator-installed templates after the control-plane WAL is cleared.
pub(super) const RESOLUTION_TEMPLATES: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resolution_templates");

/// Market statuses: market_id (u32) → msgpack(MarketStatus)
pub(super) const MARKET_STATUSES: TableDefinition<u32, &[u8]> =
    TableDefinition::new("market_statuses");

/// Market groups: group_index (u32) → msgpack(MarketGroup)
pub(super) const MARKET_GROUPS: TableDefinition<u32, &[u8]> = TableDefinition::new("market_groups");

/// Block headers: height (u64) → msgpack(BlockHeader)
pub(super) const BLOCK_HEADERS: TableDefinition<u64, &[u8]> = TableDefinition::new("block_headers");

/// Block witnesses: height (u64) -> msgpack(BlockWitness).
/// Persisted for asynchronous witgen/prover workers. Historical qMDB slots are
/// not retained yet, so proof-job export currently targets the latest block.
pub(super) const BLOCK_WITNESSES: TableDefinition<u64, &[u8]> =
    TableDefinition::new("block_witnesses");

/// Pubkey registry: compressed_point (33 bytes) → account_id (u64)
pub(super) const PUBKEY_REGISTRY: TableDefinition<&[u8], u64> =
    TableDefinition::new("pubkey_registry");

/// Pubkey auth scheme: compressed_point (33 bytes) → scheme tag.
pub(super) const PUBKEY_AUTH_SCHEMES: TableDefinition<&[u8], u8> =
    TableDefinition::new("pubkey_auth_schemes");

/// Pubkey management metadata (SYB-60): compressed_point (33 bytes) →
/// msgpack({label, scope, created_at_ms}). Written alongside the registry each
/// block; rows for revoked keys are cleared with the registry.
pub(super) const PUBKEY_META: TableDefinition<&[u8], &[u8]> = TableDefinition::new("pubkey_meta");

/// Last clearing prices: market_id (u32) → msgpack(Vec<Nanos>)
pub(super) const CLEARING_PRICES: TableDefinition<u32, &[u8]> =
    TableDefinition::new("clearing_prices");

/// Cumulative market volumes: market_id (u32) -> total traded volume in nanos.
pub(super) const MARKET_VOLUMES: TableDefinition<u32, u64> = TableDefinition::new("market_volumes");

/// Scalar counters: name → value
pub(super) const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

/// Historical-serving metadata: retained floors and maintenance cursors.
///
/// These rows describe durable history that is actually still present. They
/// are advanced only in the same transaction that deletes old rows.
pub(super) const HISTORY_META: TableDefinition<&str, u64> = TableDefinition::new("history_meta");

/// Chain-instance metadata. `genesis_hash` is the hash of the height-1 block
/// header and scopes order/cancel signatures across fresh-genesis redeploys.
pub(super) const CHAIN_META: TableDefinition<&str, &[u8]> = TableDefinition::new("chain_meta");
pub(super) const KEY_GENESIS_HASH: &str = "genesis_hash";

/// Resting order book snapshot: single row keyed "snapshot" → msgpack(Vec<RestingOrder>).
/// Rewritten atomically each block.
pub(super) const RESTING_ORDERS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resting_orders");

pub(super) const KEY_RESTING_ORDERS_SNAPSHOT: &str = "snapshot";

/// Pending bundle submissions: monotonic seq (u64) → msgpack(OrderSubmission).
/// Append-only buffer for MM / multi-market / multi-order submissions that
/// must wait for the block-time solver path. Each admit appends one row.
/// Cleared atomically inside `save_block` when the bundles get consumed into
/// a committed block. On restart, the table is replayed into the actor's
/// in-memory pending queue so nothing submitted with a 200 OK is lost.
pub(super) const PENDING_BUNDLES: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_bundles");

/// Admit log: monotonic seq (u64) → msgpack(RestingOrder).
/// Append-only log of non-MM single-market admissions that entered the
/// resting book after the last committed block. Each admit appends one row
/// before the 200 OK returns, so a crash between admit and the next block
/// commit doesn't drop orders from `try_admit_direct`. Cleared atomically
/// inside `save_block` when those admissions become part of the next
/// `RESTING_ORDERS` snapshot; restart loads the snapshot and then replays
/// this table on top.
pub(super) const ADMIT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("admit_log");

/// Control-plane command WAL: monotonic seq (u64) -> msgpack(ControlPlaneCommand).
/// Protects acknowledged account, market, resolution, cancellation, feed, and
/// template mutations accepted after the last committed block. Cleared
/// atomically inside `save_block`.
pub(super) const CONTROL_PLANE_LOG: TableDefinition<u64, &[u8]> =
    TableDefinition::new("control_plane_log");

/// Per-account fill history: account_id || block_height || order_id →
/// msgpack(AccountFillRecord). The byte key keeps records clustered by
/// account and ordered by block for efficient restoration and future scans.
pub(super) const FILL_HISTORY: TableDefinition<&[u8], &[u8]> = TableDefinition::new("fill_history");

/// Per-account equity series. Key = account_id(8B BE) ++ height(8B BE); one
/// point per (account, block). Value = rmp-serde EquityPoint. Off-block.
pub(super) const EQUITY_POINTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("equity_points");

/// Per-account history feed. Key = account_id(8B BE) ++ block_height(8B BE) ++
/// seq(8B BE). Value = rmp-serde StoredHistoryEvent. Off-block.
pub(super) const HISTORY_EVENTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("history_events");

/// L1 bridge sidecar state: consumed deposit cursor/root and withdrawal leaves.
pub(super) const BRIDGE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("bridge_state");
pub(super) const KEY_BRIDGE_STATE: &str = "state";

/// L1 deposits observed after the last committed block. They are replayed on
/// restart and cleared atomically once a block commits them into state.
pub(super) const PENDING_L1_DEPOSITS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_l1_deposits");

/// Bridge withdrawals requested after the last committed block. They are
/// replayed on restart and cleared atomically once a block commits them.
pub(super) const PENDING_BRIDGE_WITHDRAWALS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_bridge_withdrawals");

/// Confirmed L1 withdrawal events/cursor observations acknowledged after the
/// last block. Replayed on restart and cleared with the committing block.
pub(super) const PENDING_BRIDGE_L1_INPUTS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("pending_bridge_l1_inputs");

/// Trader tracker snapshot — one row keyed "snapshot" holding the full
/// `TraderTrackerSnapshot` payload. Off-block sidecar; missing table on
/// load yields `Default::default()` (cold start until activity accumulates).
pub(super) const TRADER_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("trader_tracker");
pub(super) const KEY_TRADER_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker volume extensions: platform running total +
/// rolling hourly buckets. Stored as a single blob keyed `"snapshot"`,
/// matching the pattern set by `TRADER_TRACKER`.
pub(super) const PRICE_TRACKER_VOLUME: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_volume");
pub(super) const KEY_PRICE_TRACKER_VOLUME_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker clearing-price history: per-market hourly
/// snapshot of the first-of-hour clearing price, used by
/// `price_n_hours_ago` (24h price-delta surfaces). Separate table from
/// `PRICE_TRACKER_VOLUME` so reverting B3 drops one table cleanly.
pub(super) const PRICE_TRACKER_CLEARING_HISTORY: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_clearing_history");
pub(super) const KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT: &str = "snapshot";

/// Durable raw mark-price points. Key =
/// market_id(4B BE) ++ block_height(8B BE).
pub(super) const PRICE_POINTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("price_points");

/// Ordered retention index for raw mark-price points. Key =
/// block_height(8B BE) ++ market_id(4B BE). Value is unused.
pub(super) const PRICE_POINTS_BY_HEIGHT: TableDefinition<&[u8], u64> =
    TableDefinition::new("price_points_by_height");

/// Downsampled committed-batch price candles. Key =
/// market_id(4B BE) ++ resolution_secs(4B BE) ++ bucket_start_ms(8B BE).
pub(super) const PRICE_CANDLES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("price_candles");

/// Ordered retention index for price candles. Key =
/// resolution_secs(4B BE) ++ bucket_start_ms(8B BE) ++ market_id(4B BE).
/// Value is unused.
pub(super) const PRICE_CANDLES_BY_RESOLUTION: TableDefinition<&[u8], u64> =
    TableDefinition::new("price_candles_by_resolution");

/// Off-block liquidity tracker: per-market ±band depth rings used by the
/// `liquidity_avg10` surface. Same single-blob shape as `TRADER_TRACKER`.
pub(super) const LIQUIDITY_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("liquidity_tracker");
pub(super) const KEY_LIQUIDITY_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block order stats tracker (B6): placed / matched / unmatched
/// counters per market + platform + hourly platform buckets. Single-blob
/// shape; missing table on load yields `OrderStatsTrackerSnapshot::default()`.
pub(super) const ORDER_STATS_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("order_stats_tracker");
pub(super) const KEY_ORDER_STATS_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block welfare tracker: cumulative platform welfare running total +
/// rolling hourly buckets for the 24h window. Single-blob shape; missing
/// table on load yields `WelfareTrackerSnapshot::default()`.
pub(super) const WELFARE_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("welfare_tracker");
pub(super) const KEY_WELFARE_TRACKER_SNAPSHOT: &str = "snapshot";

/// First-deposit timestamps per account (B8). Single blob keyed
/// "snapshot"; missing-row yields an empty map.
pub(super) const FIRST_DEPOSIT_MS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("first_deposit_ms");
pub(super) const KEY_FIRST_DEPOSIT_MS_SNAPSHOT: &str = "snapshot";

/// All-time per-account fill counters (B8). The bounded fill window
/// lives in FILL_HISTORY; the counter survives trim and restart.
pub(super) const FILL_TOTAL_COUNTS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("fill_total_counts");
pub(super) const KEY_FILL_TOTAL_COUNTS_SNAPSHOT: &str = "snapshot";

/// Off-block cost-basis tracker (C1): weighted-average entry price per
/// (account, market, outcome) + realized PnL per account. Single blob
/// keyed "snapshot"; missing row yields `CostBasisTrackerSnapshot::default()`
/// (cold start until activity accumulates).
pub(super) const COST_BASIS_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("cost_basis_tracker");
pub(super) const KEY_COST_BASIS_TRACKER_SNAPSHOT: &str = "snapshot";

/// Durable auto-resolution review-board decisions. Keyed by market_id and
/// off-block: these rows gate resolver automation, not settlement verification.
pub(super) const AUTO_RESOLUTION_RECORDS: TableDefinition<u32, &[u8]> =
    TableDefinition::new("auto_resolution_records");

// Counter keys
pub(super) const KEY_STORE_LAYOUT_VERSION: &str = "store_layout_version";
pub(super) const KEY_HEIGHT: &str = "height";
pub(super) const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
pub(super) const KEY_NEXT_MARKET_ID: &str = "next_market_id";
pub(super) const KEY_NEXT_ORDER_ID: &str = "next_order_id";
pub(super) const KEY_ACCOUNT_STATE_HEIGHT: &str = "account_state_height";
pub(super) const KEY_ACCOUNT_STATE_SLOT: &str = "account_state_slot";
pub(super) const KEY_HISTORY_EVENT_NEXT_SEQ: &str = "history_event_next_seq";
pub(super) const KEY_BLOCKS_FULL_MIN_HEIGHT: &str = "blocks_full_min_height";
pub(super) const KEY_PRICE_POINTS_MIN_HEIGHT: &str = "price_points_min_height";
pub(super) const KEY_LAST_HISTORY_PRUNE_HEIGHT: &str = "last_history_prune_height";
pub(super) const KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX: &str = "price_candles_min_bucket_ms:";

pub(super) const STORE_LAYOUT_VERSION: u64 = 1;

// TODO: Tier 2 tables (remaining)
// const MM_STATE: TableDefinition<u32, &[u8]> = TableDefinition::new("mm_state");

/// Full API replay block by height. Unlike `BLOCK_HEADERS`/`BLOCK_WITNESSES`,
/// this is historical serving data and is not pruned to latest-only.
pub(super) const BLOCKS_FULL: TableDefinition<u64, &[u8]> = TableDefinition::new("blocks_full");

/// Canonical witness payload bytes plus typed DA manifest by block height.
///
/// This is a serving/availability artifact, not a recovery commit fence. Rows
/// are best-effort and pruned with the existing `blocks_full` retention floor.
pub(super) const DA_ARTIFACTS: TableDefinition<u64, &[u8]> = TableDefinition::new("da_artifacts");
/// Publish-time DA metadata cached independently from the large payload row so
/// public manifest reads never deserialize or hash witness bytes.
pub(super) const DA_MANIFESTS: TableDefinition<u64, &[u8]> = TableDefinition::new("da_manifests");

// TODO: Tier 3 tables (remaining)
// const PRICE_HISTORY: TableDefinition<u64, &[u8]> = TableDefinition::new("price_history");
