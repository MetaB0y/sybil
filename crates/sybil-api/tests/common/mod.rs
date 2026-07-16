// Helpers shared across integration tests. Each test file compiles as a
// separate crate, so any helper not used by a given file trips dead_code —
// narrow the allow to the specific helpers so a genuinely unused addition
// still warns.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use matching_engine::MarketSet;
use matching_sequencer::store::Store;
use matching_sequencer::{
    AccountStore, BlockSequencer, PublicKey, SequencerConfig, SequencerHandle,
};
use p256::ecdsa::SigningKey;
use p256::elliptic_curve::rand_core::UnwrapErr;
use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;
use sybil_history::{HistoryHandle, HistoryHttpConfig, HistoryStore};
use sybil_oracle::{FeedId, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

#[allow(dead_code)]
pub mod process;

static NEXT_STORE_ID: AtomicU64 = AtomicU64::new(0);

fn temp_store_path() -> PathBuf {
    let id = NEXT_STORE_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sybil-api-test-{}-{id}.redb", std::process::id()))
}

async fn history_backed_state(
    handle: SequencerHandle,
    store: Arc<Store>,
    mut config: ApiConfig,
) -> AppState {
    let history_path = temp_store_path().with_extension("history.redb");
    let history_store =
        HistoryStore::open(history_path, vec![1, 60, 300, 3_600]).expect("history store opens");
    let history_handle = HistoryHandle::spawn(history_store.clone());
    let history_app = sybil_history::router(
        history_handle,
        history_store,
        HistoryHttpConfig {
            dev_mode: true,
            internal_token: None,
            max_query_concurrency: 4,
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("history listener");
    let addr = listener.local_addr().expect("history listener address");
    tokio::spawn(async move {
        axum::serve(listener, history_app)
            .await
            .expect("history test server");
    });
    config.history_url = format!("http://{addr}");
    config.history_poll_ms = 1;
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(handle, &config, prometheus);
    state
        .initialize_read_models()
        .await
        .expect("API read models initialize");
    let cancel = CancellationToken::new();
    let refresher_state = state.clone();
    let refresher_cancel = cancel.clone();
    tokio::spawn(async move {
        refresher_state
            .refresh_leaderboard_read_model(refresher_cancel)
            .await;
    });
    let history = state.history.clone().expect("history client configured");
    tokio::spawn(sybil_api::history::run_outbox_publisher(
        store,
        history,
        Duration::from_millis(1),
        cancel,
    ));
    state
}

/// Create a test app with optional dev mode. Bootstraps an `admin` feed +
/// `admin_immediate` template out of the box, mirroring production wiring.
/// Returns (router, handle, admin signing key, admin feed id).
#[allow(dead_code)]
pub async fn test_app_with_bootstrap(
    dev_mode: bool,
) -> (Router, SequencerHandle, SigningKey, FeedId) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let sequencer =
        BlockSequencer::with_default_solver(accounts, markets, vec![], SequencerConfig::default());
    let handle = SequencerHandle::spawn(sequencer);

    let admin_key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
        &mut UnwrapErr(getrandom::SysRng),
    );
    let admin_pubkey = PublicKey(*admin_key.verifying_key());
    let admin_feed_id = handle
        .register_feed(FeedPubkey(admin_pubkey.compressed_bytes()), "admin".into())
        .await
        .unwrap();
    handle
        .install_template(ResolutionTemplate {
            id: TemplateId("admin_immediate".into()),
            policy: ResolutionPolicy::Immediate {
                feed_id: admin_feed_id,
            },
        })
        .await
        .unwrap();

    let config = ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    };
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(handle.clone(), &config, prometheus);
    (create_router(state), handle, admin_key, admin_feed_id)
}

/// Create a test app without feed/template bootstrap. The trusted unsigned
/// dev-mode resolution path remains available.
#[allow(dead_code)]
pub async fn test_app(dev_mode: bool) -> (Router, SequencerHandle) {
    test_app_with_config(ApiConfig {
        dev_mode,
        bridge_chain_id: "1".to_string(),
        bridge_vault_address: hex::encode([0x10; 20]),
        bridge_token_address: hex::encode([0x20; 20]),
        ..ApiConfig::default()
    })
    .await
}

/// Create a test app with an explicit API config.
#[allow(dead_code)]
pub async fn test_app_with_config(config: ApiConfig) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let sequencer_config = SequencerConfig {
        min_resting_order_notional_nanos: config.min_resting_order_notional_nanos,
        ..SequencerConfig::default()
    };
    let sequencer =
        BlockSequencer::with_default_solver(accounts, markets, vec![], sequencer_config);
    let handle = SequencerHandle::spawn(sequencer);
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(handle.clone(), &config, prometheus);
    (create_router(state), handle)
}

/// Create a test app backed by the production persistent store path. Use this
/// for endpoints that depend on qMDB state roots or proofs.
#[allow(dead_code)]
pub async fn test_app_with_store(dev_mode: bool) -> (Router, SequencerHandle) {
    test_app_with_store_config(
        dev_mode,
        SequencerConfig {
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        },
    )
    .await
}

/// Create a store-backed test app with an explicit sequencer config.
#[allow(dead_code)]
pub async fn test_app_with_store_config(
    dev_mode: bool,
    sequencer_config: SequencerConfig,
) -> (Router, SequencerHandle) {
    test_app_with_store_api_config(
        ApiConfig {
            dev_mode,
            ..ApiConfig::default()
        },
        sequencer_config,
    )
    .await
}

/// Create a store-backed test app with explicit API and sequencer config.
#[allow(dead_code)]
pub async fn test_app_with_store_api_config(
    api_config: ApiConfig,
    sequencer_config: SequencerConfig,
) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let sequencer =
        BlockSequencer::with_default_solver(accounts, markets, vec![], sequencer_config);
    let store = Arc::new(Store::open(&temp_store_path()).expect("test store opens"));
    let handle = SequencerHandle::spawn_with_shared_store(sequencer, Some(Arc::clone(&store)));
    handle
        .produce_block()
        .await
        .expect("store-backed test app commits its replay baseline");
    let state = history_backed_state(handle.clone(), store, api_config).await;
    (create_router(state), handle)
}

/// Store-backed test app with the attached history projector.
/// Equity/events/fills are served only by that projector, so tests using this
/// prove the extracted-service path.
#[allow(dead_code)]
pub async fn test_app_with_store_zero_caps(dev_mode: bool) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let config = SequencerConfig {
        block_interval: Duration::from_secs(60 * 60),
        ..SequencerConfig::default()
    };
    let sequencer = BlockSequencer::with_default_solver(accounts, markets, vec![], config);
    let store = Arc::new(Store::open(&temp_store_path()).expect("test store opens"));
    let handle = SequencerHandle::spawn_with_shared_store(sequencer, Some(Arc::clone(&store)));
    handle
        .produce_block()
        .await
        .expect("store-backed test app commits its replay baseline");
    let api_config = ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    };
    let state = history_backed_state(handle.clone(), store, api_config).await;
    (create_router(state), handle)
}

/// Send a GET request and return (status, body bytes).
#[allow(dead_code)]
pub async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let is_history = uri.contains("/fills")
        || uri.contains("/events")
        || uri.contains("/equity")
        || uri.contains("/prices/history")
        || uri.contains("/prices/candles");
    let expected_height = if is_history {
        let request = Request::builder()
            .uri("/v1/health")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("height").and_then(serde_json::Value::as_u64))
    } else {
        None
    };
    if uri.contains("/leaderboard") {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        let projection_pending = is_history
            && status.is_success()
            && serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .is_some_and(|value| {
                    value
                        .get("indexed_through_height")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|indexed| expected_height.is_some_and(|head| indexed < head))
                });
        if !projection_pending || tokio::time::Instant::now() >= deadline {
            return (status, body);
        }
        // Projection is deliberately asynchronous. Poll the independent test
        // service instead of reintroducing a synchronous commit dependency.
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Send a POST request with JSON body and return (status, body bytes).
#[allow(dead_code)]
pub async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body)
}

/// Send a PUT request with a JSON body and return (status, body bytes).
#[allow(dead_code)]
pub async fn put_json(app: Router, uri: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body)
}

/// Send a POST request with JSON body and return (status, headers, body bytes).
#[allow(dead_code)]
pub async fn post_json_with_headers(
    app: Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}
