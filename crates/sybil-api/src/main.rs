use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use opentelemetry::trace::TracerProvider;
use tokio::net::TcpListener;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use matching_engine::MarketSet;
use matching_sequencer::{AccountStore, AdminOracle, BlockSequencer, SequencerConfig, SequencerHandle};

use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;

struct Telemetry {
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

fn init_telemetry() -> Telemetry {
    // Prometheus metrics recorder
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder");

    // OpenTelemetry trace exporter (OTLP over gRPC)
    // Respects OTEL_EXPORTER_OTLP_ENDPOINT env var (default: http://localhost:4317)
    let (otel_layer, tracer_provider) = match opentelemetry_otlp::SpanExporter::builder()
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
            (
                Some(tracing_opentelemetry::layer().with_tracer(tracer)),
                Some(provider),
            )
        }
        Err(e) => {
            eprintln!("OpenTelemetry OTLP exporter unavailable, traces will not be exported: {e}");
            (None, None)
        }
    };

    // Layered subscriber: console fmt + optional OTel export
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    Telemetry {
        prometheus_handle,
        tracer_provider,
    }
}

#[tokio::main]
async fn main() {
    let Telemetry {
        prometheus_handle,
        tracer_provider,
    } = init_telemetry();

    let config = ApiConfig::parse();

    tracing::info!(
        port = config.port,
        dev_mode = config.dev_mode,
        "Starting Sybil API server"
    );

    let store = if !config.data_dir.is_empty() {
        let data_dir = std::path::Path::new(&config.data_dir);
        std::fs::create_dir_all(data_dir).expect("failed to create data dir");
        let db_path = data_dir.join("sybil.redb");
        match matching_sequencer::store::Store::open(&db_path) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!(error = %e, "failed to open store, starting in-memory");
                None
            }
        }
    } else {
        None
    };

    let oracle = Arc::new(AdminOracle::new());
    let restored = if let Some(store) = store.as_ref() {
        match store.load_state().await {
            Ok(state) => state,
            Err(e) => {
                tracing::error!(error = %e, "failed to restore state, starting fresh");
                None
            }
        }
    } else {
        None
    };

    let seq_config = SequencerConfig {
        order_ttl_blocks: config.order_ttl_blocks,
        block_interval: Duration::from_millis(config.block_interval_ms),
        ..SequencerConfig::default()
    };

    let handle = if let Some(state) = restored {
        tracing::info!(
            height = state.height,
            markets = state.markets.len(),
            accounts = state.accounts.iter().count(),
            groups = state.market_groups.len(),
            resting_orders = state.resting_orders.len(),
            "Restored from persistent store"
        );

        let mut sequencer = BlockSequencer::restore(
            state.accounts,
            state.markets,
            state.market_groups,
            oracle,
            state.height,
            state.last_header,
            state.next_order_id,
            state.pubkey_registry,
            state.market_statuses,
            state.market_metadata,
            state.last_clearing_prices,
            state.market_volumes,
            state.resting_orders,
            seq_config,
        );

        // Add any seed markets not already present
        for name in &config.seed_markets {
            if !name.is_empty() && !sequencer.markets().iter().any(|m| m.name == *name) {
                sequencer.markets_mut().add_binary(name);
            }
        }

        SequencerHandle::spawn_with_store(sequencer, store)
    } else {
        let mut markets = MarketSet::new();
        for name in &config.seed_markets {
            if !name.is_empty() {
                markets.add_binary(name);
            }
        }
        let num_markets = markets.len();
        let accounts = AccountStore::new();
        let sequencer =
            BlockSequencer::with_default_solver(accounts, markets, vec![], oracle, seq_config);

        tracing::info!(num_markets, "Starting fresh (no persistent state)");

        SequencerHandle::spawn_with_store(sequencer, store)
    };

    tracing::info!(
        block_interval_ms = config.block_interval_ms,
        order_ttl_blocks = config.order_ttl_blocks,
        "Sequencer started"
    );

    let state = AppState::new(handle, &config, prometheus_handle);
    let app = create_router(state);
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Listening on {}", addr);
    tracing::info!(
        "OpenAPI spec: http://localhost:{}/openapi.json",
        config.port
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    if let Some(provider) = tracer_provider {
        if let Err(e) = provider.shutdown() {
            tracing::warn!(error = %e, "failed to flush OpenTelemetry spans on shutdown");
        }
    }

    tracing::info!("Server shut down cleanly");
}

/// Resolves on SIGTERM (Docker `docker stop`) or Ctrl-C.
/// axum drains in-flight connections before returning from `serve()`.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
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
        _ = ctrl_c => { tracing::info!("received Ctrl-C"); },
        _ = terminate => { tracing::info!("received SIGTERM"); },
    }
}
