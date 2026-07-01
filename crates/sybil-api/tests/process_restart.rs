//! Process-level restart tests for the sybil-api binary.
//!
//! These intentionally spawn the real binary instead of the in-process Axum
//! router, so they exercise CLI config, persistent-store hydration, actor
//! startup, and HTTP request/response boundaries together.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use matching_engine::{MarketId, Order};
use matching_sequencer::crypto::{canonical_cancel_bytes, canonical_order_bytes};
use matching_sequencer::AccountId;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::process::{Child, Command};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct ApiProcess {
    child: Child,
    base_url: String,
}

impl ApiProcess {
    async fn kill(mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl Drop for ApiProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn temp_root(prefix: &str) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("sybil-api-{prefix}-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&path).expect("test temp dir can be created");
    path
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral port binds");
    listener.local_addr().expect("local addr exists").port()
}

fn sybil_api_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_sybil-api") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("test executable path is available");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(format!("sybil-api{}", std::env::consts::EXE_SUFFIX));
    path
}

fn to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn new_signing_key() -> SigningKey {
    SigningKey::from_bytes((&[9u8; 32]).into()).expect("fixed signing key")
}

fn signed_buy_yes_payload(
    market_id: u32,
    limit_price_nanos: u64,
    quantity: u64,
    key: &SigningKey,
) -> Value {
    let mut order = Order::new(0);
    order.markets[0] = MarketId::new(market_id);
    order.num_markets = 1;
    order.num_states = 2;
    order.limit_price = limit_price_nanos;
    order.max_fill = quantity;
    order.payoffs[0] = 1;
    order.payoffs[1] = 0;

    let signature: Signature = key.sign(&canonical_order_bytes(&order));
    json!({
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "order": {
            "market_ids": [market_id],
            "payoffs": [1, 0],
            "limit_price_nanos": limit_price_nanos,
            "max_fill": quantity
        },
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

fn signed_cancel_payload(account_id: u64, order_id: u64, key: &SigningKey) -> Value {
    let signature: Signature = key.sign(&canonical_cancel_bytes(AccountId(account_id), order_id));
    json!({
        "account_id": account_id,
        "order_id": order_id,
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

async fn spawn_api(data_dir: &Path, admin_key_path: &Path, block_interval_ms: u64) -> ApiProcess {
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let child = Command::new(sybil_api_binary())
        .arg("--dev-mode")
        .arg("--port")
        .arg(port.to_string())
        .arg("--data-dir")
        .arg(data_dir)
        .arg("--admin-feed-key-path")
        .arg(admin_key_path)
        .arg("--block-interval-ms")
        .arg(block_interval_ms.to_string())
        .env("RUST_LOG", "warn")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sybil-api binary spawns");

    ApiProcess { child, base_url }
}

async fn wait_for_health(client: &reqwest::Client, base_url: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        let last_error = match client.get(format!("{base_url}/v1/health")).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status == StatusCode::OK {
                    return serde_json::from_str(&body).expect("health response is JSON");
                }
                format!("status {status}: {body}")
            }
            Err(err) => err.to_string(),
        };

        assert!(
            Instant::now() < deadline,
            "sybil-api did not become healthy: {last_error}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_height_at_least(
    client: &reqwest::Client,
    base_url: &str,
    min_height: u64,
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let health = wait_for_health(client, base_url).await;
        if let Some(height) = health["height"].as_u64() {
            if height >= min_height {
                return height;
            }
        }

        assert!(
            Instant::now() < deadline,
            "sybil-api did not produce block height {min_height}; last health={health}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn pause_blocks(client: &reqwest::Client, base_url: &str) {
    let resp = client
        .post(format!("{base_url}/v1/simulation/pause"))
        .send()
        .await
        .expect("pause request succeeds");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "pause failed: {}",
        resp.text().await.unwrap_or_default()
    );
}

async fn post_json(client: &reqwest::Client, base_url: &str, path: &str, body: Value) -> Value {
    let resp = client
        .post(format!("{base_url}{path}"))
        .json(&body)
        .send()
        .await
        .expect("POST request succeeds");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "POST {path} failed with {status}: {text}"
    );
    serde_json::from_str(&text).expect("POST response is JSON")
}

async fn get_json(client: &reqwest::Client, base_url: &str, path: &str) -> Value {
    let resp = client
        .get(format!("{base_url}{path}"))
        .send()
        .await
        .expect("GET request succeeds");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "GET {path} failed with {status}: {text}"
    );
    serde_json::from_str(&text).expect("GET response is JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_dev_api_writes_survive_kill_and_process_restart_before_next_block() {
    let root = temp_root("process-restart");
    let data_dir = root.join("data");
    let admin_key_path = root.join("admin-feed.key");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let writer = spawn_api(&data_dir, &admin_key_path, 50).await;
    wait_for_height_at_least(&client, &writer.base_url, 1).await;
    pause_blocks(&client, &writer.base_url).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let pre_write_health = wait_for_health(&client, &writer.base_url).await;
    assert!(
        pre_write_health["height"].as_u64().is_some(),
        "baseline height exists before WAL writes"
    );

    let created = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 1_000u64 }),
    )
    .await;
    let account_id = created["account_id"].as_u64().unwrap();

    let funded = post_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/fund"),
        json!({ "amount_nanos": 250u64 }),
    )
    .await;
    assert_eq!(funded["balance_nanos"].as_i64(), Some(1_250));

    let market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart WAL market" }),
    )
    .await;
    let market_id = market["market_id"].as_u64().unwrap();

    let metadata_market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({
            "name": "process restart metadata market",
            "description": "metadata should replay after process restart",
            "category": "restart-tests",
            "tags": ["persistence", "process"],
            "resolution_criteria": "resolved by the restart test",
            "expiry_timestamp_ms": 1_800_000_000_000u64,
            "resolution_template": "admin_immediate"
        }),
    )
    .await;
    let metadata_market_id = metadata_market["market_id"].as_u64().unwrap();

    let resolved_market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart resolved market" }),
    )
    .await;
    let resolved_market_id = resolved_market["market_id"].as_u64().unwrap();
    post_json(
        &client,
        &writer.base_url,
        &format!("/v1/markets/{resolved_market_id}/resolve"),
        json!({ "payout_nanos": 1_000_000_000u64 }),
    )
    .await;

    let signing_key = new_signing_key();
    let public_key_hex = to_hex(signing_key.verifying_key().to_sec1_point(true).as_bytes());
    post_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/keys"),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders/signed",
        signed_buy_yes_payload(market_id as u32, 400, 3, &signing_key),
    )
    .await;
    let pending = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let pending = pending
        .as_array()
        .expect("pending orders response is an array");
    assert_eq!(pending.len(), 1);
    let order_id = pending[0]["order_id"].as_u64().unwrap();

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders/cancel/signed",
        signed_cancel_payload(account_id, order_id, &signing_key),
    )
    .await;
    let pending_after_cancel = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert!(pending_after_cancel.as_array().unwrap().is_empty());
    writer.kill().await;

    let reader = spawn_api(&data_dir, &admin_key_path, 60_000).await;
    wait_for_health(&client, &reader.base_url).await;
    pause_blocks(&client, &reader.base_url).await;

    let restored_account = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}"),
    )
    .await;
    assert_eq!(restored_account["balance_nanos"].as_i64(), Some(1_250));

    let restored_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}"),
    )
    .await;
    assert_eq!(
        restored_market["name"].as_str(),
        Some("process restart WAL market")
    );
    let restored_metadata_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{metadata_market_id}"),
    )
    .await;
    assert_eq!(
        restored_metadata_market["description"].as_str(),
        Some("metadata should replay after process restart")
    );
    assert_eq!(
        restored_metadata_market["category"].as_str(),
        Some("restart-tests")
    );
    assert_eq!(
        restored_metadata_market["tags"].as_array().unwrap()[0].as_str(),
        Some("persistence")
    );
    assert_eq!(
        restored_metadata_market["resolution_criteria"].as_str(),
        Some("resolved by the restart test")
    );
    assert_eq!(
        restored_metadata_market["expiry_timestamp_ms"].as_u64(),
        Some(1_800_000_000_000)
    );

    let restored_resolved_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{resolved_market_id}"),
    )
    .await;
    assert_eq!(
        restored_resolved_market["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        restored_resolved_market["payout_nanos"].as_u64(),
        Some(1_000_000_000)
    );

    let restored_pending = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert!(
        restored_pending.as_array().unwrap().is_empty(),
        "signed cancel should survive restart without resurrecting the resting order"
    );

    post_json(
        &client,
        &reader.base_url,
        "/v1/orders/signed",
        signed_buy_yes_payload(market_id as u32, 450, 2, &signing_key),
    )
    .await;
    let post_restart_pending = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert_eq!(post_restart_pending.as_array().unwrap().len(), 1);

    reader.kill().await;
    let _ = std::fs::remove_dir_all(root);
}
