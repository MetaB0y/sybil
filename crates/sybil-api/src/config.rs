use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "sybil-api", about = "HTTP API for Sybil prediction markets")]
pub struct ApiConfig {
    /// Port to listen on.
    #[arg(long, default_value = "3000", env = "SYBIL_PORT")]
    pub port: u16,

    /// Enable local dev conveniences (free account funding, simulation
    /// controls, diagnostic orderbook listings, permissive CORS).
    #[arg(long, default_value = "false", env = "SYBIL_DEV_MODE")]
    pub dev_mode: bool,

    /// Deployment profile: `local`, `devnet`, or `prod`. Selects the intended
    /// durability/cache posture and drives the startup preflight guardrail. On
    /// `prod`, the server refuses to start when dev-only knobs are wired in
    /// (see [`crate::preflight`]) unless `SYBIL_ALLOW_DEV_KNOBS=1` is set.
    #[arg(long, default_value = "local", env = "SYBIL_DEPLOYMENT_PROFILE")]
    pub deployment_profile: String,

    /// Escape hatch: allow a `prod` profile to start despite dev-only knobs.
    /// Fail-open override, mirroring the fail-closed service-token posture.
    /// Intended for deliberate one-off operations, never steady state.
    #[arg(long, default_value = "false", env = "SYBIL_ALLOW_DEV_KNOBS")]
    pub allow_dev_knobs: bool,

    /// Bearer token required for service/operator routes when dev mode is off.
    /// Empty/unset means service routes fail closed in production.
    #[arg(long, default_value = "", env = "SYBIL_SERVICE_TOKEN")]
    pub service_token: String,

    /// Comma-separated browser origins allowed by CORS in production. Empty =
    /// no cross-origin CORS headers; same-origin browser requests still work.
    #[arg(long, env = "SYBIL_CORS_ORIGINS", value_delimiter = ',')]
    pub cors_origins: Vec<String>,

    /// Block production interval in milliseconds.
    #[arg(long, default_value = "500", env = "SYBIL_BLOCK_INTERVAL_MS")]
    pub block_interval_ms: u64,

    /// Seed markets to create on startup (comma-separated names).
    #[arg(long, env = "SYBIL_SEED_MARKETS", value_delimiter = ',')]
    pub seed_markets: Vec<String>,

    /// Order time-to-live in blocks. Default is ~1 year at 500ms blocks (GTC).
    #[arg(long, default_value = "63072000", env = "SYBIL_ORDER_TTL_BLOCKS")]
    pub order_ttl_blocks: u64,

    /// Cap on buffered MM / multi-market submissions waiting for the next block.
    #[arg(long, default_value = "10000", env = "SYBIL_MAX_PENDING_BUNDLES")]
    pub max_pending_bundles: usize,

    /// Maximum number of orders accepted in one submission.
    #[arg(long, default_value = "64", env = "SYBIL_MAX_ORDERS_PER_SUBMISSION")]
    pub max_orders_per_submission: usize,

    /// Per-account sustained order/cancel submission rate.
    #[arg(
        long,
        default_value = "50",
        env = "SYBIL_MAX_SUBMISSIONS_PER_ACCOUNT_PER_SECOND"
    )]
    pub max_submissions_per_account_per_second: u32,

    /// Per-account submission burst allowance.
    #[arg(
        long,
        default_value = "200",
        env = "SYBIL_SUBMISSION_BURST_PER_ACCOUNT"
    )]
    pub submission_burst_per_account: u32,

    /// Global sustained order/cancel submission rate.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_MAX_GLOBAL_SUBMISSIONS_PER_SECOND"
    )]
    pub max_global_submissions_per_second: u32,

    /// Global submission burst allowance.
    #[arg(long, default_value = "3000", env = "SYBIL_GLOBAL_SUBMISSION_BURST")]
    pub global_submission_burst: u32,

    /// Maximum resting non-MM orders per account.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_MAX_OPEN_ORDERS_PER_ACCOUNT"
    )]
    pub max_open_orders_per_account: usize,

    /// Minimum cash/position notional backing each non-MM resting order.
    /// Default is one tenth of a cent (1,000,000 nanodollars), matching one
    /// minimum quantity unit at a $1 limit.
    #[arg(
        long,
        default_value = "1000000",
        env = "SYBIL_MIN_RESTING_ORDER_NOTIONAL_NANOS"
    )]
    pub min_resting_order_notional_nanos: u64,

    /// Maximum deferred MM / multi-market submissions per account.
    #[arg(
        long,
        default_value = "100",
        env = "SYBIL_MAX_PENDING_BUNDLES_PER_ACCOUNT"
    )]
    pub max_pending_bundles_per_account: usize,

    /// Pre-handler global HTTP rate for order endpoints. This protects CPU
    /// before JSON parsing and P256 verification.
    #[arg(long, default_value = "500", env = "SYBIL_HTTP_ORDER_GLOBAL_RPS")]
    pub http_order_global_rps: u32,

    /// Pre-handler global HTTP burst for order endpoints.
    #[arg(long, default_value = "2000", env = "SYBIL_HTTP_ORDER_GLOBAL_BURST")]
    pub http_order_global_burst: u32,

    /// Pre-handler per-client HTTP rate for order endpoints.
    #[arg(long, default_value = "250", env = "SYBIL_HTTP_ORDER_CLIENT_RPS")]
    pub http_order_client_rps: u32,

    /// Pre-handler per-client HTTP burst for order endpoints.
    #[arg(long, default_value = "1000", env = "SYBIL_HTTP_ORDER_CLIENT_BURST")]
    pub http_order_client_burst: u32,

    /// Pre-handler global request rate for retained DA manifest/payload reads.
    #[arg(long, default_value = "20", env = "SYBIL_HTTP_DA_GLOBAL_RPS")]
    pub http_da_global_rps: u32,

    /// Global DA read burst allowance.
    #[arg(long, default_value = "40", env = "SYBIL_HTTP_DA_GLOBAL_BURST")]
    pub http_da_global_burst: u32,

    /// Per-client retained DA read rate.
    #[arg(long, default_value = "10", env = "SYBIL_HTTP_DA_CLIENT_RPS")]
    pub http_da_client_rps: u32,

    /// Per-client retained DA read burst allowance.
    #[arg(long, default_value = "20", env = "SYBIL_HTTP_DA_CLIENT_BURST")]
    pub http_da_client_burst: u32,

    /// Maximum retained DA reads executing concurrently.
    #[arg(long, default_value = "4", env = "SYBIL_HTTP_DA_MAX_CONCURRENCY")]
    pub http_da_max_concurrency: usize,

    /// Maximum anonymous public block streams held concurrently across SSE
    /// and WebSocket. The authenticated service stream has a separate trust
    /// boundary and does not consume this budget.
    #[arg(
        long,
        default_value = "256",
        env = "SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS"
    )]
    pub http_public_stream_max_connections: usize,

    /// WebAuthn relying-party id. For local frontend dev this is `localhost`.
    #[arg(long, default_value = "localhost", env = "SYBIL_WEBAUTHN_RP_ID")]
    pub webauthn_rp_id: String,

    /// Browser origin allowed in WebAuthn clientDataJSON.
    #[arg(
        long,
        default_value = "http://localhost:3000",
        env = "SYBIL_WEBAUTHN_ORIGIN"
    )]
    pub webauthn_origin: String,

    /// Require the authenticator's user-verification flag on WebAuthn assertions.
    #[arg(long, default_value = "true", env = "SYBIL_WEBAUTHN_REQUIRE_UV")]
    pub webauthn_require_uv: bool,

    /// In-memory ring-buffer size for recent blocks served by history endpoints.
    #[arg(long, default_value = "100", env = "SYBIL_BLOCK_HISTORY_CAPACITY")]
    pub block_history_capacity: usize,

    /// In-memory price-history cache points retained per market.
    #[arg(
        long,
        default_value = "2000",
        env = "SYBIL_MAX_PRICE_HISTORY_POINTS_PER_MARKET"
    )]
    pub max_price_history_points_per_market: usize,

    /// Durable full-block history rows retained by bounded pruning. 0 disables
    /// pruning for full block history.
    #[arg(
        long,
        default_value = "0",
        env = "SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS"
    )]
    pub block_history_retention_blocks: u64,

    /// Durable raw price-point rows retained by bounded pruning. 0 disables
    /// pruning for raw price history.
    #[arg(long, default_value = "0", env = "SYBIL_RAW_PRICE_RETENTION_BLOCKS")]
    pub raw_price_retention_blocks: u64,

    /// Durable account-history age limits. 0 disables age pruning.
    #[arg(long, default_value = "0", env = "SYBIL_FILL_HISTORY_RETENTION_SECS")]
    pub fill_history_retention_secs: u64,
    #[arg(long, default_value = "0", env = "SYBIL_EQUITY_RETENTION_SECS")]
    pub equity_retention_secs: u64,
    #[arg(long, default_value = "0", env = "SYBIL_ACCOUNT_EVENT_RETENTION_SECS")]
    pub account_event_retention_secs: u64,

    /// Global durable account-history row ceilings. 0 disables the ceiling.
    #[arg(long, default_value = "0", env = "SYBIL_MAX_DURABLE_FILL_ROWS")]
    pub max_durable_fill_rows: usize,
    #[arg(long, default_value = "0", env = "SYBIL_MAX_DURABLE_EQUITY_ROWS")]
    pub max_durable_equity_rows: usize,
    #[arg(
        long,
        default_value = "0",
        env = "SYBIL_MAX_DURABLE_ACCOUNT_EVENT_ROWS"
    )]
    pub max_durable_account_event_rows: usize,

    /// Block cadence for retention maintenance. 0 disables scheduled pruning.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS"
    )]
    pub history_prune_interval_blocks: u64,

    /// Maximum durable history rows deleted in one maintenance pass.
    #[arg(long, default_value = "10000", env = "SYBIL_HISTORY_PRUNE_MAX_ROWS")]
    pub history_prune_max_rows: usize,

    /// Comma-separated candle resolutions, in seconds, maintained from
    /// committed raw price points.
    #[arg(
        long,
        env = "SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS",
        value_delimiter = ',',
        default_value = "60,300,3600"
    )]
    pub price_candle_resolutions_secs: Vec<u32>,

    /// Comma-separated durable candle retention windows, in seconds, aligned
    /// by index with `SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS`. 0 disables pruning
    /// for that resolution.
    #[arg(
        long,
        env = "SYBIL_PRICE_CANDLE_RETENTION_SECS",
        value_delimiter = ',',
        default_value = "2592000,15552000,0"
    )]
    pub price_candle_retention_secs: Vec<u64>,

    /// In-memory fill-history records retained per account for API queries.
    #[arg(
        long,
        default_value = "5000",
        env = "SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT"
    )]
    pub max_fill_history_per_account: usize,

    /// In-memory equity points retained per account (serving fallback only;
    /// full series lives in redb). Set to 0 in prod.
    #[arg(long, default_value = "0", env = "SYBIL_MAX_EQUITY_POINTS_PER_ACCOUNT")]
    pub max_equity_points_per_account: usize,
    /// In-memory history events retained per account (serving fallback only).
    /// Set to 0 in prod.
    #[arg(
        long,
        default_value = "0",
        env = "SYBIL_MAX_HISTORY_EVENTS_PER_ACCOUNT"
    )]
    pub max_history_events_per_account: usize,

    /// Sequencer actor queue depth that logs a warning.
    #[arg(long, default_value = "1000", env = "SYBIL_ACTOR_QUEUE_WARN_DEPTH")]
    pub actor_queue_warn_depth: usize,

    /// Sequencer actor queue depth that logs an error and should page.
    #[arg(long, default_value = "5000", env = "SYBIL_ACTOR_QUEUE_ERROR_DEPTH")]
    pub actor_queue_error_depth: usize,

    /// Width of the ±band (in nanos) the off-block LiquidityTracker uses to
    /// score "near-the-money" depth. Default 50_000_000 nanos = $0.05.
    #[arg(long, default_value = "50000000", env = "SYBIL_LIQUIDITY_BAND_NANOS")]
    pub liquidity_band_nanos: u64,

    /// Data directory for persistent storage. Empty = in-memory only (no persistence).
    #[arg(long, default_value = "", env = "SYBIL_DATA_DIR")]
    pub data_dir: String,

    /// Import a canonical block witness payload into an empty persistent store,
    /// then exit. The imported store continues the witness chain instance; use
    /// `--genesis-hash` when the payload cannot derive the height-1 header.
    #[arg(long, default_value = "false", env = "SYBIL_IMPORT_WITNESS")]
    pub import_witness: bool,

    /// Canonical witness payload file to import when `--import-witness` is set.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_PAYLOAD", value_name = "PATH")]
    pub payload: Option<PathBuf>,

    /// Expected post-state root for the imported witness, as 32-byte hex.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_EXPECT_STATE_ROOT")]
    pub expect_state_root: Option<String>,

    /// Original chain genesis hash for witness imports when the payload does
    /// not include block 1. Required for importing heads beyond height 2.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_GENESIS_HASH")]
    pub genesis_hash: Option<String>,

    /// Path to arena's live decisions SQLite database. Empty disables native
    /// bot-decision analytics in the dashboard.
    #[arg(long, default_value = "", env = "SYBIL_ARENA_DB_PATH")]
    pub arena_db_path: String,

    /// Path to the JSON file that persists off-block `MarketRefData`
    /// (Polymarket mirror metadata: event id/title, images, end dates,
    /// category). Empty = volatile in-memory only (state lost on restart;
    /// mirror re-fills on the next sync cycle).
    #[arg(long, default_value = "", env = "SYBIL_MARKET_REF_DATA_PATH")]
    pub market_ref_data_path: String,

    /// Directory holding full Polymarket event JSON snapshots, served at
    /// `GET /v1/events/{id}/raw`. Persisted across restarts (created if missing,
    /// never wiped on boot — SYB-153) so mirror raw JSON survives a restart
    /// without waiting for the next sync. Empty = disabled (raw endpoints 404).
    #[arg(long, default_value = "", env = "SYBIL_EVENT_SNAPSHOT_DIR")]
    pub event_snapshot_dir: String,

    /// Path to the P256 signing key used by the admin feed. The file stores
    /// the raw 32-byte SEC1 scalar, hex-encoded. Empty = generate a fresh
    /// ephemeral key at startup (dev-mode convenience; will NOT persist
    /// across restarts).
    #[arg(long, default_value = "", env = "SYBIL_ADMIN_FEED_KEY_PATH")]
    pub admin_feed_key_path: String,

    /// Hex-encoded compressed SEC1 P256 pubkey (33 bytes) for the
    /// Polymarket-mirror resolution feed. When set, the server registers a
    /// `polymarket_mirror` feed and installs a matching `polymarket_mirror`
    /// resolution template. When unset, Polymarket-mirrored markets still
    /// work for trading but can only be resolved via the admin path.
    #[arg(long, default_value = "", env = "SYBIL_POLYMARKET_FEED_PUBKEY_HEX")]
    pub polymarket_feed_pubkey_hex: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            dev_mode: false,
            deployment_profile: "local".to_string(),
            allow_dev_knobs: false,
            service_token: String::new(),
            cors_origins: Vec::new(),
            block_interval_ms: 500,
            seed_markets: Vec::new(),
            order_ttl_blocks: 63_072_000,
            max_pending_bundles: 10_000,
            max_orders_per_submission: 64,
            max_submissions_per_account_per_second: 50,
            submission_burst_per_account: 200,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 3_000,
            max_open_orders_per_account: 1_000,
            min_resting_order_notional_nanos: 1_000_000,
            max_pending_bundles_per_account: 100,
            http_order_global_rps: 500,
            http_order_global_burst: 2_000,
            http_order_client_rps: 250,
            http_order_client_burst: 1_000,
            http_da_global_rps: 20,
            http_da_global_burst: 40,
            http_da_client_rps: 10,
            http_da_client_burst: 20,
            http_da_max_concurrency: 4,
            http_public_stream_max_connections: 256,
            webauthn_rp_id: "localhost".to_string(),
            webauthn_origin: "http://localhost:3000".to_string(),
            webauthn_require_uv: true,
            block_history_capacity: 100,
            max_price_history_points_per_market: 2_000,
            block_history_retention_blocks: 0,
            raw_price_retention_blocks: 0,
            fill_history_retention_secs: 0,
            equity_retention_secs: 0,
            account_event_retention_secs: 0,
            max_durable_fill_rows: 0,
            max_durable_equity_rows: 0,
            max_durable_account_event_rows: 0,
            history_prune_interval_blocks: 1_000,
            history_prune_max_rows: 10_000,
            price_candle_resolutions_secs: vec![60, 300, 3_600],
            price_candle_retention_secs: vec![2_592_000, 15_552_000, 0],
            max_fill_history_per_account: 5_000,
            max_equity_points_per_account: 0,
            max_history_events_per_account: 0,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
            liquidity_band_nanos: 50_000_000,
            data_dir: String::new(),
            import_witness: false,
            payload: None,
            expect_state_root: None,
            genesis_hash: None,
            arena_db_path: String::new(),
            market_ref_data_path: String::new(),
            event_snapshot_dir: String::new(),
            admin_feed_key_path: String::new(),
            polymarket_feed_pubkey_hex: String::new(),
        }
    }
}
