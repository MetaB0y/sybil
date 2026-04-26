use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "sybil-api", about = "HTTP API for Sybil prediction markets")]
pub struct ApiConfig {
    /// Port to listen on.
    #[arg(long, default_value = "3000", env = "SYBIL_PORT")]
    pub port: u16,

    /// Enable dev mode (account creation, funding, market creation).
    #[arg(long, default_value = "false", env = "SYBIL_DEV_MODE")]
    pub dev_mode: bool,

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

    /// In-memory ring-buffer size for recent blocks served by history endpoints.
    #[arg(long, default_value = "100", env = "SYBIL_BLOCK_HISTORY_CAPACITY")]
    pub block_history_capacity: usize,

    /// Sequencer actor queue depth that logs a warning.
    #[arg(long, default_value = "1000", env = "SYBIL_ACTOR_QUEUE_WARN_DEPTH")]
    pub actor_queue_warn_depth: usize,

    /// Sequencer actor queue depth that logs an error and should page.
    #[arg(long, default_value = "5000", env = "SYBIL_ACTOR_QUEUE_ERROR_DEPTH")]
    pub actor_queue_error_depth: usize,

    /// Data directory for persistent storage. Empty = in-memory only (no persistence).
    #[arg(long, default_value = "", env = "SYBIL_DATA_DIR")]
    pub data_dir: String,

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
            max_pending_bundles_per_account: 100,
            http_order_global_rps: 500,
            http_order_global_burst: 2_000,
            http_order_client_rps: 250,
            http_order_client_burst: 1_000,
            block_history_capacity: 100,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
            data_dir: String::new(),
            admin_feed_key_path: String::new(),
            polymarket_feed_pubkey_hex: String::new(),
        }
    }
}
