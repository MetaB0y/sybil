use axum::extract::State;
use axum::http::StatusCode;
use serde_json;

use crate::extract::Json;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{
    AttestationResponse, HealthResponse, OrderAdmissionPolicyResponse, StateRootResponse,
};

/// GET /v1/attestation
///
/// Development-only shape stub. The route is mounted only when `dev_mode` is
/// enabled; none of these empty fields are cryptographic evidence.
#[utoipa::path(
    tag = "routessystem",
    get,
    path = "/v1/attestation",
    responses(
        (status = 200, description = "Development-only unverified attestation shape", body = AttestationResponse)
    )
)]
pub async fn attestation() -> Json<AttestationResponse> {
    Json(AttestationResponse {
        pcr_values: Default::default(),
        enclave_pubkey: String::new(),
        report_data: String::new(),
        signature: String::new(),
        is_stub: true,
    })
}

/// GET /v1/health
///
/// Returns 200 when one atomic sequencer snapshot is available and canonical
/// writes are enabled. Returns 503 when the actor is unavailable or integrity-
/// halted. Height, genesis hash, and halt state are never assembled from
/// separate mailbox reads.
/// Downstream services and Docker healthchecks should treat any non-200 as
/// unhealthy and stop routing traffic.
#[utoipa::path(
    tag = "routessystem",
    get,
    path = "/v1/health",
    responses(
        (status = 200, description = "Atomic sequencer health and chain-identity snapshot", body = HealthResponse),
        (status = 503, description = "Sequencer unavailable, integrity-halted, or chain identity inconsistent", body = HealthResponse),
    )
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    match state.sequencer.get_operational_status().await {
        Ok(status) => operational_health(status),
        Err(err) => {
            tracing::warn!(error = %err, "health check: sequencer unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unhealthy".to_string(),
                    height: None,
                    genesis_hash: None,
                }),
            )
        }
    }
}

/// GET /v1/orders/policy
#[utoipa::path(
    tag = "routessystem",
    get,
    path = "/v1/orders/policy",
    responses(
        (status = 200, description = "Public order-construction admission policy", body = OrderAdmissionPolicyResponse)
    )
)]
pub async fn order_admission_policy(
    State(state): State<AppState>,
) -> Json<OrderAdmissionPolicyResponse> {
    Json(OrderAdmissionPolicyResponse {
        min_order_notional_nanos: state.min_order_notional_nanos,
        share_scale: matching_engine::SHARE_SCALE,
    })
}

fn operational_health(
    status: matching_sequencer::SequencerOperationalStatus,
) -> (StatusCode, Json<HealthResponse>) {
    if status.integrity_halted {
        tracing::error!(
            height = ?status.committed_height,
            "health check: sequencer integrity halted"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "integrity_halted".to_string(),
                height: status.committed_height,
                genesis_hash: status.genesis_hash.map(hex::encode),
            }),
        );
    }

    if matches!(
        (status.committed_height, status.genesis_hash),
        (None, None) | (Some(_), Some(_))
    ) {
        return (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok".to_string(),
                height: status.committed_height,
                genesis_hash: status.genesis_hash.map(hex::encode),
            }),
        );
    }

    tracing::error!(
        height = ?status.committed_height,
        genesis_hash = ?status.genesis_hash,
        "health check: inconsistent chain identity"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(HealthResponse {
            status: "unhealthy".to_string(),
            height: None,
            genesis_hash: None,
        }),
    )
}

/// GET /v1/state-root
#[utoipa::path(
    tag = "routessystem",
    get,
    path = "/v1/state-root",
    responses(
        (status = 200, description = "Current state root", body = StateRootResponse)
    )
)]
pub async fn state_root(
    State(state): State<AppState>,
) -> Result<Json<StateRootResponse>, AppError> {
    let root = state.sequencer.get_state_root().await?;
    Ok(Json(StateRootResponse {
        state_root: hex::encode(root),
    }))
}

/// POST /v1/simulation/pause
///
/// Dev-mode only: pauses block production. Returns 403 outside dev mode.
#[utoipa::path(
    tag = "routessystem",
    post,
    path = "/v1/simulation/pause",
    responses(
        (status = 200, description = "Block production paused", body = serde_json::Value),
        (status = 403, description = "Dev mode required"),
    )
)]
pub async fn pause(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    state.sequencer.pause_block_production().await?;
    Ok(Json(serde_json::json!({"status": "paused"})))
}

/// POST /v1/simulation/resume
///
/// Dev-mode only: resumes block production. Returns 403 outside dev mode.
#[utoipa::path(
    tag = "routessystem",
    post,
    path = "/v1/simulation/resume",
    responses(
        (status = 200, description = "Block production resumed", body = serde_json::Value),
        (status = 403, description = "Dev mode required"),
    )
)]
pub async fn resume(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    state.sequencer.resume_block_production().await?;
    Ok(Json(serde_json::json!({"status": "resumed"})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halted_health_is_unavailable_but_preserves_committed_diagnostics() {
        let (status, Json(body)) =
            operational_health(matching_sequencer::SequencerOperationalStatus {
                committed_height: Some(7),
                genesis_hash: Some([0xab; 32]),
                integrity_halted: true,
            });

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.status, "integrity_halted");
        assert_eq!(body.height, Some(7));
        assert_eq!(body.genesis_hash, Some("ab".repeat(32)));
    }
}
