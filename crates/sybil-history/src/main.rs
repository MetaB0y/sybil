use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use sybil_history::{
    DEFAULT_REDB_CACHE_BYTES, HistoryHandle, HistoryHttpConfig, HistoryStore, router,
};
use tokio::net::TcpListener;
use tokio::sync::watch;
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
    #[arg(
        long,
        env = "SYBIL_HISTORY_REDB_CACHE_BYTES",
        default_value_t = DEFAULT_REDB_CACHE_BYTES
    )]
    redb_cache_bytes: usize,
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
    let store = HistoryStore::open_with_cache(
        config.data_dir.join("history.redb"),
        config.candle_resolutions_secs,
        config.redb_cache_bytes,
    )?;
    tracing::info!(
        redb_cache_bytes = config.redb_cache_bytes,
        "Configured bounded history redb cache"
    );
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

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let server = async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while shutdown_rx.changed().await.is_ok() {
                    if *shutdown_rx.borrow() {
                        return;
                    }
                }
            })
            .await
    };
    let signal = shutdown_signal();
    tokio::pin!(server);
    tokio::pin!(signal);

    let server_result = tokio::select! {
        result = &mut signal => match result {
            Ok(()) => {
                let _ = shutdown_tx.send(true);
                server.as_mut().await
            }
            Err(error) => Err(error),
        },
        result = &mut server => match result {
            Ok(()) => Err(std::io::Error::other(
                "history HTTP server exited unexpectedly",
            )),
            Err(error) => Err(error),
        }
    };
    if !handle.stop_and_wait(Duration::from_secs(5)).await {
        tracing::warn!("history projector stop timed out");
    }
    server_result?;
    Ok(())
}

async fn shutdown_signal() -> std::io::Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        signal.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "SIGTERM signal stream closed",
            )
        })?;
        Ok(())
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<std::io::Result<()>>();
    tokio::select! {
        result = ctrl_c => result,
        result = terminate => result,
    }
}
