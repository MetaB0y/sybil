//! Route trust-boundary tests. These deliberately assert exact mount tables so
//! write routes cannot drift between public, service, and dev tiers silently.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use matching_sequencer::crypto::{canonical_bridge_withdrawal_bytes, PublicKey};
use matching_sequencer::{AccountId, BridgeWithdrawalRequest};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{json, Value};
use sybil_api::app::{RouteMount, DEV_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, SERVICE_ROUTE_TABLE};
use sybil_api::config::ApiConfig;
use tower::ServiceExt;

use common::{get, test_app_with_config};

const TOKEN: &str = "route-policy-token";

fn exact_public_routes() -> &'static [RouteMount] {
    &[
        RouteMount {
            method: "GET",
            path: "/openapi.json",
        },
        RouteMount {
            method: "GET",
            path: "/metrics",
        },
        RouteMount {
            method: "GET",
            path: "/v1/bots/decisions",
        },
        RouteMount {
            method: "GET",
            path: "/v1/bots/equity-series",
        },
        RouteMount {
            method: "GET",
            path: "/v1/leaderboard",
        },
        RouteMount {
            method: "GET",
            path: "/v1/health",
        },
        RouteMount {
            method: "GET",
            path: "/v1/state-root",
        },
        RouteMount {
            method: "GET",
            path: "/v1/da/{height}/manifest",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/keys",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/keys",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/keys/register",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/keys/revoke",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/profile",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/api-keys",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/api-keys",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/api-keys/revoke",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/private-summary",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/portfolio",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/fills",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/equity",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/events",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/bridge-key",
        },
        RouteMount {
            method: "GET",
            path: "/v1/accounts/{id}/orders",
        },
        RouteMount {
            method: "GET",
            path: "/v1/bridge/status",
        },
        RouteMount {
            method: "GET",
            path: "/v1/bridge/withdrawals/{id}",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/search",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/summary",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/groups",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/prices",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}/resolution",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}/prices/history",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}/prices/candles",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}/open-batch",
        },
        RouteMount {
            method: "GET",
            path: "/v1/activity/overview",
        },
        RouteMount {
            method: "GET",
            path: "/v1/events/{event_id}/traders",
        },
        RouteMount {
            method: "GET",
            path: "/v1/events/{event_id}/raw",
        },
        RouteMount {
            method: "GET",
            path: "/v1/feeds",
        },
        RouteMount {
            method: "POST",
            path: "/v1/orders/signed",
        },
        RouteMount {
            method: "POST",
            path: "/v1/orders/cancel/signed",
        },
        RouteMount {
            method: "GET",
            path: "/v1/blocks",
        },
        RouteMount {
            method: "GET",
            path: "/v1/blocks/latest",
        },
        RouteMount {
            method: "GET",
            path: "/v1/blocks/stream",
        },
        RouteMount {
            method: "GET",
            path: "/v1/blocks/ws",
        },
        RouteMount {
            method: "GET",
            path: "/v1/blocks/{height}",
        },
    ]
}

fn exact_service_routes() -> &'static [RouteMount] {
    &[
        RouteMount {
            method: "POST",
            path: "/v1/orders",
        },
        RouteMount {
            method: "GET",
            path: "/v1/proofs/state/{leaf_key_hex}",
        },
        RouteMount {
            method: "GET",
            path: "/v1/da/{height}/payload",
        },
        RouteMount {
            method: "POST",
            path: "/v1/accounts/{id}/fund",
        },
        RouteMount {
            method: "GET",
            path: "/v1/bridge/accounts/by-key/{key_hex}",
        },
        RouteMount {
            method: "POST",
            path: "/v1/bridge/deposits",
        },
        RouteMount {
            method: "POST",
            path: "/v1/bridge/withdrawals",
        },
        RouteMount {
            method: "POST",
            path: "/v1/bridge/withdrawals/signed",
        },
        RouteMount {
            method: "POST",
            path: "/v1/bridge/withdrawals/l1-events",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets/groups",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets/groups/{group_id}/members",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets/{id}/resolve",
        },
        RouteMount {
            method: "PUT",
            path: "/v1/events/{event_id}/raw",
        },
        RouteMount {
            method: "POST",
            path: "/v1/feeds",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets/prices/reference",
        },
        RouteMount {
            method: "POST",
            path: "/v1/markets/{id}/metadata",
        },
        RouteMount {
            method: "POST",
            path: "/v1/admin/auto-resolutions",
        },
        RouteMount {
            method: "GET",
            path: "/v1/admin/auto-resolutions",
        },
        RouteMount {
            method: "POST",
            path: "/v1/admin/auto-resolutions/{id}/approve",
        },
        RouteMount {
            method: "POST",
            path: "/v1/admin/auto-resolutions/{id}/reject",
        },
    ]
}

fn exact_dev_routes() -> &'static [RouteMount] {
    &[
        RouteMount {
            method: "POST",
            path: "/v1/simulation/pause",
        },
        RouteMount {
            method: "POST",
            path: "/v1/simulation/resume",
        },
        RouteMount {
            method: "GET",
            path: "/v1/orders/pending",
        },
        RouteMount {
            method: "GET",
            path: "/v1/markets/{id}/orderbook",
        },
    ]
}

fn temp_event_dir() -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sybil-route-policy-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("event snapshot test dir exists");
    dir.to_string_lossy().into_owned()
}

async fn prod_app() -> axum::Router {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: TOKEN.to_string(),
        event_snapshot_dir: temp_event_dir(),
        ..ApiConfig::default()
    })
    .await;
    app
}

async fn request_json(
    app: axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let resp = app
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
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

async fn request_empty_with_headers(
    app: axum::Router,
    method: Method,
    uri: &str,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        let Some(name) = name else { continue };
        builder = builder.header(name, value);
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
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

fn service_probe_requests() -> Vec<(Method, &'static str, Value)> {
    vec![
        (
            Method::POST,
            "/v1/orders",
            json!({"account_id": 0, "orders": []}),
        ),
        (
            Method::GET,
            "/v1/proofs/state/616363742f6d697373696e67",
            json!({}),
        ),
        (Method::GET, "/v1/da/1/payload", json!({})),
        (
            Method::POST,
            "/v1/accounts/1/fund",
            json!({"amount_nanos": 1000}),
        ),
        (Method::GET, "/v1/bridge/accounts/by-key/00", json!({})),
        (
            Method::POST,
            "/v1/bridge/deposits",
            json!({"deposit_id": 1}),
        ),
        (
            Method::POST,
            "/v1/bridge/withdrawals",
            json!({"account_id": 1}),
        ),
        (
            Method::POST,
            "/v1/bridge/withdrawals/signed",
            json!({"withdrawal": {"account_id": 1}}),
        ),
        (
            Method::POST,
            "/v1/markets",
            json!({"name": "service market"}),
        ),
        (
            Method::POST,
            "/v1/markets/groups",
            json!({"name": "service group", "market_ids": [0]}),
        ),
        (
            Method::POST,
            "/v1/markets/groups/0/members",
            json!({"market_id": 1}),
        ),
        (
            Method::POST,
            "/v1/markets/0/resolve",
            json!({"payout_nanos": 1_000_000_000u64}),
        ),
        (Method::PUT, "/v1/events/test/raw", json!({"id": "test"})),
        (
            Method::POST,
            "/v1/feeds",
            json!({"pubkey_hex": "00", "name": "bad"}),
        ),
        (
            Method::POST,
            "/v1/markets/prices/reference",
            json!({"prices": {"0": 500_000_000u64}}),
        ),
        (
            Method::POST,
            "/v1/markets/0/metadata",
            json!({"event_id": "event"}),
        ),
        (
            Method::POST,
            "/v1/admin/auto-resolutions",
            json!({
                "market_id": 0,
                "action": "review",
                "payout_nanos": 1_000_000_000u64,
                "confidence": 0.8,
                "reasoning": "probe"
            }),
        ),
        (Method::GET, "/v1/admin/auto-resolutions", json!({})),
        (
            Method::POST,
            "/v1/admin/auto-resolutions/0/approve",
            json!({}),
        ),
        (
            Method::POST,
            "/v1/admin/auto-resolutions/0/reject",
            json!({}),
        ),
    ]
}

fn parse_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("response body is valid JSON")
}

fn hex_bytes(byte: u8, len: usize) -> String {
    hex::encode(vec![byte; len])
}

fn expected_deposit_root(
    account_key_hex: &str,
    deposit_id: u64,
    amount_token_units: u64,
) -> String {
    let mut sybil_account_key = [0u8; 32];
    hex::decode_to_slice(account_key_hex, &mut sybil_account_key).expect("account key hex");
    let leaf = sybil_l1_protocol::DepositLeaf {
        chain_id: 1,
        vault_address: [0x10; 20],
        deposit_id,
        token_address: [0x20; 20],
        sender: [0x30; 20],
        sybil_account_key,
        amount_token_units,
    };
    hex::encode(sybil_l1_protocol::deposit_root_from_prefix(&[leaf]))
}

#[test]
fn route_policy_mount_tables_are_exact() {
    assert_eq!(PUBLIC_ROUTE_TABLE, exact_public_routes());
    assert_eq!(SERVICE_ROUTE_TABLE, exact_service_routes());
    assert_eq!(DEV_ROUTE_TABLE, exact_dev_routes());
}

#[tokio::test]
async fn service_routes_reject_missing_and_wrong_tokens_in_prod() {
    let app = prod_app().await;

    for (method, uri, body) in service_probe_requests() {
        let (status, _) = request_json(app.clone(), method.clone(), uri, None, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");

        let (status, _) = request_json(app.clone(), method, uri, Some("wrong-token"), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }
}

#[tokio::test]
async fn unsigned_orders_are_service_gated_while_signed_orders_remain_public() {
    let app = prod_app().await;
    let unsigned = json!({"account_id": 0, "orders": []});

    let (status, _) = request_json(
        app.clone(),
        Method::POST,
        "/v1/orders",
        None,
        unsigned.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = request_json(
        app.clone(),
        Method::POST,
        "/v1/orders",
        Some(TOKEN),
        unsigned,
    )
    .await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::FORBIDDEN);

    // A malformed signed request reaches public request validation rather
    // than the service gate; real web clients authorize with the signature.
    let (status, _) = request_json(app, Method::POST, "/v1/orders/signed", None, json!({})).await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn service_routes_fail_closed_when_token_is_unset_in_prod() {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: String::new(),
        ..ApiConfig::default()
    })
    .await;

    let (status, _) = request_json(
        app,
        Method::POST,
        "/v1/accounts/1/fund",
        Some("anything"),
        json!({"amount_nanos": 1000}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn onboarding_is_public_and_demo_capped_in_prod() {
    let app = prod_app().await;

    // A fresh browser user creates a demo account and bootstraps its first key
    // with NO service token — this is the passkey onboarding path.
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/accounts",
        None,
        json!({"initial_balance_nanos": 1_000_000_000_000u64}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let key = SigningKey::from_bytes((&[9u8; 32]).into()).expect("fixed signing key");
    let pubkey_hex = hex::encode(PublicKey(*key.verifying_key()).compressed_bytes());
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        &format!("/v1/accounts/{account_id}/keys"),
        None,
        json!({"public_key_hex": pubkey_hex}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Minting above the public demo ceiling is rejected (no unbounded mint).
    let (status, _) = request_json(
        app.clone(),
        Method::POST,
        "/v1/accounts",
        None,
        json!({"initial_balance_nanos": 5_000_000_000_001u64}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn service_routes_succeed_with_token_in_prod() {
    let app = prod_app().await;

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/accounts",
        Some(TOKEN),
        json!({"initial_balance_nanos": 10_000_000u64}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        &format!("/v1/accounts/{account_id}/fund"),
        Some(TOKEN),
        json!({"amount_nanos": 1_000_000u64}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/bridge-key"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let account_key = parse_json(&body)["sybil_account_key_hex"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = request_json(
        app.clone(),
        Method::GET,
        &format!("/v1/bridge/accounts/by-key/{account_key}"),
        Some(TOKEN),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(parse_json(&body)["account_id"], json!(account_id));

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/bridge/deposits",
        Some(TOKEN),
        json!({
            "deposit_id": 1,
            "account_id": account_id,
            "chain_id": 1,
            "vault_address_hex": hex_bytes(0x10, 20),
            "token_address_hex": hex_bytes(0x20, 20),
            "sender_hex": hex_bytes(0x30, 20),
            "sybil_account_key_hex": account_key,
            "amount_token_units": 10_000u64,
            "deposit_root_hex": expected_deposit_root(&account_key, 1, 10_000),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let withdrawal_key = SigningKey::from_bytes((&[8u8; 32]).into()).expect("fixed signing key");
    let withdrawal_pubkey = PublicKey(*withdrawal_key.verifying_key());
    let withdrawal_pubkey_hex = hex::encode(withdrawal_pubkey.compressed_bytes());
    // First-key bootstrap is public onboarding; supplying the service token is
    // harmless (public routes ignore it) and keeps this end-to-end flow intact.
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        &format!("/v1/accounts/{account_id}/keys"),
        Some(TOKEN),
        json!({"public_key_hex": withdrawal_pubkey_hex}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let signed_withdrawal = BridgeWithdrawalRequest {
        account_id: AccountId(account_id),
        chain_id: 1,
        vault_address: [0x10; 20],
        recipient: [0x41; 20],
        token_address: [0x20; 20],
        amount_token_units: 1_000,
        expiry_height: 10,
    };
    let msg = canonical_bridge_withdrawal_bytes(&signed_withdrawal, 1);
    let signature: Signature = withdrawal_key.sign(&msg);
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/bridge/withdrawals/signed",
        Some(TOKEN),
        json!({
            "withdrawal": {
                "account_id": account_id,
                "chain_id": 1,
                "vault_address_hex": hex_bytes(0x10, 20),
                "recipient_hex": hex_bytes(0x41, 20),
                "token_address_hex": hex_bytes(0x20, 20),
                "amount_token_units": 1_000u64,
                "expiry_height": 10u64,
                "nonce": 1u64,
            },
            "signer_pubkey_hex": withdrawal_pubkey_hex,
            "signature_hex": hex::encode(signature.to_bytes()),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/bridge/withdrawals",
        Some(TOKEN),
        json!({
            "account_id": account_id,
            "chain_id": 1,
            "vault_address_hex": hex_bytes(0x10, 20),
            "recipient_hex": hex_bytes(0x40, 20),
            "token_address_hex": hex_bytes(0x20, 20),
            "amount_token_units": 1_000u64,
            "expiry_height": 10u64,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/markets",
        Some(TOKEN),
        json!({"name": "service market"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/orders",
        Some(TOKEN),
        json!({
            "account_id": account_id,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 1u64
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/markets",
        Some(TOKEN),
        json!({"name": "service market late child"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let late_market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/markets/groups",
        Some(TOKEN),
        json!({"name": "service group", "market_ids": [market_id]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let group_id = parse_json(&body)["group_id"].as_u64().unwrap();

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        &format!("/v1/markets/groups/{group_id}/members"),
        Some(TOKEN),
        json!({"market_id": late_market_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app.clone(),
        Method::PUT,
        "/v1/events/service-event/raw",
        Some(TOKEN),
        json!({"id": "service-event"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let key = SigningKey::from_bytes((&[9u8; 32]).into()).expect("fixed signing key");
    let pubkey_hex = hex::encode(PublicKey(*key.verifying_key()).compressed_bytes());
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/feeds",
        Some(TOKEN),
        json!({"pubkey_hex": pubkey_hex, "name": "route_policy_feed"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let mut prices = serde_json::Map::new();
    prices.insert(market_id.to_string(), json!(500_000_000u64));
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        "/v1/markets/prices/reference",
        Some(TOKEN),
        json!({"prices": prices}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        &format!("/v1/markets/{market_id}/metadata"),
        Some(TOKEN),
        json!({"event_id": "service-event"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = request_json(
        app,
        Method::POST,
        &format!("/v1/markets/{market_id}/resolve"),
        Some(TOKEN),
        json!({"payout_nanos": 1_000_000_000u64}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

#[tokio::test]
async fn dev_routes_are_not_mounted_in_prod() {
    let app = prod_app().await;
    for (method, uri) in [
        (Method::POST, "/v1/simulation/pause"),
        (Method::POST, "/v1/simulation/resume"),
        (Method::GET, "/v1/orders/pending"),
        (Method::GET, "/v1/markets/0/orderbook"),
    ] {
        let (status, _, body) =
            request_empty_with_headers(app.clone(), method.clone(), uri, HeaderMap::new()).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {uri}: {}",
            String::from_utf8_lossy(&body)
        );
    }
}

#[tokio::test]
async fn cors_is_permissive_only_in_dev_mode() {
    let (dev_app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        ..ApiConfig::default()
    })
    .await;
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://elsewhere.example".parse().unwrap());
    let (status, headers, _) =
        request_empty_with_headers(dev_app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );

    let (prod_app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: TOKEN.to_string(),
        ..ApiConfig::default()
    })
    .await;
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://elsewhere.example".parse().unwrap());
    let (status, headers, _) =
        request_empty_with_headers(prod_app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

    let (prod_app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: TOKEN.to_string(),
        cors_origins: vec!["https://app.example".to_string()],
        ..ApiConfig::default()
    })
    .await;
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://app.example".parse().unwrap());
    let (status, headers, _) =
        request_empty_with_headers(prod_app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("https://app.example")
    );
}
