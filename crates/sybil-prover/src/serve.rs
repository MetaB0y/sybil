use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State as AxumState},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use clap::Args;
use serde::Serialize;
use tokio::net::TcpListener;

use crate::artifacts::{discover_proof_jobs, unix_time_ms, WorkerStatusJson};
use crate::ProverCliError;

#[derive(Args)]
pub struct ServeArgs {
    /// Directory containing per-block prover artifacts written by `worker`.
    #[arg(long)]
    pub artifacts_dir: PathBuf,
    /// Optional proof-job inbox directory, used only for queue-depth metrics.
    #[arg(long)]
    pub jobs_dir: Option<PathBuf>,
    /// Socket address for the proof-status API.
    #[arg(long, default_value = "127.0.0.1:3002")]
    pub bind: String,
}

#[derive(Clone)]
struct ProverApiState {
    artifacts_dir: Arc<PathBuf>,
    jobs_dir: Option<Arc<PathBuf>>,
}

#[derive(Serialize)]
struct ApiErrorJson {
    error: String,
}

#[derive(Serialize)]
struct ProofStatusListJson {
    count: usize,
    latest: Option<WorkerStatusJson>,
    statuses: Vec<WorkerStatusJson>,
}

pub async fn serve(args: ServeArgs) -> Result<(), ProverCliError> {
    std::fs::create_dir_all(&args.artifacts_dir).map_err(|source| ProverCliError::CreateDir {
        path: args.artifacts_dir.clone(),
        source,
    })?;
    if let Some(jobs_dir) = &args.jobs_dir {
        std::fs::create_dir_all(jobs_dir).map_err(|source| ProverCliError::CreateDir {
            path: jobs_dir.clone(),
            source,
        })?;
    }

    let listener = TcpListener::bind(&args.bind)
        .await
        .map_err(|source| ProverCliError::Bind {
            addr: args.bind.clone(),
            source,
        })?;
    let app = prover_router_with_jobs_dir(args.artifacts_dir, args.jobs_dir);
    println!("proof_api={}", args.bind);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|source| ProverCliError::Serve { source })
}

fn prover_router_with_jobs_dir(artifacts_dir: PathBuf, jobs_dir: Option<PathBuf>) -> Router {
    let state = ProverApiState {
        artifacts_dir: Arc::new(artifacts_dir),
        jobs_dir: jobs_dir.map(Arc::new),
    };
    Router::new()
        .route("/healthz", get(prover_healthz))
        .route("/metrics", get(prover_metrics))
        .route("/proofs", get(list_proof_statuses))
        .route("/proofs/latest", get(get_latest_proof_status))
        .route("/proofs/{height}", get(get_proof_status))
        .with_state(state)
}

async fn prover_healthz() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn prover_metrics(AxumState(state): AxumState<ProverApiState>) -> impl IntoResponse {
    match render_prover_metrics(
        &state.artifacts_dir,
        state.jobs_dir.as_deref().map(PathBuf::as_path),
    ) {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            body,
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            format!(
                "# sybil_prover_metrics_error {}\n",
                prometheus_quoted(&error.to_string())
            ),
        )
            .into_response(),
    }
}

async fn get_proof_status(
    AxumState(state): AxumState<ProverApiState>,
    AxumPath(height): AxumPath<u64>,
) -> impl IntoResponse {
    match read_worker_status_by_height(&state.artifacts_dir, height) {
        Ok(Some(status)) => (StatusCode::OK, Json(status)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiErrorJson {
                error: format!("proof status not found for height {height}"),
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorJson {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn get_latest_proof_status(AxumState(state): AxumState<ProverApiState>) -> impl IntoResponse {
    match read_latest_worker_status(&state.artifacts_dir) {
        Ok(Some(status)) => (StatusCode::OK, Json(status)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiErrorJson {
                error: "proof status store is empty".to_string(),
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorJson {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn list_proof_statuses(AxumState(state): AxumState<ProverApiState>) -> impl IntoResponse {
    match read_worker_statuses(&state.artifacts_dir) {
        Ok(mut statuses) => {
            statuses.sort_by(|left, right| {
                left.block_height
                    .cmp(&right.block_height)
                    .then_with(|| left.updated_at_ms.cmp(&right.updated_at_ms))
                    .then_with(|| left.artifact_dir.cmp(&right.artifact_dir))
            });
            let latest = select_latest_status(statuses.iter().cloned());
            let body = ProofStatusListJson {
                count: statuses.len(),
                latest,
                statuses,
            };
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorJson {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

fn read_worker_status_by_height(
    artifacts_dir: &Path,
    height: u64,
) -> Result<Option<WorkerStatusJson>, ProverCliError> {
    let prefix = format!("block-{height:020}-");
    let entries = std::fs::read_dir(artifacts_dir).map_err(|source| ProverCliError::ListDir {
        path: artifacts_dir.to_path_buf(),
        source,
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProverCliError::ListDir {
            path: artifacts_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        {
            candidates.push(path.join("status.json"));
        }
    }
    candidates.sort();

    let mut statuses = Vec::new();
    for status_path in candidates {
        if !status_path.exists() {
            continue;
        }
        statuses.push(read_worker_status(&status_path)?);
    }
    Ok(select_latest_status(statuses))
}

fn read_worker_statuses(artifacts_dir: &Path) -> Result<Vec<WorkerStatusJson>, ProverCliError> {
    let entries = std::fs::read_dir(artifacts_dir).map_err(|source| ProverCliError::ListDir {
        path: artifacts_dir.to_path_buf(),
        source,
    })?;
    let mut status_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProverCliError::ListDir {
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
            status_paths.push(path.join("status.json"));
        }
    }
    status_paths.sort();

    let mut statuses = Vec::new();
    for status_path in status_paths {
        if !status_path.exists() {
            continue;
        }
        statuses.push(read_worker_status(&status_path)?);
    }
    Ok(statuses)
}

fn read_latest_worker_status(
    artifacts_dir: &Path,
) -> Result<Option<WorkerStatusJson>, ProverCliError> {
    let statuses = read_worker_statuses(artifacts_dir)?;
    Ok(select_latest_status(statuses))
}

fn read_worker_status(status_path: &Path) -> Result<WorkerStatusJson, ProverCliError> {
    let file = File::open(status_path).map_err(|source| ProverCliError::Open {
        path: status_path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(|source| ProverCliError::DecodeJson {
        path: status_path.to_path_buf(),
        source,
    })
}

/// Pick the freshest status: highest block height, then newest update, then the
/// lexicographically-last artifact dir as a deterministic tiebreak.
fn select_latest_status(
    statuses: impl IntoIterator<Item = WorkerStatusJson>,
) -> Option<WorkerStatusJson> {
    statuses.into_iter().max_by(|left, right| {
        left.block_height
            .cmp(&right.block_height)
            .then_with(|| left.updated_at_ms.cmp(&right.updated_at_ms))
            .then_with(|| left.artifact_dir.cmp(&right.artifact_dir))
    })
}

fn render_prover_metrics(
    artifacts_dir: &Path,
    jobs_dir: Option<&Path>,
) -> Result<String, ProverCliError> {
    let statuses = read_worker_statuses(artifacts_dir)?;
    let now_ms = unix_time_ms();
    let latest_height = statuses
        .iter()
        .filter(|status| status.status == "prepared")
        .map(|status| status.block_height)
        .max()
        .unwrap_or(0);
    let latest_updated_at_ms = statuses
        .iter()
        .map(|status| status.updated_at_ms)
        .max()
        .unwrap_or(0);
    let age_seconds = if latest_updated_at_ms == 0 {
        0.0
    } else {
        now_ms.saturating_sub(latest_updated_at_ms) as f64 / 1_000.0
    };

    let mut status_counts: Vec<((String, String), usize)> = Vec::new();
    for status in &statuses {
        let key = (status.status.clone(), status.proof_status.clone());
        match status_counts
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            Some((_, count)) => *count += 1,
            None => status_counts.push((key, 1)),
        }
    }
    status_counts.sort_by(|(left, _), (right, _)| left.cmp(right));

    let queued_jobs = match jobs_dir {
        Some(path) => Some(discover_proof_jobs(path)?.len()),
        None => None,
    };

    let mut out = String::new();
    out.push_str("# HELP sybil_prover_artifact_store_ready Prover artifact store scan health.\n");
    out.push_str("# TYPE sybil_prover_artifact_store_ready gauge\n");
    out.push_str("sybil_prover_artifact_store_ready 1\n");
    out.push_str(
        "# HELP sybil_prover_artifact_directories_total Prepared prover artifact directories.\n",
    );
    out.push_str("# TYPE sybil_prover_artifact_directories_total gauge\n");
    append_prometheus_sample(
        &mut out,
        "sybil_prover_artifact_directories_total",
        &[],
        statuses.len() as f64,
    );
    out.push_str("# HELP sybil_prover_artifacts_total Prover artifacts grouped by worker and proof status.\n");
    out.push_str("# TYPE sybil_prover_artifacts_total gauge\n");
    for ((worker_status, proof_status), count) in status_counts {
        append_prometheus_sample(
            &mut out,
            "sybil_prover_artifacts_total",
            &[("status", &worker_status), ("proof_status", &proof_status)],
            count as f64,
        );
    }
    out.push_str("# HELP sybil_prover_latest_prepared_height Latest block height prepared by the prover worker.\n");
    out.push_str("# TYPE sybil_prover_latest_prepared_height gauge\n");
    append_prometheus_sample(
        &mut out,
        "sybil_prover_latest_prepared_height",
        &[],
        latest_height as f64,
    );
    out.push_str("# HELP sybil_prover_latest_updated_at_seconds Unix timestamp for the newest prover artifact update.\n");
    out.push_str("# TYPE sybil_prover_latest_updated_at_seconds gauge\n");
    append_prometheus_sample(
        &mut out,
        "sybil_prover_latest_updated_at_seconds",
        &[],
        latest_updated_at_ms as f64 / 1_000.0,
    );
    out.push_str("# HELP sybil_prover_latest_artifact_age_seconds Age of the newest prover artifact update.\n");
    out.push_str("# TYPE sybil_prover_latest_artifact_age_seconds gauge\n");
    append_prometheus_sample(
        &mut out,
        "sybil_prover_latest_artifact_age_seconds",
        &[],
        age_seconds,
    );
    if let Some(queued_jobs) = queued_jobs {
        out.push_str(
            "# HELP sybil_prover_jobs_queued MessagePack proof jobs waiting in the prover inbox.\n",
        );
        out.push_str("# TYPE sybil_prover_jobs_queued gauge\n");
        append_prometheus_sample(
            &mut out,
            "sybil_prover_jobs_queued",
            &[],
            queued_jobs as f64,
        );
    }
    Ok(out)
}

fn append_prometheus_sample(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (index, (key, value)) in labels.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(key);
            out.push_str("=\"");
            out.push_str(&prometheus_label_value(value));
            out.push('"');
        }
        out.push('}');
    }
    out.push(' ');
    out.push_str(&format_prometheus_number(value));
    out.push('\n');
}

fn format_prometheus_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn prometheus_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn prometheus_quoted(value: &str) -> String {
    format!("\"{}\"", prometheus_label_value(value))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::artifacts::{hex32, worker_artifact_dir, write_json_pretty, WorkerStatusJson};
    use crate::StateTransitionProofJobId;

    use super::{read_latest_worker_status, read_worker_status_by_height};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sybil-prover-{prefix}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn reads_newest_worker_status_when_height_has_multiple_artifacts() {
        let artifacts_dir = temp_path("proof-api-artifacts-multiple");
        let old_job_id = StateTransitionProofJobId {
            block_height: 7,
            block_hash: [0x11; 32],
            state_root: [0x22; 32],
        };
        let new_job_id = StateTransitionProofJobId {
            block_height: 7,
            block_hash: [0x99; 32],
            state_root: [0xaa; 32],
        };
        let old_artifact_dir = worker_artifact_dir(&artifacts_dir, &old_job_id);
        let new_artifact_dir = worker_artifact_dir(&artifacts_dir, &new_job_id);
        std::fs::create_dir_all(&old_artifact_dir).unwrap();
        std::fs::create_dir_all(&new_artifact_dir).unwrap();
        let old_status_path = old_artifact_dir.join("status.json");
        let new_status_path = new_artifact_dir.join("status.json");
        let old_status = WorkerStatusJson {
            version: 1,
            producer: "mock-live".to_string(),
            status: "prepared".to_string(),
            job_path: "mock://sybil/blocks/7".to_string(),
            artifact_dir: old_artifact_dir.display().to_string(),
            block_height: 7,
            block_hash: hex32([0x11; 32]),
            state_root: hex32([0x22; 32]),
            public_input_hash: hex32([0x33; 32]),
            da_commitment: hex32([0x44; 32]),
            da_provider_ref: "mock://sybil/blocks/7/da".to_string(),
            da_payload: old_artifact_dir
                .join("mock-block.json")
                .display()
                .to_string(),
            guest_input: old_artifact_dir
                .join("guest-input.mock.json")
                .display()
                .to_string(),
            da_manifest: old_artifact_dir
                .join("da-manifest.mock.json")
                .display()
                .to_string(),
            public_input_hash_path: Some(
                old_artifact_dir
                    .join("public-input-hash.hex")
                    .display()
                    .to_string(),
            ),
            proof_status: "mock_verified".to_string(),
            updated_at_ms: 100,
        };
        let new_status = WorkerStatusJson {
            version: 1,
            producer: "worker".to_string(),
            status: "prepared".to_string(),
            job_path: "/tmp/job.msgpack".to_string(),
            artifact_dir: new_artifact_dir.display().to_string(),
            block_height: 7,
            block_hash: hex32([0x99; 32]),
            state_root: hex32([0xaa; 32]),
            public_input_hash: hex32([0xbb; 32]),
            da_commitment: hex32([0xcc; 32]),
            da_provider_ref: "sybil-file://witness/example.witness.bin".to_string(),
            da_payload: new_artifact_dir
                .join("da/example.witness.bin")
                .display()
                .to_string(),
            guest_input: new_artifact_dir
                .join("guest-input.msgpack")
                .display()
                .to_string(),
            da_manifest: new_artifact_dir
                .join("da-manifest.json")
                .display()
                .to_string(),
            public_input_hash_path: Some(
                new_artifact_dir
                    .join("public-input-hash.hex")
                    .display()
                    .to_string(),
            ),
            proof_status: "not_started".to_string(),
            updated_at_ms: 200,
        };
        write_json_pretty(&old_status_path, &old_status).unwrap();
        write_json_pretty(&new_status_path, &new_status).unwrap();

        let loaded = read_worker_status_by_height(&artifacts_dir, 7)
            .unwrap()
            .unwrap();
        let latest = read_latest_worker_status(&artifacts_dir).unwrap().unwrap();
        let _ = std::fs::remove_file(&old_status_path);
        let _ = std::fs::remove_file(&new_status_path);
        let _ = std::fs::remove_dir(&old_artifact_dir);
        let _ = std::fs::remove_dir(&new_artifact_dir);
        let _ = std::fs::remove_dir(&artifacts_dir);

        assert_eq!(loaded.producer, "worker");
        assert_eq!(loaded.public_input_hash, hex32([0xbb; 32]));
        assert_eq!(latest.block_hash, hex32([0x99; 32]));
    }
}
