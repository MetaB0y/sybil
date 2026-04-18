use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::Json;

use crate::convert::block_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::BlockResponse;

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
#[utoipa::path(
    get,
    path = "/v1/blocks/stream",
    responses(
        (status = 200, description = "SSE stream of block events")
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
    params(
        ("from_block" = Option<u64>, Query, description = "Replay from this block height")
    ),
    responses(
        (status = 101, description = "WebSocket upgrade for block streaming")
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
