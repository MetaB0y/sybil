//! Integration tests for the SYB-48 auto-resolution review board routes.
//!
//! These assert the review board's contract: it records proposals and operator
//! decisions but never settles a market itself. Runs against a dev-mode app so
//! the service-auth middleware passes through (auth itself is pinned by
//! `route_policy.rs`).

mod common;

use axum::http::StatusCode;
use common::{get, post_json, test_app};
use serde_json::json;
use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;

fn parse(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).expect("valid JSON")
}

#[tokio::test]
async fn propose_then_reject_is_a_durable_veto() {
    let (app, _seq) = test_app(true).await;

    // Propose (high confidence) with a challenge-window deadline.
    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 1,
            "action": "propose",
            "payout_nanos": 1_000_000_000u64,
            "confidence": 0.95,
            "reasoning": "source clearly reports YES",
            "evidence_excerpts": ["YES per source"],
            "eta_ms": 9_999_999_999_999u64
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(parse(&body)["status"], "pending");

    // Listed as pending.
    let (status, body) = get(app.clone(), "/v1/admin/auto-resolutions").await;
    assert_eq!(status, StatusCode::OK);
    let entries = parse(&body)["entries"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "pending");

    // Operator rejects: terminal veto.
    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions/1/reject",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse(&body)["status"], "rejected");

    // Re-proposing the same market must NOT clear the veto.
    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 1,
            "action": "propose",
            "payout_nanos": 1_000_000_000u64,
            "confidence": 0.99,
            "reasoning": "still YES",
            "eta_ms": 9_999_999_999_999u64
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse(&body)["status"], "rejected");
}

#[tokio::test]
async fn veto_survives_api_restart_from_sequencer_store() {
    let (app, handle) = common::test_app_with_store(true).await;

    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 11,
            "action": "propose",
            "payout_nanos": 1_000_000_000u64,
            "confidence": 0.95,
            "reasoning": "first pass says yes",
            "evidence_excerpts": ["YES"],
            "eta_ms": 9_999_999_999_999u64
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions/11/reject",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(
        handle,
        &ApiConfig {
            dev_mode: true,
            ..ApiConfig::default()
        },
        prometheus,
    );
    let restarted_app = create_router(state);

    let (status, body) = get(restarted_app, "/v1/admin/auto-resolutions").await;
    assert_eq!(status, StatusCode::OK);
    let entries = parse(&body)["entries"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["market_id"], 11);
    assert_eq!(entries[0]["status"], "rejected");
}

#[tokio::test]
async fn review_entry_can_be_approved() {
    let (app, _seq) = test_app(true).await;

    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 5,
            "action": "review",
            "payout_nanos": 0u64,
            "confidence": 0.8,
            "reasoning": "borderline"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse(&body)["status"], "needs_review");

    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions/5/approve",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse(&body)["status"], "approved");
}

#[tokio::test]
async fn approval_survives_api_restart_from_sequencer_store() {
    let (app, handle) = common::test_app_with_store(true).await;

    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 12,
            "action": "propose",
            "payout_nanos": 1_000_000_000u64,
            "confidence": 0.95,
            "reasoning": "yes",
            "eta_ms": 9_999_999_999_999u64
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions/12/approve",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(
        handle,
        &ApiConfig {
            dev_mode: true,
            ..ApiConfig::default()
        },
        prometheus,
    );
    let restarted_app = create_router(state);

    let (status, body) = get(restarted_app, "/v1/admin/auto-resolutions").await;
    assert_eq!(status, StatusCode::OK);
    let entries = parse(&body)["entries"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["market_id"], 12);
    assert_eq!(entries[0]["status"], "approved");
}

#[tokio::test]
async fn propose_without_eta_is_rejected() {
    let (app, _seq) = test_app(true).await;
    let (status, _) = post_json(
        app,
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 3,
            "action": "propose",
            "payout_nanos": 1_000_000_000u64,
            "confidence": 0.95,
            "reasoning": "no eta"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn out_of_range_inputs_are_rejected() {
    let (app, _seq) = test_app(true).await;

    // payout above $1.
    let (status, _) = post_json(
        app.clone(),
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 3,
            "action": "review",
            "payout_nanos": 2_000_000_000u64,
            "confidence": 0.8,
            "reasoning": "bad payout"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // confidence out of [0,1].
    let (status, _) = post_json(
        app,
        "/v1/admin/auto-resolutions",
        json!({
            "market_id": 3,
            "action": "review",
            "payout_nanos": 0u64,
            "confidence": 1.5,
            "reasoning": "bad confidence"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn approve_missing_market_is_404() {
    let (app, _seq) = test_app(true).await;
    let (status, _) = post_json(app, "/v1/admin/auto-resolutions/999/approve", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
