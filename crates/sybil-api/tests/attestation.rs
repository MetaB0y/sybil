//! Development attestation stub and its production mount boundary (SYB-43).

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::{get, test_app};

#[tokio::test]
async fn dev_mode_returns_an_explicit_empty_stub() {
    let (app, _) = test_app(true).await;
    let (status, body) = get(app, "/v1/attestation").await;
    assert_eq!(status, StatusCode::OK);

    let response: Value = serde_json::from_slice(&body).expect("JSON response");
    assert_eq!(response["is_stub"], true);
    assert_eq!(response["pcr_values"], serde_json::json!({}));
    assert_eq!(response["enclave_pubkey"], "");
    assert_eq!(response["report_data"], "");
    assert_eq!(response["signature"], "");
}

#[tokio::test]
async fn production_does_not_mount_the_stub() {
    let (app, _) = test_app(false).await;
    let (status, _) = get(app, "/v1/attestation").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
