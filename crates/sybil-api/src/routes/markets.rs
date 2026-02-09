use axum::extract::{Path, Query, State};
use axum::Json;

use matching_engine::{MarketId, NANOS_PER_DOLLAR};
use matching_sequencer::MarketMetadata;

use crate::convert::prices_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    CreateMarketGroupRequest, CreateMarketRequest, MarketSearchParams, ResolveMarketRequest,
};
use crate::types::response::*;

/// Helper to build a MarketResponse with optional metadata.
fn build_market_response(
    market_id: u32,
    name: String,
    yes_price: Option<u64>,
    no_price: Option<u64>,
    status: &matching_sequencer::MarketStatus,
    metadata: Option<&MarketMetadata>,
    volume: u64,
) -> MarketResponse {
    MarketResponse {
        market_id,
        name,
        yes_price_nanos: yes_price,
        no_price_nanos: no_price,
        status: status.as_str().to_string(),
        payout_nanos: status.payout_nanos(),
        challenge_deadline_ms: status.challenge_deadline_ms(),
        description: metadata
            .map(|m| m.description.clone())
            .filter(|s| !s.is_empty()),
        category: metadata
            .map(|m| m.category.clone())
            .filter(|s| !s.is_empty()),
        tags: metadata.map(|m| m.tags.clone()).filter(|v| !v.is_empty()),
        resolution_criteria: metadata
            .map(|m| m.resolution_criteria.clone())
            .filter(|s| !s.is_empty()),
        expiry_timestamp_ms: metadata.map(|m| m.expiry_timestamp_ms).filter(|&v| v != 0),
        created_at_ms: metadata.map(|m| m.created_at_ms).filter(|&v| v != 0),
        volume_nanos: volume,
    }
}

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

    let mut response = Vec::new();
    for m in markets.iter() {
        let market_prices = prices.get(&m.id);
        let status = statuses
            .get(&m.id)
            .cloned()
            .unwrap_or(matching_sequencer::MarketStatus::Active);
        let metadata = state.sequencer.get_market_metadata(m.id).await?;

        response.push(build_market_response(
            m.id.0,
            m.name.clone(),
            market_prices.and_then(|p| p.first().copied()),
            market_prices.and_then(|p| p.get(1).copied()),
            &status,
            metadata.as_ref(),
            0, // volume not tracked in list (would need separate query)
        ));
    }

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
    let metadata = state.sequencer.get_market_metadata(mid).await?;

    Ok(Json(build_market_response(
        market.id.0,
        market.name.clone(),
        market_prices.and_then(|p| p.first().copied()),
        market_prices.and_then(|p| p.get(1).copied()),
        &status,
        metadata.as_ref(),
        0,
    )))
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

    let has_metadata = req.description.is_some()
        || req.category.is_some()
        || req.tags.is_some()
        || req.resolution_criteria.is_some()
        || req.expiry_timestamp_ms.is_some();

    let market_id = if has_metadata {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let metadata = MarketMetadata {
            description: req.description.unwrap_or_default(),
            category: req.category.unwrap_or_default(),
            tags: req.tags.unwrap_or_default(),
            resolution_criteria: req.resolution_criteria.unwrap_or_default(),
            expiry_timestamp_ms: req.expiry_timestamp_ms.unwrap_or(0),
            created_at_ms: now_ms,
        };
        state
            .sequencer
            .create_market_with_metadata(req.name.clone(), metadata)
            .await?
    } else {
        state.sequencer.create_market(req.name.clone()).await?
    };

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

/// GET /v1/markets/{id}/prices/history
#[utoipa::path(
    get,
    path = "/v1/markets/{id}/prices/history",
    params(
        ("id" = u32, Path, description = "Market ID"),
        ("from_ms" = Option<u64>, Query, description = "Start timestamp filter"),
        ("to_ms" = Option<u64>, Query, description = "End timestamp filter"),
    ),
    responses(
        (status = 200, description = "Price history", body = PriceHistoryResponse)
    )
)]
pub async fn get_price_history(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<PriceHistoryParams>,
) -> Result<Json<PriceHistoryResponse>, AppError> {
    let mid = MarketId::new(id);
    let points = state
        .sequencer
        .get_price_history(mid, params.from_ms, params.to_ms)
        .await?;

    let response = PriceHistoryResponse {
        market_id: id,
        points: points
            .into_iter()
            .map(|p| PricePointResponse {
                height: p.height,
                timestamp_ms: p.timestamp_ms,
                yes_price_nanos: p.yes_price,
                no_price_nanos: p.no_price,
                volume_nanos: p.volume_nanos,
            })
            .collect(),
    };

    Ok(Json(response))
}

#[derive(Debug, serde::Deserialize)]
pub struct PriceHistoryParams {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
}

/// GET /v1/markets/search
#[utoipa::path(
    get,
    path = "/v1/markets/search",
    params(
        ("q" = Option<String>, Query, description = "Text search"),
        ("tags" = Option<String>, Query, description = "Comma-separated tags"),
        ("category" = Option<String>, Query, description = "Category filter"),
        ("status" = Option<String>, Query, description = "Status filter"),
        ("min_volume" = Option<u64>, Query, description = "Minimum volume"),
        ("sort" = Option<String>, Query, description = "Sort field"),
        ("limit" = Option<usize>, Query, description = "Result limit"),
        ("offset" = Option<usize>, Query, description = "Result offset"),
    ),
    responses(
        (status = 200, description = "Search results", body = Vec<MarketResponse>)
    )
)]
pub async fn search_markets(
    State(state): State<AppState>,
    Query(params): Query<MarketSearchParams>,
) -> Result<Json<Vec<MarketResponse>>, AppError> {
    use matching_sequencer::{MarketSearchQuery, MarketSortField};

    let sort_by = params.sort.as_deref().map(|s| match s {
        "volume" => MarketSortField::Volume,
        "created_at" => MarketSortField::CreatedAt,
        "name" => MarketSortField::Name,
        "price" => MarketSortField::Price,
        _ => MarketSortField::Volume,
    });

    let tags = params
        .tags
        .as_ref()
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let query = MarketSearchQuery {
        text: params.q,
        tags,
        category: params.category,
        status: params.status,
        min_yes_price: params.min_yes_price,
        max_yes_price: params.max_yes_price,
        min_volume: params.min_volume,
        sort_by,
        limit: params.limit,
        offset: params.offset,
    };

    let results = state.sequencer.search_markets(query).await?;

    let response: Vec<MarketResponse> = results
        .into_iter()
        .map(|r| {
            build_market_response(
                r.market_id.0,
                r.name,
                r.yes_price_nanos,
                r.no_price_nanos,
                &r.status,
                r.metadata.as_ref(),
                r.volume_nanos,
            )
        })
        .collect();

    Ok(Json(response))
}
