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
    spawn_api_with_env(data_dir, admin_key_path, block_interval_ms, &[]).await
}

async fn spawn_api_with_env(
    data_dir: &Path,
    admin_key_path: &Path,
    block_interval_ms: u64,
    extra_env: &[(&str, &str)],
) -> ApiProcess {
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut command = Command::new(sybil_api_binary());
    command
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
        .stderr(Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let child = command.spawn().expect("sybil-api binary spawns");

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

async fn resume_blocks(client: &reqwest::Client, base_url: &str) {
    let resp = client
        .post(format!("{base_url}/v1/simulation/resume"))
        .send()
        .await
        .expect("resume request succeeds");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "resume failed: {}",
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

async fn get_status_and_body(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{base_url}{path}"))
        .send()
        .await
        .expect("GET request succeeds");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
    (status, body)
}

async fn wait_for_block(client: &reqwest::Client, base_url: &str, height: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let last_error = match client
            .get(format!("{base_url}/v1/blocks/{height}"))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status == StatusCode::OK {
                    return serde_json::from_str(&body).expect("block response is JSON");
                }
                format!("status {status}: {body}")
            }
            Err(err) => err.to_string(),
        };

        assert!(
            Instant::now() < deadline,
            "sybil-api did not serve block {height}: {last_error}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn submit_crossing_trade(
    client: &reqwest::Client,
    base_url: &str,
    buyer: u64,
    seller: u64,
    market_id: u64,
    yes_price_nanos: u64,
    no_price_nanos: u64,
    quantity: u64,
) {
    post_json(
        client,
        base_url,
        "/v1/orders",
        json!({
            "account_id": buyer,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": yes_price_nanos,
                "quantity": quantity
            }]
        }),
    )
    .await;
    post_json(
        client,
        base_url,
        "/v1/orders",
        json!({
            "account_id": seller,
            "orders": [{
                "type": "BuyNo",
                "market_id": market_id,
                "limit_price_nanos": no_price_nanos,
                "quantity": quantity
            }]
        }),
    )
    .await;
}

fn assert_funding_history_once(history: &Value) {
    let events = history
        .as_array()
        .expect("funding history response is an array");
    let created: Vec<_> = events
        .iter()
        .filter(|event| event["type"].as_str() == Some("created"))
        .collect();
    assert_eq!(
        created.len(),
        1,
        "account creation history must appear once; history={history}"
    );
    assert_eq!(created[0]["amount_nanos"].as_i64(), Some(1_000));

    let deposits: Vec<_> = events
        .iter()
        .filter(|event| event["type"].as_str() == Some("deposit"))
        .collect();
    assert_eq!(
        deposits.len(),
        1,
        "deposit history must appear once; history={history}"
    );
    assert_eq!(deposits[0]["amount_nanos"].as_i64(), Some(250));
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
    let pre_write_height = pre_write_health["height"]
        .as_u64()
        .expect("baseline height exists before WAL writes");

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
    let post_restart_health = wait_for_health(&client, &reader.base_url).await;
    assert_eq!(
        post_restart_health["height"].as_u64(),
        Some(pre_write_height),
        "health should report restored committed height before a new block is produced"
    );
    pause_blocks(&client, &reader.base_url).await;

    let restored_account = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}"),
    )
    .await;
    assert_eq!(restored_account["balance_nanos"].as_i64(), Some(1_250));
    let restored_funding_history = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/events?category=funding&limit=10"),
    )
    .await;
    assert_funding_history_once(&restored_funding_history);

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
    let committer = spawn_api(&data_dir, &admin_key_path, 50).await;
    wait_for_height_at_least(&client, &committer.base_url, pre_write_height + 1).await;
    pause_blocks(&client, &committer.base_url).await;
    let committed_funding_history = get_json(
        &client,
        &committer.base_url,
        &format!("/v1/accounts/{account_id}/events?category=funding&limit=10"),
    )
    .await;
    assert_funding_history_once(&committed_funding_history);

    committer.kill().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_retention_and_candles_survive_process_restart() {
    let root = temp_root("process-restart-history");
    let data_dir = root.join("data");
    let admin_key_path = root.join("admin-feed.key");
    let retention_env = [
        ("SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS", "2"),
        ("SYBIL_RAW_PRICE_RETENTION_BLOCKS", "100"),
        ("SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS", "1"),
        ("SYBIL_HISTORY_PRUNE_MAX_ROWS", "100"),
        ("SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS", "60"),
    ];
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let writer = spawn_api_with_env(&data_dir, &admin_key_path, 50, &retention_env).await;
    wait_for_height_at_least(&client, &writer.base_url, 1).await;
    pause_blocks(&client, &writer.base_url).await;

    let buyer = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await["account_id"]
        .as_u64()
        .unwrap();
    let seller = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await["account_id"]
        .as_u64()
        .unwrap();
    let market_id = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart history market" }),
    )
    .await["market_id"]
        .as_u64()
        .unwrap();

    let mut first_trade_height = None;
    let mut last_trade_height = 0;
    for (yes, no) in [
        (600_000_000u64, 500_000_000u64),
        (650_000_000u64, 450_000_000u64),
        (700_000_000u64, 400_000_000u64),
    ] {
        let before = wait_for_health(&client, &writer.base_url).await["height"]
            .as_u64()
            .expect("height while paused");
        submit_crossing_trade(
            &client,
            &writer.base_url,
            buyer,
            seller,
            market_id,
            yes,
            no,
            5,
        )
        .await;
        resume_blocks(&client, &writer.base_url).await;
        let committed = wait_for_height_at_least(&client, &writer.base_url, before + 1).await;
        pause_blocks(&client, &writer.base_url).await;
        first_trade_height.get_or_insert(before + 1);
        last_trade_height = committed;
    }
    let first_trade_height = first_trade_height.expect("at least one trade committed");

    writer.kill().await;

    let reader = spawn_api_with_env(&data_dir, &admin_key_path, 60_000, &retention_env).await;
    let restored_height = wait_for_health(&client, &reader.base_url).await["height"]
        .as_u64()
        .expect("restored height");
    assert!(
        restored_height >= last_trade_height,
        "restart should restore at least the last committed trade block"
    );
    pause_blocks(&client, &reader.base_url).await;

    let history = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}/prices/history?limit=10"),
    )
    .await;
    let points = history["points"].as_array().unwrap();
    assert_eq!(
        points.len(),
        3,
        "raw price history after restart: {history}"
    );
    assert!(points
        .iter()
        .all(|point| point["volume_nanos"].as_u64().unwrap() > 0));

    let candles = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}/prices/candles?resolution=1m&limit=10"),
    )
    .await;
    let point_count: u64 = candles["candles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candle| candle["point_count"].as_u64().unwrap())
        .sum();
    assert_eq!(point_count, 3, "candle history after restart: {candles}");

    let (status, body) = get_status_and_body(
        &client,
        &reader.base_url,
        &format!("/v1/blocks/{first_trade_height}"),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "body={body}");
    assert_eq!(body["code"].as_str(), Some("RETENTION_GONE"));

    reader.kill().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_bundle_revalidates_against_replayed_admit_after_process_restart() {
    let root = temp_root("process-restart-deferred");
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
    let pre_write_height = pre_write_health["height"]
        .as_u64()
        .expect("baseline height exists before deferred writes");

    let created = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 1_000_000_000u64 }),
    )
    .await;
    let account_id = created["account_id"].as_u64().unwrap();

    let market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart deferred market" }),
    )
    .await;
    let market_id = market["market_id"].as_u64().unwrap();

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 800_000_000u64,
                "quantity": 1u64
            }]
        }),
    )
    .await;
    let pending_after_direct = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let direct_orders = pending_after_direct
        .as_array()
        .expect("pending orders response is an array");
    assert_eq!(direct_orders.len(), 1);
    let direct_order_id = direct_orders[0]["order_id"].as_u64().unwrap();

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 600_000_000u64,
                    "quantity": 1u64
                },
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 600_000_000u64,
                    "quantity": 1u64
                }
            ]
        }),
    )
    .await;
    let pending_after_deferred = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert_eq!(
        pending_after_deferred.as_array().unwrap().len(),
        1,
        "deferred bundle must not appear as directly admitted resting orders"
    );

    writer.kill().await;

    let committer = spawn_api(&data_dir, &admin_key_path, 50).await;
    let target_height = pre_write_height + 1;
    wait_for_height_at_least(&client, &committer.base_url, target_height).await;
    pause_blocks(&client, &committer.base_url).await;
    let restored_block = wait_for_block(&client, &committer.base_url, target_height).await;

    assert_eq!(
        restored_block["order_count"].as_u64(),
        Some(3),
        "restarted block should contain the replayed direct order plus two rejected deferred orders"
    );
    let rejections = restored_block["rejections"]
        .as_array()
        .expect("block rejections response is an array");
    assert_eq!(rejections.len(), 2, "restored block={restored_block}");
    let mut rejection_ids: Vec<u64> = rejections
        .iter()
        .map(|rejection| rejection["order_id"].as_u64().unwrap())
        .collect();
    rejection_ids.sort_unstable();
    assert_eq!(
        rejection_ids,
        vec![direct_order_id + 1, direct_order_id + 2],
        "restored deferred bundle must allocate fresh IDs after the replayed direct admit"
    );
    assert!(rejections.iter().all(|rejection| {
        rejection["account_id"].as_u64() == Some(account_id)
            && rejection["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("InsufficientBalance"))
    }));

    let post_restart_pending = get_json(
        &client,
        &committer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let pending = post_restart_pending.as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["order_id"].as_u64(), Some(direct_order_id));

    committer.kill().await;
    let _ = std::fs::remove_dir_all(root);
}
