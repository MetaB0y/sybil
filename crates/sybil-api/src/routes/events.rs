use std::io::Write;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::State;
use axum::response::{IntoResponse, Response};

use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::types::error::AppError;

/// Resolve `{dir}/{event_id}.json`, rejecting ids that could escape the dir.
fn snapshot_path(dir: &FsPath, event_id: &str) -> Option<PathBuf> {
    let safe = !event_id.is_empty()
        && event_id.len() <= 128
        && event_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    safe.then(|| dir.join(format!("{event_id}.json")))
}

/// Durably persist `body` as the snapshot at `path` (SYB-153).
///
/// Refresh semantics: the file is named by source event id, so each mirror
/// cycle overwrites in place (idempotent upsert); the file's mtime is the
/// durable last-updated timestamp / version marker. The write is atomic — we
/// write a uniquely-named temp file in the same directory and `rename` it over
/// the target — so a crash or restart mid-write can never leave a torn/partial
/// snapshot: readers see either the old snapshot or the fully-written new one.
async fn store_snapshot(path: &FsPath, body: &[u8]) -> std::io::Result<()> {
    let path = path.to_path_buf();
    let body = body.to_vec();
    tokio::task::spawn_blocking(move || store_snapshot_blocking(&path, &body))
        .await
        .map_err(|error| std::io::Error::other(format!("snapshot writer task failed: {error}")))?
}

fn store_snapshot_blocking(path: &FsPath, body: &[u8]) -> std::io::Result<()> {
    static TEMP_NONCE: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| FsPath::new("."));
    std::fs::create_dir_all(parent)?;
    // Timestamp, process id, and monotonic process-local nonce avoid collisions
    // with overlapping PUTs and stale temp files left by a prior process.
    let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{timestamp_nanos}.{}.{nonce}.tmp",
        std::process::id()
    ));
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        sync_parent_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn sync_parent_directory(parent: &FsPath) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &FsPath) -> std::io::Result<()> {
    Ok(())
}

/// PUT /v1/events/{event_id}/raw — store the full Polymarket event JSON.
/// Service/operator route. Body must be valid JSON.
#[utoipa::path(
    tag = "routesevents",
    put,
    path = "/v1/events/{event_id}/raw",
    params(("event_id" = String, Path, pattern = r"^[A-Za-z0-9_-]+$", max_length = 128, description = "Event identifier (alphanumeric, '_' or '-')")),
    request_body(content = serde_json::Value, description = "Raw event JSON snapshot", content_type = "application/json"),
    responses(
        (status = 200, description = "Snapshot stored", body = serde_json::Value),
        (status = 400, description = "Invalid event_id or non-JSON body"),
        (status = 401, description = "Missing service bearer token"),
        (status = 403, description = "Invalid service bearer token"),
        (status = 404, description = "Event snapshots disabled"),
    )
)]
pub async fn put_event_raw(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let dir = state
        .event_snapshot_dir
        .as_ref()
        .ok_or_else(|| AppError::not_found("event snapshots disabled"))?;
    let path =
        snapshot_path(dir, &event_id).ok_or_else(|| AppError::bad_request("invalid event_id"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| AppError::bad_request(format!("body is not JSON: {e}")))?;
    store_snapshot(&path, &body)
        .await
        .map_err(|e| AppError::internal(format!("snapshot write failed: {e}")))?;
    Ok(Json(serde_json::json!({ "stored": true })))
}

/// GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
/// Readable in any mode (only the PUT is dev-mode gated) so the frontend can
/// fetch snapshots without dev mode. Public read route.
#[utoipa::path(
    tag = "routesevents",
    get,
    path = "/v1/events/{event_id}/raw",
    params(("event_id" = String, Path, pattern = r"^[A-Za-z0-9_-]+$", max_length = 128, description = "Event identifier (alphanumeric, '_' or '-')")),
    responses(
        (status = 200, description = "Stored raw event JSON", body = serde_json::Value),
        (status = 400, description = "Invalid event_id"),
        (status = 404, description = "Snapshot not found or snapshots disabled"),
    )
)]
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
