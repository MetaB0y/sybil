use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use sybil_history::{HistoryHandle, HistoryHttpConfig, HistoryStore, router};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "sybil-history")]
struct Config {
    #[arg(long, env = "SYBIL_HISTORY_BIND", default_value = "0.0.0.0:3003")]
    bind: String,
    #[arg(long, env = "SYBIL_HISTORY_DATA_DIR", default_value = "./data/history")]
    data_dir: PathBuf,
    #[arg(long, env = "SYBIL_HISTORY_TOKEN", default_value = "")]
    internal_token: String,
    #[arg(long, env = "SYBIL_HISTORY_DEV_MODE", default_value_t = false)]
    dev_mode: bool,
    #[arg(
        long,
        env = "SYBIL_HISTORY_CANDLE_RESOLUTIONS_SECS",
        value_delimiter = ',',
        default_value = "60,300,3600"
    )]
    candle_resolutions_secs: Vec<u32>,
    #[arg(
        long,
        env = "SYBIL_HISTORY_MAX_QUERY_CONCURRENCY",
        default_value_t = 16
    )]
    max_query_concurrency: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let config = Config::parse();
    if !config.dev_mode && config.internal_token.trim().is_empty() {
        return Err("SYBIL_HISTORY_TOKEN is required outside dev mode".into());
    }
    std::fs::create_dir_all(&config.data_dir)?;
    let store = HistoryStore::open(
        config.data_dir.join("history.redb"),
        config.candle_resolutions_secs,
    )?;
    let handle = HistoryHandle::spawn(store.clone());
    let app = router(
        handle.clone(),
        store,
        HistoryHttpConfig {
            dev_mode: config.dev_mode,
            internal_token: (!config.internal_token.is_empty()).then_some(config.internal_token),
            max_query_concurrency: config.max_query_concurrency,
        },
    );
    let listener = TcpListener::bind(&config.bind).await?;
    tracing::info!(bind = %config.bind, "Sybil history service listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    if !handle.stop_and_wait(Duration::from_secs(5)).await {
        tracing::warn!("history projector stop timed out");
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
