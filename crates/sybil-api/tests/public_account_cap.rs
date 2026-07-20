//! Permanent public-account stock policy.
//!
//! Anonymous onboarding has a dedicated DTO with no funding field, receives a
//! server-selected grant, and consumes a monotonic lifetime account-id budget.
//! Service/dev creation remains a separate trusted operation.

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
static NEXT_PROVISIONING_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn capped_config(capacity: u64, grant_nanos: u64) -> ApiConfig {
    ApiConfig {
        dev_mode: false,
        service_token: SERVICE_TOKEN.to_string(),
        public_account_capacity: capacity,
        public_account_grant_nanos: grant_nanos,
        http_onboarding_global_rps: 1_000,
        http_onboarding_global_burst: 1_000,
        http_onboarding_client_rps: 1_000,
        http_onboarding_client_burst: 1_000,
        ..ApiConfig::default()
    }
}

async fn request(
    app: &axum::Router,
    method: Method,
    path: &str,
    mut body: Value,
    bearer: Option<&str>,
) -> (StatusCode, Value) {
    if path == "/v1/accounts"
        && let Some(object) = body.as_object_mut()
    {
        object.entry("provisioning_key").or_insert_with(|| {
            Value::String(format!(
                "public-cap-test/{}",
                NEXT_PROVISIONING_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
        });
    }
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

fn onboarding_body(seed: u8) -> Value {
    let key = SigningKey::from_bytes((&[seed; 32]).into()).expect("fixed signing key");
    json!({
        "initial_key": {
            "public_key_hex": hex::encode(PublicKey(*key.verifying_key()).compressed_bytes())
        }
    })
}

#[tokio::test]
async fn public_onboarding_uses_the_server_grant_and_rejects_funding_fields() {
    let grant = 123_000_000_000;
    let (app, _) = test_app_with_config(capped_config(10, grant)).await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        onboarding_body(1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(common::nanos_i64(&body["balance_nanos"]), grant as i64);

    let mut caller_funded = onboarding_body(2);
    caller_funded["initial_balance_nanos"] = json!(9_999_000_000_000u64);
    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        caller_funded,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn accumulated_public_account_stock_stops_at_the_lifetime_capacity() {
    let (app, handle) = test_app_with_config(capped_config(2, 100)).await;
    for seed in [1, 2] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/v1/onboarding/accounts",
            onboarding_body(seed),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (status, body) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        onboarding_body(3),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED");
    assert_eq!(handle.account_stock().await.unwrap(), 2);
    assert_eq!(handle.public_account_stock().await.unwrap(), 2);

    let (status, policy) = request(&app, Method::GET, "/v1/onboarding", Value::Null, None).await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    assert_eq!(policy["enabled"], false);
    assert_eq!(policy["account_capacity"], 2);
    assert_eq!(policy["accounts_allocated"], 2);
    assert_eq!(policy["accounts_remaining"], 0);
    assert_eq!(common::nanos_u64(&policy["grant_nanos"]), 100);
}

#[tokio::test]
async fn concurrent_callers_cannot_overshoot_the_stock_limit() {
    let (app, handle) = test_app_with_config(capped_config(3, 0)).await;
    let mut tasks = tokio::task::JoinSet::new();
    for seed in 1..=12 {
        let app = app.clone();
        tasks.spawn(async move {
            request(
                &app,
                Method::POST,
                "/v1/onboarding/accounts",
                onboarding_body(seed),
                None,
            )
            .await
            .0
        });
    }

    let mut created = 0;
    let mut exhausted = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            StatusCode::OK => created += 1,
            StatusCode::CONFLICT => exhausted += 1,
            status => panic!("unexpected onboarding status {status}"),
        }
    }
    assert_eq!(created, 3);
    assert_eq!(exhausted, 9);
    assert_eq!(handle.account_stock().await.unwrap(), 3);
    assert_eq!(handle.public_account_stock().await.unwrap(), 3);
}

#[tokio::test]
async fn service_creation_is_a_separate_explicitly_funded_operator_path() {
    let (app, handle) = test_app_with_config(capped_config(1, 10)).await;
    let operator_balance = 9_999_000_000_000u64;
    let (status, body) = request(
        &app,
        Method::POST,
        "/v1/accounts",
        json!({ "initial_balance_nanos": operator_balance }),
        Some(SERVICE_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        common::nanos_i64(&body["balance_nanos"]),
        operator_balance as i64
    );
    assert_eq!(handle.public_account_stock().await.unwrap(), 0);

    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        onboarding_body(1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(handle.account_stock().await.unwrap(), 2);
    assert_eq!(handle.public_account_stock().await.unwrap(), 1);

    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 1 }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn service_provisioning_retries_return_one_account_and_conflicts_are_explicit() {
    let (app, handle) = test_app_with_config(capped_config(2, 10)).await;
    let request_body = json!({
        "provisioning_key": "operator-mm/v1",
        "initial_balance_nanos": 123
    });
    let (first_status, first) = request(
        &app,
        Method::POST,
        "/v1/accounts",
        request_body.clone(),
        Some(SERVICE_TOKEN),
    )
    .await;
    assert_eq!(first_status, StatusCode::OK, "{first}");
    let stock = handle.account_stock().await.unwrap();

    // This is the caller's recovery path after losing the first HTTP response.
    let (retry_status, retry) = request(
        &app,
        Method::POST,
        "/v1/accounts",
        request_body,
        Some(SERVICE_TOKEN),
    )
    .await;
    assert_eq!(retry_status, StatusCode::OK, "{retry}");
    assert_eq!(retry["account_id"], first["account_id"]);
    assert_eq!(handle.account_stock().await.unwrap(), stock);
    assert_eq!(handle.public_account_stock().await.unwrap(), 0);

    let (conflict_status, conflict) = request(
        &app,
        Method::POST,
        "/v1/accounts",
        json!({
            "provisioning_key": "operator-mm/v1",
            "initial_balance_nanos": 124
        }),
        Some(SERVICE_TOKEN),
    )
    .await;
    assert_eq!(conflict_status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "ACCOUNT_PROVISIONING_CONFLICT");
    assert_eq!(handle.account_stock().await.unwrap(), stock);
}

#[tokio::test]
async fn onboarding_has_a_dedicated_pre_handler_rate_budget() {
    let mut config = capped_config(10, 0);
    config.http_onboarding_global_rps = 1;
    config.http_onboarding_global_burst = 1;
    config.http_onboarding_client_rps = 1;
    config.http_onboarding_client_burst = 1;
    let (app, _) = test_app_with_config(config).await;

    let (first, _) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        onboarding_body(1),
        None,
    )
    .await;
    let (second, _) = request(
        &app,
        Method::POST,
        "/v1/onboarding/accounts",
        onboarding_body(2),
        None,
    )
    .await;
    assert_eq!(first, StatusCode::OK);
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
}
