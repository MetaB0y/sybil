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
    AccountStore, AdminOracle, BlockSequencer, SequencerConfig, SequencerHandle,
};
use sybil_oracle::{FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;

struct Telemetry {
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

fn spawn_process_metrics_task() {
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            record_process_metrics();
        }
    });
}

fn record_process_metrics() {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return;
    };
    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Some(kib) = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<f64>().ok())
        else {
            continue;
        };
        match key {
            "VmRSS" => metrics::gauge!("sybil_process_resident_memory_bytes").set(kib * 1024.0),
            "VmHWM" => {
                metrics::gauge!("sybil_process_resident_memory_high_water_bytes").set(kib * 1024.0)
            }
            _ => {}
        }
    }
}

fn init_telemetry() -> Telemetry {
    // Prometheus metrics recorder
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder");

    // OpenTelemetry trace export is intentionally opt-in. The public demo runs
    // on a small 2 GB host where Tempo can starve the metrics/alerting path.
    let otel_enabled = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .is_some_and(|endpoint| !endpoint.trim().is_empty());
    let (otel_layer, tracer_provider) = if otel_enabled {
        match opentelemetry_otlp::SpanExporter::builder()
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
                eprintln!(
                    "OpenTelemetry OTLP exporter unavailable, traces will not be exported: {e}"
                );
                (None, None)
            }
        }
    } else {
        (None, None)
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Telemetry {
        prometheus_handle,
        tracer_provider,
    } = init_telemetry();
    spawn_process_metrics_task();

    let config = ApiConfig::parse();

    if !config.event_snapshot_dir.is_empty() {
        let dir = std::path::Path::new(&config.event_snapshot_dir);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(dir) {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to wipe event snapshot dir");
            }
        }
        match std::fs::create_dir_all(dir) {
            Ok(()) => {
                tracing::info!(dir = %dir.display(), "event snapshot dir ready (wiped on startup)")
            }
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to create event snapshot dir")
            }
        }
    }

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
                tracing::error!(error = %e, "failed to open persistent store");
                return Err(
                    std::io::Error::other(format!("failed to open persistent store: {e}")).into(),
                );
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
                tracing::error!(error = %e, "failed to restore persistent state");
                return Err(std::io::Error::other(format!(
                    "failed to restore persistent state: {e}"
                ))
                .into());
            }
        }
    } else {
        None
    };

    let seq_config = SequencerConfig {
        order_ttl_blocks: config.order_ttl_blocks,
        block_interval: Duration::from_millis(config.block_interval_ms),
        max_pending_bundles: config.max_pending_bundles,
        max_orders_per_submission: config.max_orders_per_submission,
        max_submissions_per_account_per_second: config.max_submissions_per_account_per_second,
        submission_burst_per_account: config.submission_burst_per_account,
        max_global_submissions_per_second: config.max_global_submissions_per_second,
        global_submission_burst: config.global_submission_burst,
        max_open_orders_per_account: config.max_open_orders_per_account,
        max_pending_bundles_per_account: config.max_pending_bundles_per_account,
        block_history_capacity: config.block_history_capacity,
        max_price_history_points_per_market: config.max_price_history_points_per_market,
        block_history_retention_blocks: config.block_history_retention_blocks,
        raw_price_retention_blocks: config.raw_price_retention_blocks,
        history_prune_interval_blocks: config.history_prune_interval_blocks,
        history_prune_max_rows: config.history_prune_max_rows,
        price_candle_resolutions_secs: config.price_candle_resolutions_secs.clone(),
        max_fill_history_per_account: config.max_fill_history_per_account,
        max_equity_points_per_account: config.max_equity_points_per_account,
        max_history_events_per_account: config.max_history_events_per_account,
        actor_queue_warn_depth: config.actor_queue_warn_depth,
        actor_queue_error_depth: config.actor_queue_error_depth,
        liquidity_band_nanos: config.liquidity_band_nanos,
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

        let mut sequencer = BlockSequencer::restore(state, oracle, seq_config);

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
        max_open_orders_per_account = config.max_open_orders_per_account,
        max_global_submissions_per_second = config.max_global_submissions_per_second,
        "Sequencer started"
    );

    // Fail fast: without the admin feed + templates installed, attestation-
    // based resolution is silently broken. Better to crash at startup than
    // to discover it when operators go to resolve a market.
    if let Err(err) = bootstrap_oracle_feeds(&handle, &config).await {
        panic!("failed to bootstrap oracle feeds: {err}");
    }

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
    Ok(())
}

/// Register the admin data feed, optionally register the Polymarket-mirror
/// feed from its configured pubkey, and install the matching resolution
/// templates. Idempotent on re-start (registrations dedupe on pubkey).
async fn bootstrap_oracle_feeds(
    handle: &SequencerHandle,
    config: &ApiConfig,
) -> Result<(), String> {
    let admin_pubkey_bytes = load_or_generate_admin_pubkey(&config.admin_feed_key_path)
        .map_err(|e| format!("admin feed key: {e}"))?;

    let admin_feed_id = handle
        .register_feed(FeedPubkey(admin_pubkey_bytes), "admin".to_string())
        .await
        .map_err(|e| format!("register admin feed: {e}"))?;

    handle
        .install_template(ResolutionTemplate {
            id: TemplateId("admin_immediate".to_string()),
            policy: ResolutionPolicy::Immediate {
                feed_id: admin_feed_id,
            },
        })
        .await
        .map_err(|e| format!("install admin_immediate template: {e}"))?;

    tracing::info!(
        feed_id = admin_feed_id.0,
        "admin feed registered and admin_immediate template installed"
    );

    if !config.polymarket_feed_pubkey_hex.is_empty() {
        let pk_bytes = hex::decode(&config.polymarket_feed_pubkey_hex)
            .map_err(|e| format!("decode polymarket_feed_pubkey_hex: {e}"))?;
        if pk_bytes.len() != 33 {
            return Err(format!(
                "polymarket_feed_pubkey_hex must decode to 33 bytes, got {}",
                pk_bytes.len()
            ));
        }
        let pm_feed_id = handle
            .register_feed(FeedPubkey(pk_bytes), "polymarket_mirror".to_string())
            .await
            .map_err(|e| format!("register polymarket_mirror feed: {e}"))?;
        handle
            .install_template(ResolutionTemplate {
                id: TemplateId("polymarket_mirror".to_string()),
                policy: ResolutionPolicy::Immediate {
                    feed_id: pm_feed_id,
                },
            })
            .await
            .map_err(|e| format!("install polymarket_mirror template: {e}"))?;
        tracing::info!(
            feed_id = pm_feed_id.0,
            "polymarket_mirror feed registered and template installed"
        );
    }

    Ok(())
}

/// Load a P256 admin feed key from disk or generate a fresh one. On disk the
/// key is stored as the raw 32-byte SEC1 scalar, hex-encoded (matching the
/// format the polymarket signer uses).
///
/// Returns the compressed SEC1 pubkey bytes.
fn load_or_generate_admin_pubkey(key_path: &str) -> Result<Vec<u8>, String> {
    use p256::ecdsa::SigningKey;
    use p256::elliptic_curve::rand_core::UnwrapErr;

    if key_path.is_empty() {
        let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut UnwrapErr(getrandom::SysRng),
        );
        let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
        tracing::warn!(
            "SYBIL_ADMIN_FEED_KEY_PATH empty; generated ephemeral admin key (will not persist across restarts)"
        );
        return Ok(pubkey.compressed_bytes());
    }

    let path = std::path::Path::new(key_path);
    if path.exists() {
        let hex_str =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let bytes =
            hex::decode(hex_str.trim()).map_err(|e| format!("decode {}: {e}", path.display()))?;
        let key = SigningKey::from_slice(&bytes)
            .map_err(|e| format!("parse SEC1 scalar at {}: {e}", path.display()))?;
        let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
        Ok(pubkey.compressed_bytes())
    } else {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
            }
        }
        let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut UnwrapErr(getrandom::SysRng),
        );
        let scalar_bytes = key.to_bytes();
        std::fs::write(path, hex::encode(scalar_bytes))
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
        tracing::info!(path = %path.display(), "generated new admin feed key");
        Ok(pubkey.compressed_bytes())
    }
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
