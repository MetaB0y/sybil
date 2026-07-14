use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, MatchedPath, State};
use axum::http::{Method, Request, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes as openapi_routes;

use crate::arena;
use crate::routes;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::*;
use crate::types::response::*;
use crate::util::now_ms;

#[derive(OpenApi)]
#[openapi(
    components(schemas(
        CreateAccountRequest,
        FundAccountRequest,
        SubmitL1DepositRequest,
        SubmitL1WithdrawalEventRequest,
        ObserveL1HeightRequest,
        CreateBridgeWithdrawalRequest,
        CreateSignedBridgeWithdrawalRequest,
        RegisterKeyRequest,
        SignedRegisterKeyRequest,
        KeyScope,
        SetProfileRequest,
        RevokeKeyRequest,
        CreateApiKeyRequest,
        RevokeApiKeyRequest,
        AccountKeyResponse,
        KeyOpStateResponse,
        ApiKeyResponse,
        CreateApiKeyResponse,
        PrivateAccountSummaryResponse,
        CreateMarketRequest,
        CreateMarketGroupRequest,
        ExtendMarketGroupRequest,
        ResolveMarketRequest,
        SignedAttestationDto,
        RegisterFeedRequest,
        RegisteredFeedResponse,
        ResolutionResponse,
        SubmitOrderRequest,
        SubmitSignedOrderRequest,
        CancelSignedOrderRequest,
        AuthScheme,
        BridgeWithdrawalL1Status,
        WebAuthnAssertion,
        WebAuthnRegistration,
        SetReferencePricesRequest,
        SetMarketMetadataRequest,
        SignedOrderData,
        OrderSpec,
        MarketSearchParams,
        AccountResponse,
        PositionResponse,
        BridgeStatusResponse,
        BridgeAccountKeyResponse,
        BridgeDepositResponse,
        BridgeDepositEventResponse,
        BridgeWithdrawalResponse,
        BridgeWithdrawalL1EventResponse,
        ObserveL1HeightResponse,
        BridgeBlockResponse,
        MarketResponse,
        MarketSummaryResponse,
        MarketGroupResponse,
        MarketPricesResponse,
        MarketPriceResponse,
        CreateMarketResponse,
        OrderAcceptedResponse,
        CancelOrderResponse,
        FillResponse,
        RejectionResponse,
        BlockResponse,
        PublicBlockResponse,
        PublicBridgeBlockResponse,
        HealthResponse,
        AttestationResponse,
        StateRootResponse,
        DaManifestResponse,
        DaProviderRefResponse,
        StateProofResponse,
        QmdbStateInclusionProofResponse,
        QmdbStateExclusionProofResponse,
        QmdbStateOperationProofResponse,
        QmdbStateRangeProofResponse,
        ResolveMarketResponse,
        PortfolioResponse,
        PositionValueResponse,
        PriceCandleResponse,
        PriceCandlesResponse,
        PriceHistoryResponse,
        PricePointResponse,
        AccountFillResponse,
        AccountFillPageResponse,
        AccountHistoryPageResponse,
        EquityPointResponse,
        EquitySeriesResponse,
        LeaderboardResponse,
        LeaderboardEntryResponse,
        PendingOrderResponse,
        PositionDeltaResponse,
        BlockMarketStats,
        DerivedViewSidecarResponse,
        RemovedOrderViewResponse,
        ReservedPositionReleaseResponse,
        AdmitTimingViewResponse,
        RejectedOrderViewResponse,
        ActivityOverviewResponse,
        OverviewBucketResponse,
        OverviewOrderStatsResponse,
        OpenBatchResponse,
        EventTradersResponse,
        HistoryEventResponse,
        routes::bots::BotDecisionFeedResponse,
        routes::bots::BotStatsResponse,
        routes::bots::BotSummaryResponse,
        routes::bots::BotDecisionResponse,
        routes::bots::TokenUsageResponse,
        routes::bots::BotEquitySeriesResponse,
        routes::bots::BotEquityPointResponse,
        SubmitAutoResolutionRequest,
        AutoResolutionActionDto,
        AutoResolutionEntryResponse,
        AutoResolutionListResponse,
    )),
    modifiers(&BearerReadAddon),
    info(
        title = "Sybil API",
        description = "HTTP API for AI agent trading on Sybil prediction markets.\n\nUnits: protocol quantity fields use integer share-units (1000 units = 1 share). Money and `*_nanos` fields use integer nanodollars (1_000_000_000 = $1); prices are per-share probabilities in [0, 1e9]. See [REST API units](docs/architecture/REST%20API.md#units).",
        version = "0.1.0"
    )
)]
pub struct ApiDoc;

/// Defines the `bearer_read` security scheme referenced by SYB-60's
/// read-only, bearer-gated private endpoints (e.g. `GET
/// /v1/accounts/{id}/private-summary`). These bearer tokens are READ-ONLY —
/// mutating actions always require a P256 signature, never a bearer token.
struct BearerReadAddon;

impl utoipa::Modify for BearerReadAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_read",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "Read-scoped API key (SYB-60). Read-only: cannot place orders, \
                         cancels, or withdrawals — those require a P256 signature.",
                    ))
                    .build(),
            ),
        );
    }
}

async fn prometheus_metrics(State(state): State<AppState>) -> impl IntoResponse {
    // A slow or wedged sequencer must not stall the scrape indefinitely; bound
    // the live-market collection and fall back to the cached gauges on timeout.
    if tokio::time::timeout(Duration::from_secs(2), record_live_market_metrics(&state))
        .await
        .is_err()
    {
        metrics::counter!("sybil_metrics_collection_timeouts_total", "collector" => "live_market")
            .increment(1);
        tracing::warn!("live market metrics collection timed out; rendering cached metrics");
    }
    record_bot_metrics(&state).await;
    state.prometheus.render()
}

async fn record_live_market_metrics(state: &AppState) {
    let (markets, prices, statuses, volumes, bridge) = match tokio::try_join!(
        state.sequencer.list_markets(),
        state.sequencer.get_market_prices(),
        state.sequencer.get_all_market_statuses(),
        state.sequencer.get_all_market_volumes(),
        state.sequencer.get_bridge_state(),
    ) {
        Ok(values) => values,
        Err(error) => {
            tracing::warn!(error = %error, "failed to collect live market metrics");
            return;
        }
    };

    let ref_prices = state.reference_prices.read().await;
    let market_ref_data = state.market_ref_data.read().await;
    let updated_at_ms = *state.reference_prices_updated_at_ms.read().await;
    let mut active_markets = 0u64;
    let mut active_reference_eligible_markets = 0u64;
    let mut active_reference_prices = 0u64;
    let mut priced_markets = 0u64;
    let mut volume_markets = 0u64;
    let mut diff_count = 0u64;
    let mut diff_sum = 0u64;
    let mut diff_max = 0u64;

    for market in markets.iter() {
        let status = statuses
            .get(&market.id)
            .cloned()
            .unwrap_or(matching_sequencer::MarketStatus::Active);
        let is_active = status.as_str() == "active";
        if is_active {
            active_markets += 1;
            // Coverage alerts must compare like with like: only active mirror
            // markets are eligible for a Polymarket reference. The raw map is
            // append/update-only and intentionally remains a separate gauge.
            let is_reference_eligible = market_ref_data
                .get(&market.id.0)
                .and_then(|data| data.polymarket_condition_id.as_ref())
                .is_some();
            if is_reference_eligible {
                active_reference_eligible_markets += 1;
                if ref_prices.contains_key(&market.id.0) {
                    active_reference_prices += 1;
                }
            }
        }

        let yes_price = prices
            .get(&market.id)
            .and_then(|market_prices| market_prices.first().copied());
        if yes_price.is_some() {
            priced_markets += 1;
        }
        if volumes.get(&market.id).copied().unwrap_or(0) > 0 {
            volume_markets += 1;
        }

        let Some(reference_price) = ref_prices.get(&market.id.0).copied() else {
            continue;
        };
        metrics::gauge!("sybil_reference_price_nanos", "market_id" => market.id.0.to_string())
            .set(reference_price as f64);

        if let Some(yes_price) = yes_price {
            let diff = yes_price.0.abs_diff(reference_price);
            metrics::gauge!("sybil_price_reference_diff_nanos", "market_id" => market.id.0.to_string())
                .set(diff as f64);
            diff_count += 1;
            diff_sum = diff_sum.saturating_add(diff);
            diff_max = diff_max.max(diff);
        }
    }

    metrics::gauge!("sybil_markets_active_total").set(active_markets as f64);
    metrics::gauge!("sybil_markets_priced_total").set(priced_markets as f64);
    metrics::gauge!("sybil_markets_with_volume_total").set(volume_markets as f64);
    metrics::gauge!("sybil_reference_prices_total").set(ref_prices.len() as f64);
    metrics::gauge!("sybil_reference_eligible_markets_active_total")
        .set(active_reference_eligible_markets as f64);
    metrics::gauge!("sybil_reference_prices_active_total").set(active_reference_prices as f64);
    metrics::gauge!("sybil_price_reference_pairs_total").set(diff_count as f64);
    metrics::gauge!("sybil_price_reference_max_abs_diff_nanos").set(diff_max as f64);
    metrics::gauge!("sybil_price_reference_avg_abs_diff_nanos").set(if diff_count == 0 {
        0.0
    } else {
        diff_sum as f64 / diff_count as f64
    });

    let updated_at_seconds = updated_at_ms as f64 / 1000.0;
    metrics::gauge!("sybil_reference_prices_last_updated_seconds").set(updated_at_seconds);
    metrics::gauge!("sybil_reference_prices_age_seconds").set(if updated_at_ms == 0 {
        0.0
    } else {
        (now_ms().saturating_sub(updated_at_ms) as f64) / 1000.0
    });
    metrics::gauge!("sybil_quarantine_ledger_size").set(bridge.quarantine.len() as f64);
    metrics::gauge!("sybil_quarantined_amount_nanos").set(
        bridge
            .quarantine
            .values()
            .copied()
            .fold(0i64, i64::saturating_add) as f64,
    );
}

async fn record_bot_metrics(state: &AppState) {
    let path = state.arena_db_path.clone();
    let snapshot =
        match tokio::task::spawn_blocking(move || arena::load_bot_metrics_snapshot(&path)).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(error = %error, "bot metrics task failed");
                arena::BotMetricsSnapshot::unavailable()
            }
        };

    metrics::gauge!("sybil_bot_db_available").set(if snapshot.db_available { 1.0 } else { 0.0 });
    metrics::gauge!("sybil_bot_traders_total").set(snapshot.traders.len() as f64);

    for trader in snapshot.traders {
        metrics::gauge!("sybil_bot_total_fills", "trader" => trader.name.clone())
            .set(trader.total_fills.unwrap_or(0) as f64);
        metrics::gauge!("sybil_bot_total_orders", "trader" => trader.name)
            .set(trader.total_orders.unwrap_or(0) as f64);
    }
}

async fn http_metrics(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = metric_path_label(&req);
    let start = Instant::now();

    let response = next.run(req).await;

    let duration_secs = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    metrics::counter!("sybil_http_requests_total", "method" => method.to_string(), "path" => path.clone(), "status" => status.to_string()).increment(1);
    metrics::histogram!("sybil_http_request_duration_seconds", "method" => method.to_string(), "path" => path)
        .record(duration_secs);

    response
}

/// Prometheus `path` label for a request. Matched routes reuse axum's
/// [`MatchedPath`] extension — the registered route template (e.g.
/// `/v1/markets/{id}`) is exactly the label we want, so there is no
/// hand-maintained route table to keep in sync. Unmatched requests (no
/// `MatchedPath`, i.e. 404s) bucket by their first path segment.
fn metric_path_label(req: &Request<axum::body::Body>) -> String {
    match req.extensions().get::<MatchedPath>() {
        Some(matched) => matched.as_str().to_string(),
        None => unmatched_metric_label(req.uri().path()).to_string(),
    }
}

/// Bucket label for a request that matched no route. Keeps `/v1`-prefixed
/// probes separate from everything else, mirroring the previous hand-match.
fn unmatched_metric_label(path: &str) -> &'static str {
    let first = path.trim_matches('/').split('/').next().unwrap_or("");
    if first == "v1" {
        "/v1/{unmatched}"
    } else {
        "/{unmatched}"
    }
}

fn http_rate_limit_client_key(
    req: &Request<axum::body::Body>,
    trusted_proxies: &[ipnet::IpNet],
) -> String {
    let Some(peer) = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.ip())
    else {
        return "direct".to_string();
    };
    if !is_trusted_proxy(peer, trusted_proxies) {
        return peer.to_string();
    }

    forwarded_for_client(req, peer, trusted_proxies)
        .or_else(|| header_ip(req, "x-real-ip"))
        .unwrap_or(peer)
        .to_string()
}

fn is_trusted_proxy(ip: IpAddr, trusted_proxies: &[ipnet::IpNet]) -> bool {
    trusted_proxies.iter().any(|network| network.contains(&ip))
}

fn header_ip(req: &Request<axum::body::Body>, name: &str) -> Option<IpAddr> {
    req.headers().get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Walk X-Forwarded-For from the trusted edge inward. A client cannot spoof
/// an address to the left of its own address because the first untrusted hop
/// encountered from the right wins.
fn forwarded_for_client(
    req: &Request<axum::body::Body>,
    peer: IpAddr,
    trusted_proxies: &[ipnet::IpNet],
) -> Option<IpAddr> {
    let value = req.headers().get("x-forwarded-for")?.to_str().ok()?;
    let mut chain = value
        .split(',')
        .map(str::trim)
        .map(str::parse::<IpAddr>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if chain.is_empty() {
        return None;
    }
    chain.push(peer);
    chain
        .iter()
        .rev()
        .copied()
        .find(|ip| !is_trusted_proxy(*ip, trusted_proxies))
        .or_else(|| chain.first().copied())
}

fn is_order_write_path(path: &str) -> bool {
    matches!(
        path,
        "/v1/orders" | "/v1/orders/signed" | "/v1/orders/cancel/signed"
    )
}

async fn order_rate_limit(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.method() == axum::http::Method::POST && is_order_write_path(req.uri().path()) {
        let client_key =
            http_rate_limit_client_key(&req, state.http_trusted_proxy_cidrs.as_slice());
        let allowed = state.http_order_limiter.allow(&client_key);
        if let Err(retry_after_secs) = allowed {
            metrics::counter!("sybil_http_order_rate_limited_total").increment(1);
            return AppError::rate_limited(retry_after_secs).into_response();
        }
    }
    next.run(req).await
}

fn is_da_read_path(path: &str) -> bool {
    let mut segments = path.trim_matches('/').split('/');
    matches!(segments.next(), Some("v1"))
        && matches!(segments.next(), Some("da"))
        && segments
            .next()
            .is_some_and(|height| height.parse::<u64>().is_ok())
        && matches!(segments.next(), Some("manifest" | "payload"))
        && segments.next().is_none()
}

async fn da_read_limit(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.method() != axum::http::Method::GET || !is_da_read_path(req.uri().path()) {
        return next.run(req).await;
    }

    let client_key = http_rate_limit_client_key(&req, state.http_trusted_proxy_cidrs.as_slice());
    let allowed = state.http_da_limiter.allow(&client_key);
    if let Err(retry_after_secs) = allowed {
        metrics::counter!("sybil_http_da_rate_limited_total", "reason" => "rate").increment(1);
        return AppError::rate_limited(retry_after_secs).into_response();
    }

    let Ok(permit) = state.http_da_concurrency.clone().try_acquire_owned() else {
        metrics::counter!("sybil_http_da_rate_limited_total", "reason" => "concurrency")
            .increment(1);
        return AppError::rate_limited(1).into_response();
    };
    let response = next.run(req).await;
    drop(permit);
    response
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteMount {
    pub method: &'static str,
    pub path: &'static str,
}

pub const PUBLIC_ROUTE_TABLE: &[RouteMount] = &[
    RouteMount {
        method: "GET",
        path: "/openapi.json",
    },
    RouteMount {
        method: "GET",
        path: "/metrics",
    },
    RouteMount {
        method: "GET",
        path: "/v1/bots/decisions",
    },
    RouteMount {
        method: "GET",
        path: "/v1/bots/equity-series",
    },
    RouteMount {
        method: "GET",
        path: "/v1/leaderboard",
    },
    RouteMount {
        method: "GET",
        path: "/v1/health",
    },
    RouteMount {
        method: "GET",
        path: "/v1/state-root",
    },
    RouteMount {
        method: "GET",
        path: "/v1/da/{height}/manifest",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/keyop-state",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/keys/register",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/keys/revoke",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/profile",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/api-keys",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/api-keys/revoke",
    },
    RouteMount {
        method: "GET",
        path: "/v1/bridge/status",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/search",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/summary",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/groups",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/prices",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}/resolution",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}/prices/history",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}/prices/candles",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}/open-batch",
    },
    RouteMount {
        method: "GET",
        path: "/v1/activity/overview",
    },
    RouteMount {
        method: "GET",
        path: "/v1/events/{event_id}/traders",
    },
    RouteMount {
        method: "GET",
        path: "/v1/events/{event_id}/raw",
    },
    RouteMount {
        method: "GET",
        path: "/v1/feeds",
    },
    RouteMount {
        method: "POST",
        path: "/v1/orders/signed",
    },
    RouteMount {
        method: "POST",
        path: "/v1/orders/cancel/signed",
    },
    RouteMount {
        method: "GET",
        path: "/v1/blocks",
    },
    RouteMount {
        method: "GET",
        path: "/v1/blocks/latest",
    },
    RouteMount {
        method: "GET",
        path: "/v1/blocks/stream",
    },
    RouteMount {
        method: "GET",
        path: "/v2/blocks/ws",
    },
    RouteMount {
        method: "GET",
        path: "/v1/blocks/{height}",
    },
];

/// Per-account reads that accept either an owner read key or the service token.
pub const OWNER_ROUTE_TABLE: &[RouteMount] = &[
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/portfolio",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/fills",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/equity",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/events",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/orders",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/keys",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/api-keys",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/bridge-key",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/withdrawals",
    },
    RouteMount {
        method: "GET",
        path: "/v1/accounts/{id}/private-summary",
    },
];

pub const SERVICE_ROUTE_TABLE: &[RouteMount] = &[
    RouteMount {
        method: "GET",
        path: "/v1/blocks/ws",
    },
    RouteMount {
        method: "POST",
        path: "/v1/orders",
    },
    RouteMount {
        method: "GET",
        path: "/v1/proofs/state/{leaf_key_hex}",
    },
    RouteMount {
        method: "GET",
        path: "/v1/da/{height}/payload",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/fund",
    },
    RouteMount {
        method: "POST",
        path: "/v1/accounts/{id}/keys",
    },
    RouteMount {
        method: "GET",
        path: "/v1/bridge/accounts/by-key/{key_hex}",
    },
    RouteMount {
        method: "POST",
        path: "/v1/bridge/deposits",
    },
    RouteMount {
        method: "POST",
        path: "/v1/bridge/withdrawals",
    },
    RouteMount {
        method: "POST",
        path: "/v1/bridge/withdrawals/signed",
    },
    RouteMount {
        method: "POST",
        path: "/v1/bridge/withdrawals/l1-events",
    },
    RouteMount {
        method: "POST",
        path: "/v1/bridge/l1-height",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets/groups",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets/groups/{group_id}/members",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets/{id}/resolve",
    },
    RouteMount {
        method: "PUT",
        path: "/v1/events/{event_id}/raw",
    },
    RouteMount {
        method: "POST",
        path: "/v1/feeds",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets/prices/reference",
    },
    RouteMount {
        method: "POST",
        path: "/v1/markets/{id}/metadata",
    },
    RouteMount {
        method: "POST",
        path: "/v1/admin/auto-resolutions",
    },
    RouteMount {
        method: "GET",
        path: "/v1/admin/auto-resolutions",
    },
    RouteMount {
        method: "POST",
        path: "/v1/admin/auto-resolutions/{id}/approve",
    },
    RouteMount {
        method: "POST",
        path: "/v1/admin/auto-resolutions/{id}/reject",
    },
];

pub const DEV_ROUTE_TABLE: &[RouteMount] = &[
    RouteMount {
        method: "GET",
        path: "/v1/attestation",
    },
    RouteMount {
        method: "POST",
        path: "/v1/simulation/pause",
    },
    RouteMount {
        method: "POST",
        path: "/v1/simulation/resume",
    },
    RouteMount {
        method: "GET",
        path: "/v1/orders/pending",
    },
    RouteMount {
        method: "GET",
        path: "/v1/markets/{id}/orderbook",
    },
];

fn public_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .route("/metrics", axum::routing::get(prometheus_metrics))
        .routes(openapi_routes!(routes::bots::get_bot_decisions))
        .routes(openapi_routes!(routes::bots::get_bot_equity_series))
        .routes(openapi_routes!(routes::leaderboard::get_leaderboard))
        .routes(openapi_routes!(routes::system::health))
        .routes(openapi_routes!(routes::system::state_root))
        .routes(openapi_routes!(routes::da::get_da_manifest))
        // Self-service onboarding is public only in its atomic form. The
        // deprecated unsigned forms enforce service auth in their handlers.
        .routes(openapi_routes!(routes::accounts::create_account))
        .routes(openapi_routes!(routes::accounts::get_account))
        .routes(openapi_routes!(routes::accounts::get_keyop_state))
        .routes(openapi_routes!(routes::accounts::list_account_keys))
        .routes(openapi_routes!(routes::accounts::register_signed_key))
        .routes(openapi_routes!(routes::accounts::revoke_key))
        .routes(openapi_routes!(routes::accounts::set_profile))
        .routes(openapi_routes!(
            routes::accounts::list_api_keys,
            routes::accounts::create_api_key,
        ))
        .routes(openapi_routes!(routes::accounts::revoke_api_key))
        .routes(openapi_routes!(routes::accounts::get_private_summary))
        .routes(openapi_routes!(routes::accounts::history::get_portfolio))
        .routes(openapi_routes!(
            routes::accounts::history::get_account_fills
        ))
        .routes(openapi_routes!(routes::accounts::history::get_equity))
        .routes(openapi_routes!(
            routes::accounts::history::get_account_history
        ))
        .routes(openapi_routes!(routes::bridge::account_key))
        .routes(openapi_routes!(routes::bridge::list_account_withdrawals))
        .routes(openapi_routes!(routes::orders::get_account_orders))
        .routes(openapi_routes!(routes::bridge::status))
        .routes(openapi_routes!(routes::markets::search_markets))
        .routes(openapi_routes!(routes::markets::list_markets_summary))
        .routes(openapi_routes!(routes::markets::list_markets))
        .routes(openapi_routes!(routes::markets::list_market_groups))
        .routes(openapi_routes!(routes::markets::get_prices))
        .routes(openapi_routes!(routes::markets::get_market))
        .routes(openapi_routes!(routes::markets::get_resolution))
        .routes(openapi_routes!(routes::markets::get_price_history))
        .routes(openapi_routes!(routes::markets::get_price_candles))
        .routes(openapi_routes!(routes::aggregates::get_open_batch))
        .routes(openapi_routes!(routes::aggregates::get_activity_overview))
        .routes(openapi_routes!(routes::aggregates::get_event_traders))
        .routes(openapi_routes!(routes::events::get_event_raw))
        .routes(openapi_routes!(routes::feeds::list_feeds))
        // Signed trader orders carry their own P256/WebAuthn authorization.
        .routes(openapi_routes!(routes::orders::submit_signed_order))
        .routes(openapi_routes!(routes::orders::cancel_signed_order))
        .routes(openapi_routes!(routes::blocks::get_recent_blocks))
        .routes(openapi_routes!(routes::blocks::get_latest_block))
        .routes(openapi_routes!(routes::blocks::stream_blocks))
        .routes(openapi_routes!(routes::blocks::ws_blocks))
        .routes(openapi_routes!(routes::blocks::get_block_by_height))
}

fn service_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(openapi_routes!(routes::blocks::ws_service_blocks))
        // Unsigned orders can name arbitrary accounts and MM budgets.
        .routes(openapi_routes!(routes::orders::submit_orders))
        .routes(openapi_routes!(routes::proofs::get_state_proof))
        .routes(openapi_routes!(routes::da::get_da_payload))
        .routes(openapi_routes!(routes::prover::get_next_proof_job))
        .routes(openapi_routes!(routes::prover::acknowledge_proof_job))
        .routes(openapi_routes!(routes::accounts::fund_account))
        .routes(openapi_routes!(routes::accounts::register_key))
        .routes(openapi_routes!(routes::bridge::account_by_key))
        .routes(openapi_routes!(routes::bridge::submit_l1_deposit))
        .routes(openapi_routes!(routes::bridge::create_withdrawal))
        .routes(openapi_routes!(routes::bridge::create_signed_withdrawal))
        .routes(openapi_routes!(routes::bridge::submit_l1_withdrawal_event))
        .routes(openapi_routes!(routes::bridge::observe_l1_height))
        .routes(openapi_routes!(routes::markets::create_market))
        .routes(openapi_routes!(routes::markets::create_market_group))
        .routes(openapi_routes!(routes::markets::extend_market_group))
        .routes(openapi_routes!(routes::markets::resolve_market))
        .routes(openapi_routes!(routes::events::put_event_raw))
        .routes(openapi_routes!(routes::feeds::register_feed))
        .routes(openapi_routes!(routes::markets::set_reference_prices))
        .routes(openapi_routes!(routes::markets::set_market_metadata))
        .routes(openapi_routes!(
            routes::auto_resolution::submit_auto_resolution,
            routes::auto_resolution::list_auto_resolutions,
        ))
        .routes(openapi_routes!(
            routes::auto_resolution::approve_auto_resolution
        ))
        .routes(openapi_routes!(
            routes::auto_resolution::reject_auto_resolution
        ))
}

fn dev_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(openapi_routes!(routes::system::attestation))
        .routes(openapi_routes!(routes::system::pause))
        .routes(openapi_routes!(routes::system::resume))
        .routes(openapi_routes!(routes::orders::get_all_pending_orders))
        .routes(openapi_routes!(routes::orders::get_market_orderbook))
}

/// Generate the same OpenAPI document that the runtime router serves. Route
/// annotations are collected from the actual handler registrations, so adding
/// a route no longer requires editing a second `ApiDoc::paths` list.
pub fn openapi_document(include_dev_routes: bool) -> utoipa::openapi::OpenApi {
    let mut routes = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(public_routes())
        .merge(service_routes());
    if include_dev_routes {
        routes = routes.merge(dev_routes());
    }
    routes.split_for_parts().1
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&left, &right) in a.iter().zip(b) {
        diff |= left ^ right;
    }
    diff == 0
}

fn bearer_token_from_headers(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
}

fn bearer_token(req: &Request<axum::body::Body>) -> Option<&str> {
    bearer_token_from_headers(req.headers())
}

/// Returns true iff `headers` carry a bearer token that matches the configured
/// service token, using the SAME source of truth (`state.service_token`) and the
/// SAME constant-time comparison the `service_auth` middleware applies. Public
/// handlers on the public tier call this to grant trusted service infra an
/// elevated privilege (e.g. skipping the demo-balance cap) without moving the
/// whole route behind `service_auth`. A missing/garbage header, or an unset
/// service token, simply returns false (never an error).
pub(crate) fn request_has_valid_service_token(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> bool {
    let Some(expected) = state.service_token.as_deref() else {
        return false;
    };
    let Some(actual) = bearer_token_from_headers(headers) else {
        return false;
    };
    constant_time_eq(actual.as_bytes(), expected.as_bytes())
}

/// Apply the service-tier bearer policy to a handler-level hybrid route.
/// Dev mode mirrors `service_auth`; production distinguishes missing (401)
/// from invalid (403) credentials.
pub(crate) fn require_service_token(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), AppError> {
    if state.dev_mode {
        return Ok(());
    }
    let Some(expected) = state.service_token.as_deref() else {
        return Err(AppError::unauthorized("Service token is not configured"));
    };
    let Some(actual) = bearer_token_from_headers(headers) else {
        return Err(AppError::unauthorized("Missing service bearer token"));
    };
    if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
        return Err(AppError::forbidden("Invalid service bearer token"));
    }
    Ok(())
}

async fn service_auth(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if state.dev_mode {
        return next.run(req).await;
    }

    let Some(expected) = state.service_token.as_deref() else {
        return AppError::unauthorized("Service token is not configured").into_response();
    };
    let Some(actual) = bearer_token(&req) else {
        return AppError::unauthorized("Missing service bearer token").into_response();
    };
    if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
        return AppError::forbidden("Invalid service bearer token").into_response();
    }

    next.run(req).await
}

fn cors_layer(state: &AppState) -> CorsLayer {
    if state.dev_mode {
        return CorsLayer::permissive();
    }
    if state.cors_origins.is_empty() {
        return CorsLayer::new();
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(state.cors_origins.clone()))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

pub fn create_router(state: AppState) -> Router {
    let mut app = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(public_routes())
        .merge(
            service_routes()
                .route_layer(middleware::from_fn_with_state(state.clone(), service_auth)),
        );
    if state.dev_mode {
        app = app.merge(dev_routes());
    }
    let (app, openapi) = app.split_for_parts();
    let app = app.route(
        "/openapi.json",
        axum::routing::get(move || {
            let openapi = openapi.clone();
            async move { Json(openapi) }
        }),
    );
    let cors = cors_layer(&state);

    app.layer(middleware::from_fn_with_state(
        state.clone(),
        order_rate_limit,
    ))
    .layer(middleware::from_fn_with_state(state.clone(), da_read_limit))
    .layer(middleware::from_fn(http_metrics))
    .layer(
        TraceLayer::new_for_http()
            .make_span_with(|req: &Request<axum::body::Body>| {
                tracing::info_span!(
                    "http.request",
                    method = %req.method(),
                    path = %req.uri().path(),
                )
            })
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
    .layer(cors)
    .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::middleware::{self, Next};
    use axum::response::Response;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::{
        DEV_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, SERVICE_ROUTE_TABLE, http_rate_limit_client_key,
        metric_path_label, unmatched_metric_label,
    };

    fn client_key_request(peer: Option<IpAddr>, forwarded_for: Option<&str>) -> Request<Body> {
        let mut request = Request::builder().uri("/v1/orders");
        if let Some(forwarded_for) = forwarded_for {
            request = request.header("x-forwarded-for", forwarded_for);
        }
        let mut request = request.body(Body::empty()).unwrap();
        if let Some(peer) = peer {
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::new(peer, 9_999)));
        }
        request
    }

    /// Middleware that stamps the derived metric label onto the response so a
    /// test can observe what `http_metrics` would record.
    async fn label_probe(req: Request<Body>, next: Next) -> Response {
        let label = metric_path_label(&req);
        let mut resp = next.run(req).await;
        resp.headers_mut()
            .insert("x-metric-label", label.parse().unwrap());
        resp
    }

    /// Turn a route template into a concrete request path (`{id}` -> `1`).
    fn concretize(template: &str) -> String {
        template
            .split('/')
            .map(|seg| if seg.starts_with('{') { "1" } else { seg })
            .collect::<Vec<_>>()
            .join("/")
    }

    async fn label_for(router: &Router, uri: &str) -> String {
        let resp = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        resp.headers()
            .get("x-metric-label")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    /// Every mounted route's `MatchedPath`-derived label must equal its template
    /// string — the parity that lets the derivation replace the old hand-match.
    #[tokio::test]
    async fn matched_path_labels_equal_route_templates() {
        let paths: BTreeSet<&str> = PUBLIC_ROUTE_TABLE
            .iter()
            .chain(SERVICE_ROUTE_TABLE)
            .chain(DEV_ROUTE_TABLE)
            .map(|mount| mount.path)
            .collect();

        // `MatchedPath` keys on the path template, not the method, so registering
        // each template once under a GET handler is enough to exercise the label.
        let mut router = Router::new();
        for path in &paths {
            router = router.route(path, get(|| async { StatusCode::OK }));
        }
        let router = router.layer(middleware::from_fn(label_probe));

        for path in &paths {
            let uri = concretize(path);
            assert_eq!(label_for(&router, &uri).await, *path, "uri {uri}");
        }
    }

    #[test]
    fn unmatched_routes_bucket_by_prefix() {
        assert_eq!(unmatched_metric_label("/trade"), "/{unmatched}");
        assert_eq!(
            unmatched_metric_label("/v1/accounts/1/fills/extra"),
            "/v1/{unmatched}"
        );
        assert_eq!(unmatched_metric_label("/wp-login.php"), "/{unmatched}");
    }

    #[test]
    fn untrusted_peers_cannot_spoof_forwarding_headers() {
        let request = client_key_request(
            Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7))),
            Some("192.0.2.123"),
        );
        let trusted = ["10.0.0.0/8".parse().unwrap()];
        assert_eq!(
            http_rate_limit_client_key(&request, &trusted),
            "198.51.100.7"
        );
    }

    #[test]
    fn trusted_proxy_chain_selects_first_untrusted_hop_from_right() {
        let request = client_key_request(
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))),
            Some("192.0.2.123, 203.0.113.9, 10.1.0.4"),
        );
        let trusted = ["10.0.0.0/8".parse().unwrap()];
        assert_eq!(
            http_rate_limit_client_key(&request, &trusted),
            "203.0.113.9"
        );
    }

    #[test]
    fn requests_without_connection_metadata_share_the_safe_direct_bucket() {
        let request = client_key_request(None, Some("192.0.2.123"));
        let trusted = ["0.0.0.0/0".parse().unwrap()];
        assert_eq!(http_rate_limit_client_key(&request, &trusted), "direct");
    }

    /// Requests that match no route carry no `MatchedPath`, so the middleware
    /// falls back to the prefix buckets.
    #[tokio::test]
    async fn unmatched_requests_use_bucket_labels() {
        let router = Router::new()
            .route("/v1/health", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(label_probe));

        for (uri, expected) in [
            ("/trade", "/{unmatched}"),
            ("/v1/accounts/1/fills/extra", "/v1/{unmatched}"),
            ("/wp-login.php", "/{unmatched}"),
        ] {
            assert_eq!(label_for(&router, uri).await, expected, "uri {uri}");
        }
    }
}
