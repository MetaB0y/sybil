use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json;

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{HealthResponse, StateRootResponse};

/// GET /v1/health
///
/// Returns 200 when the sequencer is running, 503 when it is unavailable.
/// Downstream services and Docker healthchecks should treat any non-200 as
/// unhealthy and stop routing traffic.
#[utoipa::path(
    get,
    path = "/v1/health",
    responses(
        (status = 200, description = "Sequencer healthy", body = HealthResponse),
        (status = 503, description = "Sequencer unavailable", body = HealthResponse),
    )
)]
pub async fn health(
    State(state): State<AppState>,
) -> (StatusCode, Json<HealthResponse>) {
    match state.sequencer.get_latest_block().await {
        Ok(block) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok".to_string(),
                height: block.map(|b| b.header.height),
            }),
        ),
        Err(err) => {
            tracing::warn!(error = %err, "health check: sequencer unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unhealthy".to_string(),
                    height: None,
                }),
            )
        }
    }
}

/// GET /v1/state-root
#[utoipa::path(
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
pub async fn pause(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    state.sequencer.pause_block_production().await?;
    Ok(Json(serde_json::json!({"status": "paused"})))
}

/// POST /v1/simulation/resume
pub async fn resume(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    state.sequencer.resume_block_production().await?;
    Ok(Json(serde_json::json!({"status": "resumed"})))
}
