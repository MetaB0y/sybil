use axum::Json;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use tokio::sync::OwnedSemaphorePermit;

use matching_sequencer::MAX_BLOCK_REPLAY_QUERY_BLOCKS;

use crate::convert::public_block_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{ApiErrorResponse, PublicBlockResponse};

const DEFAULT_BLOCK_REPLAY_QUERY_BLOCKS: usize = 20;

pub(crate) struct PublicStreamPermit {
    _permit: OwnedSemaphorePermit,
    transport: &'static str,
}

impl PublicStreamPermit {
    fn try_acquire(state: &AppState, transport: &'static str) -> Result<Self, AppError> {
        match state
            .http_public_stream_concurrency
            .clone()
            .try_acquire_owned()
        {
            Ok(permit) => {
                metrics::gauge!(
                    "sybil_public_stream_connections",
                    "transport" => transport
                )
                .increment(1.0);
                Ok(Self {
                    _permit: permit,
                    transport,
                })
            }
            Err(_) => {
                metrics::counter!(
                    "sybil_public_stream_connections_rejected_total",
                    "transport" => transport
                )
                .increment(1);
                Err(AppError::rate_limited(1))
            }
        }
    }
}

impl Drop for PublicStreamPermit {
    fn drop(&mut self) {
        metrics::gauge!(
            "sybil_public_stream_connections",
            "transport" => self.transport
        )
        .decrement(1.0);
    }
}

#[derive(serde::Deserialize)]
pub struct RecentBlocksQuery {
    limit: Option<usize>,
    before_height: Option<u64>,
}

/// GET /v1/blocks?limit=N&before_height=H — blocks newest-first, paged by height.
#[utoipa::path(
    tag = "routesblocks",
    get,
    path = "/v1/blocks",
    params(
        ("limit" = Option<usize>, Query, description = "Recent blocks, newest-first; default 20, cap 500"),
        ("before_height" = Option<u64>, Query, description = "Return blocks with height strictly below this cursor")
    ),
    responses((status = 200, description = "Public block market tape, newest-first", body = [PublicBlockResponse]))
)]
pub async fn get_recent_blocks(
    State(state): State<AppState>,
    Query(q): Query<RecentBlocksQuery>,
) -> Result<Json<Vec<PublicBlockResponse>>, AppError> {
    let limit = q
        .limit
        .unwrap_or(DEFAULT_BLOCK_REPLAY_QUERY_BLOCKS)
        .min(MAX_BLOCK_REPLAY_QUERY_BLOCKS);
    let blocks = state
        .sequencer
        .get_block_page(q.before_height, limit)
        .await?;
    Ok(Json(blocks.iter().map(public_block_to_response).collect()))
}

/// GET /v1/blocks/latest
#[utoipa::path(
    tag = "routesblocks",
    get,
    path = "/v1/blocks/latest",
    responses(
        (status = 200, description = "Latest public block market tape", body = PublicBlockResponse),
        (status = 404, description = "No blocks produced yet")
    )
)]
pub async fn get_latest_block(
    State(state): State<AppState>,
) -> Result<Json<PublicBlockResponse>, AppError> {
    let block = state
        .sequencer
        .get_latest_block()
        .await?
        .ok_or_else(|| AppError::not_found("No blocks produced yet"))?;
    Ok(Json(public_block_to_response(&block)))
}

/// GET /v1/blocks/{height}
#[utoipa::path(
    tag = "routesblocks",
    get,
    path = "/v1/blocks/{height}",
    params(("height" = u64, Path, description = "Block height")),
    responses(
        (status = 200, description = "Public block market tape at height", body = PublicBlockResponse),
        (status = 404, description = "Block not found"),
        (status = 410, description = "Block predates retained history", body = ApiErrorResponse)
    )
)]
pub async fn get_block_by_height(
    State(state): State<AppState>,
    Path(height): Path<u64>,
) -> Result<Json<PublicBlockResponse>, AppError> {
    let block = state.sequencer.get_block(height).await?;
    Ok(Json(public_block_to_response(&block)))
}

/// GET /v2/blocks/ws
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
    tag = "routesblocks",
    get,
    path = "/v2/blocks/ws",
    description = "Privacy-preserving public WebSocket block stream. Supports ?from_block=N replay and exposes only commitments, prices, aggregate analytics, and sanitized market lifecycle.",
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
    let permit = match PublicStreamPermit::try_acquire(&state, "websocket") {
        Ok(permit) => permit,
        Err(error) => return axum::response::IntoResponse::into_response(error),
    };
    ws.on_upgrade(move |socket| async move {
        let _permit = permit;
        crate::ws::handle_block_ws(
            socket,
            &state.sequencer,
            query,
            crate::ws::BlockStreamVisibility::Public,
            state.ws_client_idle_timeout,
        )
        .await;
    })
}

/// GET /v1/blocks/ws — authenticated canonical service stream.
#[utoipa::path(
    tag = "routesblocks",
    get,
    path = "/v1/blocks/ws",
    description = "Authenticated service WebSocket stream containing the full canonical block response.",
    params(
        ("from_block" = Option<u64>, Query, description = "Replay retained committed blocks from this height before switching to live")
    ),
    responses(
        (status = 101, description = "Authenticated service WebSocket upgrade")
    ),
    security(("bearer_service" = []))
)]
pub async fn ws_service_blocks(
    ws: WebSocketUpgrade,
    Query(query): Query<crate::ws::WsQuery>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        crate::ws::handle_block_ws(
            socket,
            &state.sequencer,
            query,
            crate::ws::BlockStreamVisibility::Service,
            state.ws_client_idle_timeout,
        )
        .await;
    })
}
