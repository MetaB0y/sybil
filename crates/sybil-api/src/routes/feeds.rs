use axum::Json;
use axum::extract::State;

use sybil_oracle::FeedPubkey;

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::RegisterFeedRequest;
use crate::types::response::RegisteredFeedResponse;

fn feed_to_response(feed: &sybil_oracle::DataFeed) -> RegisteredFeedResponse {
    RegisteredFeedResponse {
        feed_id: feed.id.0,
        pubkey_hex: hex::encode(&feed.pubkey.0),
        name: feed.name.clone(),
        created_at_ms: feed.created_at_ms,
    }
}

/// POST /v1/feeds — register a data feed.
#[utoipa::path(
    tag = "routesfeeds",
    post,
    path = "/v1/feeds",
    request_body = RegisterFeedRequest,
    responses(
        (status = 200, description = "Feed registered", body = RegisteredFeedResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn register_feed(
    State(state): State<AppState>,
    Json(req): Json<RegisterFeedRequest>,
) -> Result<Json<RegisteredFeedResponse>, AppError> {
    let pubkey_bytes = hex::decode(
        req.pubkey_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
    )
    .map_err(|_| AppError::bad_request("Invalid pubkey_hex"))?;
    if pubkey_bytes.len() != 33 {
        return Err(AppError::bad_request(
            "Pubkey must be 33 bytes (compressed SEC1)",
        ));
    }
    let pubkey = FeedPubkey(pubkey_bytes);
    // Reject name conflicts before delegating to the idempotent register
    // path on the sequencer — same-pubkey/same-name re-registration is fine
    // (returns the existing feed), but same-pubkey with a *different* name
    // would silently resolve to the old identity and confuse operators.
    if let Some(existing) = state.sequencer.get_feed_by_pubkey(pubkey.clone()).await? {
        if existing.name != req.name {
            return Err(AppError::conflict(format!(
                "pubkey already registered as feed {} with name {:?}",
                existing.id.0, existing.name
            )));
        }
        return Ok(Json(feed_to_response(&existing)));
    }
    let feed_id = state.sequencer.register_feed(pubkey, req.name).await?;
    let feed = state
        .sequencer
        .get_feed(feed_id)
        .await?
        .ok_or_else(|| AppError::internal("feed registration succeeded but lookup failed"))?;
    Ok(Json(feed_to_response(&feed)))
}

/// GET /v1/feeds — list registered data feeds.
#[utoipa::path(
    tag = "routesfeeds",
    get,
    path = "/v1/feeds",
    responses(
        (status = 200, description = "Feeds", body = Vec<RegisteredFeedResponse>)
    )
)]
pub async fn list_feeds(
    State(state): State<AppState>,
) -> Result<Json<Vec<RegisteredFeedResponse>>, AppError> {
    let feeds = state.sequencer.list_feeds().await?;
    Ok(Json(feeds.iter().map(feed_to_response).collect()))
}
