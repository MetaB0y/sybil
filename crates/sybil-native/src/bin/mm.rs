#[path = "mm/monitoring.rs"]
mod monitoring;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use serde::{Deserialize, Serialize};
use sybil_client::SybilClient;
use sybil_market_maker::{
    MmActor, MmConfig, MmMessage, MmProgress, PriceSnapshot, QuoteRange, dollars_to_nanos,
};
use sybil_native::{Error, NativeDeployment};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Parser)]
#[command(
    name = "sybil-native-mm",
    about = "Provide static-anchor flash liquidity to provisioned native markets"
)]
struct Config {
    #[arg(long, default_value = "http://localhost:3000", env = "SYBIL_URL")]
    sybil_url: String,
    #[arg(long, env = "NATIVE_DEPLOYMENT_PATH")]
    deployment_path: PathBuf,
    #[arg(long, env = "NATIVE_MM_STATE_PATH")]
    state_path: PathBuf,
    #[arg(
        long,
        default_value = "1000000",
        env = "NATIVE_MM_INITIAL_BALANCE_DOLLARS"
    )]
    initial_balance_dollars: f64,
    #[arg(long, default_value = "0.02", env = "NATIVE_MM_HALF_SPREAD")]
    half_spread: f64,
    #[arg(long, default_value = "5000", env = "NATIVE_MM_BUDGET_DOLLARS")]
    budget_dollars: f64,
    #[arg(long, default_value = "100", env = "NATIVE_MM_QUOTE_SIZE_DOLLARS")]
    quote_size_dollars: f64,
    #[arg(long, default_value = "0.05", env = "NATIVE_MM_GAMMA")]
    gamma: f64,
    #[arg(long, default_value = "5000", env = "NATIVE_MM_MAX_POSITION")]
    max_position: u64,
    #[arg(long, default_value = "512", env = "NATIVE_MM_MAX_ORDERS_PER_BLOCK")]
    max_orders_per_block: usize,
    #[arg(long, default_value = "50000", env = "NATIVE_MM_MAX_EXPOSURE_DOLLARS")]
    max_exposure_dollars: f64,
    #[arg(
        long,
        default_value = "0.0.0.0:9104",
        env = "NATIVE_MM_MONITORING_BIND"
    )]
    monitoring_bind: SocketAddr,
    #[arg(
        long,
        default_value = "60",
        env = "NATIVE_MM_HEALTH_STALE_AFTER_SECS",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    health_stale_after_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MmState {
    genesis_hash: String,
    account_id: u64,
}

impl Config {
    fn mm_config(&self) -> MmConfig {
        MmConfig {
            mm_half_spread: self.half_spread,
            mm_budget_dollars: self.budget_dollars,
            mm_quote_size_dollars: self.quote_size_dollars,
            mm_gamma: self.gamma,
            mm_max_position: self.max_position,
            mm_max_orders_per_block: self.max_orders_per_block,
            mm_max_exposure_dollars: self.max_exposure_dollars,
            ..MmConfig::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sybil_native=info,sybil_market_maker=info".into()),
        )
        .init();
    let config = Config::parse();
    let mm_config = config.mm_config().validate()?;
    let initial_balance_nanos = dollars_to_nanos(
        "native_mm_initial_balance_dollars",
        config.initial_balance_dollars,
    )?;
    let deployment = NativeDeployment::load(&config.deployment_path)?;
    let token = std::env::var("SYBIL_SERVICE_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let client = SybilClient::with_defaults(config.sybil_url.clone(), token);
    let health = client.health().await?;
    if health
        .genesis_hash
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        != deployment.genesis_hash
    {
        return Err(Error::Deployment(
            "native deployment belongs to a different genesis; rerun sybil-native-admin"
                .to_string(),
        )
        .into());
    }

    let account_id = resolve_account(
        &client,
        &config.state_path,
        &deployment,
        initial_balance_nanos,
    )
    .await?;
    let channel_size = deployment.markets.len().max(1);
    let (mm_tx, mm_rx) = mpsc::channel(channel_size);
    for market in &deployment.markets {
        mm_tx
            .send(MmMessage::MarketNative {
                sybil_market_id: market.market_id,
                native_market_key: market.market_key.clone(),
                quote_range: QuoteRange {
                    min: market.quote_range.min,
                    max: market.quote_range.max,
                    initial: market.quote_range.initial,
                },
                group_key: market.group_key.clone(),
                group_size: market.group_size,
            })
            .await
            .map_err(|error| Error::Deployment(error.to_string()))?;
    }
    let (_price_tx, price_rx) = watch::channel(PriceSnapshot::default());
    let (progress_tx, progress_rx) = watch::channel(MmProgress::default());
    let actor = MmActor::new(mm_config, client, account_id, price_rx, mm_rx, progress_tx);
    let cancel = CancellationToken::new();
    let monitoring_listener = tokio::net::TcpListener::bind(config.monitoring_bind).await?;
    tracing::info!(
        address = %config.monitoring_bind,
        "native MM monitoring listening"
    );
    let monitoring_state = monitoring::MonitoringState::new(
        progress_rx,
        Duration::from_secs(config.health_stale_after_secs),
    );
    let monitoring_cancel = cancel.clone();
    let mut monitoring_handle = tokio::spawn(async move {
        monitoring::serve(monitoring_listener, monitoring_state, monitoring_cancel).await
    });
    let actor_cancel = cancel.clone();
    let mut actor_handle = tokio::spawn(async move { actor.run(actor_cancel).await });
    let unexpected_exit = tokio::select! {
        _ = shutdown_signal() => None,
        result = &mut actor_handle => Some(task_exit("MmActor", result)),
        result = &mut monitoring_handle => Some(monitoring_exit(result)),
    };
    if let Some(message) = unexpected_exit.as_deref() {
        tracing::error!(%message, "critical native MM task exited");
    }
    cancel.cancel();
    let stopped = tokio::time::timeout(TASK_SHUTDOWN_TIMEOUT, async {
        let actor_result = if actor_handle.is_finished() {
            None
        } else {
            Some((&mut actor_handle).await)
        };
        let monitoring_result = if monitoring_handle.is_finished() {
            None
        } else {
            Some((&mut monitoring_handle).await)
        };
        (actor_result, monitoring_result)
    })
    .await;
    let (actor_result, monitoring_result) = match stopped {
        Ok(results) => results,
        Err(_) => {
            return Err(std::io::Error::other(format!(
                "native MM tasks did not stop within {}s",
                TASK_SHUTDOWN_TIMEOUT.as_secs()
            ))
            .into());
        }
    };
    if let Some(result) = actor_result {
        result?;
    }
    if let Some(result) = monitoring_result {
        result??;
    }
    tracing::info!("native MM shutdown complete");
    match unexpected_exit {
        Some(message) => Err(std::io::Error::other(message).into()),
        None => Ok(()),
    }
}

fn task_exit(task: &'static str, result: Result<(), tokio::task::JoinError>) -> String {
    match result {
        Ok(()) => format!("{task} exited unexpectedly"),
        Err(error) => format!("{task} panicked or was cancelled: {error}"),
    }
}

fn monitoring_exit(result: Result<Result<(), std::io::Error>, tokio::task::JoinError>) -> String {
    match result {
        Ok(Ok(())) => "monitoring server exited unexpectedly".to_string(),
        Ok(Err(error)) => format!("monitoring server failed: {error}"),
        Err(error) => format!("monitoring server panicked or was cancelled: {error}"),
    }
}

/// Resolve on either interactive Ctrl-C or Docker's SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("received Ctrl-C, shutting down"),
        () = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}

async fn resolve_account(
    client: &SybilClient,
    state_path: &Path,
    deployment: &NativeDeployment,
    initial_balance_nanos: u64,
) -> Result<u64, Error> {
    let persisted = match load_state(state_path) {
        Ok(state) => Some(state),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(state) = persisted
        && state.genesis_hash == deployment.genesis_hash
    {
        match client.get_account(state.account_id).await {
            Ok(_) => {
                tracing::info!(
                    account_id = state.account_id,
                    "reattached native MM account"
                );
                return Ok(state.account_id);
            }
            Err(error) if error.api_status() == Some(404) => {
                tracing::warn!(
                    account_id = state.account_id,
                    %error,
                    "persisted native MM account no longer exists; minting a new one"
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    let account = client
        .provision_bare_account("native-mm/v1", initial_balance_nanos)
        .await?;
    save_state(
        state_path,
        &MmState {
            genesis_hash: deployment.genesis_hash.clone(),
            account_id: account.account_id,
        },
    )?;
    tracing::info!(account_id = account.account_id, "created native MM account");
    Ok(account.account_id)
}

fn load_state(path: &Path) -> Result<MmState, Error> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn save_state(path: &Path, state: &MmState) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}
