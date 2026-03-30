use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use opentelemetry::trace::TracerProvider;
use tokio::net::TcpListener;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use matching_engine::MarketSet;
use matching_sequencer::{
    AccountStore, AdminOracle, BlockSequencer, MempoolConfig, SequencerHandle,
};

use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;

fn init_telemetry() -> metrics_exporter_prometheus::PrometheusHandle {
    // Prometheus metrics recorder
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder");

    // OpenTelemetry trace exporter (OTLP over gRPC)
    // Respects OTEL_EXPORTER_OTLP_ENDPOINT env var (default: http://localhost:4317)
    let otel_layer = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
    {
        Ok(exporter) => {
            let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    opentelemetry_sdk::Resource::builder()
                        .with_service_name("sybil-api")
                        .build(),
                )
                .build();
            opentelemetry::global::set_tracer_provider(provider.clone());
            let tracer = provider.tracer("sybil-api");
            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        Err(e) => {
            eprintln!("OpenTelemetry OTLP exporter unavailable, traces will not be exported: {e}");
            None
        }
    };

    // Layered subscriber: console fmt + optional OTel export
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    prometheus_handle
}

#[tokio::main]
async fn main() {
    let prometheus_handle = init_telemetry();

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
    let state = AppState::new(handle, &config, prometheus_handle);
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
