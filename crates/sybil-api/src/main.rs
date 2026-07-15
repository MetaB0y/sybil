use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use opentelemetry::trace::TracerProvider;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use matching_engine::MarketSet;
use matching_sequencer::{
    AccountStore, AdminOracle, BlockSequencer, SequencerConfig, SequencerHandle,
};
use sybil_oracle::{FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;
use sybil_api::types::response::HealthResponse;

const SEQUENCER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

struct Telemetry {
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

#[derive(Clone)]
struct RestoreFailureState {
    prometheus: metrics_exporter_prometheus::PrometheusHandle,
}

async fn restore_failure_metrics(State(state): State<RestoreFailureState>) -> String {
    state.prometheus.render()
}

async fn restore_failure_health() -> (StatusCode, Json<HealthResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(HealthResponse {
            status: "restore_failed".to_string(),
            height: None,
            genesis_hash: None,
        }),
    )
}

/// Keep the integrity signal scrapeable without mounting any exchange surface.
///
/// A process-local counter incremented during cold-start recovery would be lost
/// if startup simply exited before the HTTP listener existed. This deliberately
/// unhealthy mode holds the process at the incident boundary until an operator
/// preserves/repairs the store and restarts it.
async fn serve_restore_failure_mode(
    port: u16,
    prometheus: metrics_exporter_prometheus::PrometheusHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/metrics", get(restore_failure_metrics))
        .route("/v1/health", get(restore_failure_health))
        .with_state(RestoreFailureState { prometheus });
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::error!(
        address = %addr,
        "persistent restore failed; serving only unhealthy health and metrics endpoints"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn shutdown_tracer_provider(tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>) {
    if let Some(provider) = tracer_provider
        && let Err(e) = provider.shutdown()
    {
        tracing::warn!(error = %e, "failed to flush OpenTelemetry spans on shutdown");
    }
}

async fn run_process_metrics(cancel: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = interval.tick() => {
                record_process_metrics();
            }
        }
    }
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

fn sequencer_config_from_api(config: &ApiConfig) -> SequencerConfig {
    SequencerConfig {
        order_ttl_blocks: config.order_ttl_blocks,
        block_interval: Duration::from_millis(config.block_interval_ms),
        max_pending_bundles: config.max_pending_bundles,
        max_orders_per_submission: config.max_orders_per_submission,
        max_submissions_per_account_per_second: config.max_submissions_per_account_per_second,
        submission_burst_per_account: config.submission_burst_per_account,
        max_global_submissions_per_second: config.max_global_submissions_per_second,
        global_submission_burst: config.global_submission_burst,
        max_open_orders_per_account: config.max_open_orders_per_account,
        min_resting_order_notional_nanos: config.min_resting_order_notional_nanos,
        max_pending_bundles_per_account: config.max_pending_bundles_per_account,
        recent_block_cache_capacity: config.recent_block_cache_capacity,
        max_recent_price_points_per_market: config.max_recent_price_points_per_market,
        canonical_archive_retention_blocks: config.canonical_archive_retention_blocks,
        canonical_archive_maintenance_interval_blocks: config
            .canonical_archive_maintenance_interval_blocks,
        canonical_archive_max_rows_per_pass: config.canonical_archive_max_rows_per_pass,
        acknowledged_proof_job_retention_blocks: config.acknowledged_proof_job_retention_blocks,
        acknowledged_proof_job_maintenance_interval_blocks: config
            .acknowledged_proof_job_maintenance_interval_blocks,
        acknowledged_proof_job_max_rows_per_pass: config.acknowledged_proof_job_max_rows_per_pass,
        max_recent_fills_per_account: config.max_recent_fills_per_account,
        max_recent_equity_points_per_account: config.max_recent_equity_points_per_account,
        max_recent_account_events_per_account: config.max_recent_account_events_per_account,
        actor_queue_warn_depth: config.actor_queue_warn_depth,
        actor_queue_error_depth: config.actor_queue_error_depth,
        liquidity_band_nanos: config.liquidity_band_nanos,
        verification_fail_open: false,
        debug_verify_full: false,
    }
}

fn parse_hash_arg(value: &str, flag: &str) -> Result<[u8; 32], String> {
    let hex = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(hex).map_err(|e| format!("decode {flag}: {e}"))?;
    let len = bytes.len();
    bytes
        .try_into()
        .map_err(|_| format!("{flag} must decode to 32 bytes, got {len}"))
}

fn parse_expected_state_root(root: &str) -> Result<[u8; 32], String> {
    parse_hash_arg(root, "--expect-state-root")
}

fn parse_genesis_hash(hash: &str) -> Result<[u8; 32], String> {
    parse_hash_arg(hash, "--genesis-hash")
}

fn hex32(bytes: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

async fn run_witness_import(config: &ApiConfig) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Error, ErrorKind};

    if config.data_dir.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "--import-witness requires --data-dir or SYBIL_DATA_DIR",
        )
        .into());
    }
    let payload = config.payload.as_ref().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "--import-witness requires --payload or SYBIL_IMPORT_WITNESS_PAYLOAD",
        )
    })?;
    let expect_state_root = config
        .expect_state_root
        .as_deref()
        .map(parse_expected_state_root)
        .transpose()
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
    let genesis_hash = config
        .genesis_hash
        .as_deref()
        .map(parse_genesis_hash)
        .transpose()
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;

    let payload_bytes = std::fs::read(payload).map_err(|e| {
        Error::new(
            e.kind(),
            format!("read witness payload {}: {e}", payload.display()),
        )
    })?;
    let witness =
        sybil_verifier::commitments::witness_schema::decode_canonical_witness_bytes(&payload_bytes)
            .map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("decode witness payload: {e}"),
                )
            })?;

    let data_dir = std::path::Path::new(&config.data_dir);
    std::fs::create_dir_all(data_dir).map_err(|e| {
        Error::new(
            e.kind(),
            format!("create data dir {}: {e}", data_dir.display()),
        )
    })?;
    let db_path = data_dir.join("sybil.redb");
    let store = matching_sequencer::store::Store::open(&db_path)
        .map_err(|e| Error::other(format!("open persistent store {}: {e}", db_path.display())))?;
    let summary = store
        .import_witness_genesis(
            witness,
            expect_state_root,
            genesis_hash,
            sequencer_config_from_api(config),
        )
        .await
        .map_err(|e| Error::other(format!("import witness genesis: {e}")))?;

    println!("imported canonical witness into {}", db_path.display());
    println!("height={}", summary.height);
    println!("state_root={}", hex32(&summary.state_root));
    println!("genesis_hash={}", hex32(&summary.genesis_hash));
    println!(
        "accounts={} markets={} market_groups={} resting_orders={} reservations={} withdrawals={}",
        summary.accounts,
        summary.markets,
        summary.market_groups,
        summary.resting_orders,
        summary.account_reservations,
        summary.withdrawals
    );
    println!(
        "deposit_cursor={} next_account_id={} next_market_id={} next_order_id={} next_withdrawal_id={}",
        summary.deposit_cursor,
        summary.next_account_id,
        summary.next_market_id,
        summary.next_order_id,
        summary.next_withdrawal_id
    );
    println!("replay nonces reset during import (SYB-224); clients must re-sign submissions");
    Ok(())
}

fn init_telemetry() -> Telemetry {
    // Prometheus metrics recorder
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder");

    // OpenTelemetry trace export is intentionally opt-in. The public demo runs
    // on a small 2 GB host; metrics and alerts are the default observability path.
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
    let config = ApiConfig::parse();

    if config.import_witness {
        let result = run_witness_import(&config).await;
        shutdown_tracer_provider(tracer_provider);
        return result;
    }

    // Deployment-profile preflight (SYB-133): log the active profile + every
    // knob diverging from the prod-intended baseline, and fail closed when a
    // `prod` start has dev-only knobs wired in. Runs before any store/socket
    // setup so a misconfigured prod box never comes up serving.
    if let Err(msg) = sybil_api::preflight::run_preflight(&config) {
        tracing::error!("{msg}");
        return Err(std::io::Error::other(msg).into());
    }

    // SYB-153: raw Polymarket event JSON must survive restart. The snapshot dir
    // lives on the durable data volume; ensure it exists WITHOUT wiping it, so
    // previously mirrored raw event JSON is served immediately on boot (no ~2 min
    // 404 window until the mirror re-syncs). Each mirror cycle re-pushes an
    // idempotent overwrite-by-event-id upsert, so stale entries self-heal.
    if !config.event_snapshot_dir.is_empty() {
        let dir = std::path::Path::new(&config.event_snapshot_dir);
        match std::fs::create_dir_all(dir) {
            Ok(()) => {
                tracing::info!(dir = %dir.display(), "event snapshot dir ready (persisted across restart)")
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
            Ok(s) => Some(Arc::new(s)),
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
                let result = serve_restore_failure_mode(config.port, prometheus_handle).await;
                shutdown_tracer_provider(tracer_provider);
                return result;
            }
        }
    } else {
        None
    };

    let seq_config = sequencer_config_from_api(&config);
    let needs_persistent_baseline = restored.is_none() && store.is_some();

    let handle = if let Some(state) = restored {
        tracing::info!(
            height = state.height,
            markets = state.markets.len(),
            accounts = state.accounts.iter().count(),
            groups = state.market_groups.len(),
            resting_orders = state.resting_orders.len(),
            "Restored from persistent store"
        );

        let mut sequencer = match BlockSequencer::try_restore(state, oracle, seq_config) {
            Ok(sequencer) => sequencer,
            Err(e) => {
                tracing::error!(error = %e, "failed to replay acknowledged writes");
                let result = serve_restore_failure_mode(config.port, prometheus_handle).await;
                shutdown_tracer_provider(tracer_provider);
                return result;
            }
        };

        // Add any seed markets not already present
        for name in &config.seed_markets {
            if !name.is_empty() && !sequencer.markets().iter().any(|m| m.name == *name) {
                sequencer.markets_mut().add_binary(name);
            }
        }

        SequencerHandle::spawn_with_shared_store(sequencer, store.clone())
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

        SequencerHandle::spawn_with_shared_store(sequencer, store.clone())
    };

    if needs_persistent_baseline {
        let baseline = handle.produce_block().await.map_err(|error| {
            std::io::Error::other(format!(
                "failed to commit the initial persistence baseline: {error}"
            ))
        })?;
        tracing::info!(
            height = baseline.canonical.header.height,
            "Committed initial persistence baseline before accepting writes"
        );
    }

    let worker_cancel = CancellationToken::new();
    let workers = TaskTracker::new();
    workers.spawn(run_process_metrics(worker_cancel.child_token()));

    tracing::info!(
        block_interval_ms = config.block_interval_ms,
        order_ttl_blocks = config.order_ttl_blocks,
        max_open_orders_per_account = config.max_open_orders_per_account,
        max_global_submissions_per_second = config.max_global_submissions_per_second,
        public_account_capacity = config.public_account_capacity,
        public_account_grant_nanos = config.public_account_grant_nanos,
        "Sequencer started"
    );

    // Fail fast: without the admin feed + templates installed, attestation-
    // based resolution is silently broken. Better to crash at startup than
    // to discover it when operators go to resolve a market.
    if let Err(err) = bootstrap_oracle_feeds(&handle, &config).await {
        panic!("failed to bootstrap oracle feeds: {err}");
    }

    let shutdown_handle = handle.clone();
    let state = AppState::new(handle, &config, prometheus_handle);
    match (store.clone(), state.history.clone()) {
        (Some(store), Some(client)) => {
            workers.spawn(sybil_api::history::run_outbox_publisher(
                store,
                client,
                Duration::from_millis(config.history_poll_ms),
                worker_cancel.child_token(),
            ));
        }
        (Some(store), None) => {
            tracing::warn!(
                "history service is not configured; durable outbox rows will accumulate"
            );
            workers.spawn(sybil_api::history::run_outbox_monitor(
                store,
                Duration::from_millis(config.history_poll_ms),
                worker_cancel.child_token(),
            ));
        }
        (None, Some(_)) => {
            tracing::warn!(
                "history service configured without persistent sequencer storage; no durable outbox can be delivered"
            );
        }
        (None, None) => {}
    }
    if let Err(err) = state.initialize_read_models().await {
        panic!("failed to initialize API read models: {err}");
    }
    let leaderboard_state = state.clone();
    let leaderboard_cancel = worker_cancel.child_token();
    workers.spawn(async move {
        leaderboard_state
            .refresh_leaderboard_read_model(leaderboard_cancel)
            .await;
    });
    if let Err(err) = state.rehydrate_auto_resolutions().await {
        tracing::warn!(error = %err, "failed to rehydrate auto-resolution review board");
    }
    let app = create_router(state);
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Listening on {}", addr);
    tracing::info!(
        "OpenAPI spec: http://localhost:{}/openapi.json",
        config.port
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();

    worker_cancel.cancel();
    workers.close();
    if tokio::time::timeout(WORKER_SHUTDOWN_TIMEOUT, workers.wait())
        .await
        .is_err()
    {
        tracing::warn!(
            "API worker shutdown timed out after {}s",
            WORKER_SHUTDOWN_TIMEOUT.as_secs()
        );
    } else {
        tracing::info!("API background workers stopped cleanly");
    }

    if shutdown_handle
        .stop_and_wait(SEQUENCER_SHUTDOWN_TIMEOUT)
        .await
    {
        tracing::info!("sequencer actor stopped cleanly");
    } else {
        tracing::warn!(
            "sequencer actor stop timed out after {}s",
            SEQUENCER_SHUTDOWN_TIMEOUT.as_secs()
        );
    }

    shutdown_tracer_provider(tracer_provider);

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
    use std::io::Write;

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
        harden_admin_key_permissions(path)?;
        let hex_str =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let bytes =
            hex::decode(hex_str.trim()).map_err(|e| format!("decode {}: {e}", path.display()))?;
        let key = SigningKey::from_slice(&bytes)
            .map_err(|e| format!("parse SEC1 scalar at {}: {e}", path.display()))?;
        let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
        Ok(pubkey.compressed_bytes())
    } else {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
        }
        let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut UnwrapErr(getrandom::SysRng),
        );
        let scalar_bytes = key.to_bytes();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|e| format!("create {}: {e}", path.display()))?;
        file.write_all(hex::encode(scalar_bytes).as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        harden_admin_key_permissions(path)?;
        let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
        tracing::info!(path = %path.display(), "generated new admin feed key");
        Ok(pubkey.compressed_bytes())
    }
}

fn harden_admin_key_permissions(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = std::fs::symlink_metadata(path)
            .map_err(|e| format!("inspect {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "admin feed key path {} must be a regular file",
                path.display()
            ));
        }
        let mode = metadata.mode() & 0o777;
        if mode != 0o600 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod 0600 {}: {e}", path.display()))?;
            tracing::warn!(path = %path.display(), previous_mode = format_args!("{mode:04o}"), "hardened admin feed key permissions");
        }
    }
    Ok(())
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
