use axum::extract::{Path, State};
use axum::Json;

use matching_engine::{MarketId, NANOS_PER_DOLLAR};

use crate::convert::prices_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{CreateMarketGroupRequest, CreateMarketRequest, ResolveMarketRequest};
use crate::types::response::*;

/// GET /v1/markets
#[utoipa::path(
    get,
    path = "/v1/markets",
    responses(
        (status = 200, description = "List of markets", body = Vec<MarketResponse>)
    )
)]
pub async fn list_markets(
    State(state): State<AppState>,
) -> Result<Json<Vec<MarketResponse>>, AppError> {
    let markets = state.sequencer.list_markets().await?;
    let prices = state.sequencer.get_market_prices().await?;
    let statuses = state.sequencer.get_all_market_statuses().await?;

    let response: Vec<MarketResponse> = markets
        .iter()
        .map(|m| {
            let market_prices = prices.get(&m.id);
            let status = statuses
                .get(&m.id)
                .cloned()
                .unwrap_or(matching_sequencer::MarketStatus::Active);
            MarketResponse {
                market_id: m.id.0,
                name: m.name.clone(),
                yes_price_nanos: market_prices.and_then(|p| p.first().copied()),
                no_price_nanos: market_prices.and_then(|p| p.get(1).copied()),
                status: status.as_str().to_string(),
                payout_nanos: status.payout_nanos(),
                challenge_deadline_ms: status.challenge_deadline_ms(),
            }
        })
        .collect();

    Ok(Json(response))
}

/// GET /v1/markets/{id}
#[utoipa::path(
    get,
    path = "/v1/markets/{id}",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Market details", body = MarketResponse),
        (status = 404, description = "Market not found")
    )
)]
pub async fn get_market(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<MarketResponse>, AppError> {
    let markets = state.sequencer.list_markets().await?;
    let mid = MarketId::new(id);
    let market = markets
        .get(mid)
        .ok_or_else(|| AppError::not_found(format!("Market {} not found", id)))?;

    let prices = state.sequencer.get_market_prices().await?;
    let market_prices = prices.get(&mid);
    let status = state.sequencer.get_market_status(mid).await?;

    Ok(Json(MarketResponse {
        market_id: market.id.0,
        name: market.name.clone(),
        yes_price_nanos: market_prices.and_then(|p| p.first().copied()),
        no_price_nanos: market_prices.and_then(|p| p.get(1).copied()),
        status: status.as_str().to_string(),
        payout_nanos: status.payout_nanos(),
        challenge_deadline_ms: status.challenge_deadline_ms(),
    }))
}

/// POST /v1/markets
#[utoipa::path(
    post,
    path = "/v1/markets",
    request_body = CreateMarketRequest,
    responses(
        (status = 200, description = "Market created", body = CreateMarketResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn create_market(
    State(state): State<AppState>,
    Json(req): Json<CreateMarketRequest>,
) -> Result<Json<CreateMarketResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let market_id = state.sequencer.create_market(req.name.clone()).await?;
    Ok(Json(CreateMarketResponse {
        market_id: market_id.0,
        name: req.name,
    }))
}

/// GET /v1/markets/groups
#[utoipa::path(
    get,
    path = "/v1/markets/groups",
    responses(
        (status = 200, description = "List of market groups", body = Vec<MarketGroupResponse>)
    )
)]
pub async fn list_market_groups(
    State(state): State<AppState>,
) -> Result<Json<Vec<MarketGroupResponse>>, AppError> {
    let groups = state.sequencer.list_market_groups().await?;
    let response: Vec<MarketGroupResponse> = groups
        .iter()
        .map(|g| MarketGroupResponse {
            name: g.name.clone(),
            market_ids: g.markets.iter().map(|m| m.0).collect(),
        })
        .collect();
    Ok(Json(response))
}

/// POST /v1/markets/groups
#[utoipa::path(
    post,
    path = "/v1/markets/groups",
    request_body = CreateMarketGroupRequest,
    responses(
        (status = 200, description = "Market group created", body = MarketGroupResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn create_market_group(
    State(state): State<AppState>,
    Json(req): Json<CreateMarketGroupRequest>,
) -> Result<Json<MarketGroupResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let market_ids: Vec<MarketId> = req.market_ids.iter().map(|&id| MarketId::new(id)).collect();
    let group = state
        .sequencer
        .create_market_group(req.name, market_ids)
        .await?;

    Ok(Json(MarketGroupResponse {
        name: group.name,
        market_ids: group.markets.iter().map(|m| m.0).collect(),
    }))
}

/// GET /v1/markets/prices
#[utoipa::path(
    get,
    path = "/v1/markets/prices",
    responses(
        (status = 200, description = "Market prices", body = MarketPricesResponse)
    )
)]
pub async fn get_prices(
    State(state): State<AppState>,
) -> Result<Json<MarketPricesResponse>, AppError> {
    let prices = state.sequencer.get_market_prices().await?;
    Ok(Json(prices_to_response(&prices)))
}

/// POST /v1/markets/{id}/resolve
#[utoipa::path(
    post,
    path = "/v1/markets/{id}/resolve",
    params(("id" = u32, Path, description = "Market ID")),
    request_body = ResolveMarketRequest,
    responses(
        (status = 200, description = "Market resolved", body = ResolveMarketResponse),
        (status = 403, description = "Dev mode required"),
        (status = 404, description = "Market not found")
    )
)]
pub async fn resolve_market(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(req): Json<ResolveMarketRequest>,
) -> Result<Json<ResolveMarketResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    if req.payout_nanos > NANOS_PER_DOLLAR {
        return Err(AppError::bad_request(format!(
            "Payout must be between 0 and {} nanos, got {}",
            NANOS_PER_DOLLAR, req.payout_nanos
        )));
    }

    let mid = MarketId::new(id);
    let _record = state
        .sequencer
        .resolve_market(mid, req.payout_nanos)
        .await?;

    let status = state.sequencer.get_market_status(mid).await?;

    Ok(Json(ResolveMarketResponse {
        market_id: id,
        payout_nanos: req.payout_nanos,
        status: status.as_str().to_string(),
        challenge_deadline_ms: status.challenge_deadline_ms(),
    }))
}
