use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use matching_engine::MarketSet;
use matching_sequencer::{
    AccountStore, AdminOracle, BlockSequencer, MempoolConfig, SequencerHandle,
};

use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = ApiConfig::parse();

    tracing::info!(
        port = config.port,
        dev_mode = config.dev_mode,
        "Starting Sybil API server"
    );

    // Initialize markets
    let mut markets = MarketSet::new();
    if config.seed_markets.is_empty() {
        // Default seed markets
        markets.add_binary("BTC > $100k by end of year");
        markets.add_binary("ETH > $10k by end of year");
        markets.add_binary("US GDP growth > 3%");
    } else {
        for name in &config.seed_markets {
            if !name.is_empty() {
                markets.add_binary(name);
            }
        }
    }

    let num_markets = markets.len();

    // Initialize sequencer
    let accounts = AccountStore::new();
    let oracle = Arc::new(AdminOracle::new());
    let sequencer = BlockSequencer::new(accounts, markets, vec![], oracle);
    let block_interval = Duration::from_millis(config.block_interval_ms);
    let handle =
        SequencerHandle::spawn_with_interval(sequencer, MempoolConfig::default(), block_interval);

    tracing::info!(
        num_markets,
        block_interval_ms = config.block_interval_ms,
        "Sequencer started with seed markets"
    );

    // Build app
    let state = AppState::new(handle, &config);
    let app = create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Listening on {}", addr);
    tracing::info!(
        "OpenAPI spec: http://localhost:{}/openapi.json",
        config.port
    );

    axum::serve(listener, app).await.unwrap();
}
