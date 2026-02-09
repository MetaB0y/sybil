use axum::extract::State;
use axum::Json;

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{HealthResponse, StateRootResponse};

/// GET /v1/health
#[utoipa::path(
    get,
    path = "/v1/health",
    responses(
        (status = 200, description = "System health", body = HealthResponse)
    )
)]
pub async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    let block = state.sequencer.get_latest_block().await?;
    Ok(Json(HealthResponse {
        status: "ok".to_string(),
        height: block.map(|b| b.header.height),
    }))
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
