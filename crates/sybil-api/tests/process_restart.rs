//! Process-level restart tests for the sybil-api binary.
//!
//! These intentionally spawn the real binary instead of the in-process Axum
//! router, so they exercise CLI config, persistent-store hydration, actor
//! startup, and HTTP request/response boundaries together.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

    reader.kill().await;
    let _ = std::fs::remove_dir_all(root);
}
