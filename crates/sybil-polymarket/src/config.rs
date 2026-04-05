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

    /// REST price poll interval in seconds (WebSocket fallback).
    #[arg(long, default_value = "5", env = "REST_POLL_INTERVAL_SECS")]
    pub rest_poll_interval_secs: u64,

    /// Path to persist mapping store (empty = in-memory only).
    #[arg(long, default_value = "", env = "MAPPING_STORE_PATH")]
    pub mapping_store_path: String,
}
