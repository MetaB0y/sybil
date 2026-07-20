use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};

use matching_engine::{MarketId, NANOS_PER_DOLLAR, Nanos};
use matching_sequencer::aggregates::OrderStats;
use matching_sequencer::{
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_PRICE_HISTORY_QUERY_POINTS, MarketMetadata,
    ResolutionConfig,
};
use sybil_history_types::{PriceCandleQuery, PriceHistoryQuery};
use sybil_oracle::{FeedPubkey, ResolutionAttestation, SignedAttestation};

use crate::convert::prices_to_response;
use crate::state::{AppState, MarketRefData, save_market_ref_data};
use crate::types::error::AppError;
use crate::types::request::{
    CreateMarketGroupRequest, CreateMarketRequest, ExtendMarketGroupRequest, MarketSearchParams,
    ResolveMarketRequest, SetMarketMetadataRequest, SetReferencePricesRequest,
};
use crate::types::response::*;
use crate::util::now_ms;
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
    reference_price_expires_at_ms: Option<u64>,
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

/// Per-market rolling/aggregate stats shared by every market-response shape
/// (`list`, `summary`, `get`, `search`). Gathered once per market from the bulk
/// snapshots so the handlers don't each re-spell the same map lookups. The
/// liquidity band width is intentionally excluded — it is a single scalar for
/// the whole snapshot, not a per-market value.
struct MarketRollingStats {
    trader_count: u32,
    volume_24h_nanos: u64,
    price_24h_ago: Option<(u64, u64)>,
    liquidity_avg10_nanos: u64,
    orders_placed_total: u64,
    orders_matched_total: u64,
    orders_unmatched_total: u64,
}

impl MarketRollingStats {
    fn gather(
        mid: &MarketId,
        trader_counts: &HashMap<MarketId, u32>,
        volumes_24h: &HashMap<MarketId, u64>,
        prices_24h_ago: &HashMap<MarketId, (u64, u64)>,
        liquidity_by_market: &HashMap<MarketId, u64>,
        order_stats_by_market: &HashMap<MarketId, OrderStats>,
    ) -> Self {
        let order_stats = order_stats_by_market.get(mid).copied().unwrap_or_default();
        Self {
            trader_count: trader_counts.get(mid).copied().unwrap_or(0),
            volume_24h_nanos: volumes_24h.get(mid).copied().unwrap_or(0),
            price_24h_ago: prices_24h_ago.get(mid).copied(),
            liquidity_avg10_nanos: liquidity_by_market.get(mid).copied().unwrap_or(0),
            orders_placed_total: order_stats.placed,
            orders_matched_total: order_stats.matched,
            orders_unmatched_total: order_stats.unmatched,
        }
    }
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
        payout_nanos: args.status.payout_nanos().map(|n| n.0),
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
        reference_price_expires_at_ms: args.reference_price_expires_at_ms,
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
        polymarket_condition_id: args
            .ref_data
            .and_then(|r| r.polymarket_condition_id.clone()),
        event_start_date_ms: args.ref_data.and_then(|r| r.event_start_date_ms),
        market_start_date_ms: args.ref_data.and_then(|r| r.market_start_date_ms),
        group_item_title: args.ref_data.and_then(|r| r.group_item_title.clone()),
        closed: args.ref_data.and_then(|r| r.closed),
    }
}

/// GET /v1/markets
#[utoipa::path(
    tag = "routesmarkets",
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
    let now_ms = now_ms();
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
            state
                .sequencer
                .get_all_market_prices_n_hours_ago(24, now_ms),
            state.sequencer.get_liquidity_snapshot(),
            state.sequencer.get_order_stats_by_market(),
        )
    }
    .instrument(tracing::info_span!("list_markets.fetch"))
    .await?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;

    let ref_prices = state.fresh_reference_prices().await;
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
            let stats = MarketRollingStats::gather(
                &m.id,
                &trader_counts,
                &volumes_24h,
                &prices_24h_ago,
                &liquidity_by_market,
                &order_stats_by_market,
            );
            build_market_response(BuildMarketResponseArgs {
                market_id: m.id.0,
                name: m.name.clone(),
                yes_price_nanos: market_prices.and_then(|p| p.first().map(|n| n.0)),
                no_price_nanos: market_prices.and_then(|p| p.get(1).map(|n| n.0)),
                status: &status,
                metadata: metadata.get(&m.id),
                volume_nanos: volumes.get(&m.id).copied().unwrap_or(0),
                reference_price_nanos: ref_prices.get(&m.id.0).map(|price| price.price_nanos),
                reference_price_expires_at_ms: ref_prices
                    .get(&m.id.0)
                    .map(|price| price.expires_at_ms),
                ref_data: market_ref_data.get(&m.id.0),
                trader_count: stats.trader_count,
                volume_24h_nanos: stats.volume_24h_nanos,
                price_24h_ago: stats.price_24h_ago,
                liquidity_avg10_nanos: stats.liquidity_avg10_nanos,
                liquidity_band_nanos,
                orders_placed_total: stats.orders_placed_total,
                orders_matched_total: stats.orders_matched_total,
                orders_unmatched_total: stats.orders_unmatched_total,
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
    tag = "routesmarkets",
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
    let now_ms = now_ms();
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
            state
                .sequencer
                .get_all_market_prices_n_hours_ago(24, now_ms),
            state.sequencer.get_liquidity_snapshot(),
            state.sequencer.get_order_stats_by_market(),
        )
    }
    .instrument(tracing::info_span!("list_markets_summary.fetch"))
    .await?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;

    let ref_prices = state.fresh_reference_prices().await;

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
            let stats = MarketRollingStats::gather(
                &m.id,
                &trader_counts,
                &volumes_24h,
                &prices_24h_ago,
                &liquidity_by_market,
                &order_stats_by_market,
            );
            MarketSummaryResponse {
                market_id: m.id.0,
                name: m.name.clone(),
                yes_price_nanos: market_prices.and_then(|p| p.first().map(|n| n.0)),
                no_price_nanos: market_prices.and_then(|p| p.get(1).map(|n| n.0)),
                reference_price_nanos: ref_prices.get(&m.id.0).map(|price| price.price_nanos),
                reference_price_expires_at_ms: ref_prices
                    .get(&m.id.0)
                    .map(|price| price.expires_at_ms),
                volume_nanos: volumes.get(&m.id).copied().unwrap_or(0),
                status: status.as_str().to_string(),
                trader_count: stats.trader_count,
                volume_24h_nanos: stats.volume_24h_nanos,
                yes_price_24h_ago_nanos: stats.price_24h_ago.map(|(y, _)| y),
                no_price_24h_ago_nanos: stats.price_24h_ago.map(|(_, n)| n),
                liquidity_avg10_nanos: stats.liquidity_avg10_nanos,
                liquidity_band_nanos,
                orders_placed_total: stats.orders_placed_total,
                orders_matched_total: stats.orders_matched_total,
                orders_unmatched_total: stats.orders_unmatched_total,
            }
        })
        .collect();

    Ok(Json(response))
}

/// GET /v1/markets/{id}
#[utoipa::path(
    tag = "routesmarkets",
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
    let now_ms = now_ms();
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
        state
            .sequencer
            .get_all_market_prices_n_hours_ago(24, now_ms),
        state.sequencer.get_liquidity_snapshot(),
        state.sequencer.get_order_stats_by_market(),
    )?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;
    let stats = MarketRollingStats::gather(
        &mid,
        &trader_counts,
        &volume_24h,
        &price_24h_ago_all,
        &liquidity_by_market,
        &order_stats_by_market,
    );
    let ref_price = state.fresh_reference_price(id).await;
    let ref_data = state.market_ref_data.read().await.get(&id).cloned();

    Ok(Json(build_market_response(BuildMarketResponseArgs {
        market_id: market.id.0,
        name: market.name.clone(),
        yes_price_nanos: market_prices.and_then(|p| p.first().map(|n| n.0)),
        no_price_nanos: market_prices.and_then(|p| p.get(1).map(|n| n.0)),
        status: &status,
        metadata: metadata.as_ref(),
        volume_nanos: volume,
        reference_price_nanos: ref_price.map(|price| price.price_nanos),
        reference_price_expires_at_ms: ref_price.map(|price| price.expires_at_ms),
        ref_data: ref_data.as_ref(),
        trader_count: stats.trader_count,
        volume_24h_nanos: stats.volume_24h_nanos,
        price_24h_ago: stats.price_24h_ago,
        liquidity_avg10_nanos: stats.liquidity_avg10_nanos,
        liquidity_band_nanos,
        orders_placed_total: stats.orders_placed_total,
        orders_matched_total: stats.orders_matched_total,
        orders_unmatched_total: stats.orders_unmatched_total,
    })))
}

/// POST /v1/markets
#[utoipa::path(
    tag = "routesmarkets",
    post,
    path = "/v1/markets",
    request_body = CreateMarketRequest,
    responses(
        (status = 200, description = "Market created", body = CreateMarketResponse),
        (status = 400, description = "Invalid market creation key", body = ApiErrorResponse),
        (status = 409, description = "Creation key conflicts with an existing market", body = ApiErrorResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn create_market(
    State(state): State<AppState>,
    Json(req): Json<CreateMarketRequest>,
) -> Result<Json<CreateMarketResponse>, AppError> {
    if let Some(template) = req.resolution_template.as_ref()
        && !state.sequencer.template_exists(template.clone()).await?
    {
        return Err(AppError::bad_request(format!(
            "Unknown resolution_template: {:?}. Install it before creating markets that reference it.",
            template
        )));
    }

    let has_metadata = req.creation_key.is_some()
        || req.description.is_some()
        || req.category.is_some()
        || req.tags.is_some()
        || req.resolution_criteria.is_some()
        || req.expiry_timestamp_ms.is_some()
        || req.resolution_template.is_some();

    let market_id = if has_metadata {
        let now_ms = now_ms();

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
            creation_key: req.creation_key,
            committed_metadata_digest: None,
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
    tag = "routesmarkets",
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
        .enumerate()
        .map(|(group_id, g)| MarketGroupResponse {
            group_id: group_id as u64,
            name: g.name.clone(),
            creation_key: g.creation_key.clone(),
            market_ids: g.markets.iter().map(|m| m.0).collect(),
        })
        .collect();
    Ok(Json(response))
}

/// POST /v1/markets/groups
#[utoipa::path(
    tag = "routesmarkets",
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
    let market_ids: Vec<MarketId> = req.market_ids.iter().map(|&id| MarketId::new(id)).collect();
    let (group_id, group) = state
        .sequencer
        .create_market_group_with_key(req.name, req.creation_key, market_ids)
        .await?;

    Ok(Json(MarketGroupResponse {
        group_id,
        name: group.name,
        creation_key: group.creation_key,
        market_ids: group.markets.iter().map(|m| m.0).collect(),
    }))
}

/// POST /v1/markets/groups/{group_id}/members
#[utoipa::path(
    tag = "routesmarkets",
    post,
    path = "/v1/markets/groups/{group_id}/members",
    params(
        ("group_id" = u64, Path, description = "Current market group index")
    ),
    request_body = ExtendMarketGroupRequest,
    responses(
        (status = 200, description = "Market group extended or already contained the member", body = MarketGroupResponse),
        (status = 404, description = "Market group or market not found"),
        (status = 409, description = "Market is resolved or already belongs to another group")
    )
)]
pub async fn extend_market_group(
    State(state): State<AppState>,
    Path(group_id): Path<u64>,
    Json(req): Json<ExtendMarketGroupRequest>,
) -> Result<Json<MarketGroupResponse>, AppError> {
    let (group, _) = state
        .sequencer
        .extend_market_group(group_id, MarketId::new(req.market_id))
        .await?;

    Ok(Json(MarketGroupResponse {
        group_id,
        name: group.name,
        creation_key: group.creation_key,
        market_ids: group.markets.iter().map(|m| m.0).collect(),
    }))
}

/// GET /v1/markets/prices
#[utoipa::path(
    tag = "routesmarkets",
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
    tag = "routesmarkets",
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
                    payout_nanos: Nanos(req.payout_nanos),
                    nonce: att_dto.nonce,
                },
                signer: FeedPubkey(pubkey_bytes),
                signature_der,
            };
            let _record = state.sequencer.resolve_market_attested(mid, signed).await?;
        }
        None => {
            let _record = state
                .sequencer
                .resolve_market(mid, Nanos(req.payout_nanos))
                .await?;
        }
    }

    let status = state.sequencer.get_market_status(mid).await?;

    Ok(Json(ResolveMarketResponse {
        market_id: id,
        payout_nanos: req.payout_nanos,
        status: status.as_str().to_string(),
    }))
}

/// GET /v1/markets/{id}/prices/history
#[utoipa::path(
    tag = "routesmarkets",
    get,
    path = "/v1/markets/{id}/prices/history",
    params(
        ("id" = u32, Path, description = "Market ID"),
        ("from_ms" = Option<u64>, Query, description = "Start timestamp filter"),
        ("to_ms" = Option<u64>, Query, description = "End timestamp filter"),
        ("before_height" = Option<u64>, Query, description = "Return points with height strictly below this cursor"),
        ("limit" = Option<usize>, Query, description = "Maximum returned points, newest matching points first by cap, clamped server-side"),
    ),
    responses(
        (status = 200, description = "Price history", body = PriceHistoryResponse),
        (status = 503, description = "History service unavailable")
    )
)]
pub async fn get_price_history(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<PriceHistoryParams>,
) -> Result<Json<PriceHistoryResponse>, AppError> {
    let mid = MarketId::new(id);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PRICE_HISTORY_QUERY_POINTS)
        .min(MAX_PRICE_HISTORY_QUERY_POINTS);
    let history = state.history.as_ref().ok_or_else(|| {
        AppError::history_unavailable("Historical data service is not configured")
    })?;
    let page = history
        .prices(&PriceHistoryQuery {
            market_id: mid.0,
            from_ms: params.from_ms,
            to_ms: params.to_ms,
            before_height: params.before_height,
            limit,
        })
        .await?;

    let retention_min_height = page.status.first_height.filter(|height| *height > 1);
    let indexed_through_height = page.status.indexed_through_height;
    let history_complete_from_height = page.status.first_height;

    let response = PriceHistoryResponse {
        market_id: id,
        next_before_height: page.next_before_height,
        retention_min_height,
        indexed_through_height,
        history_complete_from_height,
        points: page
            .points
            .into_iter()
            .map(|p| PricePointResponse {
                height: p.height,
                timestamp_ms: p.timestamp_ms,
                yes_price_nanos: p.yes_price_nanos,
                no_price_nanos: p.no_price_nanos,
                volume_nanos: p.volume_nanos,
            })
            .collect(),
    };

    Ok(Json(response))
}

/// GET /v1/markets/{id}/prices/candles
#[utoipa::path(
    tag = "routesmarkets",
    get,
    path = "/v1/markets/{id}/prices/candles",
    params(
        ("id" = u32, Path, description = "Market ID"),
        ("resolution" = String, Query, description = "Candle resolution: seconds or one of 1m, 5m, 1h"),
        ("from_ms" = Option<u64>, Query, description = "Start bucket timestamp filter"),
        ("to_ms" = Option<u64>, Query, description = "End bucket timestamp filter"),
        ("before_ms" = Option<u64>, Query, description = "Return candles with bucket_start_ms strictly below this cursor"),
        ("limit" = Option<usize>, Query, description = "Maximum returned candles, clamped server-side"),
    ),
    responses(
        (status = 200, description = "Price candles", body = PriceCandlesResponse),
        (status = 503, description = "History service unavailable")
    )
)]
pub async fn get_price_candles(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<PriceCandlesParams>,
) -> Result<Json<PriceCandlesResponse>, AppError> {
    let mid = MarketId::new(id);
    let resolution_secs = parse_candle_resolution(&params.resolution)?;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PRICE_HISTORY_QUERY_POINTS)
        .min(MAX_PRICE_HISTORY_QUERY_POINTS);
    let history = state.history.as_ref().ok_or_else(|| {
        AppError::history_unavailable("Historical data service is not configured")
    })?;
    let page = history
        .candles(&PriceCandleQuery {
            market_id: mid.0,
            resolution_secs,
            from_ms: params.from_ms,
            to_ms: params.to_ms,
            before_ms: params.before_ms,
            limit,
        })
        .await?;

    let retention_min_bucket_ms = page
        .status
        .first_height
        .filter(|height| *height > 1)
        .and(page.status.first_timestamp_ms)
        .map(|timestamp_ms| {
            let resolution_ms = u64::from(resolution_secs).saturating_mul(1_000);
            timestamp_ms - timestamp_ms % resolution_ms.max(1)
        });
    let indexed_through_height = page.status.indexed_through_height;
    let history_complete_from_height = page.status.first_height;

    Ok(Json(PriceCandlesResponse {
        market_id: id,
        resolution_secs: page.resolution_secs,
        next_before_ms: page.next_before_ms,
        retention_min_bucket_ms,
        indexed_through_height,
        history_complete_from_height,
        candles: page
            .candles
            .into_iter()
            .map(|c| PriceCandleResponse {
                bucket_start_ms: c.bucket_start_ms,
                bucket_end_ms: c.bucket_end_ms,
                first_height: c.first_height,
                last_height: c.last_height,
                open_yes_price_nanos: c.open_yes_price_nanos,
                high_yes_price_nanos: c.high_yes_price_nanos,
                low_yes_price_nanos: c.low_yes_price_nanos,
                close_yes_price_nanos: c.close_yes_price_nanos,
                open_no_price_nanos: c.open_no_price_nanos,
                high_no_price_nanos: c.high_no_price_nanos,
                low_no_price_nanos: c.low_no_price_nanos,
                close_no_price_nanos: c.close_no_price_nanos,
                volume_nanos: c.volume_nanos,
                point_count: c.point_count,
            })
            .collect(),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct PriceHistoryParams {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub before_height: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
pub struct PriceCandlesParams {
    pub resolution: String,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub before_ms: Option<u64>,
    pub limit: Option<usize>,
}

fn parse_candle_resolution(input: &str) -> Result<u32, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("resolution is required"));
    }
    if let Some(minutes) = trimmed.strip_suffix('m') {
        let value = minutes
            .parse::<u32>()
            .map_err(|_| AppError::bad_request("invalid candle resolution"))?;
        return value
            .checked_mul(60)
            .filter(|seconds| *seconds > 0)
            .ok_or_else(|| AppError::bad_request("invalid candle resolution"));
    }
    if let Some(hours) = trimmed.strip_suffix('h') {
        let value = hours
            .parse::<u32>()
            .map_err(|_| AppError::bad_request("invalid candle resolution"))?;
        return value
            .checked_mul(3_600)
            .filter(|seconds| *seconds > 0)
            .ok_or_else(|| AppError::bad_request("invalid candle resolution"));
    }
    trimmed
        .parse::<u32>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| AppError::bad_request("invalid candle resolution"))
}

/// GET /v1/markets/search
#[utoipa::path(
    tag = "routesmarkets",
    get,
    path = "/v1/markets/search",
    params(
        ("q" = Option<String>, Query, description = "Text search"),
        ("tags" = Option<String>, Query, description = "Comma-separated tags"),
        ("category" = Option<String>, Query, description = "Category filter"),
        ("status" = Option<String>, Query, description = "Status filter"),
        ("min_yes_price_nanos" = Option<String>, Query, description = "Minimum YES price. Integer nanodollars; per-share probabilities in [0, 1e9]"),
        ("max_yes_price_nanos" = Option<String>, Query, description = "Maximum YES price. Integer nanodollars; per-share probabilities in [0, 1e9]"),
        ("min_volume_nanos" = Option<String>, Query, description = "Minimum cumulative traded notional. Integer nanodollars"),
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
        min_yes_price: params.min_yes_price_nanos.map(Nanos),
        max_yes_price: params.max_yes_price_nanos.map(Nanos),
        min_volume: params.min_volume_nanos,
        sort_by,
        limit: params.limit,
        offset: params.offset,
    };

    let now_ms = now_ms();
    let (results, trader_counts, volumes_24h, prices_24h_ago, liquidity, order_stats_by_market) = tokio::try_join!(
        state.sequencer.search_markets(query),
        state.sequencer.get_all_trader_counts(),
        state.sequencer.get_all_market_volumes_24h(now_ms),
        state
            .sequencer
            .get_all_market_prices_n_hours_ago(24, now_ms),
        state.sequencer.get_liquidity_snapshot(),
        state.sequencer.get_order_stats_by_market(),
    )?;
    let (liquidity_by_market, liquidity_band_nanos) = liquidity;
    let ref_prices = state.fresh_reference_prices().await;
    let market_ref_data = state.market_ref_data.read().await;

    let response: Vec<MarketResponse> = results
        .into_iter()
        .map(|r| {
            let mid = r.market_id.0;
            let stats = MarketRollingStats::gather(
                &r.market_id,
                &trader_counts,
                &volumes_24h,
                &prices_24h_ago,
                &liquidity_by_market,
                &order_stats_by_market,
            );
            build_market_response(BuildMarketResponseArgs {
                market_id: mid,
                name: r.name,
                yes_price_nanos: r.yes_price_nanos.map(|n| n.0),
                no_price_nanos: r.no_price_nanos.map(|n| n.0),
                status: &r.status,
                metadata: r.metadata.as_ref(),
                volume_nanos: r.volume_nanos,
                reference_price_nanos: ref_prices.get(&mid).map(|price| price.price_nanos),
                reference_price_expires_at_ms: ref_prices
                    .get(&mid)
                    .map(|price| price.expires_at_ms),
                ref_data: market_ref_data.get(&mid),
                trader_count: stats.trader_count,
                volume_24h_nanos: stats.volume_24h_nanos,
                price_24h_ago: stats.price_24h_ago,
                liquidity_avg10_nanos: stats.liquidity_avg10_nanos,
                liquidity_band_nanos,
                orders_placed_total: stats.orders_placed_total,
                orders_matched_total: stats.orders_matched_total,
                orders_unmatched_total: stats.orders_unmatched_total,
            })
        })
        .collect();

    Ok(Json(response))
}

/// POST /v1/markets/prices/reference — set reference prices from external system.
#[utoipa::path(
    tag = "routesmarkets",
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
    if let Some((market_id, price)) = req
        .prices_nanos
        .iter()
        .find(|(_, price)| **price > NANOS_PER_DOLLAR)
    {
        return Err(AppError::bad_request(format!(
            "reference price for market {market_id} exceeds {NANOS_PER_DOLLAR}: {price}"
        )));
    }
    state.update_reference_prices(req.prices_nanos).await;
    Ok(Json(serde_json::json!({"updated": true})))
}

/// GET /v1/markets/{id}/resolution
#[utoipa::path(
    tag = "routesmarkets",
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
                Some(record.payout_nanos.0),
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

/// POST /v1/markets/{id}/metadata — set external metadata for a market.
#[utoipa::path(
    tag = "routesmarkets",
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
    if let Some(v) = req.polymarket_condition_id {
        entry.polymarket_condition_id = Some(v);
    }
    if let Some(v) = req.event_start_date_ms {
        entry.event_start_date_ms = Some(v);
    }
    if let Some(v) = req.market_start_date_ms {
        entry.market_start_date_ms = Some(v);
    }
    if let Some(v) = req.group_item_title {
        entry.group_item_title = Some(v);
    }
    if let Some(v) = req.closed {
        entry.closed = Some(v);
    }
    save_market_ref_data(&ref_data, state.market_ref_data_path.as_deref());
    Ok(Json(serde_json::json!({"updated": true})))
}
