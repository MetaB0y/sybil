use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::process::{Child, Command};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Temp root for process-level API tests.
///
/// The API owns subpaths under this directory (`data`, `admin-feed.key`).
/// Removing the whole root on drop keeps restart tests from leaving redb/qMDB
/// artifacts behind after success.
pub struct ProcessTestRoot {
    root: PathBuf,
    data_dir: PathBuf,
    admin_key_path: PathBuf,
}

impl ProcessTestRoot {
    pub fn new(prefix: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("sybil-api-{prefix}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("test temp dir can be created");
        let data_dir = root.join("data");
        let admin_key_path = root.join("admin-feed.key");
        Self {
            root,
            data_dir,
            admin_key_path,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn admin_key_path(&self) -> &Path {
        &self.admin_key_path
    }
}

impl Drop for ProcessTestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct ApiProcess {
    child: Child,
    pub base_url: String,
}

impl ApiProcess {
    pub async fn kill(mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl Drop for ApiProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
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

pub async fn spawn_api(
    data_dir: &Path,
    admin_key_path: &Path,
    block_interval_ms: u64,
) -> ApiProcess {
    spawn_api_with_env(data_dir, admin_key_path, block_interval_ms, &[]).await
}

pub async fn spawn_api_with_env(
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

pub async fn restart_api(
    process: ApiProcess,
    root: &ProcessTestRoot,
    block_interval_ms: u64,
) -> ApiProcess {
    process.kill().await;
    spawn_api(root.data_dir(), root.admin_key_path(), block_interval_ms).await
}

pub async fn restart_api_with_env(
    process: ApiProcess,
    root: &ProcessTestRoot,
    block_interval_ms: u64,
    extra_env: &[(&str, &str)],
) -> ApiProcess {
    process.kill().await;
    spawn_api_with_env(
        root.data_dir(),
        root.admin_key_path(),
        block_interval_ms,
        extra_env,
    )
    .await
}

pub async fn wait_for_health(client: &reqwest::Client, base_url: &str) -> Value {
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

pub async fn wait_for_height_at_least(
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

pub async fn pause_blocks(client: &reqwest::Client, base_url: &str) {
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

pub async fn resume_blocks(client: &reqwest::Client, base_url: &str) {
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

pub async fn post_json(client: &reqwest::Client, base_url: &str, path: &str, body: Value) -> Value {
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

pub async fn get_json(client: &reqwest::Client, base_url: &str, path: &str) -> Value {
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

pub async fn get_status_and_body(
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

pub async fn wait_for_block(client: &reqwest::Client, base_url: &str, height: u64) -> Value {
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
