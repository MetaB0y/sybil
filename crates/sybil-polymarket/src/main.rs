use std::path::PathBuf;

use clap::Parser;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use sybil_polymarket::config::Config;
use sybil_polymarket::feed::{FeedActor, PriceSnapshot};
use sybil_polymarket::mapping::MappingStore;
use sybil_polymarket::mm::MmActor;
use sybil_polymarket::polymarket::gamma::GammaClient;
use sybil_polymarket::sybil::client::SybilClient;
use sybil_polymarket::sybil::types::NANOS_PER_DOLLAR;
use sybil_polymarket::sync::SyncActor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install rustls crypto provider (needed for WebSocket TLS)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sybil_polymarket=info".into()),
        )
        .init();

    let config = Config::parse();
    info!(?config, "starting sybil-polymarket");

    // Shared HTTP client
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Load or create mapping store
    let mapping = if config.mapping_store_path.is_empty() {
        MappingStore::new(None)
    } else {
        let path = PathBuf::from(&config.mapping_store_path);
        MappingStore::load(&path)?
    };
    info!(
        events = mapping.event_count(),
        markets = mapping.market_count(),
        "loaded mapping store"
    );

    // Clients
    let gamma_client = GammaClient::new(
        http.clone(),
        config.gamma_url.clone(),
        config.clob_url.clone(),
    );
    let sybil_client_sync = SybilClient::new(http.clone(), config.sybil_url.clone());
    let sybil_client_mm = SybilClient::new(http.clone(), config.sybil_url.clone());
    let gamma_client_feed = GammaClient::new(
        http.clone(),
        config.gamma_url.clone(),
        config.clob_url.clone(),
    );

    // Wait for Sybil to be healthy
    info!(url = &config.sybil_url, "waiting for Sybil API...");
    loop {
        match sybil_client_sync.health().await {
            Ok(h) if h.status == "ok" => break,
            Ok(h) => {
                info!(status = h.status, "Sybil not ready, retrying...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) => {
                info!(error = %e, "Sybil not reachable, retrying...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
    info!("Sybil API is healthy");

    // Create MM account
    let balance_nanos = (config.mm_initial_balance_dollars * NANOS_PER_DOLLAR as f64) as u64;
    let mm_account = sybil_client_sync.create_account(balance_nanos).await?;
    info!(
        account_id = mm_account.account_id,
        balance_dollars = config.mm_initial_balance_dollars,
        "created MM account"
    );

    // Channels
    let (feed_tx, feed_rx) = mpsc::channel(64);
    let (mm_tx, mm_rx) = mpsc::channel(256);
    let (price_tx, price_rx) = watch::channel(PriceSnapshot::default());

    // Cancellation
    let cancel = CancellationToken::new();
    let cancel_sync = cancel.clone();
    let cancel_feed = cancel.clone();
    let cancel_mm = cancel.clone();

    // Spawn actors
    let config_sync = config.clone();
    let config_feed = config.clone();
    let config_mm = config.clone();

    let sync_handle = tokio::spawn(async move {
        let actor = SyncActor::new(
            config_sync,
            gamma_client,
            sybil_client_sync,
            mapping,
            feed_tx,
            mm_tx,
        );
        actor.run(cancel_sync).await;
    });

    let feed_handle = tokio::spawn(async move {
        let actor = FeedActor::new(config_feed, gamma_client_feed, price_tx, feed_rx);
        actor.run(cancel_feed).await;
    });

    let mm_handle = tokio::spawn(async move {
        let actor = MmActor::new(
            config_mm,
            sybil_client_mm,
            mm_account.account_id,
            price_rx,
            mm_rx,
        );
        actor.run(cancel_mm).await;
    });

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down...");
        }
        r = sync_handle => {
            if let Err(e) = r {
                error!(error = %e, "SyncActor panicked");
            }
        }
        r = feed_handle => {
            if let Err(e) = r {
                error!(error = %e, "FeedActor panicked");
            }
        }
        r = mm_handle => {
            if let Err(e) = r {
                error!(error = %e, "MmActor panicked");
            }
        }
    }

    cancel.cancel();
    info!("shutdown complete");
    Ok(())
}
