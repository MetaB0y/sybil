use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde_json::{Value, json};
use sybil_history::{HistoryHandle, HistoryHttpConfig, HistoryStore};
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

async fn ensure_history_server(data_dir: &Path) -> String {
    let root = data_dir.parent().expect("process test data has a root");
    let port_path = root.join("history-port");
    let port = if port_path.exists() {
        std::fs::read_to_string(&port_path)
            .expect("history port file reads")
            .parse::<u16>()
            .expect("history port is numeric")
    } else {
        let port = free_port();
        std::fs::write(&port_path, port.to_string()).expect("history port persists");
        let history_dir = root.join("history");
        std::fs::create_dir_all(&history_dir).expect("history dir creates");
        let store = HistoryStore::open(history_dir.join("history.redb"), vec![1, 60, 300, 3_600])
            .expect("history store opens");
        let handle = HistoryHandle::spawn(store.clone());
        let app = sybil_history::router(
            handle,
            store,
            HistoryHttpConfig {
                dev_mode: true,
                internal_token: None,
                max_query_concurrency: 4,
            },
        );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .expect("history process-test listener binds");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("history process-test server");
        });
        port
    };
    format!("http://127.0.0.1:{port}")
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
    let history_url = ensure_history_server(data_dir).await;
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
        .env("SYBIL_HISTORY_URL", history_url)
        .env("SYBIL_HISTORY_POLL_MS", "1")
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
    // A full `cargo test --workspace` can leave this process competing with
    // linker/test cleanup for several seconds even though the same process test
    // starts immediately in isolation. Keep the assertion bounded, but allow
    // enough headroom for the workspace gate on loaded CI and dev hosts.
    let deadline = Instant::now() + Duration::from_secs(30);

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
        if let Some(height) = health["height"].as_u64()
            && height >= min_height
        {
            return height;
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
    let is_history = path.contains("/fills")
        || path.contains("/events")
        || path.contains("/equity")
        || path.contains("/prices/history")
        || path.contains("/prices/candles");
    let expected_height = if is_history {
        wait_for_health(client, base_url).await["height"].as_u64()
    } else {
        None
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let resp = client
            .get(format!("{base_url}{path}"))
            .send()
            .await
            .expect("GET request succeeds");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let value = serde_json::from_str::<Value>(&text).ok();
        let pending = is_history
            && (status == StatusCode::SERVICE_UNAVAILABLE
                || (status.is_success()
                    && value.as_ref().is_some_and(|value| {
                        value
                            .get("indexed_through_height")
                            .and_then(Value::as_u64)
                            .is_none_or(|indexed| {
                                expected_height.is_some_and(|head| indexed < head)
                            })
                    })));
        if !pending || Instant::now() >= deadline {
            assert!(
                status.is_success(),
                "GET {path} failed with {status}: {text}"
            );
            return value.expect("GET response is JSON");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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
