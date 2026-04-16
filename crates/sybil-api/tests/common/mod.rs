use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use matching_engine::MarketSet;
use matching_sequencer::{AccountStore, AdminOracle, BlockSequencer, SequencerConfig, SequencerHandle};
use sybil_api::app::create_router;
use sybil_api::state::AppState;
use tower::ServiceExt;

/// Create a test app with optional dev mode. Returns the router and sequencer handle.
pub async fn test_app(dev_mode: bool) -> (Router, SequencerHandle) {
    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let sequencer =
        BlockSequencer::with_default_solver(accounts, markets, vec![], oracle, SequencerConfig::default());
    let handle = SequencerHandle::spawn(sequencer);
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState {
        sequencer: handle.clone(),
        dev_mode,
        prometheus,
        reference_prices: Default::default(),
        market_ref_data: Default::default(),
    };
    (create_router(state), handle)
}

/// Send a GET request and return (status, body bytes).
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
