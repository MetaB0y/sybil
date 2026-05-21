use std::path::{Path as FsPath, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::state::AppState;
use crate::types::error::AppError;

/// Resolve `{dir}/{event_id}.json`, rejecting ids that could escape the dir.
fn snapshot_path(dir: &FsPath, event_id: &str) -> Option<PathBuf> {
    let safe = !event_id.is_empty()
        && event_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    safe.then(|| dir.join(format!("{event_id}.json")))
}

/// PUT /v1/events/{event_id}/raw — store the full Polymarket event JSON.
/// Dev-mode only (mirrors the metadata push). Body must be valid JSON.
pub async fn put_event_raw(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let dir = state
        .event_snapshot_dir
        .as_ref()
        .ok_or_else(|| AppError::not_found("event snapshots disabled"))?;
    let path =
        snapshot_path(dir, &event_id).ok_or_else(|| AppError::bad_request("invalid event_id"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| AppError::bad_request(format!("body is not JSON: {e}")))?;
    tokio::fs::write(&path, &body)
        .await
        .map_err(|e| AppError::internal(format!("snapshot write failed: {e}")))?;
    Ok(Json(serde_json::json!({ "stored": true })))
}

/// GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
/// Readable in any mode (only the PUT is dev-mode gated) so the frontend can
/// fetch snapshots without dev mode.
pub async fn get_event_raw(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
) -> Result<Response, AppError> {
    let dir = state
        .event_snapshot_dir
        .as_ref()
        .ok_or_else(|| AppError::not_found("event snapshots disabled"))?;
    let path =
        snapshot_path(dir, &event_id).ok_or_else(|| AppError::bad_request("invalid event_id"))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::not_found("event snapshot not found"))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bytes,
    )
        .into_response())
}
