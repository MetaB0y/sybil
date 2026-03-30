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
}
