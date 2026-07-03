use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde::{Deserialize, Serialize};
use sha3::{Digest as _, Keccak256};
use sybil_api_types::BlockResponse;

#[derive(Parser)]
#[command(name = "sybil-prover-mock")]
#[command(about = "Sybil mock proof artifact producer", version)]
pub struct MockLiveArgs {
    /// Sybil API base URL.
    #[arg(long, default_value = "http://localhost:3000")]
    pub sybil_url: String,
    /// Directory where per-block mock prover artifacts and status JSON are written.
    #[arg(long)]
    pub artifacts_dir: PathBuf,
    /// Poll interval for live latest-block checks.
    #[arg(long, default_value_t = 10_000)]
    pub poll_ms: u64,
    /// Retain at most this many mock artifact directories. 0 disables pruning.
    #[arg(long, default_value_t = 1_000)]
    pub max_artifacts: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum MockLiveError {
    #[error("create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("list directory {path}: {source}")]
    ListDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("encode JSON artifact for {path}: {source}")]
    EncodeJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("http request failed: {source}")]
    Http {
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WorkerStatusJson {
    version: u8,
    #[serde(default = "crate::artifacts::unknown_status_producer")]
    producer: String,
    status: String,
    job_path: String,
    artifact_dir: String,
    block_height: u64,
    block_hash: String,
    state_root: String,
    public_input_hash: String,
    da_commitment: String,
    da_provider_ref: String,
    da_payload: String,
    guest_input: String,
    da_manifest: String,
    public_input_hash_path: Option<String>,
    proof_status: String,
    updated_at_ms: u128,
}

pub async fn run(args: MockLiveArgs) -> Result<(), MockLiveError> {
    std::fs::create_dir_all(&args.artifacts_dir).map_err(|source| MockLiveError::CreateDir {
        path: args.artifacts_dir.clone(),
        source,
    })?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|source| MockLiveError::Http { source })?;
    let sybil_url = args.sybil_url.trim_end_matches('/').to_string();
    let mut last_height = None::<u64>;
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            result = fetch_latest_block(&client, &sybil_url) => {
                match result {
                    Ok(block) => {
                        if Some(block.height) != last_height {
                            let wrote = write_mock_artifact(&args.artifacts_dir, &block)?;
                            prune_mock_artifacts(&args.artifacts_dir, args.max_artifacts)?;
                            last_height = Some(block.height);
                            if wrote {
                                println!("mock_proof_height={}", block.height);
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("mock_live_error={error}");
                    }
                }
            }
        }

        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(args.poll_ms)) => {}
        }
    }
}

async fn fetch_latest_block(
    client: &reqwest::Client,
    sybil_url: &str,
) -> Result<BlockResponse, MockLiveError> {
    let resp = client
        .get(format!("{sybil_url}/v1/blocks/latest"))
        .send()
        .await
        .map_err(|source| MockLiveError::Http { source })?
        .error_for_status()
        .map_err(|source| MockLiveError::Http { source })?;
    resp.json()
        .await
        .map_err(|source| MockLiveError::Http { source })
}

fn write_mock_artifact(artifacts_dir: &Path, block: &BlockResponse) -> Result<bool, MockLiveError> {
    let block_hash = mock_hash(
        b"sybil/mock-prover/block",
        &[
            &block.height.to_be_bytes(),
            block.parent_hash.as_bytes(),
            block.state_root.as_bytes(),
            block.events_root.as_bytes(),
        ],
    );
    let artifact_dir = artifacts_dir.join(format!(
        "block-{:020}-{}",
        block.height,
        hex::encode(block_hash)
    ));
    let status_path = artifact_dir.join("status.json");
    if status_path.exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(&artifact_dir).map_err(|source| MockLiveError::CreateDir {
        path: artifact_dir.clone(),
        source,
    })?;

    let block_payload = artifact_dir.join("mock-block.json");
    let guest_input = artifact_dir.join("guest-input.mock.json");
    let manifest = artifact_dir.join("da-manifest.mock.json");
    let public_input_hash_path = artifact_dir.join("public-input-hash.hex");
    let public_input_hash = mock_hash(
        b"sybil/mock-prover/public-input",
        &[
            &block.height.to_be_bytes(),
            block.state_root.as_bytes(),
            block.events_root.as_bytes(),
        ],
    );
    let da_commitment = mock_hash(
        b"sybil/mock-prover/da",
        &[block.parent_hash.as_bytes(), block.events_root.as_bytes()],
    );

    write_json_pretty(&block_payload, block)?;
    write_json_pretty(
        &guest_input,
        &serde_json::json!({
            "kind": "mock-live-sybil-guest-input",
            "block_height": block.height,
            "block_hash": hex32(block_hash),
            "state_root": hex0x(&block.state_root),
            "events_root": hex0x(&block.events_root),
        }),
    )?;
    write_json_pretty(
        &manifest,
        &serde_json::json!({
            "kind": "mock-live-file-da",
            "block_height": block.height,
            "payload": block_payload.display().to_string(),
            "da_commitment": hex32(da_commitment),
        }),
    )?;
    write_hex_hash(&public_input_hash_path, public_input_hash)?;

    let status = WorkerStatusJson {
        version: 1,
        producer: "mock-live".to_string(),
        status: "prepared".to_string(),
        job_path: format!("mock://sybil/blocks/{}", block.height),
        artifact_dir: artifact_dir.display().to_string(),
        block_height: block.height,
        block_hash: hex32(block_hash),
        state_root: hex0x(&block.state_root),
        public_input_hash: hex32(public_input_hash),
        da_commitment: hex32(da_commitment),
        da_provider_ref: format!("mock://sybil/blocks/{}/da", block.height),
        da_payload: block_payload.display().to_string(),
        guest_input: guest_input.display().to_string(),
        da_manifest: manifest.display().to_string(),
        public_input_hash_path: Some(public_input_hash_path.display().to_string()),
        proof_status: "mock_verified".to_string(),
        updated_at_ms: unix_time_ms(),
    };
    write_json_pretty(&status_path, &status)?;

    Ok(true)
}

fn prune_mock_artifacts(artifacts_dir: &Path, max_artifacts: usize) -> Result<(), MockLiveError> {
    if max_artifacts == 0 {
        return Ok(());
    }
    let entries = std::fs::read_dir(artifacts_dir).map_err(|source| MockLiveError::ListDir {
        path: artifacts_dir.to_path_buf(),
        source,
    })?;
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| MockLiveError::ListDir {
            path: artifacts_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("block-"))
        {
            dirs.push(path);
        }
    }
    dirs.sort();
    let excess = dirs.len().saturating_sub(max_artifacts);
    for dir in dirs.into_iter().take(excess) {
        std::fs::remove_dir_all(&dir)
            .map_err(|source| MockLiveError::Write { path: dir, source })?;
    }
    Ok(())
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), MockLiveError> {
    let json = serde_json::to_vec_pretty(value).map_err(|source| MockLiveError::EncodeJson {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, json).map_err(|source| MockLiveError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn write_hex_hash(path: &Path, hash: [u8; 32]) -> Result<(), MockLiveError> {
    std::fs::write(path, format!("0x{}\n", hex::encode(hash))).map_err(|source| {
        MockLiveError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn mock_hash(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn hex0x(value: &str) -> String {
    if value.starts_with("0x") {
        value.to_string()
    } else {
        format!("0x{value}")
    }
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to install Ctrl-C handler: {error}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                eprintln!("failed to install SIGTERM handler: {error}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
