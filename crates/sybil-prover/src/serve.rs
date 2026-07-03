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

    for status_path in candidates {
        if !status_path.exists() {
            continue;
        }
        let file = File::open(&status_path).map_err(|source| ProverCliError::Open {
            path: status_path.clone(),
            source,
        })?;
        let reader = BufReader::new(file);
        let status =
            serde_json::from_reader(reader).map_err(|source| ProverCliError::DecodeJson {
                path: status_path,
                source,
            })?;
        return Ok(Some(status));
    }
    Ok(None)
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
        let file = File::open(&status_path).map_err(|source| ProverCliError::Open {
            path: status_path.clone(),
            source,
        })?;
        let reader = BufReader::new(file);
        let status =
            serde_json::from_reader(reader).map_err(|source| ProverCliError::DecodeJson {
                path: status_path,
                source,
            })?;
        statuses.push(status);
    }
    Ok(statuses)
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
