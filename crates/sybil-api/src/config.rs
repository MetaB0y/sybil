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
