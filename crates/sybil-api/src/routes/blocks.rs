use axum::extract::{Path, State};
use axum::Json;
use axum::response::sse::{Event, Sse};

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
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
    AppError,
> {
    crate::sse::block_stream(&state.sequencer).await
}
