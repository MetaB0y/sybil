use std::path::PathBuf;

use clap::Parser;
use sybil_client::SybilClient;
use sybil_native::{NativeMarketCatalog, apply_catalog};

#[derive(Debug, Parser)]
#[command(
    name = "sybil-native-admin",
    about = "Idempotently apply the native Sybil market catalog"
)]
struct Config {
    #[arg(long, default_value = "http://localhost:3000", env = "SYBIL_URL")]
    sybil_url: String,
    #[arg(long, env = "NATIVE_CATALOG_PATH")]
    catalog_path: PathBuf,
    #[arg(long, env = "NATIVE_DEPLOYMENT_PATH")]
    deployment_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sybil_native=info".into()),
        )
        .init();
    let config = Config::parse();
    let catalog = NativeMarketCatalog::load(&config.catalog_path)?;
    let token = std::env::var("SYBIL_SERVICE_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let client = SybilClient::new(reqwest::Client::new(), config.sybil_url, token);
    let deployment = apply_catalog(&client, &catalog).await?;
    deployment.save(&config.deployment_path)?;
    tracing::info!(
        markets = deployment.markets.len(),
        genesis_hash = %deployment.genesis_hash,
        path = %config.deployment_path.display(),
        "native catalog applied"
    );
    Ok(())
}
