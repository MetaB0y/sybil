// Helpers shared across integration tests. Each test file compiles as a
// separate crate, so any helper not used by a given file trips dead_code —
// narrow the allow to the specific helpers so a genuinely unused addition
// still warns.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use matching_engine::MarketSet;
use matching_sequencer::store::Store;
use matching_sequencer::{
    AccountStore, AdminOracle, BlockSequencer, PublicKey, SequencerConfig, SequencerHandle,
};
use p256::ecdsa::SigningKey;
use p256::elliptic_curve::rand_core::UnwrapErr;
use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;
use sybil_oracle::{FeedId, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};
use tower::ServiceExt;

static NEXT_STORE_ID: AtomicU64 = AtomicU64::new(0);

fn temp_store_path() -> PathBuf {
    let id = NEXT_STORE_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sybil-api-test-{}-{id}.redb", std::process::id()))
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
    let oracle = Arc::new(AdminOracle::new());
    let sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
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

    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let config = ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    };
    let state = AppState::new(handle.clone(), &config, prometheus);
    (create_router(state), handle, admin_key, admin_feed_id)
}

/// Create a test app without the oracle bootstrap (legacy path used by older
/// integration tests — the admin unsigned dev-mode resolve path still works).
#[allow(dead_code)]
pub async fn test_app(dev_mode: bool) -> (Router, SequencerHandle) {
    test_app_with_config(ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    })
    .await
}

/// Create a test app with an explicit API config.
#[allow(dead_code)]
pub async fn test_app_with_config(config: ApiConfig) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
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
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    let store = Store::open(&temp_store_path()).expect("test store opens");
    let handle = SequencerHandle::spawn_with_store(sequencer, Some(store));
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let config = ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    };
    let state = AppState::new(handle.clone(), &config, prometheus);
    (create_router(state), handle)
}

/// Store-backed test app with the in-memory off-block caps set to 0 — the
/// production config. Equity/history are served ONLY from redb (the in-memory
/// rings stay empty), so tests using this prove the store read path rather than
/// the in-memory fallback.
#[allow(dead_code)]
pub async fn test_app_with_store_zero_caps(dev_mode: bool) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let config = SequencerConfig {
        max_equity_points_per_account: 0,
        max_history_events_per_account: 0,
        ..SequencerConfig::default()
    };
    let sequencer =
        BlockSequencer::with_default_solver(accounts, markets, vec![], oracle, config);
    let store = Store::open(&temp_store_path()).expect("test store opens");
    let handle = SequencerHandle::spawn_with_store(sequencer, Some(store));
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let api_config = ApiConfig {
        dev_mode,
        ..ApiConfig::default()
    };
    let state = AppState::new(handle.clone(), &api_config, prometheus);
    (create_router(state), handle)
}

/// Send a GET request and return (status, body bytes).
#[allow(dead_code)]
pub async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
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
