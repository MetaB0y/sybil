use axum::extract::{Path, Query, State};
use axum::Json;

use matching_engine::{MarketId, NANOS_PER_DOLLAR};
use matching_sequencer::{MarketMetadata, ResolutionConfig};
use sybil_oracle::{FeedPubkey, ResolutionAttestation, SignedAttestation};

use crate::convert::prices_to_response;
use crate::state::{save_market_ref_data, AppState, MarketRefData};
use crate::types::error::AppError;
use crate::types::request::{
    CreateMarketGroupRequest, CreateMarketRequest, MarketSearchParams, ResolveMarketRequest,
    SetMarketMetadataRequest, SetReferencePricesRequest,
};
use crate::types::response::*;
use sybil_oracle::OracleSource;

struct BuildMarketResponseArgs<'a> {
    market_id: u32,
    name: String,
    yes_price_nanos: Option<u64>,
    no_price_nanos: Option<u64>,
    status: &'a matching_sequencer::MarketStatus,
    metadata: Option<&'a MarketMetadata>,
    volume_nanos: u64,
    reference_price_nanos: Option<u64>,
    /// Off-block reference data (Polymarket mirror metadata). When `Some`,
    /// its `category` field wins over `metadata.category` and its other
    /// fields pass through directly (no on-block equivalent).
    ref_data: Option<&'a MarketRefData>,
    /// All-time unique-trader count for this market (B1). Wear a
    /// `<RestartCaveatBadge />` until persistence is wired in prod.
    trader_count: u32,
    /// Rolling 24h volume for this market (B2). Same "since last restart"
    /// caveat as `trader_count`.
    volume_24h_nanos: u64,
    /// Clearing prices 24h ago (B3). `None` for markets too young to have
    /// a bucket bracketing `now - 24h`.
    price_24h_ago: Option<(u64, u64)>,
    /// Last-10-batch ±band depth average (B4). Zero for markets without a
    /// clearing price yet — FE falls back to "—" via existing format guard.
    liquidity_avg10_nanos: u64,
    /// Width of the band the liquidity score uses (B4). FE labels read
    /// "(±$0.0X)" from this value.
    liquidity_band_nanos: u64,
    /// All-time placed/matched/unmatched per market (B6). Cancels excluded.
    /// Same restart caveat as `trader_count` until persistence runs in prod.
    orders_placed_total: u64,
    orders_matched_total: u64,
    orders_unmatched_total: u64,
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Helper to build a MarketResponse with optional metadata.
fn build_market_response(args: BuildMarketResponseArgs<'_>) -> MarketResponse {
    // Merge rule: ref_data.category (mirror-derived) wins over metadata.category
    // (on-block, only set for sybil-native markets). Mirrored markets pass
    // `category: None` to `create_market` and receive the real category via
    // ref-data.
    let category = args.ref_data.and_then(|r| r.category.clone()).or_else(|| {
        args.metadata
            .map(|m| m.category.clone())
            .filter(|s| !s.is_empty())
    });

    MarketResponse {
        market_id: args.market_id,
        name: args.name,
        yes_price_nanos: args.yes_price_nanos,
        no_price_nanos: args.no_price_nanos,
        status: args.status.as_str().to_string(),
        payout_nanos: args.status.payout_nanos(),
        challenge_deadline_ms: args.status.challenge_deadline_ms(),
        description: args
            .metadata
            .map(|m| m.description.clone())
            .filter(|s| !s.is_empty()),
        category,
        tags: args
            .metadata
            .map(|m| m.tags.clone())
            .filter(|v| !v.is_empty()),
        resolution_criteria: args
            .metadata
            .map(|m| m.resolution_criteria.clone())
            .filter(|s| !s.is_empty()),
        expiry_timestamp_ms: args
            .metadata
            .map(|m| m.expiry_timestamp_ms)
            .filter(|&v| v != 0),
        created_at_ms: args.metadata.map(|m| m.created_at_ms).filter(|&v| v != 0),
        volume_nanos: args.volume_nanos,
        reference_price_nanos: args.reference_price_nanos,
        external_url: args.ref_data.and_then(|r| r.external_url.clone()),
        event_id: args.ref_data.and_then(|r| r.event_id.clone()),
        event_title: args.ref_data.and_then(|r| r.event_title.clone()),
        event_image_url: args.ref_data.and_then(|r| r.event_image_url.clone()),
        event_icon_url: args.ref_data.and_then(|r| r.event_icon_url.clone()),
        event_end_date_ms: args.ref_data.and_then(|r| r.event_end_date_ms),
        market_image_url: args.ref_data.and_then(|r| r.market_image_url.clone()),
        market_icon_url: args.ref_data.and_then(|r| r.market_icon_url.clone()),
        market_end_date_ms: args.ref_data.and_then(|r| r.market_end_date_ms),
        categories: args.ref_data.and_then(|r| r.categories.clone()),
        trader_count: args.trader_count,
        volume_24h_nanos: args.volume_24h_nanos,
        yes_price_24h_ago_nanos: args.price_24h_ago.map(|(y, _)| y),
        no_price_24h_ago_nanos: args.price_24h_ago.map(|(_, n)| n),
        liquidity_avg10_nanos: args.liquidity_avg10_nanos,
        liquidity_band_nanos: args.liquidity_band_nanos,
        orders_placed_total: args.orders_placed_total,
        orders_matched_total: args.orders_matched_total,
        orders_unmatched_total: args.orders_unmatched_total,
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
#[tracing::instrument(skip_all, name = "list_markets.handler")]
pub async fn list_markets(
    State(state): State<AppState>,
) -> Result<Json<Vec<MarketResponse>>, AppError> {
    use tracing::Instrument;
    let now_ms = now_unix_ms();
    let (
        markets,
        prices,
        statuses,
        volumes,
        metadata,
        trader_counts,
        volumes_24h,
        prices_24h_ago,
        liquidity,
        order_stats_by_market,
    ) = async {
        tokio::try_join!(
            state.sequencer.list_markets(),
            state.sequencer.get_market_prices(),
            state.sequencer.get_all_market_statuses(),
            state.sequencer.get_all_market_volumes(),
            state.sequencer.get_all_market_metadata(),
            state.sequencer.get_all_trader_counts(),
            state.sequencer.get_all_market_volumes_24h(now_ms),
            state.sequencer.get_all_market_prices_n_hours_ago(24, now_ms),
            state.sequencer.get_liquidity_snapshot(),
            state.sequencer.get_order_stats_by_market(),
        )
    }
    .instrument(tracing::info_span!("list_markets.fetch"))
    .await?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;

    let ref_prices = state.reference_prices.read().await;
    let market_ref_data = state.market_ref_data.read().await;

    let _build_span =
        tracing::info_span!("list_markets.build_response", markets = markets.len()).entered();
    let response: Vec<MarketResponse> = markets
        .iter()
        .map(|m| {
            let market_prices = prices.get(&m.id);
            let status = statuses
                .get(&m.id)
                .cloned()
                .unwrap_or(matching_sequencer::MarketStatus::Active);
            build_market_response(BuildMarketResponseArgs {
                market_id: m.id.0,
                name: m.name.clone(),
                yes_price_nanos: market_prices.and_then(|p| p.first().copied()),
                no_price_nanos: market_prices.and_then(|p| p.get(1).copied()),
                status: &status,
                metadata: metadata.get(&m.id),
                volume_nanos: volumes.get(&m.id).copied().unwrap_or(0),
                reference_price_nanos: ref_prices.get(&m.id.0).copied(),
                ref_data: market_ref_data.get(&m.id.0),
                trader_count: trader_counts.get(&m.id).copied().unwrap_or(0),
                volume_24h_nanos: volumes_24h.get(&m.id).copied().unwrap_or(0),
                price_24h_ago: prices_24h_ago.get(&m.id).copied(),
                liquidity_avg10_nanos: liquidity_by_market.get(&m.id).copied().unwrap_or(0),
                liquidity_band_nanos,
                orders_placed_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.placed)
                    .unwrap_or(0),
                orders_matched_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.matched)
                    .unwrap_or(0),
                orders_unmatched_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.unmatched)
                    .unwrap_or(0),
            })
        })
        .collect();

    Ok(Json(response))
}

/// GET /v1/markets/summary
///
/// Minimal market data for dashboard polling — drops metadata strings
/// (description, tags, resolution criteria, external URL). ~5-10x smaller
/// wire size than /v1/markets.
#[utoipa::path(
    get,
    path = "/v1/markets/summary",
    responses(
        (status = 200, description = "Slim list of markets", body = Vec<MarketSummaryResponse>)
    )
)]
#[tracing::instrument(skip_all, name = "list_markets_summary.handler")]
pub async fn list_markets_summary(
    State(state): State<AppState>,
) -> Result<Json<Vec<MarketSummaryResponse>>, AppError> {
    use tracing::Instrument;
    let now_ms = now_unix_ms();
    let (
        markets,
        prices,
        statuses,
        volumes,
        trader_counts,
        volumes_24h,
        prices_24h_ago,
        liquidity,
        order_stats_by_market,
    ) = async {
        tokio::try_join!(
            state.sequencer.list_markets(),
            state.sequencer.get_market_prices(),
            state.sequencer.get_all_market_statuses(),
            state.sequencer.get_all_market_volumes(),
            state.sequencer.get_all_trader_counts(),
            state.sequencer.get_all_market_volumes_24h(now_ms),
            state.sequencer.get_all_market_prices_n_hours_ago(24, now_ms),
            state.sequencer.get_liquidity_snapshot(),
            state.sequencer.get_order_stats_by_market(),
        )
    }
    .instrument(tracing::info_span!("list_markets_summary.fetch"))
    .await?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;

    let ref_prices = state.reference_prices.read().await;

    let _build_span = tracing::info_span!(
        "list_markets_summary.build_response",
        markets = markets.len()
    )
    .entered();
    let response: Vec<MarketSummaryResponse> = markets
        .iter()
        .map(|m| {
            let market_prices = prices.get(&m.id);
            let status = statuses
                .get(&m.id)
                .cloned()
                .unwrap_or(matching_sequencer::MarketStatus::Active);
            let price_24h = prices_24h_ago.get(&m.id).copied();
            MarketSummaryResponse {
                market_id: m.id.0,
                name: m.name.clone(),
                yes_price_nanos: market_prices.and_then(|p| p.first().copied()),
                no_price_nanos: market_prices.and_then(|p| p.get(1).copied()),
                reference_price_nanos: ref_prices.get(&m.id.0).copied(),
                volume_nanos: volumes.get(&m.id).copied().unwrap_or(0),
                status: status.as_str().to_string(),
                trader_count: trader_counts.get(&m.id).copied().unwrap_or(0),
                volume_24h_nanos: volumes_24h.get(&m.id).copied().unwrap_or(0),
                yes_price_24h_ago_nanos: price_24h.map(|(y, _)| y),
                no_price_24h_ago_nanos: price_24h.map(|(_, n)| n),
                liquidity_avg10_nanos: liquidity_by_market.get(&m.id).copied().unwrap_or(0),
                liquidity_band_nanos,
                orders_placed_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.placed)
                    .unwrap_or(0),
                orders_matched_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.matched)
                    .unwrap_or(0),
                orders_unmatched_total: order_stats_by_market
                    .get(&m.id)
                    .map(|s| s.unmatched)
                    .unwrap_or(0),
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
    let now_ms = now_unix_ms();
    let (
        status,
        metadata,
        volume,
        trader_counts,
        volume_24h,
        price_24h_ago_all,
        liquidity,
        order_stats_by_market,
    ) = tokio::try_join!(
        state.sequencer.get_market_status(mid),
        state.sequencer.get_market_metadata(mid),
        state.sequencer.get_market_volume(mid),
        state.sequencer.get_all_trader_counts(),
        state.sequencer.get_all_market_volumes_24h(now_ms),
        state.sequencer.get_all_market_prices_n_hours_ago(24, now_ms),
        state.sequencer.get_liquidity_snapshot(),
        state.sequencer.get_order_stats_by_market(),
    )?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;
    let market_order_stats = order_stats_by_market.get(&mid).copied().unwrap_or_default();
    let ref_price = state.reference_prices.read().await.get(&id).copied();
    let ref_data = state.market_ref_data.read().await.get(&id).cloned();

    Ok(Json(build_market_response(BuildMarketResponseArgs {
        market_id: market.id.0,
        name: market.name.clone(),
        yes_price_nanos: market_prices.and_then(|p| p.first().copied()),
        no_price_nanos: market_prices.and_then(|p| p.get(1).copied()),
        status: &status,
        metadata: metadata.as_ref(),
        volume_nanos: volume,
        reference_price_nanos: ref_price,
        ref_data: ref_data.as_ref(),
        trader_count: trader_counts.get(&mid).copied().unwrap_or(0),
        volume_24h_nanos: volume_24h.get(&mid).copied().unwrap_or(0),
        price_24h_ago: price_24h_ago_all.get(&mid).copied(),
        liquidity_avg10_nanos: liquidity_by_market.get(&mid).copied().unwrap_or(0),
        liquidity_band_nanos,
        orders_placed_total: market_order_stats.placed,
        orders_matched_total: market_order_stats.matched,
        orders_unmatched_total: market_order_stats.unmatched,
    })))
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

    if let Some(template) = req.resolution_template.as_ref() {
        if !state.sequencer.template_exists(template.clone()).await? {
            return Err(AppError::bad_request(format!(
                "Unknown resolution_template: {:?}. Install it before creating markets that reference it.",
                template
            )));
        }
    }

    let has_metadata = req.description.is_some()
        || req.category.is_some()
        || req.tags.is_some()
        || req.resolution_criteria.is_some()
        || req.expiry_timestamp_ms.is_some()
        || req.resolution_template.is_some();

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
            resolution_config: req
                .resolution_template
                .map(|template| ResolutionConfig { template }),
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
    if req.payout_nanos > NANOS_PER_DOLLAR {
        return Err(AppError::bad_request(format!(
            "Payout must be between 0 and {} nanos, got {}",
            NANOS_PER_DOLLAR, req.payout_nanos
        )));
    }

    let mid = MarketId::new(id);

    match req.attestation {
        Some(att_dto) => {
            let pubkey_bytes = hex::decode(&att_dto.pubkey_hex)
                .map_err(|_| AppError::bad_request("Invalid pubkey_hex"))?;
            let signature_der = hex::decode(&att_dto.signature_hex)
                .map_err(|_| AppError::bad_request("Invalid signature_hex"))?;
            let signed = SignedAttestation {
                attestation: ResolutionAttestation {
                    market_id: mid,
                    payout_nanos: req.payout_nanos,
                    nonce: att_dto.nonce,
                },
                signer: FeedPubkey(pubkey_bytes),
                signature_der,
            };
            let _record = state.sequencer.resolve_market_attested(mid, signed).await?;
        }
        None => {
            if !state.dev_mode {
                return Err(AppError::dev_mode_required());
            }
            let _record = state
                .sequencer
                .resolve_market(mid, req.payout_nanos)
                .await?;
        }
    }

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

    let now_ms = now_unix_ms();
    let (results, trader_counts, volumes_24h, prices_24h_ago, liquidity, order_stats_by_market) = tokio::try_join!(
        state.sequencer.search_markets(query),
        state.sequencer.get_all_trader_counts(),
        state.sequencer.get_all_market_volumes_24h(now_ms),
        state.sequencer.get_all_market_prices_n_hours_ago(24, now_ms),
        state.sequencer.get_liquidity_snapshot(),
        state.sequencer.get_order_stats_by_market(),
    )?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;
    let ref_prices = state.reference_prices.read().await;
    let market_ref_data = state.market_ref_data.read().await;

    let response: Vec<MarketResponse> = results
        .into_iter()
        .map(|r| {
            let mid = r.market_id.0;
            let count = trader_counts.get(&r.market_id).copied().unwrap_or(0);
            let vol_24h = volumes_24h.get(&r.market_id).copied().unwrap_or(0);
            let p_24h_ago = prices_24h_ago.get(&r.market_id).copied();
            build_market_response(BuildMarketResponseArgs {
                market_id: mid,
                name: r.name,
                yes_price_nanos: r.yes_price_nanos,
                no_price_nanos: r.no_price_nanos,
                status: &r.status,
                metadata: r.metadata.as_ref(),
                volume_nanos: r.volume_nanos,
                reference_price_nanos: ref_prices.get(&mid).copied(),
                ref_data: market_ref_data.get(&mid),
                trader_count: count,
                volume_24h_nanos: vol_24h,
                price_24h_ago: p_24h_ago,
                liquidity_avg10_nanos: liquidity_by_market.get(&r.market_id).copied().unwrap_or(0),
                liquidity_band_nanos,
                orders_placed_total: order_stats_by_market
                    .get(&r.market_id)
                    .map(|s| s.placed)
                    .unwrap_or(0),
                orders_matched_total: order_stats_by_market
                    .get(&r.market_id)
                    .map(|s| s.matched)
                    .unwrap_or(0),
                orders_unmatched_total: order_stats_by_market
                    .get(&r.market_id)
                    .map(|s| s.unmatched)
                    .unwrap_or(0),
            })
        })
        .collect();

    Ok(Json(response))
}

/// POST /v1/markets/prices/reference — set reference prices from external system (dev mode)
#[utoipa::path(
    post,
    path = "/v1/markets/prices/reference",
    request_body = SetReferencePricesRequest,
    responses(
        (status = 200, description = "Prices updated"),
    )
)]
pub async fn set_reference_prices(
    State(state): State<AppState>,
    Json(req): Json<SetReferencePricesRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let mut prices = state.reference_prices.write().await;
    for (market_id, price) in req.prices {
        prices.insert(market_id, price);
    }
    *state.reference_prices_updated_at_ms.write().await = now_ms();
    Ok(Json(serde_json::json!({"updated": true})))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// GET /v1/markets/{id}/resolution
#[utoipa::path(
    get,
    path = "/v1/markets/{id}/resolution",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Resolution state", body = ResolutionResponse),
        (status = 404, description = "Market not found"),
    )
)]
pub async fn get_resolution(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<ResolutionResponse>, AppError> {
    let markets = state.sequencer.list_markets().await?;
    let mid = MarketId::new(id);
    if markets.get(mid).is_none() {
        return Err(AppError::not_found(format!("Market {} not found", id)));
    }

    let (status, metadata) = tokio::try_join!(
        state.sequencer.get_market_status(mid),
        state.sequencer.get_market_metadata(mid),
    )?;

    let template = metadata
        .as_ref()
        .map(|m| m.effective_template().to_string())
        .unwrap_or_else(|| "admin_immediate".to_string());

    let (payout, resolved_at, feed_id_opt) = match &status {
        matching_sequencer::MarketStatus::Resolved { record } => {
            let feed_id = match record.resolved_by {
                OracleSource::DataFeed(fid) => Some(fid),
                _ => None,
            };
            (
                Some(record.payout_nanos),
                Some(record.resolved_at_ms),
                feed_id,
            )
        }
        _ => (None, None, None),
    };

    let (feed_id_num, feed_name) = if let Some(fid) = feed_id_opt {
        let feed = state.sequencer.get_feed(fid).await?;
        let name = feed.as_ref().map(|f| f.name.clone());
        (Some(fid.0), name)
    } else {
        (None, None)
    };

    Ok(Json(ResolutionResponse {
        market_id: id,
        status: status.as_str().to_string(),
        payout_nanos: payout,
        resolved_at_ms: resolved_at,
        resolved_by_feed_id: feed_id_num,
        resolved_by_feed_name: feed_name,
        template,
    }))
}

/// POST /v1/markets/{id}/metadata — set external metadata for a market (dev mode)
#[utoipa::path(
    post,
    path = "/v1/markets/{id}/metadata",
    params(("id" = u32, Path, description = "Market ID")),
    request_body = SetMarketMetadataRequest,
    responses(
        (status = 200, description = "Metadata updated"),
    )
)]
pub async fn set_market_metadata(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(req): Json<SetMarketMetadataRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let mut ref_data = state.market_ref_data.write().await;
    let entry = ref_data.entry(id).or_insert_with(MarketRefData::default);
    if let Some(v) = req.external_url {
        entry.external_url = Some(v);
    }
    if let Some(v) = req.event_id {
        entry.event_id = Some(v);
    }
    if let Some(v) = req.event_title {
        entry.event_title = Some(v);
    }
    if let Some(v) = req.event_image_url {
        entry.event_image_url = Some(v);
    }
    if let Some(v) = req.event_icon_url {
        entry.event_icon_url = Some(v);
    }
    if let Some(v) = req.event_end_date_ms {
        entry.event_end_date_ms = Some(v);
    }
    if let Some(v) = req.market_image_url {
        entry.market_image_url = Some(v);
    }
    if let Some(v) = req.market_icon_url {
        entry.market_icon_url = Some(v);
    }
    if let Some(v) = req.market_end_date_ms {
        entry.market_end_date_ms = Some(v);
    }
    if let Some(v) = req.category {
        entry.category = Some(v);
    }
    if let Some(v) = req.categories {
        entry.categories = Some(v);
    }
    save_market_ref_data(&ref_data, state.market_ref_data_path.as_deref());
    Ok(Json(serde_json::json!({"updated": true})))
}
