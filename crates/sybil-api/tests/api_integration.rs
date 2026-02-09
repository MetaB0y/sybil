//! In-process API integration tests using tower::ServiceExt::oneshot().
//!
//! These tests exercise the full Axum router with a real BlockSequencer
//! underneath, without binding to a port.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

use common::{get, post_json, test_app};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("response body is valid JSON")
}

// ---------------------------------------------------------------------------
// A. Dev mode gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_account_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) = post_json(app, "/v1/accounts", json!({ "initial_balance_nanos": 100 })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_market_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) = post_json(app, "/v1/markets", json!({ "name": "Test" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn fund_account_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) =
        post_json(app, "/v1/accounts/0/fund", json!({ "amount_nanos": 100 })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn resolve_market_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) = post_json(
        app,
        "/v1/markets/0/resolve",
        json!({ "payout_nanos": 1_000_000_000u64 }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// B. 404 handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_nonexistent_account_404() {
    let (app, _) = test_app(true).await;
    let (status, _) = get(app, "/v1/accounts/9999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_nonexistent_market_404() {
    let (app, _) = test_app(true).await;
    let (status, _) = get(app, "/v1/markets/9999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_nonexistent_block_404() {
    let (app, _) = test_app(true).await;
    // No blocks produced yet
    let (status, _) = get(app, "/v1/blocks/latest").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// C. CRUD lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_get_account() {
    let (app, _) = test_app(true).await;

    let balance = 100_000_000_000u64; // $100

    // Create
    let (status, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    let account_id = resp["account_id"].as_u64().unwrap();

    // Get
    let (status, body) = get(app, &format!("/v1/accounts/{}", account_id)).await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(resp["account_id"].as_u64().unwrap(), account_id);
    assert_eq!(resp["balance_nanos"].as_i64().unwrap(), balance as i64);
}

#[tokio::test]
async fn fund_account_increases_balance() {
    let (app, _) = test_app(true).await;

    let initial = 50_000_000_000u64;
    let fund_amount = 25_000_000_000u64;

    // Create
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": initial }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    // Fund
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{}/fund", account_id),
        json!({ "amount_nanos": fund_amount }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(
        resp["balance_nanos"].as_i64().unwrap(),
        (initial + fund_amount) as i64
    );
}

#[tokio::test]
async fn create_market_with_metadata() {
    let (app, _) = test_app(true).await;

    let (status, body) = post_json(
        app.clone(),
        "/v1/markets",
        json!({
            "name": "Will it rain?",
            "description": "Whether it rains tomorrow",
            "category": "weather",
            "tags": ["rain", "forecast"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    let market_id = resp["market_id"].as_u64().unwrap();

    // Get and verify
    let (status, body) = get(app, &format!("/v1/markets/{}", market_id)).await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(resp["name"].as_str().unwrap(), "Will it rain?");
    assert_eq!(resp["description"].as_str().unwrap(), "Whether it rains tomorrow");
    assert_eq!(resp["category"].as_str().unwrap(), "weather");
    assert_eq!(resp["status"].as_str().unwrap(), "active");
}

#[tokio::test]
async fn market_search_by_tag() {
    let (app, _) = test_app(true).await;

    // Create two markets with different tags
    post_json(
        app.clone(),
        "/v1/markets",
        json!({ "name": "Rain?", "tags": ["weather"] }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/markets",
        json!({ "name": "Election?", "tags": ["politics"] }),
    )
    .await;

    // Search by tag
    let (status, body) = get(app, "/v1/markets/search?tags=weather").await;
    assert_eq!(status, StatusCode::OK);
    let results = parse_json(&body);
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"].as_str().unwrap(), "Rain?");
}

// ---------------------------------------------------------------------------
// D. Order validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submit_order_invalid_market_400() {
    let (app, _) = test_app(true).await;

    // Create account but no market
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;

    let (status, _) = post_json(
        app,
        "/v1/orders",
        json!({
            "account_id": 0,
            "orders": [{
                "type": "BuyYes",
                "market_id": 999,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_order_invalid_price_400() {
    let (app, _) = test_app(true).await;

    // Create account and market
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let (status, _) = post_json(
        app,
        "/v1/orders",
        json!({
            "account_id": 0,
            "orders": [{
                "type": "BuyYes",
                "market_id": 0,
                "limit_price_nanos": 2_000_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// E. End-to-end trade lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_trade_lifecycle() {
    let (app, handle) = test_app(true).await;

    let balance = 100_000_000_000u64; // $100

    // Create 2 accounts
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    let acct_a = parse_json(&body)["account_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    let acct_b = parse_json(&body)["account_id"].as_u64().unwrap();

    // Create 1 market
    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Test Market" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    // Account A: BuyYes at 60%, qty 10
    let (status, _) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": acct_a,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 600_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Account B: BuyNo at 50%, qty 10
    let (status, _) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": acct_b,
            "orders": [{
                "type": "BuyNo",
                "market_id": market_id,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Force block production
    let block = handle.produce_block().await.unwrap();
    assert!(!block.fills.is_empty(), "Expected fills from matching orders");

    // Verify block via API
    let (status, body) = get(app.clone(), "/v1/blocks/latest").await;
    assert_eq!(status, StatusCode::OK);
    let block_resp = parse_json(&body);
    assert!(block_resp["fill_count"].as_u64().unwrap() > 0);

    // Verify account A has positions
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}", acct_a)).await;
    assert_eq!(status, StatusCode::OK);
    let acct_a_resp = parse_json(&body);
    assert!(
        acct_a_resp["balance_nanos"].as_i64().unwrap() < balance as i64,
        "Account A balance should have decreased"
    );
    assert!(
        !acct_a_resp["positions"].as_array().unwrap().is_empty(),
        "Account A should have positions"
    );

    // Verify portfolio
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}/portfolio", acct_a)).await;
    assert_eq!(status, StatusCode::OK);
    let portfolio = parse_json(&body);
    assert!(
        !portfolio["positions"].as_array().unwrap().is_empty(),
        "Portfolio should show positions"
    );

    // Verify fills
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}/fills", acct_a)).await;
    assert_eq!(status, StatusCode::OK);
    let fills = parse_json(&body);
    assert!(
        !fills.as_array().unwrap().is_empty(),
        "Account A should have fill records"
    );

    // Resolve market (YES wins)
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/markets/{}/resolve", market_id),
        json!({ "payout_nanos": 1_000_000_000u64 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify positions cleared after resolution
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}", acct_a)).await;
    assert_eq!(status, StatusCode::OK);
    let acct_a_post = parse_json(&body);

    // After YES wins, account A (who bought YES) should have gained
    let yes_positions: Vec<&Value> = acct_a_post["positions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            p["market_id"].as_u64().unwrap() == market_id
        })
        .collect();
    assert!(
        yes_positions.is_empty(),
        "Positions should be cleared after resolution"
    );
}

// ---------------------------------------------------------------------------
// F. Portfolio and fills
// ---------------------------------------------------------------------------

#[tokio::test]
async fn portfolio_reflects_positions() {
    let (app, handle) = test_app(true).await;

    let balance = 100_000_000_000u64;

    // Setup
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    // Crossing orders
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 0,
            "orders": [{ "type": "BuyYes", "market_id": 0, "limit_price_nanos": 600_000_000u64, "quantity": 5 }]
        }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 1,
            "orders": [{ "type": "BuyNo", "market_id": 0, "limit_price_nanos": 500_000_000u64, "quantity": 5 }]
        }),
    )
    .await;

    handle.produce_block().await.unwrap();

    let (status, body) = get(app, "/v1/accounts/0/portfolio").await;
    assert_eq!(status, StatusCode::OK);
    let portfolio = parse_json(&body);
    assert_eq!(portfolio["account_id"].as_u64().unwrap(), 0);
    // Total deposited should match initial balance
    assert_eq!(
        portfolio["total_deposited_nanos"].as_i64().unwrap(),
        balance as i64
    );
}

#[tokio::test]
async fn fills_paginated_correctly() {
    let (app, handle) = test_app(true).await;

    let balance = 100_000_000_000u64;

    // Setup accounts and market
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": balance }),
    )
    .await;
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    // Submit and produce block 1
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 0,
            "orders": [{ "type": "BuyYes", "market_id": 0, "limit_price_nanos": 600_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 1,
            "orders": [{ "type": "BuyNo", "market_id": 0, "limit_price_nanos": 500_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    handle.produce_block().await.unwrap();

    // Submit and produce block 2
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 0,
            "orders": [{ "type": "BuyYes", "market_id": 0, "limit_price_nanos": 600_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": 1,
            "orders": [{ "type": "BuyNo", "market_id": 0, "limit_price_nanos": 500_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    handle.produce_block().await.unwrap();

    // Get all fills
    let (_, body) = get(app.clone(), "/v1/accounts/0/fills").await;
    let all_fills = parse_json(&body);
    let total = all_fills.as_array().unwrap().len();
    assert!(total >= 2, "Expected at least 2 fills across 2 blocks");

    // Paginate: limit=1
    let (_, body) = get(app.clone(), "/v1/accounts/0/fills?limit=1").await;
    let page1 = parse_json(&body);
    assert_eq!(page1.as_array().unwrap().len(), 1);

    // Paginate: offset=1, limit=1
    let (_, body) = get(app, "/v1/accounts/0/fills?offset=1&limit=1").await;
    let page2 = parse_json(&body);
    assert_eq!(page2.as_array().unwrap().len(), 1);

    // Pages should be different fills
    assert_ne!(
        page1.as_array().unwrap()[0]["order_id"],
        page2.as_array().unwrap()[0]["order_id"],
    );
}

// ---------------------------------------------------------------------------
// System endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_endpoint() {
    let (app, _) = test_app(true).await;
    let (status, body) = get(app, "/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(resp["status"].as_str().unwrap(), "ok");
}
