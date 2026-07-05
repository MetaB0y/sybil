use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::Json;

use crate::convert::block_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::BlockResponse;

#[derive(serde::Deserialize)]
pub struct RecentBlocksQuery {
    limit: Option<usize>,
}

/// GET /v1/blocks?limit=N — last N blocks, newest-first, from in-memory history.
#[utoipa::path(
    get,
    path = "/v1/blocks",
    params(("limit" = Option<usize>, Query, description = "Recent blocks, newest-first; clamped to history capacity (default 20)")),
    responses((status = 200, description = "Recent blocks, newest-first", body = [BlockResponse]))
)]
pub async fn get_recent_blocks(
    State(state): State<AppState>,
    Query(q): Query<RecentBlocksQuery>,
) -> Result<Json<Vec<BlockResponse>>, AppError> {
    let limit = q.limit.unwrap_or(20);
    let blocks = state.sequencer.get_recent_blocks(limit).await?;
    Ok(Json(blocks.iter().map(block_to_response).collect()))
}

/// GET /v1/blocks/latest
#[utoipa::path(
    get,
    path = "/v1/blocks/latest",
    responses(
        (status = 200, description = "Latest block", body = BlockResponse),
        (status = 404, description = "No blocks produced yet")
    )
)]
pub async fn get_latest_block(
    State(state): State<AppState>,
) -> Result<Json<BlockResponse>, AppError> {
    let block = state
        .sequencer
        .get_latest_block()
        .await?
        .ok_or_else(|| AppError::not_found("No blocks produced yet"))?;
    Ok(Json(block_to_response(&block)))
}

/// GET /v1/blocks/{height}
#[utoipa::path(
    get,
    path = "/v1/blocks/{height}",
    params(("height" = u64, Path, description = "Block height")),
    responses(
        (status = 200, description = "Block at height", body = BlockResponse),
        (status = 404, description = "Block not found")
    )
)]
pub async fn get_block_by_height(
    State(state): State<AppState>,
    Path(height): Path<u64>,
) -> Result<Json<BlockResponse>, AppError> {
    let block = state.sequencer.get_block(height).await?;
    Ok(Json(block_to_response(&block)))
}

/// GET /v1/blocks/stream
///
/// Third-party convenience SSE stream. First-party clients should use
/// `GET /v1/blocks/ws?from_block=N` for versioned replay/resume and explicit
/// lag/retention-gap signalling.
#[utoipa::path(
    get,
    path = "/v1/blocks/stream",
    description = "Third-party convenience SSE stream of block events. First-party clients should use GET /v1/blocks/ws?from_block=N for replay/resume, versioned envelopes, and lag/retention-gap signalling.",
    responses(
        (status = 200, description = "Third-party convenience SSE stream of block events")
    )
)]
pub async fn stream_blocks(
    State(state): State<AppState>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, AppError>
{
    crate::sse::block_stream(&state.sequencer).await
}

/// GET /v1/blocks/ws
///
/// WebSocket stream of committed blocks. See
/// `docs/architecture/WebSocket Block Stream.md` for the message schema,
/// backpressure policy, and reconnect semantics.
///
/// Query parameters:
/// - `from_block=<height>` — replay every block from `height` up to the
///   current head before switching to live. Used by clients to resume
///   after a `lagged` close without gaps.
#[utoipa::path(
    get,
    path = "/v1/blocks/ws",
    description = "First-party WebSocket block stream. Supports ?from_block=N to replay retained committed blocks from that height before following live blocks. If from_block is below the retained blocks_full floor, the stream emits a retention_gap envelope and closes so clients can cold-resync.",
    params(
        ("from_block" = Option<u64>, Query, description = "Replay retained committed blocks from this height before switching to live")
    ),
    responses(
        (status = 101, description = "First-party WebSocket upgrade for block streaming")
    )
)]
pub async fn ws_blocks(
    ws: WebSocketUpgrade,
    Query(query): Query<crate::ws::WsQuery>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        crate::ws::handle_block_ws(socket, &state.sequencer, query).await;
    })
}
