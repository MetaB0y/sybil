//! In-process API integration tests using tower::ServiceExt::oneshot().
//!
//! These tests exercise the full Axum router with a real BlockSequencer
//! underneath, without binding to a port.

mod common;

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use http_body_util::BodyExt;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use tower::ServiceExt;

use common::{
    get, post_json, post_json_with_headers, put_json, test_app, test_app_with_config,
    test_app_with_store, test_app_with_store_api_config, test_app_with_store_config,
    test_app_with_store_zero_caps,
};
use matching_engine::{MarketSet, Nanos, Qty};
use matching_sequencer::SequencerConfig;
use matching_sequencer::SequencerHandle;
use matching_sequencer::crypto::{canonical_cancel_bytes, canonical_order_bytes};
use std::time::Duration;
use sybil_api::config::ApiConfig;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const SERVICE_TOKEN: &str = "api-integration-service-token";

fn parse_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("response body is valid JSON")
}

fn to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn hex_bytes(byte: u8, len: usize) -> String {
    hex::encode(vec![byte; len])
}

fn service_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {SERVICE_TOKEN}").parse().unwrap(),
    );
    headers
}

async fn test_service_app_with_store() -> (axum::Router, SequencerHandle) {
    test_app_with_store_api_config(
        ApiConfig {
            dev_mode: false,
            service_token: SERVICE_TOKEN.to_string(),
            ..ApiConfig::default()
        },
        SequencerConfig {
            block_interval: Duration::from_secs(60 * 60),
            ..SequencerConfig::default()
        },
    )
    .await
}

async fn get_with_headers(
    app: axum::Router,
    uri: &str,
    request_headers: HeaderMap,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = axum::http::Request::builder().uri(uri);
    for (name, value) in request_headers {
        let Some(name) = name else { continue };
        builder = builder.header(name, value);
    }
    let req = builder.body(Body::empty()).unwrap();
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

fn parse_hex32(input: &str) -> [u8; 32] {
    let bytes = hex::decode(input.strip_prefix("0x").unwrap_or(input)).expect("valid hex");
    bytes.try_into().expect("32-byte hex field")
}

fn decode_provider_refs(manifest: &Value) -> Vec<Vec<u8>> {
    manifest["provider_refs"]
        .as_array()
        .expect("provider_refs array")
        .iter()
        .map(|provider_ref| {
            let bytes = provider_ref["bytes"]
                .as_str()
                .expect("provider ref bytes string");
            hex::decode(bytes.strip_prefix("0x").unwrap_or(bytes)).expect("provider ref hex")
        })
        .collect()
}

async fn wait_for_da_manifest(app: axum::Router, handle: &SequencerHandle, height: u64) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let lookup = handle.get_da_manifest(height).await.unwrap();
            if lookup.manifest.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("DA manifest was not persisted for height {height}"));

    let path = format!("/v1/da/{height}/manifest");
    let (status, body) = get(app, &path).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected DA manifest status: {status}, body={}",
        String::from_utf8_lossy(&body)
    );
    parse_json(&body)
}

async fn ensure_genesis_hash(handle: &SequencerHandle) -> [u8; 32] {
    if let Some(genesis_hash) = handle.get_genesis_hash().await.unwrap() {
        return genesis_hash;
    }
    handle.produce_block().await.unwrap();
    handle
        .get_genesis_hash()
        .await
        .unwrap()
        .expect("genesis hash after first committed block")
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

fn new_signing_key() -> SigningKey {
    SigningKey::from_bytes((&[7u8; 32]).into()).expect("fixed signing key")
}

fn account_state_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(13);
    key.extend_from_slice(b"acct/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

#[tokio::test]
async fn http_order_rate_limit_returns_429_before_handler_work() {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        http_order_global_rps: 1,
        http_order_global_burst: 1,
        http_order_client_rps: 1,
        http_order_client_burst: 1,
        ..ApiConfig::default()
    })
    .await;

    let payload = json!({
        "account_id": 999,
        "orders": [{
            "type": "BuyYes",
            "market_id": 999,
            "limit_price_nanos": 500_000_000u64,
            "quantity": 1
        }]
    });

    let (status, _) = post_json(app.clone(), "/v1/orders", payload.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, headers, body) = post_json_with_headers(app, "/v1/orders", payload).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "body: {}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(
        headers
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );
}

#[tokio::test]
async fn public_da_manifest_reads_are_rate_limited_before_store_work() {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        http_da_global_rps: 1,
        http_da_global_burst: 1,
        http_da_client_rps: 1,
        http_da_client_burst: 1,
        ..ApiConfig::default()
    })
    .await;

    let (status, _) = get(app.clone(), "/v1/da/1/manifest").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = get(app, "/v1/da/1/manifest").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn public_sse_connection_cap_lives_with_the_response_body() {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        http_public_stream_max_connections: 1,
        ..ApiConfig::default()
    })
    .await;

    let request = || {
        axum::http::Request::builder()
            .uri("/v1/blocks/stream")
            .body(Body::empty())
            .unwrap()
    };

    let first = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        second
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );

    drop(first);
    let third = app.oneshot(request()).await.unwrap();
    assert_eq!(third.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_key_label_limit_is_rejected_at_http_admission() {
    let (app, _) = test_app(true).await;
    let (status, body) = post_json(
        app,
        "/v1/accounts/0/api-keys",
        json!({
            "label": "x".repeat(matching_sequencer::MAX_API_KEY_LABEL_BYTES + 1),
            "nonce": 1
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("API-key label exceeds"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

fn signed_buy_yes_payload(
    _account_id: u64,
    market_id: u32,
    limit_price_nanos: u64,
    quantity: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    signed_order_payload(
        market_id,
        &[1, 0],
        limit_price_nanos,
        quantity,
        nonce,
        genesis_hash,
        key,
    )
}

fn signed_sell_yes_payload(
    market_id: u32,
    limit_price_nanos: u64,
    quantity: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    signed_order_payload(
        market_id,
        &[-1, 0],
        limit_price_nanos,
        quantity,
        nonce,
        genesis_hash,
        key,
    )
}

fn signed_order_payload(
    market_id: u32,
    payoffs: &[i8],
    limit_price_nanos: u64,
    quantity: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    let mut markets = MarketSet::new();
    let mid = markets.add_binary("Test");
    assert_eq!(mid.0, market_id);
    let mut order = matching_engine::Order::new(0);
    order.markets[0] = mid;
    order.num_markets = 1;
    order.num_states = 2;
    order.limit_price = Nanos(limit_price_nanos);
    order.max_fill = Qty(quantity);
    for (idx, payoff) in payoffs.iter().enumerate() {
        order.payoffs[idx] = *payoff;
    }
    let signature: Signature = key.sign(&canonical_order_bytes(&order, nonce, genesis_hash));
    json!({
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "order": {
            "market_ids": [market_id],
            "payoffs": payoffs,
            "limit_price_nanos": limit_price_nanos,
            "max_fill": quantity
        },
        "nonce": nonce,
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

fn signed_cancel_payload(
    account_id: u64,
    order_id: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    let signature: Signature = key.sign(&canonical_cancel_bytes(
        matching_sequencer::AccountId(account_id),
        order_id,
        nonce,
        genesis_hash,
    ));
    json!({
        "account_id": account_id,
        "order_id": order_id,
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "nonce": nonce,
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

// ---------------------------------------------------------------------------
// A. Dev mode gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_account_is_public_onboarding_without_dev_mode() {
    // Self-service onboarding is PUBLIC (SYB-245): a fresh browser/passkey user
    // creates a demo-capped account with no service token, even in non-dev mode.
    // (It used to 401 here, which was the production onboarding regression.)
    let (app, _) = test_app(false).await;
    let key = new_signing_key();
    let (status, _) = post_json(
        app,
        "/v1/accounts",
        json!({
            "initial_balance_nanos": 100,
            "initial_key": {
                "public_key_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes())
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn create_market_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) = post_json(app, "/v1/markets", json!({ "name": "Test" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn fund_account_forbidden_without_dev_mode() {
    let (app, _) = test_app(false).await;
    let (status, _) = post_json(app, "/v1/accounts/0/fund", json!({ "amount_nanos": 100 })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
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
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn stale_trade_page_is_not_mounted() {
    let (app, _) = test_app(true).await;
    let (status, _) = get(app, "/trade").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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

#[tokio::test]
async fn state_proof_returns_inclusion_for_committed_account_leaf() {
    let (app, handle) = test_service_app_with_store().await;
    let account = handle.create_account(1_000_000).await.unwrap();
    let block = handle.produce_block().await.unwrap();
    let leaf_key = account_state_leaf_key(account.id.0);

    let (status, _, body) = get_with_headers(
        app,
        &format!("/v1/proofs/state/{}", hex::encode(&leaf_key)),
        service_headers(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let proof = parse_json(&body);
    assert_eq!(proof["block_height"], json!(block.canonical.header.height));
    assert_eq!(
        proof["state_root"],
        json!(hex::encode(block.canonical.header.state_root))
    );
    assert_eq!(proof["leaf_key_hex"], json!(hex::encode(leaf_key)));
    assert_eq!(proof["proof_kind"], json!("inclusion"));
    assert_eq!(proof["verified"], json!(true));
    assert!(proof["leaf_value_hex"].as_str().is_some());
    assert!(
        proof["inclusion_proof"]["operation"]["location"]
            .as_u64()
            .is_some()
    );
}

#[tokio::test]
async fn state_proof_returns_exclusion_for_missing_leaf() {
    let (app, handle) = test_service_app_with_store().await;
    handle.create_account(1_000_000).await.unwrap();
    let block = handle.produce_block().await.unwrap();
    let leaf_key = b"acct/missing".to_vec();

    let (status, _, body) = get_with_headers(
        app,
        &format!("/v1/proofs/state/{}", hex::encode(&leaf_key)),
        service_headers(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let proof = parse_json(&body);
    assert_eq!(proof["block_height"], json!(block.canonical.header.height));
    assert_eq!(
        proof["state_root"],
        json!(hex::encode(block.canonical.header.state_root))
    );
    assert_eq!(proof["leaf_key_ascii"], json!("acct/missing"));
    assert_eq!(proof["proof_kind"], json!("exclusion"));
    assert_eq!(proof["verified"], json!(true));
    assert!(proof.get("leaf_value_hex").is_none());
    assert!(
        proof["exclusion_proof"]["operation"]["location"]
            .as_u64()
            .is_some()
    );
}

#[tokio::test]
async fn state_proof_rejects_invalid_leaf_key_hex() {
    let (app, _) = test_service_app_with_store().await;
    let (status, _, _) = get_with_headers(app, "/v1/proofs/state/not-hex", service_headers()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn da_manifest_404s_when_no_artifact_is_retained() {
    let (app, _) = test_app(true).await;
    let (status, body) = get(app, "/v1/da/1/manifest").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let error = parse_json(&body);
    assert_eq!(error["code"], json!("NOT_FOUND"));
    assert!(
        error["error"]
            .as_str()
            .expect("error string")
            .contains("DA artifact not retained for height 1")
    );
}

#[tokio::test]
async fn da_manifest_and_payload_verify_binding_chain() {
    let (app, handle) = test_app_with_store(true).await;
    handle.produce_block().await.unwrap();
    let block = handle.produce_block().await.unwrap();
    let height = block.canonical.header.height;

    let manifest = wait_for_da_manifest(app.clone(), &handle, height).await;
    let (status, headers, payload) =
        get_with_headers(app, &format!("/v1/da/{height}/payload"), service_headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/octet-stream")
    );
    let expected_content_length = payload.len().to_string();
    assert_eq!(
        headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()),
        Some(expected_content_length.as_str())
    );

    assert_eq!(manifest["height"], json!(height));
    assert_eq!(
        manifest["state_root"],
        json!(hex::encode(block.canonical.header.state_root))
    );
    assert_eq!(
        manifest["block_hash"],
        json!(hex::encode(sybil_zk::hash_header(
            &block.canonical.header.to_witness_header()
        )))
    );
    assert_eq!(
        manifest["payload_encoding"],
        json!("sybil-canonical-witness-v3")
    );

    let payload_root = sybil_zk::da_witness_payload_root(&payload);
    assert_eq!(
        parse_hex32(manifest["payload_root"].as_str().unwrap()),
        payload_root
    );
    assert_eq!(manifest["payload_len"], json!(payload.len() as u64));

    let mut witness_hasher = blake3::Hasher::new();
    witness_hasher.update(sybil_zk::WITNESS_ROOT_DOMAIN);
    witness_hasher.update(&payload);
    let witness_root = *witness_hasher.finalize().as_bytes();
    assert_eq!(
        parse_hex32(manifest["witness_root"].as_str().unwrap()),
        witness_root
    );

    let provider_refs = decode_provider_refs(&manifest);
    assert_eq!(provider_refs.len(), 1);
    let provider_refs_hash = sybil_zk::da_provider_refs_hash(&provider_refs);
    assert_eq!(
        parse_hex32(manifest["provider_refs_hash"].as_str().unwrap()),
        provider_refs_hash
    );

    let da_commitment = sybil_zk::da_commitment_from_parts(
        height,
        parse_hex32(manifest["state_root"].as_str().unwrap()),
        witness_root,
        payload_root,
        payload.len() as u64,
        provider_refs_hash,
    );
    assert_eq!(
        parse_hex32(manifest["da_commitment"].as_str().unwrap()),
        da_commitment
    );

    let provider_ref = &manifest["provider_refs"][0];
    assert_eq!(provider_ref["kind"], json!("file"));
    assert_eq!(provider_ref["encoding"], json!("sybil-da-file-ref-v1"));
    assert_eq!(
        provider_ref["payload_root"],
        json!(manifest["payload_root"].as_str().unwrap())
    );
    assert_eq!(provider_ref["payload_len"], json!(payload.len() as u64));
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
async fn bridge_commitment_is_public_but_individual_rows_stay_private() {
    let (app, handle) = test_app(true).await;

    let (status, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/bridge-key"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let account_key = parse_json(&body)["sybil_account_key_hex"]
        .as_str()
        .unwrap()
        .to_string();

    let deposit_root = expected_deposit_root(&account_key, 1, 10_000);
    let (status, body) = post_json(
        app.clone(),
        "/v1/bridge/deposits",
        json!({
            "deposit_id": 1,
            "account_id": account_id,
            "chain_id": 1,
            "vault_address_hex": hex_bytes(0x10, 20),
            "token_address_hex": hex_bytes(0x20, 20),
            "sender_hex": hex_bytes(0x30, 20),
            "sybil_account_key_hex": account_key,
            "amount_token_units": 10_000u64,
            "deposit_root_hex": deposit_root,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(parse_json(&body)["balance_nanos"], json!(10_000_000i64));

    let (status, body) = post_json(
        app.clone(),
        "/v1/bridge/withdrawals",
        json!({
            "account_id": account_id,
            "chain_id": 1,
            "vault_address_hex": hex_bytes(0x10, 20),
            "recipient_hex": hex_bytes(0x40, 20),
            "token_address_hex": hex_bytes(0x20, 20),
            "amount_token_units": 4_000u64,
            "expiry_height": 10u64,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let withdrawal = parse_json(&body);
    assert_eq!(withdrawal["withdrawal_id"], json!(1));
    assert_eq!(withdrawal["amount_nanos"], json!(4_000_000u64));
    assert!(withdrawal["withdrawal_leaf_digest_hex"].as_str().is_some());
    let nullifier_hex = withdrawal["nullifier_hex"].as_str().unwrap().to_string();

    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/withdrawals"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let active = parse_json(&body);
    assert_eq!(active.as_array().unwrap().len(), 1);
    assert_eq!(active[0]["l1_status"], json!("not_requested"));

    let (status, body) = post_json(
        app.clone(),
        "/v1/bridge/withdrawals/l1-events",
        json!({
            "nullifier_hex": nullifier_hex,
            "status": "finalized",
            "event_at_unix": 1_700_000_000u64,
            "l1_block_height": 1u64,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/withdrawals"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let active = parse_json(&body);
    assert_eq!(active.as_array().unwrap().len(), 1);
    assert_eq!(active[0]["l1_status"], json!("finalized"));

    handle.produce_block().await.unwrap();

    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/withdrawals"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body).as_array().unwrap().is_empty());

    let (status, body) = get(app, "/v1/blocks/latest").await;
    assert_eq!(status, StatusCode::OK);
    let block = parse_json(&body);
    assert_eq!(block["bridge"]["deposit_count"], json!(1));
    assert!(block["bridge"].get("consumed_deposits").is_none());
    assert!(block["bridge"].get("withdrawal_leaves").is_none());
    for forbidden in [
        "fills",
        "rejections",
        "system_events",
        "derived_view_sidecar",
    ] {
        assert!(
            block.get(forbidden).is_none(),
            "public block leaked {forbidden}"
        );
    }
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
    assert_eq!(
        resp["description"].as_str().unwrap(),
        "Whether it rains tomorrow"
    );
    assert_eq!(resp["category"].as_str().unwrap(), "weather");
    assert_eq!(resp["status"].as_str().unwrap(), "active");
}

#[tokio::test]
async fn resolved_market_rejects_new_orders() {
    let (app, _) = test_app(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/markets",
        json!({ "name": "Will it resolve?" }),
    )
    .await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/markets/{market_id}/resolve"),
        json!({ "payout_nanos": 1_000_000_000u64 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(
        app,
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 600_000_000u64,
                "quantity": 2
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let resp = parse_json(&body);
    assert!(
        resp["error"]
            .as_str()
            .unwrap()
            .contains("Invalid market state")
    );
}

#[tokio::test]
async fn order_visible_immediately_after_submit() {
    let (app, _) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Fast admit?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, body) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "submit failed: {}",
        String::from_utf8_lossy(&body)
    );
    let submit_response = parse_json(&body);
    assert_eq!(submit_response["accepted"], json!(true));
    let submitted_order_ids = submit_response["order_ids"]
        .as_array()
        .expect("submit response order_ids");
    assert_eq!(submitted_order_ids.len(), 1);
    let submitted_order_id = submitted_order_ids[0]
        .as_u64()
        .expect("numeric submitted order id");

    // No block has been produced — with the mempool-free admit path the order
    // must already be visible on the resting book.
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{account_id}/orders")).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    let pending = pending.as_array().unwrap();
    assert_eq!(
        pending.len(),
        1,
        "expected order visible without waiting for a block, got {pending:?}"
    );
    assert_eq!(pending[0]["account_id"].as_u64().unwrap(), account_id);
    assert_eq!(pending[0]["order_id"].as_u64().unwrap(), submitted_order_id);
    assert_eq!(pending[0]["market_id"].as_u64().unwrap(), market_id);
    assert_eq!(pending[0]["side"].as_str().unwrap(), "BuyYes");
    assert_eq!(pending[0]["remaining_quantity"].as_u64().unwrap(), 10);

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}")).await;
    assert_eq!(status, StatusCode::OK);
    let account = parse_json(&body);
    let total = account["balance_nanos"].as_i64().unwrap();
    let reserved = account["reserved_balance_nanos"].as_i64().unwrap();
    let available = account["available_balance_nanos"].as_i64().unwrap();
    assert!(reserved > 0, "resting buy must reserve balance");
    assert_eq!(available, total - reserved);
}

#[tokio::test]
async fn multi_order_submit_returns_ids_preserved_when_orders_rest() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Bundle IDs" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, body) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 400_000_000u64,
                    "quantity": 10
                },
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 500_000_000u64,
                    "quantity": 10
                }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response = parse_json(&body);
    let order_ids: Vec<u64> = response["order_ids"]
        .as_array()
        .expect("submit response order_ids")
        .iter()
        .map(|value| value.as_u64().expect("numeric submitted order id"))
        .collect();
    assert_eq!(order_ids.len(), 2);
    assert_ne!(order_ids[0], order_ids[1]);

    handle.produce_block().await.unwrap();
    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/orders")).await;
    assert_eq!(status, StatusCode::OK);
    let resting_ids: std::collections::HashSet<u64> = parse_json(&body)
        .as_array()
        .unwrap()
        .iter()
        .map(|order| order["order_id"].as_u64().unwrap())
        .collect();
    assert!(
        order_ids
            .iter()
            .all(|order_id| resting_ids.contains(order_id))
    );
}

#[tokio::test]
async fn ioc_order_is_removed_after_one_batch() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "IOC" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let (status, body) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "time_in_force": "IOC",
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "submit failed: {}",
        String::from_utf8_lossy(&body)
    );

    handle.produce_block().await.unwrap();
    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/orders")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body).as_array().unwrap().is_empty());
}

#[tokio::test]
async fn gtd_requires_expires_at_block() {
    let (app, _) = test_app(true).await;

    post_json(app.clone(), "/v1/markets", json!({ "name": "GTD" })).await;
    post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;

    let (status, body) = post_json(
        app,
        "/v1/orders",
        json!({
            "account_id": 0,
            "time_in_force": "GTD",
            "orders": [{
                "type": "BuyYes",
                "market_id": 0,
                "limit_price_nanos": 500_000_000u64,
                "quantity": 10
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        parse_json(&body)["error"]
            .as_str()
            .unwrap()
            .contains("GTD orders require expires_at_block")
    );
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

#[tokio::test]
async fn list_markets_reports_traded_volume() {
    let (app, handle) = test_app(true).await;

    let balance = 100_000_000_000u64;
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

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Volume test" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

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

    let block = handle.produce_block().await.unwrap();
    assert!(!block.canonical.fills.is_empty());

    let (status, body) = get(app.clone(), "/v1/markets").await;
    assert_eq!(status, StatusCode::OK);
    let markets = parse_json(&body).as_array().unwrap().clone();
    let market = markets
        .iter()
        .find(|market| market["market_id"].as_u64().unwrap() == market_id)
        .expect("market should be returned");
    assert!(
        market["volume_nanos"].as_u64().unwrap() > 0,
        "list endpoint should expose traded volume"
    );

    let (status, body) = get(app, &format!("/v1/markets/{market_id}")).await;
    assert_eq!(status, StatusCode::OK);
    let market = parse_json(&body);
    assert!(
        market["volume_nanos"].as_u64().unwrap() > 0,
        "detail endpoint should expose traded volume"
    );
}

#[tokio::test]
async fn market_price_history_is_projected_beyond_recent_cache() {
    let (app, handle) = test_app_with_store_config(
        true,
        SequencerConfig {
            max_recent_price_points_per_market: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        },
    )
    .await;

    let balance = 100_000_000_000u64;
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

    let (_, body) = post_json(
        app.clone(),
        "/v1/markets",
        json!({ "name": "Price history" }),
    )
    .await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let mut traded_heights = Vec::new();
    for (yes_price, no_price) in [(600_000_000u64, 500_000_000u64), (700_000_000, 400_000_000)] {
        let (status, _) = post_json(
            app.clone(),
            "/v1/orders",
            json!({
                "account_id": acct_a,
                "orders": [{
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": yes_price,
                    "quantity": 10
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = post_json(
            app.clone(),
            "/v1/orders",
            json!({
                "account_id": acct_b,
                "orders": [{
                    "type": "BuyNo",
                    "market_id": market_id,
                    "limit_price_nanos": no_price,
                    "quantity": 10
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let block = handle.produce_block().await.unwrap();
        assert!(
            !block.canonical.fills.is_empty(),
            "expected fills from crossing orders"
        );
        traded_heights.push(block.canonical.header.height);
    }

    let (status, body) = get(
        app.clone(),
        &format!("/v1/markets/{market_id}/prices/history"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response = parse_json(&body);
    assert_eq!(response["market_id"].as_u64().unwrap(), market_id);
    let points = response["points"].as_array().unwrap();
    assert_eq!(
        points.len(),
        2,
        "history service should return points older than the one-point recent cache: {response}"
    );
    assert!(
        response.get("next_before_height").is_none(),
        "full page should not advertise another page: {response}"
    );
    assert_eq!(points[0]["height"].as_u64().unwrap(), traded_heights[0]);
    assert_eq!(points[1]["height"].as_u64().unwrap(), traded_heights[1]);
    assert!(
        points
            .iter()
            .all(|point| point["volume_nanos"].as_u64().unwrap() > 0)
    );

    let (status, body) = get(
        app.clone(),
        &format!("/v1/markets/{market_id}/prices/candles?resolution=1m"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let candles_response = parse_json(&body);
    assert_eq!(candles_response["market_id"].as_u64().unwrap(), market_id);
    assert_eq!(candles_response["resolution_secs"].as_u64().unwrap(), 60);
    let candles = candles_response["candles"].as_array().unwrap();
    assert!(
        !candles.is_empty(),
        "expected candle rows: {candles_response}"
    );
    let point_count: u64 = candles
        .iter()
        .map(|candle| candle["point_count"].as_u64().unwrap())
        .sum();
    assert_eq!(point_count, 2);
    assert!(
        candles
            .iter()
            .all(|candle| candle["volume_nanos"].as_u64().unwrap() > 0)
    );

    let (status, body) = get(
        app.clone(),
        &format!("/v1/markets/{market_id}/prices/history?limit=1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let limited = parse_json(&body);
    let limited_points = limited["points"].as_array().unwrap();
    assert_eq!(limited_points.len(), 1);
    assert_eq!(
        limited_points[0]["height"].as_u64().unwrap(),
        traded_heights[1]
    );
    assert_eq!(
        limited["next_before_height"].as_u64().unwrap(),
        traded_heights[1]
    );

    let (status, body) = get(
        app,
        &format!(
            "/v1/markets/{market_id}/prices/history?limit=1&before_height={}",
            traded_heights[1]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let older = parse_json(&body);
    let older_points = older["points"].as_array().unwrap();
    assert_eq!(older_points.len(), 1);
    assert_eq!(
        older_points[0]["height"].as_u64().unwrap(),
        traded_heights[0]
    );
    assert!(
        older.get("next_before_height").is_none(),
        "oldest page should not advertise another page: {older}"
    );
}

#[tokio::test]
async fn history_service_candles_are_independent_of_sequencer_recent_cache() {
    let (app, handle) = test_app_with_store_config(
        true,
        SequencerConfig {
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        },
    )
    .await;

    let balance = 100_000_000_000u64;
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
    let (_, body) = post_json(
        app.clone(),
        "/v1/markets",
        json!({ "name": "Candle retention" }),
    )
    .await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    for (index, (yes_price, no_price)) in [
        (600_000_000u64, 500_000_000u64),
        (700_000_000u64, 400_000_000u64),
    ]
    .into_iter()
    .enumerate()
    {
        let (status, _) = post_json(
            app.clone(),
            "/v1/orders",
            json!({
                "account_id": acct_a,
                "orders": [{
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": yes_price,
                    "quantity": 10
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = post_json(
            app.clone(),
            "/v1/orders",
            json!({
                "account_id": acct_b,
                "orders": [{
                    "type": "BuyNo",
                    "market_id": market_id,
                    "limit_price_nanos": no_price,
                    "quantity": 10
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let block = handle.produce_block().await.unwrap();
        assert!(!block.canonical.fills.is_empty());

        if index == 0 {
            tokio::time::sleep(Duration::from_millis(1_200)).await;
        }
    }

    let (status, body) = get(
        app.clone(),
        &format!("/v1/markets/{market_id}/prices/candles?resolution=1&limit=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let retained = parse_json(&body);
    let retained_candles = retained["candles"].as_array().unwrap();
    assert_eq!(
        retained_candles.len(),
        2,
        "the history service owns long-lived candles independently of the sequencer's one-second legacy cache setting: {retained}"
    );
    assert!(retained["retention_min_bucket_ms"].is_null());
    assert_eq!(retained["history_complete_from_height"].as_u64(), Some(1));

    let latest_bucket = retained_candles[1]["bucket_start_ms"].as_u64().unwrap();
    let future_ms = latest_bucket + 2_000;
    let (status, body) = get(
        app,
        &format!(
            "/v1/markets/{market_id}/prices/candles?resolution=1&from_ms={future_ms}&to_ms={future_ms}&limit=10"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let no_data_range = parse_json(&body);
    assert!(no_data_range["candles"].as_array().unwrap().is_empty());
    assert!(no_data_range["retention_min_bucket_ms"].is_null());
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

#[tokio::test]
async fn signed_cancel_removes_pending_order() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let key = new_signing_key();
    let public_key_hex = to_hex(key.verifying_key().to_sec1_point(true).as_bytes());
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{}/keys", account_id),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let genesis_hash = ensure_genesis_hash(&handle).await;
    let order_payload =
        signed_buy_yes_payload(account_id, 0, 500_000_000, 3, 1, genesis_hash, &key);
    let (status, _) = post_json(app.clone(), "/v1/orders/signed", order_payload).await;
    assert_eq!(status, StatusCode::OK);

    handle.produce_block().await.unwrap();

    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}/orders", account_id)).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    let order_id = pending.as_array().unwrap()[0]["order_id"].as_u64().unwrap();

    let cancel_payload = signed_cancel_payload(account_id, order_id, 2, genesis_hash, &key);
    let (status, body) = post_json(app.clone(), "/v1/orders/cancel/signed", cancel_payload).await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body)["cancelled"].as_bool().unwrap());

    let (status, body) = get(app, &format!("/v1/accounts/{}/orders", account_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body).as_array().unwrap().is_empty());
}

#[tokio::test]
async fn signed_order_replay_returns_409_and_cancel_replay_returns_404() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let key = new_signing_key();
    let public_key_hex = to_hex(key.verifying_key().to_sec1_point(true).as_bytes());
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{}/keys", account_id),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let genesis_hash = ensure_genesis_hash(&handle).await;
    let order_payload =
        signed_buy_yes_payload(account_id, 0, 500_000_000, 3, 1, genesis_hash, &key);
    let (status, _) = post_json(app.clone(), "/v1/orders/signed", order_payload.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post_json(app.clone(), "/v1/orders/signed", order_payload).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        parse_json(&body)["code"].as_str(),
        Some("REPLAY_NONCE_STALE")
    );

    handle.produce_block().await.unwrap();
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}/orders", account_id)).await;
    assert_eq!(status, StatusCode::OK);
    let order_id = parse_json(&body).as_array().unwrap()[0]["order_id"]
        .as_u64()
        .unwrap();

    let cancel_payload = signed_cancel_payload(account_id, order_id, 2, genesis_hash, &key);
    let (status, _) = post_json(
        app.clone(),
        "/v1/orders/cancel/signed",
        cancel_payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post_json(app, "/v1/orders/cancel/signed", cancel_payload).await;
    // Cancel validation runs before replay-nonce validation, so after the first
    // cancel removes the order, the replay is rejected as not found without
    // consuming or otherwise consulting the stale nonce.
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(parse_json(&body)["code"].as_str(), Some("NOT_FOUND"));
}

#[tokio::test]
async fn signed_cancel_rejects_wrong_account_claim() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let key = new_signing_key();
    let public_key_hex = to_hex(key.verifying_key().to_sec1_point(true).as_bytes());
    post_json(
        app.clone(),
        &format!("/v1/accounts/{}/keys", account_id),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;

    let genesis_hash = ensure_genesis_hash(&handle).await;
    let order_payload =
        signed_buy_yes_payload(account_id, 0, 500_000_000, 3, 1, genesis_hash, &key);
    post_json(app.clone(), "/v1/orders/signed", order_payload).await;
    handle.produce_block().await.unwrap();

    let (_, body) = get(app.clone(), &format!("/v1/accounts/{}/orders", account_id)).await;
    let pending = parse_json(&body);
    let order_id = pending.as_array().unwrap()[0]["order_id"].as_u64().unwrap();

    let cancel_payload = signed_cancel_payload(account_id + 1, order_id, 2, genesis_hash, &key);
    let (status, _) = post_json(app, "/v1/orders/cancel/signed", cancel_payload).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn signed_sell_order_creates_pending_resting_order() {
    let (app, handle) = test_app(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let seller = parse_json(&body)["account_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let buyer = parse_json(&body)["account_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let key = new_signing_key();
    let public_key_hex = to_hex(key.verifying_key().to_sec1_point(true).as_bytes());
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{}/keys", seller),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": seller,
            "orders": [{ "type": "BuyYes", "market_id": 0, "limit_price_nanos": 600_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": buyer,
            "orders": [{ "type": "BuyNo", "market_id": 0, "limit_price_nanos": 500_000_000u64, "quantity": 3 }]
        }),
    )
    .await;
    handle.produce_block().await.unwrap();

    let genesis_hash = ensure_genesis_hash(&handle).await;
    let payload = signed_sell_yes_payload(0, 550_000_000, 2, 1, genesis_hash, &key);
    let (status, _) = post_json(app.clone(), "/v1/orders/signed", payload).await;
    assert_eq!(status, StatusCode::OK);
    handle.produce_block().await.unwrap();

    let (status, body) = get(app, &format!("/v1/accounts/{}/orders", seller)).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(
        pending.as_array().unwrap()[0]["side"].as_str().unwrap(),
        "SellYes"
    );
}

// ---------------------------------------------------------------------------
// E. End-to-end trade lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_trade_lifecycle() {
    let (app, handle) = test_app_with_store(true).await;

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
    assert!(
        !block.canonical.fills.is_empty(),
        "Expected fills from matching orders"
    );

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
        !fills["fills"].as_array().unwrap().is_empty(),
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
        .filter(|p| p["market_id"].as_u64().unwrap() == market_id)
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
    let (app, handle) = test_app_with_store(true).await;

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
    let total = all_fills["fills"].as_array().unwrap().len();
    assert!(total >= 2, "Expected at least 2 fills across 2 blocks");

    // Paginate: limit=1
    let (_, body) = get(app.clone(), "/v1/accounts/0/fills?limit=1").await;
    let page1 = parse_json(&body);
    assert_eq!(page1["fills"].as_array().unwrap().len(), 1);
    assert!(page1["fills"][0]["cursor"].as_str().is_some());

    let (_, body) = get(app.clone(), "/v1/accounts/0/fills?limit=0").await;
    assert!(parse_json(&body)["fills"].as_array().unwrap().is_empty());

    // Paginate: offset=1, limit=1
    let (_, body) = get(app.clone(), "/v1/accounts/0/fills?offset=1&limit=1").await;
    let page2 = parse_json(&body);
    assert_eq!(page2["fills"].as_array().unwrap().len(), 1);

    // Pages should be different fills
    assert_ne!(page1["fills"][0]["order_id"], page2["fills"][0]["order_id"],);

    // Cursor pagination: after=0.0 returns oldest-first, then strictly after
    // the returned cursor advances without offset-from-newest shifting.
    let (_, body) = get(app.clone(), "/v1/accounts/0/fills?after=0.0&limit=1").await;
    let first_forward = parse_json(&body);
    assert_eq!(first_forward["fills"].as_array().unwrap().len(), 1);
    assert_eq!(first_forward["cursor_gap"], false);
    assert!(first_forward["next_after"].as_str().is_some());
    let cursor = first_forward["fills"][0]["cursor"].as_str().unwrap();
    let (_, body) = get(
        app.clone(),
        &format!("/v1/accounts/0/fills?after={cursor}&limit=10"),
    )
    .await;
    let rest_forward = parse_json(&body);
    assert!(
        !rest_forward["fills"].as_array().unwrap().is_empty(),
        "expected at least one fill after first cursor"
    );
    assert!(rest_forward["next_after"].is_null());
    assert!(
        rest_forward["fills"]
            .as_array()
            .unwrap()
            .iter()
            .all(|fill| fill["cursor"].as_str().unwrap() != cursor)
    );

    let (status, _) = get(app, "/v1/accounts/0/fills?after=not-a-cursor").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// System endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_endpoint_is_an_atomic_chain_snapshot_and_fails_closed() {
    let (app, handle) = test_app(true).await;

    let (status, body) = get(app.clone(), "/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(resp["status"].as_str(), Some("ok"));
    assert!(resp["height"].is_null());
    assert!(resp["genesis_hash"].is_null());

    let produced = handle.produce_block().await.unwrap();
    let (status, body) = get(app.clone(), "/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    let resp = parse_json(&body);
    assert_eq!(resp["status"].as_str().unwrap(), "ok");
    assert_eq!(resp["height"].as_u64(), Some(1));
    let expected_genesis = hex::encode(matching_sequencer::block::hash_header(
        &produced.canonical.header,
    ));
    assert_eq!(
        resp["genesis_hash"].as_str(),
        Some(expected_genesis.as_str())
    );

    assert!(handle.stop_and_wait(Duration::from_secs(5)).await);
    let (status, body) = get(app, "/v1/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let resp = parse_json(&body);
    assert_eq!(resp["status"].as_str(), Some("unhealthy"));
    assert!(resp["height"].is_null());
    assert!(resp["genesis_hash"].is_null());
}

#[tokio::test]
async fn recent_blocks_returns_newest_first() {
    let (app, handle) = test_app(true).await;

    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();
    let b2 = handle.produce_block().await.unwrap();
    assert!(
        b2.canonical.header.height > b1.canonical.header.height
            && b1.canonical.header.height > b0.canonical.header.height
    );

    // newest-first, clamped to the requested limit
    let (status, body) = get(app.clone(), "/v1/blocks?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    let arr = parse_json(&body);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2, "got {arr:?}");
    assert_eq!(
        arr[0]["height"].as_u64().unwrap(),
        b2.canonical.header.height
    );
    assert_eq!(
        arr[1]["height"].as_u64().unwrap(),
        b1.canonical.header.height
    );

    // asking for more than exist returns all produced
    let (status, body) = get(app.clone(), "/v1/blocks?limit=1000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse_json(&body).as_array().unwrap().len(), 3);

    // limit=0 → empty
    let (status, body) = get(app, "/v1/blocks?limit=0").await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body).as_array().unwrap().is_empty());
}

#[tokio::test]
async fn blocks_endpoint_pages_canonical_archive_beyond_recent_cache() {
    let (app, handle) = test_app_with_store_config(
        true,
        SequencerConfig {
            recent_block_cache_capacity: 1,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        },
    )
    .await;

    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();
    let b2 = handle.produce_block().await.unwrap();

    let (status, body) = get(app.clone(), "/v1/blocks?limit=3").await;
    assert_eq!(status, StatusCode::OK);
    let arr = parse_json(&body);
    let arr = arr.as_array().unwrap();
    assert_eq!(
        arr.iter()
            .map(|block| block["height"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![
            b2.canonical.header.height,
            b1.canonical.header.height,
            b0.canonical.header.height
        ],
        "canonical archive should include blocks evicted from the one-block recent cache"
    );

    let (status, body) = get(
        app.clone(),
        &format!(
            "/v1/blocks?limit=1&before_height={}",
            b2.canonical.header.height
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page = parse_json(&body);
    let page = page.as_array().unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(
        page[0]["height"].as_u64().unwrap(),
        b1.canonical.header.height
    );

    let (status, body) = get(
        app,
        &format!(
            "/v1/blocks?limit=10&before_height={}",
            b0.canonical.header.height
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page = parse_json(&body);
    let page = page.as_array().unwrap();
    assert_eq!(page.len(), 1, "the persistent replay baseline is retained");
    let baseline_height = b0
        .canonical
        .header
        .height
        .checked_sub(1)
        .expect("explicit block follows the replay baseline");
    assert_eq!(page[0]["height"].as_u64().unwrap(), baseline_height);
}

#[tokio::test]
async fn pruned_block_returns_410_retention_gone() {
    let (app, handle) = test_app_with_store_config(
        true,
        SequencerConfig {
            recent_block_cache_capacity: 1,
            canonical_archive_retention_blocks: 1,
            canonical_archive_maintenance_interval_blocks: 1,
            canonical_archive_max_rows_per_pass: 10,
            block_interval: Duration::from_secs(60),
            ..SequencerConfig::default()
        },
    )
    .await;

    let first = handle.produce_block().await.unwrap();
    handle.produce_block().await.unwrap();
    handle.produce_block().await.unwrap();

    let (status, body) = get(
        app,
        &format!("/v1/blocks/{}", first.canonical.header.height),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::GONE,
        "body: {}",
        String::from_utf8_lossy(&body)
    );
    let resp = parse_json(&body);
    assert_eq!(resp["code"].as_str().unwrap(), "RETENTION_GONE");
}

#[tokio::test]
async fn account_orders_include_created_at_ms() {
    let (app, _) = test_app(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "ts?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let (status, _) = post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/orders")).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    let pending = pending.as_array().unwrap();
    assert_eq!(pending.len(), 1, "got {pending:?}");
    let created_at_ms = pending[0]["created_at_ms"].as_u64().unwrap();
    assert!(
        created_at_ms >= before,
        "created_at_ms {created_at_ms} not >= submit time {before}"
    );
}

#[tokio::test]
async fn event_raw_snapshot_put_then_get() {
    let dir = std::env::temp_dir().join(format!("sybil-snap-{}-{}", std::process::id(), 1));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        event_snapshot_dir: dir.to_string_lossy().into_owned(),
        ..ApiConfig::default()
    })
    .await;

    let payload = json!({ "id": "evt123", "description": "hi", "negRisk": true });
    let (status, _) = put_json(app.clone(), "/v1/events/evt123/raw", payload.clone()).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(app.clone(), "/v1/events/evt123/raw").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse_json(&body), payload);

    // Unknown event → 404.
    let (status, _) = get(app, "/v1/events/nope/raw").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn account_equity_series_populates_after_trades() {
    let (app, handle) = test_app_with_store(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "Eq?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_b = parse_json(&body)["account_id"].as_u64().unwrap();

    // Two crossing orders so fills are generated and the accounts enter `touched`.
    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 600_000_000u64, "quantity": 10 }]
    })).await;
    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_b,
        "orders": [{ "type": "BuyNo", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
    })).await;

    // Produce a block so the orders fill and equity is sampled.
    let block = handle.produce_block().await.unwrap();
    assert!(
        !block.canonical.fills.is_empty(),
        "expected fills from crossing orders"
    );

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/equity?range=all")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    assert_eq!(v["account_id"].as_u64().unwrap(), account_id);
    assert!(
        !v["points"].as_array().unwrap().is_empty(),
        "expected >=1 equity point: {v}"
    );
}

#[tokio::test]
async fn account_history_shows_placed_then_cancelled() {
    let (app, handle) = test_app_with_store(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    post_json(app.clone(), "/v1/markets", json!({ "name": "Test" })).await;

    let key = new_signing_key();
    let public_key_hex = to_hex(key.verifying_key().to_sec1_point(true).as_bytes());
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{}/keys", account_id),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let genesis_hash = ensure_genesis_hash(&handle).await;
    let order_payload =
        signed_buy_yes_payload(account_id, 0, 500_000_000, 3, 1, genesis_hash, &key);
    let (status, _) = post_json(app.clone(), "/v1/orders/signed", order_payload).await;
    assert_eq!(status, StatusCode::OK);

    handle.produce_block().await.unwrap();

    let (status, body) = get(app.clone(), &format!("/v1/accounts/{}/orders", account_id)).await;
    assert_eq!(status, StatusCode::OK);
    let pending = parse_json(&body);
    let order_id = pending.as_array().unwrap()[0]["order_id"].as_u64().unwrap();

    let cancel_payload = signed_cancel_payload(account_id, order_id, 2, genesis_hash, &key);
    let (status, body) = post_json(app.clone(), "/v1/orders/cancel/signed", cancel_payload).await;
    assert_eq!(status, StatusCode::OK);
    assert!(parse_json(&body)["cancelled"].as_bool().unwrap());
    handle.produce_block().await.unwrap();

    // History is committed-only: the cancellation becomes visible after the
    // next block exports its staged private event facts.
    let (status, body) = get(
        app.clone(),
        &format!("/v1/accounts/{}/events?limit=20", account_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = parse_json(&body);
    assert!(events["next_before"].is_null());
    let events = events["events"].as_array().unwrap();
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert!(types.contains(&"placed"), "history: {types:?}");
    assert!(types.contains(&"cancelled"), "history: {types:?}");
    // newest-first: cancelled appears before placed
    let pc = types.iter().position(|t| *t == "cancelled").unwrap();
    let pp = types.iter().position(|t| *t == "placed").unwrap();
    assert!(pc < pp, "expected cancelled newest-first: {types:?}");
    let (status, _) = get(
        app,
        &format!("/v1/accounts/{account_id}/events?before=not-a-cursor"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn account_equity_series_is_served_by_history_service() {
    // Zero recent-cache caps so the response can only come from the extracted
    // history service.
    let (app, handle) = test_app_with_store_zero_caps(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "EqDb?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_b = parse_json(&body)["account_id"].as_u64().unwrap();

    // Two crossing orders so fills are generated.
    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 600_000_000u64, "quantity": 10 }]
    })).await;
    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_b,
        "orders": [{ "type": "BuyNo", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
    })).await;

    // Produce a block — this exports equity through the product-history outbox.
    let block = handle.produce_block().await.unwrap();
    assert!(
        !block.canonical.fills.is_empty(),
        "expected fills from crossing orders"
    );

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/equity?range=all")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    assert_eq!(v["account_id"].as_u64().unwrap(), account_id);
    assert!(
        !v["points"].as_array().unwrap().is_empty(),
        "equity must come back from the history service: {v}"
    );
}

#[tokio::test]
async fn account_fills_are_served_by_history_service_with_zero_recent_cap() {
    let (app, handle) = test_app_with_store_zero_caps(true).await;

    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "FillDb?" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 10_000_000_000u64 }),
    )
    .await;
    let account_b = parse_json(&body)["account_id"].as_u64().unwrap();

    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 600_000_000u64, "quantity": 10 }]
        }),
    )
    .await;
    post_json(
        app.clone(),
        "/v1/orders",
        json!({
            "account_id": account_b,
            "orders": [{ "type": "BuyNo", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 10 }]
        }),
    )
    .await;

    let block = handle.produce_block().await.unwrap();
    assert!(
        !block.canonical.fills.is_empty(),
        "expected fills from crossing orders"
    );

    let (status, body) = get(
        app,
        &format!("/v1/accounts/{account_id}/fills?after=0.0&limit=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let fills = parse_json(&body);
    let fills = fills["fills"].as_array().unwrap();
    assert!(
        !fills.is_empty(),
        "fills must come back from the history service at recent cap 0"
    );
    assert!(fills[0]["cursor"].as_str().is_some());
}

#[tokio::test]
async fn account_events_are_served_by_history_service() {
    // Zero recent-cache caps so events can only come from the extracted
    // history service.
    let (app, handle) = test_app_with_store_zero_caps(true).await;

    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse_json(&body)["account_id"].as_u64().unwrap();
    let (_, body) = post_json(app.clone(), "/v1/markets", json!({ "name": "HistDb" })).await;
    let market_id = parse_json(&body)["market_id"].as_u64().unwrap();

    post_json(app.clone(), "/v1/orders", json!({
        "account_id": account_id,
        "orders": [{ "type": "BuyYes", "market_id": market_id, "limit_price_nanos": 500_000_000u64, "quantity": 5 }]
    })).await;

    // Produce a block — this exports account events through the outbox.
    handle.produce_block().await.unwrap();

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/events?limit=20")).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse_json(&body);
    let arr = v["events"].as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "events must come back from the history service: {v}"
    );
    assert!(
        arr.iter().any(|e| e["type"] == "placed"),
        "expected a projected 'placed' event: {v}"
    );
}
