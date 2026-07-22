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

/// Private, versioned product-history batches awaiting durable ingestion by
/// `sybil-history`. One compact row is written atomically with the block fence;
/// per-record indexes and rollups belong to the projector, not this database.
pub(super) const PRODUCT_HISTORY_OUTBOX: TableDefinition<u64, &[u8]> =
    TableDefinition::new("history_outbox_v1");
/// Exact logical payload-byte accounting for the product-history source
/// outbox. This is updated in the same transaction as row insertion/deletion;
/// it deliberately excludes redb keys, pages, indexes, and fragmentation.
pub(super) const PRODUCT_HISTORY_OUTBOX_META: TableDefinition<&str, u64> =
    TableDefinition::new("history_outbox_meta");
pub(super) const KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES: &str = "payload_bytes";
pub(super) const KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS: &str = "oldest_committed_at_ms";

/// Portable state-transition proof jobs captured before qMDB slot rotation.
/// Rows are retained until the prover has durably acknowledged the exact byte
/// digest; pruning policy is intentionally separate from block-history
/// retention because old qMDB proof material cannot be reconstructed.
pub(super) const PROOF_JOB_OUTBOX: TableDefinition<u64, &[u8]> =
    TableDefinition::new("proof_job_outbox");
/// Height -> acknowledged exact transport digest.
pub(super) const PROOF_JOB_ACKS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("proof_job_acks");
/// Durable cursor for bounded proof-job retention scans. A rotating cursor is
/// required because old unacknowledged jobs must survive without permanently
/// starving acknowledged rows at greater heights.
pub(super) const PROOF_JOB_RETENTION_META: TableDefinition<&str, u64> =
    TableDefinition::new("proof_job_retention_meta");
pub(super) const KEY_PROOF_JOB_RETENTION_NEXT_SCAN_HEIGHT: &str = "next_scan_height";

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

/// Genesis-bound service account provisioning receipts. The key is a
/// domain-separated digest of `(genesis_hash, caller_key)` and the value binds
/// the exact request digest to the allocated account id.
pub(super) const SERVICE_ACCOUNT_RECEIPTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("service_account_receipts");

/// Last clearing prices: market_id (u32) → msgpack(Vec<Nanos>)
pub(super) const CLEARING_PRICES: TableDefinition<u32, &[u8]> =
    TableDefinition::new("clearing_prices");

/// Cumulative market volumes: market_id (u32) -> total traded volume in nanos.
pub(super) const MARKET_VOLUMES: TableDefinition<u32, u64> = TableDefinition::new("market_volumes");

/// Scalar counters: name → value
pub(super) const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

/// Retention metadata for canonical replay blocks and paired DA serving rows.
/// The physical name is retained so existing dev stores remain readable;
/// product/account-history metadata belongs to `sybil-history`.
pub(super) const CANONICAL_ARCHIVE_META: TableDefinition<&str, u64> =
    TableDefinition::new("history_meta");

/// Chain-instance metadata. `genesis_hash` is the hash of the height-1 block
/// header and scopes order/cancel signatures across fresh-genesis redeploys.
pub(super) const CHAIN_META: TableDefinition<&str, &[u8]> = TableDefinition::new("chain_meta");
pub(super) const KEY_GENESIS_HASH: &str = "genesis_hash";
pub(super) const KEY_VALIDITY_ARTIFACT_RETENTION: &str = "validity_artifact_retention";

/// Resting order book snapshot: single row keyed "snapshot" → msgpack(Vec<RestingOrder>).
/// Rewritten atomically each block.
pub(super) const RESTING_ORDERS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resting_orders");

pub(super) const KEY_RESTING_ORDERS_SNAPSHOT: &str = "snapshot";

/// Global acknowledged-write WAL: sequence -> named MessagePack envelope.
///
/// Every actor mutation that can affect the next proven state uses this one
/// keyspace. `KEY_ACKNOWLEDGED_WRITE_FLOOR..KEY_NEXT_ACKNOWLEDGED_WRITE_SEQ`
/// is therefore a gap-free interval, preserving exact cross-subsystem actor
/// acceptance order across restart.
pub(super) const ACKNOWLEDGED_WRITES: TableDefinition<u64, &[u8]> =
    TableDefinition::new("acknowledged_writes");

/// L1 bridge sidecar state: consumed deposit cursor/root and withdrawal leaves.
pub(super) const BRIDGE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("bridge_state");
pub(super) const KEY_BRIDGE_STATE: &str = "state";

/// Trader tracker snapshot — one row keyed "snapshot" holding the full
/// `TraderTrackerSnapshot` payload. Off-block sidecar; missing table on
/// load yields `Default::default()` (cold start until activity accumulates).
pub(super) const TRADER_TRACKER: TableDefinition<&str, &[u8]> =
    TableDefinition::new("trader_tracker");
pub(super) const KEY_TRADER_TRACKER_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker volume extensions: platform running total +
/// rolling hourly buckets. Stored as a single blob keyed `"snapshot"`,
/// matching the pattern set by `TRADER_TRACKER`. The physical table name is
/// retained for existing dev stores.
pub(super) const ROLLING_VOLUME: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_volume");
pub(super) const KEY_ROLLING_VOLUME_SNAPSHOT: &str = "snapshot";

/// Off-block price-tracker clearing-price history: per-market hourly
/// snapshot of the first-of-hour clearing price, used by
/// `price_n_hours_ago` (24h price-delta surfaces). Separate table from
/// `ROLLING_VOLUME`; the physical table name is retained for existing stores.
pub(super) const ROLLING_PRICE_ANCHORS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("price_tracker_clearing_history");
pub(super) const KEY_ROLLING_PRICE_ANCHORS_SNAPSHOT: &str = "snapshot";

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

/// All-time per-account fill counters (B8). The counter is current analytics
/// state and does not depend on the separate product-history projection.
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

// Counter keys
pub(super) const KEY_STORE_LAYOUT_VERSION: &str = "store_layout_version";
pub(super) const KEY_HEIGHT: &str = "height";
pub(super) const KEY_NEXT_ACCOUNT_ID: &str = "next_account_id";
pub(super) const KEY_NEXT_MARKET_ID: &str = "next_market_id";
pub(super) const KEY_NEXT_ORDER_ID: &str = "next_order_id";
pub(super) const KEY_ACCOUNT_STATE_HEIGHT: &str = "account_state_height";
pub(super) const KEY_ACCOUNT_STATE_SLOT: &str = "account_state_slot";
// Physical counter key retained so existing dev stores do not reuse event ids.
pub(super) const KEY_NEXT_PRODUCT_EVENT_SEQ: &str = "history_event_next_seq";
pub(super) const KEY_CANONICAL_ARCHIVE_OLDEST_HEIGHT: &str = "blocks_full_min_height";
pub(super) const KEY_LAST_CANONICAL_ARCHIVE_MAINTENANCE_HEIGHT: &str = "last_history_prune_height";
pub(super) const KEY_ACKNOWLEDGED_WRITE_FLOOR: &str = "acknowledged_write_floor";
pub(super) const KEY_NEXT_ACKNOWLEDGED_WRITE_SEQ: &str = "next_acknowledged_write_seq";
pub(super) const KEY_PUBLIC_ACCOUNTS_ALLOCATED: &str = "public_accounts_allocated";

pub(super) const STORE_LAYOUT_VERSION: u64 = 5;

// TODO: Tier 2 tables (remaining)
// const MM_STATE: TableDefinition<u32, &[u8]> = TableDefinition::new("mm_state");

/// Full canonical API replay block by height. Unlike the latest-only recovery
/// header/witness tables, this is a bounded archive. The physical table name
/// is retained for existing dev stores.
pub(super) const CANONICAL_BLOCK_ARCHIVE: TableDefinition<u64, &[u8]> =
    TableDefinition::new("blocks_full");

/// Canonical witness payload bytes plus typed DA manifest by block height.
///
/// This is a serving/availability artifact, not a recovery commit fence. Rows
/// are best-effort and pruned with the canonical replay archive floor.
pub(super) const DA_ARTIFACTS: TableDefinition<u64, &[u8]> = TableDefinition::new("da_artifacts");
/// Publish-time DA metadata cached independently from the large payload row so
/// public manifest reads never deserialize or hash witness bytes.
pub(super) const DA_MANIFESTS: TableDefinition<u64, &[u8]> = TableDefinition::new("da_manifests");
