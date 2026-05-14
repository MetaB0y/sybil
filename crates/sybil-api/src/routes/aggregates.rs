//! Aggregate-tracker endpoints (B1 onward).
//!
//! - `GET /v1/activity/overview` — platform totals (all-time + 24h).
//!   B1 populates `unique_traders`; volume and orders fill in via B2 / B6.
//! - `GET /v1/markets/{id}/open-batch` — open-batch state per market.
//!   `unique_placers` is real; indicative fields are stubbed by B1 and
//!   light up in C2.
//! - `GET /v1/events/{event_id}/traders` — per-event union of placers.

use axum::extract::{Path, State};
use axum::Json;
use matching_engine::MarketId;

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::*;

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// GET /v1/activity/overview
#[utoipa::path(
    get,
    path = "/v1/activity/overview",
    responses(
        (status = 200, description = "Platform-wide aggregates", body = ActivityOverviewResponse)
    )
)]
#[tracing::instrument(skip_all, name = "activity_overview.handler")]
pub async fn get_activity_overview(
    State(state): State<AppState>,
) -> Result<Json<ActivityOverviewResponse>, AppError> {
    let now_ms = now_unix_ms();
    let ((all_time_traders, traders_24h), (all_time_volume, volume_24h)) = tokio::try_join!(
        state.sequencer.get_platform_trader_counts(now_ms),
        state.sequencer.get_platform_volumes(now_ms),
    )?;

    Ok(Json(ActivityOverviewResponse {
        all_time: OverviewBucketResponse {
            unique_traders: all_time_traders as u64,
            total_volume_nanos: all_time_volume,
            ..Default::default()
        },
        last_24h: OverviewBucketResponse {
            unique_traders: traders_24h as u64,
            total_volume_nanos: volume_24h,
            ..Default::default()
        },
    }))
}

/// GET /v1/markets/{id}/open-batch
#[utoipa::path(
    get,
    path = "/v1/markets/{id}/open-batch",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Open-batch state for this market", body = OpenBatchResponse)
    )
)]
#[tracing::instrument(skip_all, name = "open_batch.handler", fields(market_id = id))]
pub async fn get_open_batch(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<OpenBatchResponse>, AppError> {
    let mid = MarketId::new(id);
    let unique_placers = state.sequencer.get_open_batch_placers(mid).await?;
    Ok(Json(OpenBatchResponse {
        unique_placers,
        // Indicative fields stay zero/None until the C2 scheduler ships.
        ..Default::default()
    }))
}

/// GET /v1/events/{event_id}/traders
#[utoipa::path(
    get,
    path = "/v1/events/{event_id}/traders",
    params(("event_id" = String, Path, description = "Polymarket event id")),
    responses(
        (status = 200, description = "Unique placers across the event's markets", body = EventTradersResponse)
    )
)]
#[tracing::instrument(skip_all, name = "event_traders.handler", fields(event_id = %event_id))]
pub async fn get_event_traders(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
) -> Result<Json<EventTradersResponse>, AppError> {
    // Resolve event_id → market_ids via the mirror metadata. If no markets
    // are mirrored under this event_id, return zero (the FE renders "—").
    let market_ids: Vec<MarketId> = {
        let ref_data = state.market_ref_data.read().await;
        ref_data
            .iter()
            .filter_map(|(sybil_id, data)| {
                data.event_id.as_deref().and_then(|eid| {
                    (eid == event_id).then_some(MarketId::new(*sybil_id))
                })
            })
            .collect()
    };

    if market_ids.is_empty() {
        return Ok(Json(EventTradersResponse { trader_count: 0 }));
    }

    let trader_count = state.sequencer.get_event_trader_count(market_ids).await?;
    Ok(Json(EventTradersResponse { trader_count }))
}
