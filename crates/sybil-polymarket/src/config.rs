use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "sybil-polymarket",
    about = "Mirror Polymarket markets onto Sybil"
)]
pub struct Config {
    /// Sybil API base URL.
    #[arg(long, default_value = "http://localhost:3000", env = "SYBIL_URL")]
    pub sybil_url: String,

    /// Polymarket Gamma API URL.
    #[arg(
        long,
        default_value = "https://gamma-api.polymarket.com",
        env = "POLYMARKET_GAMMA_URL"
    )]
    pub gamma_url: String,

    /// Polymarket CLOB WebSocket URL.
    #[arg(
        long,
        default_value = "wss://ws-subscriptions-clob.polymarket.com/ws/market",
        env = "POLYMARKET_WS_URL"
    )]
    pub ws_url: String,

    /// Polymarket CLOB REST URL (fallback for prices).
    #[arg(
        long,
        default_value = "https://clob.polymarket.com",
        env = "POLYMARKET_CLOB_URL"
    )]
    pub clob_url: String,

    /// Market sync interval in seconds.
    #[arg(long, default_value = "60", env = "SYNC_INTERVAL_SECS")]
    pub sync_interval_secs: u64,

    /// Categories to mirror (comma-separated, empty = all).
    #[arg(long, env = "MIRROR_CATEGORIES", value_delimiter = ',')]
    pub mirror_categories: Vec<String>,

    /// Categories to exclude from mirroring (comma-separated). Matched against
    /// Polymarket event tag labels and slugs.
    #[arg(long, env = "MIRROR_EXCLUDED_CATEGORIES", value_delimiter = ',')]
    pub mirror_excluded_categories: Vec<String>,

    /// Minimum Polymarket volume (USD) to mirror. 0 = all.
    #[arg(long, default_value = "0", env = "MIN_VOLUME_USD")]
    pub min_volume_usd: f64,

    /// Maximum number of events to mirror.
    #[arg(long, default_value = "50", env = "MAX_EVENTS")]
    pub max_events: usize,

    /// MM half-spread (e.g. 0.02 = 2 cents).
    #[arg(long, default_value = "0.02", env = "MM_HALF_SPREAD")]
    pub mm_half_spread: f64,

    /// MM budget in dollars (flash liquidity constraint).
    #[arg(long, default_value = "1000.0", env = "MM_BUDGET_DOLLARS")]
    pub mm_budget_dollars: f64,

    /// MM quote size per side in dollars.
    #[arg(long, default_value = "100.0", env = "MM_QUOTE_SIZE_DOLLARS")]
    pub mm_quote_size_dollars: f64,

    /// MM initial account balance in dollars.
    #[arg(long, default_value = "100000.0", env = "MM_INITIAL_BALANCE_DOLLARS")]
    pub mm_initial_balance_dollars: f64,

    /// MM risk aversion (Avellaneda-Stoikov γ). Higher = more aggressive inventory skewing.
    #[arg(long, default_value = "0.05", env = "MM_GAMMA")]
    pub mm_gamma: f64,

    /// Max position per market (shares). At limit, only unwind side is quoted.
    #[arg(long, default_value = "5000", env = "MM_MAX_POSITION")]
    pub mm_max_position: u64,

    /// Maximum mirrored markets the live MM should actively quote. 0 = all.
    #[arg(long, default_value = "200", env = "MM_MAX_MARKETS")]
    pub mm_max_markets: usize,

    /// Maximum MM orders to submit per block. Keeps live quoting inside the
    /// sybil-api per-submission DOS guard while rotating across tracked markets.
    #[arg(long, default_value = "64", env = "MM_MAX_ORDERS_PER_BLOCK")]
    pub mm_max_orders_per_block: usize,

    /// Max total dollar exposure across all markets. Budget → 0 as exposure approaches this.
    #[arg(long, default_value = "50000.0", env = "MM_MAX_EXPOSURE_DOLLARS")]
    pub mm_max_exposure_dollars: f64,

    /// Rolling window (blocks) for variance estimation.
    #[arg(long, default_value = "30", env = "MM_VOL_WINDOW")]
    pub mm_vol_window: usize,

    /// Minimum half-spread (floor). Below this, adverse selection eats all profit.
    #[arg(long, default_value = "0.005", env = "MM_MIN_SPREAD")]
    pub mm_min_spread: f64,

    /// Blocks between position syncs via GET /v1/accounts/{id}.
    #[arg(long, default_value = "5", env = "MM_SYNC_INTERVAL_BLOCKS")]
    pub mm_sync_interval_blocks: u64,

    /// Per-token price staleness threshold in milliseconds (PM-4/PM-6). A token
    /// whose midpoint has not refreshed within this window stops being quoted
    /// and its reference price is evicted (pushed as the 0 sentinel) so
    /// downstream consumers do not trade on a frozen value. Default preserves
    /// the previous global 30s behaviour, now applied per token.
    #[arg(long, default_value = "30000", env = "MM_STALENESS_MS")]
    pub mm_staleness_ms: u64,

    /// REST price poll interval in seconds (WebSocket fallback).
    #[arg(long, default_value = "5", env = "REST_POLL_INTERVAL_SECS")]
    pub rest_poll_interval_secs: u64,

    /// Path to persist mapping store (empty = in-memory only).
    #[arg(long, default_value = "", env = "MAPPING_STORE_PATH")]
    pub mapping_store_path: String,

    /// Path to the P256 signing key used to attest to resolutions. Empty
    /// disables the ResolutionActor (mirrored markets won't auto-resolve).
    /// The key's compressed SEC1 pubkey must be registered on sybil-api as
    /// the `polymarket_mirror` feed (see `--polymarket-feed-pubkey-hex`).
    #[arg(long, default_value = "", env = "SIGNER_KEY_PATH")]
    pub signer_key_path: String,

    /// Resolution poll interval in seconds.
    #[arg(long, default_value = "120", env = "RESOLUTION_POLL_INTERVAL_SECS")]
    pub resolution_poll_interval_secs: u64,
}
