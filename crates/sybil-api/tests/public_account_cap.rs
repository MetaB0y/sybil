//! Public onboarding demo-balance cap (POST /v1/accounts).
//!
//! The onboarding route lives on the PUBLIC tier, so anonymous callers are
//! capped at the demo ceiling ($5,000) outside dev mode to stop them minting an
//! arbitrary play-money balance. Trusted service infra (arena bots) hits the
//! same endpoint with a valid `Authorization: Bearer <SYBIL_SERVICE_TOKEN>` and
//! must remain UNCAPPED — the in-handler service-token check restores the
//! pre-public-move behaviour without re-gating the whole route.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::test_app_with_config;
use http_body_util::BodyExt;
use matching_sequencer::crypto::PublicKey;
use p256::ecdsa::SigningKey;
use serde_json::{Value, json};
use sybil_api::config::ApiConfig;
use tower::ServiceExt;

const SERVICE_TOKEN: &str = "test-service-token";
/// One nano-dollar above the $5,000 public demo ceiling.
const OVER_CAP_NANOS: u64 = 5_000_000_000_001;
/// Comfortably within the $5,000 public demo ceiling.
const WITHIN_CAP_NANOS: u64 = 1_000_000_000_000;

/// Non-dev app with a configured service token, mirroring prod wiring.
fn non_dev_config() -> ApiConfig {
    ApiConfig {
        dev_mode: false,
        service_token: SERVICE_TOKEN.to_string(),
        ..ApiConfig::default()
    }
}

/// POST /v1/accounts with an optional bearer token; returns (status, body).
async fn create_account(
    app: &axum::Router,
    body: Value,
    bearer: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/v1/accounts")
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let req = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes)
}

fn atomic_body(balance: u64) -> Value {
    let key = SigningKey::from_bytes((&[31u8; 32]).into()).expect("fixed signing key");
    json!({
        "initial_balance_nanos": balance,
        "initial_key": {
            "public_key_hex": hex::encode(PublicKey(*key.verifying_key()).compressed_bytes())
        }
    })
}

#[tokio::test]
async fn non_dev_no_token_over_cap_is_rejected() {
    let (app, _handle) = test_app_with_config(non_dev_config()).await;
    let (status, body) = create_account(&app, atomic_body(OVER_CAP_NANOS), None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "anonymous over-cap onboarding must be rejected: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn non_dev_valid_service_token_over_cap_is_uncapped() {
    let (app, _handle) = test_app_with_config(non_dev_config()).await;
    let (status, body) = create_account(
        &app,
        json!({ "initial_balance_nanos": OVER_CAP_NANOS }),
        Some(SERVICE_TOKEN),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "valid service token must lift the demo cap: {}",
        String::from_utf8_lossy(&body)
    );
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        parsed["balance_nanos"].as_u64(),
        Some(OVER_CAP_NANOS),
        "the full uncapped balance must be credited"
    );
}

#[tokio::test]
async fn non_dev_no_token_within_cap_is_allowed() {
    let (app, _handle) = test_app_with_config(non_dev_config()).await;
    let (status, body) = create_account(&app, atomic_body(WITHIN_CAP_NANOS), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "anonymous within-cap onboarding must succeed: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn non_dev_invalid_token_over_cap_stays_capped() {
    let (app, _handle) = test_app_with_config(non_dev_config()).await;
    let (status, _body) =
        create_account(&app, atomic_body(OVER_CAP_NANOS), Some("wrong-token")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a garbage bearer token must NOT lift the demo cap"
    );
}
