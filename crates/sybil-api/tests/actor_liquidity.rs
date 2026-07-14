mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sybil_api::config::ApiConfig;
use tower::ServiceExt;

use common::test_app_with_config;

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
const MM_TOKEN: &str = "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm";
const NOISE_TOKEN: &str = "00000000000000000000000000000000";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn credential_file() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sybil-actor-liquidity-{}-{}.json",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut actors = vec![json!({
        "principal_id": "mm",
        "role": "market_maker",
        "account_id": 1,
        "token": MM_TOKEN,
    })];
    actors.extend((0..15).map(|index| {
        json!({
            "principal_id": format!("noise-{index}"),
            "role": "noise",
            "account_id": index + 2,
            "token": format!("{index:032}"),
        })
    }));
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "actors": actors })).unwrap(),
    )
    .unwrap();
    path
}

async fn request(
    app: axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
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

#[tokio::test]
async fn actor_epochs_support_sparse_noise_exact_mm_ioc_and_observability() {
    let credentials = credential_file();
    let (app, handle) = test_app_with_config(ApiConfig {
        dev_mode: true,
        actor_credentials_path: credentials.to_string_lossy().into_owned(),
        ..ApiConfig::default()
    })
    .await;
    for _ in 0..17 {
        handle.create_account(10_000_000_000).await.unwrap();
    }
    let first = handle.create_market("first".into()).await.unwrap();
    let second = handle.create_market("second".into()).await.unwrap();
    handle
        .activate_liquidity_universe(1, [9; 32], vec![first, second])
        .await
        .unwrap();
    handle.produce_block().await.unwrap();

    let (status, inventory) = request(
        app.clone(),
        Method::POST,
        "/v1/actor/inventory",
        Some(MM_TOKEN),
        json!({"actions": [
            {"action": "collateralize", "market_id": first.0, "quantity": 2000},
            {"action": "collateralize", "market_id": second.0, "quantity": 2000}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inventory}");

    let (status, actor_view) = request(
        app.clone(),
        Method::GET,
        "/v1/actor/universe",
        Some(NOISE_TOKEN),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(actor_view["actor_role"], "noise");
    assert_eq!(actor_view["account_id"], 2);
    assert_eq!(actor_view["market_ids"], json!([first.0, second.0]));

    let timestamp = now_ms();
    let payload = json!({
        "epoch_id": "noise-0-1-2",
        "target_height": 2,
        "universe_generation": 1,
        "observed_at_ms": timestamp,
        "valid_until_ms": timestamp + 25_000,
        "market_intents": [
            {"market_id": first.0, "orders": [{"type": "BuyYes", "market_id": first.0, "limit_price_nanos": 600_000_000, "quantity": 1000}]}
        ]
    });
    let (status, response) = request(
        app.clone(),
        Method::POST,
        "/v1/actor/epochs",
        Some(MM_TOKEN),
        json!({
            "epoch_id": "mm-1-2",
            "target_height": 2,
            "universe_generation": 1,
            "observed_at_ms": timestamp,
            "valid_until_ms": timestamp + 25_000,
            "mm_budget_nanos": 5_000_000_000u64,
            "market_intents": [
                {"market_id": first.0, "orders": [
                    {"type": "SellYes", "market_id": first.0, "limit_price_nanos": 550_000_000, "quantity": 1000},
                    {"type": "SellNo", "market_id": first.0, "limit_price_nanos": 550_000_000, "quantity": 1000}
                ]},
                {"market_id": second.0, "orders": [
                    {"type": "SellYes", "market_id": second.0, "limit_price_nanos": 550_000_000, "quantity": 1000},
                    {"type": "SellNo", "market_id": second.0, "limit_price_nanos": 550_000_000, "quantity": 1000}
                ]}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");

    let (status, quotes) = request(
        app.clone(),
        Method::GET,
        "/v1/actor/mm-quotes?target_height=2",
        Some(NOISE_TOKEN),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{quotes}");
    assert_eq!(quotes["markets"].as_array().unwrap().len(), 2);

    let (status, response) = request(
        app.clone(),
        Method::POST,
        "/v1/actor/epochs",
        Some(NOISE_TOKEN),
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["considered"], 2);
    assert_eq!(response["selected"], 1);
    assert_eq!(response["markets"].as_array().unwrap().len(), 1);

    let (status, open_batch) = request(
        app.clone(),
        Method::GET,
        &format!("/v1/markets/{}/open-batch", first.0),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{open_batch}");
    assert_eq!(open_batch["unique_placers"], 2);

    let (status, response) = request(
        app.clone(),
        Method::POST,
        "/v1/actor/epochs",
        Some(NOISE_TOKEN),
        json!({
            "epoch_id": "incomplete",
            "target_height": 2,
            "universe_generation": 1,
            "observed_at_ms": timestamp,
            "valid_until_ms": timestamp + 25_000,
            "market_intents": [{"market_id": 999_999, "skip_reason": "invalid"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");

    let block = handle.produce_block().await.unwrap();
    assert_eq!(block.canonical.header.order_count, 5);
    assert_eq!(block.analytics.unique_placers, 2);
    assert_eq!(block.analytics.placers_by_market[&first], 2);
    assert_eq!(block.analytics.placers_by_market[&second], 1);
    assert!(handle.get_pending_orders(None).await.unwrap().is_empty());

    let (status, overview) = request(
        app.clone(),
        Method::GET,
        "/v1/activity/overview",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["all_time"]["unique_traders"], 2);

    let (status, health) = request(
        app.clone(),
        Method::GET,
        "/v1/liquidity/health",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{health}");
    assert_eq!(health["active_markets"], 2);
    assert_eq!(health["observed_noise_actors"], 1);
    assert_eq!(health["markets"][0]["noise_orders"], 1);
    assert_eq!(health["noise_markets_crossing_mm"], 1);

    let (status, _) = request(
        app,
        Method::GET,
        "/v1/actor/universe",
        Some("wrong-token"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    std::fs::remove_file(credentials).ok();
}

#[tokio::test]
async fn mm_inventory_endpoint_collateralizes_exact_complete_sets() {
    let credentials = credential_file();
    let (app, handle) = test_app_with_config(ApiConfig {
        dev_mode: true,
        actor_credentials_path: credentials.to_string_lossy().into_owned(),
        ..ApiConfig::default()
    })
    .await;
    for _ in 0..17 {
        handle.create_account(10_000_000_000).await.unwrap();
    }
    let market = handle.create_market("inventory".into()).await.unwrap();

    let (status, response) = request(
        app.clone(),
        Method::POST,
        "/v1/actor/inventory",
        Some(MM_TOKEN),
        json!({"actions": [{"action": "collateralize", "market_id": market.0, "quantity": 2000}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let account = handle
        .get_account(matching_sequencer::AccountId(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.position(market, 0), 2000);
    assert_eq!(account.position(market, 1), 2000);

    let (status, _) = request(
        app,
        Method::POST,
        "/v1/actor/inventory",
        Some(NOISE_TOKEN),
        json!({"actions": [{"action": "redeem", "market_id": market.0, "quantity": 1000}]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    std::fs::remove_file(credentials).ok();
}
