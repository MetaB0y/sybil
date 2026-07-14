use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use sybil_proof_protocol::ProofKind;

use super::{DaemonError, EpochState, Runtime, now_ms};

pub fn router(runtime: Arc<Runtime>) -> Router {
    let max_job_bytes = runtime.max_job_bytes;
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/proofs/latest", get(latest_proof_compat))
        .route("/v1/status", get(status))
        .route("/v1/epochs", get(list_epochs))
        .route("/v1/epochs/{first_height}", get(get_epoch))
        .route("/v1/jobs", post(ingest_job))
        .route("/v1/admin/seal", post(seal_epoch))
        .route("/v1/admin/retry/{first_height}", post(retry_epoch))
        .layer(DefaultBodyLimit::max(max_job_bytes))
        .with_state(runtime)
}

async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn readyz(State(runtime): State<Arc<Runtime>>) -> Response {
    if !runtime.ready.load(Ordering::Acquire) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "reconciling" })),
        )
            .into_response();
    }
    match runtime
        .store
        .status(runtime.owner, runtime.backend_kind, true)
    {
        Ok(_) => Json(serde_json::json!({ "status": "ready" })).into_response(),
        Err(error) => api_error(error),
    }
}

async fn status(State(runtime): State<Arc<Runtime>>) -> Response {
    let ready = runtime.ready.load(Ordering::Acquire);
    match runtime
        .store
        .status(runtime.owner, runtime.backend_kind, ready)
    {
        Ok(status) => Json(status).into_response(),
        Err(error) => api_error(error),
    }
}

async fn list_epochs(State(runtime): State<Arc<Runtime>>) -> Response {
    match runtime.store.list_epochs() {
        Ok(epochs) => Json(serde_json::json!({
            "count": epochs.len(),
            "epochs": epochs,
        }))
        .into_response(),
        Err(error) => api_error(error),
    }
}

async fn get_epoch(State(runtime): State<Arc<Runtime>>, Path(first_height): Path<u64>) -> Response {
    match runtime.store.read_epoch(first_height) {
        Ok(Some(epoch)) => Json(epoch).into_response(),
        Ok(None) => api_error(DaemonError::NotFound(format!("epoch {first_height}"))),
        Err(error) => api_error(error),
    }
}

/// Compatibility projection for existing synthetic monitoring. The durable
/// authority is the epoch API and redb, not this JSON view.
async fn latest_proof_compat(State(runtime): State<Arc<Runtime>>) -> Response {
    match runtime.store.list_epochs() {
        Ok(epochs) => match epochs
            .into_iter()
            .filter(|epoch| matches!(epoch.state, EpochState::Proven))
            .max_by_key(|epoch| epoch.last_block_height)
        {
            Some(epoch) => Json(LegacyProofStatus {
                block_height: epoch.last_block_height,
                state_root: format!("0x{}", hex::encode(epoch.public_inputs.end_state_root)),
                status: "prepared",
                proof_status: verified_status(epoch.proof_kind),
                updated_at_ms: epoch.updated_at_ms,
            })
            .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "proof epoch store has no proven epoch".to_string(),
                }),
            )
                .into_response(),
        },
        Err(error) => api_error(error),
    }
}

async fn ingest_job(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(response) = authorize(&runtime, &headers) {
        return response;
    }
    if body.len() > runtime.max_job_bytes {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiError {
                error: format!(
                    "proof job is {} bytes; maximum is {}",
                    body.len(),
                    runtime.max_job_bytes
                ),
            }),
        )
            .into_response();
    }
    let store = Arc::clone(&runtime.store);
    let result = tokio::task::spawn_blocking(move || store.ingest(body.to_vec(), now_ms())).await;
    match result {
        Ok(Ok(ack)) => {
            if ack.duplicate {
                runtime
                    .metrics
                    .duplicate_total
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                runtime
                    .metrics
                    .ingested_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            (StatusCode::OK, Json(ack)).into_response()
        }
        Ok(Err(error)) => api_error(error),
        Err(error) => api_error(DaemonError::Task(error)),
    }
}

async fn seal_epoch(State(runtime): State<Arc<Runtime>>, headers: HeaderMap) -> Response {
    if let Some(response) = authorize(&runtime, &headers) {
        return response;
    }
    let store = Arc::clone(&runtime.store);
    let kind = runtime.backend_kind.proof_kind();
    let result = tokio::task::spawn_blocking(move || {
        store.assemble_next(kind, true, now_ms(), "authenticated-admin")
    })
    .await;
    match result {
        Ok(Ok(Some(epoch))) => (StatusCode::OK, Json(epoch)).into_response(),
        Ok(Ok(None)) => (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "no unassembled contiguous jobs are available".to_string(),
            }),
        )
            .into_response(),
        Ok(Err(error)) => api_error(error),
        Err(error) => api_error(DaemonError::Task(error)),
    }
}

async fn retry_epoch(
    State(runtime): State<Arc<Runtime>>,
    Path(first_height): Path<u64>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = authorize(&runtime, &headers) {
        return response;
    }
    let store = Arc::clone(&runtime.store);
    let result = tokio::task::spawn_blocking(move || {
        store.manual_retry(first_height, "authenticated-admin", now_ms())
    })
    .await;
    match result {
        Ok(Ok(epoch)) => (StatusCode::OK, Json(epoch)).into_response(),
        Ok(Err(error)) => api_error(error),
        Err(error) => api_error(DaemonError::Task(error)),
    }
}

async fn metrics(State(runtime): State<Arc<Runtime>>) -> Response {
    let ready = runtime.ready.load(Ordering::Acquire);
    match runtime
        .store
        .status(runtime.owner, runtime.backend_kind, ready)
    {
        Ok(status) => {
            let policy = &status.policy;
            let metric_height = |height: Option<u64>| height.unwrap_or(0);
            let mut body = format!(
                concat!(
                    "# TYPE sybil_prover_ready gauge\n",
                    "sybil_prover_ready {}\n",
                    "# TYPE sybil_prover_jobs gauge\n",
                    "sybil_prover_jobs {}\n",
                    "# TYPE sybil_prover_job_bytes gauge\n",
                    "sybil_prover_job_bytes {}\n",
                    "# TYPE sybil_prover_epochs gauge\n",
                    "sybil_prover_epochs {}\n",
                    "# TYPE sybil_prover_ingested_frontier gauge\n",
                    "sybil_prover_ingested_frontier {}\n",
                    "# TYPE sybil_prover_assembled_frontier gauge\n",
                    "sybil_prover_assembled_frontier {}\n",
                    "# TYPE sybil_prover_proven_frontier gauge\n",
                    "sybil_prover_proven_frontier {}\n",
                    "# TYPE sybil_prover_ingested_total counter\n",
                    "sybil_prover_ingested_total {}\n",
                    "# TYPE sybil_prover_duplicate_total counter\n",
                    "sybil_prover_duplicate_total {}\n",
                    "# TYPE sybil_prover_proofs_total counter\n",
                    "sybil_prover_proofs_total {}\n",
                    "# TYPE sybil_prover_retryable_failures_total counter\n",
                    "sybil_prover_retryable_failures_total {}\n",
                    "# TYPE sybil_prover_permanent_failures_total counter\n",
                    "sybil_prover_permanent_failures_total {}\n",
                    "# TYPE sybil_prover_recovered_leases_total counter\n",
                    "sybil_prover_recovered_leases_total {}\n",
                    "# TYPE sybil_prover_adopted_artifacts_total counter\n",
                    "sybil_prover_adopted_artifacts_total {}\n",
                    "# TYPE sybil_prover_last_proof_timestamp_milliseconds gauge\n",
                    "sybil_prover_last_proof_timestamp_milliseconds {}\n",
                    "# TYPE sybil_prover_source_failures_total counter\n",
                    "sybil_prover_source_failures_total {}\n",
                    "# TYPE sybil_prover_source_acks_total counter\n",
                    "sybil_prover_source_acks_total {}\n"
                ),
                u8::from(status.ready),
                status.jobs,
                status.queued_job_bytes,
                status.epochs,
                metric_height(policy.ingested_frontier),
                metric_height(policy.assembled_frontier),
                metric_height(policy.proven_frontier),
                runtime.metrics.ingested_total.load(Ordering::Relaxed),
                runtime.metrics.duplicate_total.load(Ordering::Relaxed),
                runtime.metrics.proofs_total.load(Ordering::Relaxed),
                runtime
                    .metrics
                    .retryable_failures_total
                    .load(Ordering::Relaxed),
                runtime
                    .metrics
                    .permanent_failures_total
                    .load(Ordering::Relaxed),
                runtime
                    .metrics
                    .recovered_leases_total
                    .load(Ordering::Relaxed),
                runtime
                    .metrics
                    .adopted_artifacts_total
                    .load(Ordering::Relaxed),
                runtime.metrics.last_proof_at_ms.load(Ordering::Relaxed),
                runtime
                    .metrics
                    .source_failures_total
                    .load(Ordering::Relaxed),
                runtime.metrics.source_acks_total.load(Ordering::Relaxed),
            );
            let epochs = match runtime.store.list_epochs() {
                Ok(epochs) => epochs,
                Err(error) => return api_error(error),
            };
            let proven = epochs
                .iter()
                .filter(|epoch| matches!(epoch.state, EpochState::Proven))
                .collect::<Vec<_>>();
            let latest_updated_at_ms = proven
                .iter()
                .map(|epoch| epoch.updated_at_ms)
                .max()
                .unwrap_or(0);
            let latest_age_seconds = if latest_updated_at_ms == 0 {
                0
            } else {
                now_ms().saturating_sub(latest_updated_at_ms) / 1_000
            };
            let queued_jobs = policy
                .ingested_frontier
                .unwrap_or(0)
                .saturating_sub(policy.proven_frontier.unwrap_or(0));
            body.push_str(&format!(
                concat!(
                    "# TYPE sybil_prover_artifact_store_ready gauge\n",
                    "sybil_prover_artifact_store_ready {}\n",
                    "# TYPE sybil_prover_artifact_directories_total gauge\n",
                    "sybil_prover_artifact_directories_total {}\n",
                    "# TYPE sybil_prover_latest_prepared_height gauge\n",
                    "sybil_prover_latest_prepared_height {}\n",
                    "# TYPE sybil_prover_latest_updated_at_seconds gauge\n",
                    "sybil_prover_latest_updated_at_seconds {}\n",
                    "# TYPE sybil_prover_latest_artifact_age_seconds gauge\n",
                    "sybil_prover_latest_artifact_age_seconds {}\n",
                    "# TYPE sybil_prover_jobs_queued gauge\n",
                    "sybil_prover_jobs_queued {}\n",
                    "# TYPE sybil_prover_artifacts_total gauge\n"
                ),
                u8::from(status.ready),
                proven.len(),
                policy.proven_frontier.unwrap_or(0),
                latest_updated_at_ms / 1_000,
                latest_age_seconds,
                queued_jobs,
            ));
            let mut epoch_counts = std::collections::BTreeMap::new();
            for epoch in &epochs {
                let proof_status = match epoch.state {
                    EpochState::Proven => verified_status(epoch.proof_kind),
                    EpochState::FailedPermanent => "failed",
                    _ => "not_started",
                };
                *epoch_counts
                    .entry((
                        epoch.state.label(),
                        proof_kind_label(epoch.proof_kind),
                        proof_status,
                    ))
                    .or_insert(0u64) += 1;
            }
            for ((state, proof_kind, proof_status), count) in epoch_counts {
                body.push_str(&format!(
                    "sybil_prover_epochs_by_state{{state=\"{state}\",proof_kind=\"{proof_kind}\"}} {count}\n",
                ));
                body.push_str(&format!(
                    "sybil_prover_artifacts_total{{status=\"{state}\",proof_status=\"{proof_status}\"}} {count}\n",
                ));
            }
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                body,
            )
                .into_response()
        }
        Err(error) => api_error(error),
    }
}

fn authorize(runtime: &Runtime, headers: &HeaderMap) -> Option<Response> {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if provided.is_some_and(|provided| constant_time_eq(provided, &runtime.auth_token)) {
        return None;
    }
    Some(
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "missing or invalid bearer token".to_string(),
            }),
        )
            .into_response(),
    )
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    let length = left.len().max(right.len());
    for index in 0..length {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

#[derive(Serialize)]
struct LegacyProofStatus {
    block_height: u64,
    state_root: String,
    status: &'static str,
    proof_status: &'static str,
    updated_at_ms: u64,
}

const fn verified_status(kind: ProofKind) -> &'static str {
    match kind {
        ProofKind::Mock => "mock_verified",
        ProofKind::OpenVmStark => "stark_verified",
        ProofKind::OpenVmEvm => "evm_verified",
    }
}

const fn proof_kind_label(kind: ProofKind) -> &'static str {
    match kind {
        ProofKind::Mock => "mock",
        ProofKind::OpenVmStark => "openvm_stark",
        ProofKind::OpenVmEvm => "openvm_evm",
    }
}

fn api_error(error: DaemonError) -> Response {
    let status = match error {
        DaemonError::NotFound(_) => StatusCode::NOT_FOUND,
        DaemonError::Conflict(_) | DaemonError::Gap { .. } | DaemonError::LeaseLost { .. } => {
            StatusCode::CONFLICT
        }
        DaemonError::Config(_)
        | DaemonError::ProofJob(_)
        | DaemonError::Zk(_)
        | DaemonError::Epoch(_)
        | DaemonError::Decode(_)
        | DaemonError::Envelope(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ApiError {
            error: error.to_string(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_comparison_handles_equal_and_different_lengths() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "secreu"));
        assert!(!constant_time_eq("secret", "secret-long"));
    }
}
