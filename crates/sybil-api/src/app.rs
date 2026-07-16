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
        OnboardAccountRequest,
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
        OnboardingPolicyResponse,
        PositionResponse,
        BridgeStatusResponse,
        BridgeDomainResponse,
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

    let reference_snapshot = state.reference_price_snapshot().await;
    let ref_prices = &reference_snapshot.fresh_prices;
    let market_ref_data = state.market_ref_data.read().await;
    let updated_at_ms = reference_snapshot.last_publisher_update_at_ms;
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
            // markets are eligible for a Polymarket reference. Stored stock
            // and fresh eligible coverage intentionally remain separate gauges.
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

        let market_id = market.id.0;
        let reference_price = ref_prices.get(&market_id).map(|price| price.price_nanos);
        let reference_age_ms = reference_snapshot.age_ms_by_market.get(&market_id).copied();
        let reference_expired = reference_age_ms.is_some() && reference_price.is_none();
        metrics::gauge!("sybil_reference_price_available", "market_id" => market_id.to_string())
            .set(if reference_price.is_some() { 1.0 } else { 0.0 });
        metrics::gauge!("sybil_reference_price_expired", "market_id" => market_id.to_string())
            .set(if reference_expired { 1.0 } else { 0.0 });
        metrics::gauge!("sybil_reference_price_age_seconds", "market_id" => market_id.to_string())
            .set(reference_age_ms.unwrap_or(0) as f64 / 1_000.0);
        metrics::gauge!("sybil_reference_price_nanos", "market_id" => market_id.to_string())
            .set(reference_price.unwrap_or(0) as f64);
        metrics::gauge!("sybil_price_reference_diff_nanos", "market_id" => market_id.to_string())
            .set(0.0);

        let Some(reference_price) = reference_price else {
            continue;
        };

        if let Some(yes_price) = yes_price {
            let diff = yes_price.0.abs_diff(reference_price);
            metrics::gauge!("sybil_price_reference_diff_nanos", "market_id" => market_id.to_string())
                .set(diff as f64);
            diff_count += 1;
            diff_sum = diff_sum.saturating_add(diff);
            diff_max = diff_max.max(diff);
        }
    }

    metrics::gauge!("sybil_markets_active_total").set(active_markets as f64);
    metrics::gauge!("sybil_markets_priced_total").set(priced_markets as f64);
    metrics::gauge!("sybil_markets_with_volume_total").set(volume_markets as f64);
    metrics::gauge!("sybil_reference_prices_total").set(reference_snapshot.stored_count as f64);
    metrics::gauge!("sybil_reference_prices_expired_total")
        .set(reference_snapshot.expired_count as f64);
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

fn is_onboarding_write_path(path: &str) -> bool {
    path == "/v1/onboarding/accounts"
}

async fn onboarding_rate_limit(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.method() == axum::http::Method::POST && is_onboarding_write_path(req.uri().path()) {
        let client_key =
            http_rate_limit_client_key(&req, state.http_trusted_proxy_cidrs.as_slice());
        if let Err(retry_after_secs) = state.http_onboarding_limiter.allow(&client_key) {
            metrics::counter!("sybil_http_onboarding_rate_limited_total").increment(1);
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

/// Declare one trust tier's audited route registry. Each entry emits both the
/// runtime mount and the policy manifest consumed by auth/OpenAPI tests. The
/// handler's `utoipa::path` annotation remains an independent schema contract;
/// `openapi_drift.rs` proves its method/path agrees with this registry.
macro_rules! declare_route_registry {
    (
        $table:ident, $builder:ident,
        manual { $( $manual_method:literal $manual_path:literal => $manual_route:expr; )* }
        documented { $( $method:literal $path:literal => $handler:path; )* }
    ) => {
        pub const $table: &[RouteMount] = &[
            $( RouteMount { method: $manual_method, path: $manual_path }, )*
            $( RouteMount { method: $method, path: $path }, )*
        ];

        fn $builder() -> OpenApiRouter<AppState> {
            let router = OpenApiRouter::default();
            $( let router = router.route($manual_path, $manual_route); )*
            $( let router = router.routes(openapi_routes!($handler)); )*
            router
        }
    };
}

async fn openapi_json(State(state): State<AppState>) -> Json<utoipa::openapi::OpenApi> {
    Json(openapi_document(state.dev_mode))
}

declare_route_registry! {
    PUBLIC_ROUTE_TABLE, public_routes,
    manual {
        "GET" "/openapi.json" => axum::routing::get(openapi_json);
        "GET" "/metrics" => axum::routing::get(prometheus_metrics);
    }
    documented {
        "GET" "/v1/bots/decisions" => routes::bots::get_bot_decisions;
        "GET" "/v1/bots/equity-series" => routes::bots::get_bot_equity_series;
        "GET" "/v1/leaderboard" => routes::leaderboard::get_leaderboard;
        "GET" "/v1/health" => routes::system::health;
        "GET" "/v1/state-root" => routes::system::state_root;
        "GET" "/v1/da/{height}/manifest" => routes::da::get_da_manifest;
        "GET" "/v1/onboarding" => routes::accounts::get_onboarding_policy;
        "POST" "/v1/onboarding/accounts" => routes::accounts::onboard_account;
        "GET" "/v1/accounts/{id}/keyop-state" => routes::accounts::get_keyop_state;
        "POST" "/v1/accounts/{id}/keys/register" => routes::accounts::register_signed_key;
        "POST" "/v1/accounts/{id}/keys/revoke" => routes::accounts::revoke_key;
        "POST" "/v1/accounts/{id}/profile" => routes::accounts::set_profile;
        "POST" "/v1/accounts/{id}/api-keys" => routes::accounts::create_api_key;
        "POST" "/v1/accounts/{id}/api-keys/revoke" => routes::accounts::revoke_api_key;
        "GET" "/v1/bridge/status" => routes::bridge::status;
        "GET" "/v1/markets/search" => routes::markets::search_markets;
        "GET" "/v1/markets/summary" => routes::markets::list_markets_summary;
        "GET" "/v1/markets" => routes::markets::list_markets;
        "GET" "/v1/markets/groups" => routes::markets::list_market_groups;
        "GET" "/v1/markets/prices" => routes::markets::get_prices;
        "GET" "/v1/markets/{id}" => routes::markets::get_market;
        "GET" "/v1/markets/{id}/resolution" => routes::markets::get_resolution;
        "GET" "/v1/markets/{id}/prices/history" => routes::markets::get_price_history;
        "GET" "/v1/markets/{id}/prices/candles" => routes::markets::get_price_candles;
        "GET" "/v1/markets/{id}/open-batch" => routes::aggregates::get_open_batch;
        "GET" "/v1/activity/overview" => routes::aggregates::get_activity_overview;
        "GET" "/v1/events/{event_id}/traders" => routes::aggregates::get_event_traders;
        "GET" "/v1/events/{event_id}/raw" => routes::events::get_event_raw;
        "GET" "/v1/feeds" => routes::feeds::list_feeds;
        "POST" "/v1/orders/signed" => routes::orders::submit_signed_order;
        "POST" "/v1/orders/cancel/signed" => routes::orders::cancel_signed_order;
        "GET" "/v1/blocks" => routes::blocks::get_recent_blocks;
        "GET" "/v1/blocks/latest" => routes::blocks::get_latest_block;
        "GET" "/v2/blocks/ws" => routes::blocks::ws_blocks;
        "GET" "/v1/blocks/{height}" => routes::blocks::get_block_by_height;
    }
}

// Per-account reads accept either an owner read key or the service token.
declare_route_registry! {
    OWNER_ROUTE_TABLE, owner_routes,
    manual {}
    documented {
        "GET" "/v1/accounts/{id}" => routes::accounts::get_account;
        "GET" "/v1/accounts/{id}/portfolio" => routes::accounts::history::get_portfolio;
        "GET" "/v1/accounts/{id}/fills" => routes::accounts::history::get_account_fills;
        "GET" "/v1/accounts/{id}/equity" => routes::accounts::history::get_equity;
        "GET" "/v1/accounts/{id}/events" => routes::accounts::history::get_account_history;
        "GET" "/v1/accounts/{id}/orders" => routes::orders::get_account_orders;
        "GET" "/v1/accounts/{id}/keys" => routes::accounts::list_account_keys;
        "GET" "/v1/accounts/{id}/api-keys" => routes::accounts::list_api_keys;
        "GET" "/v1/accounts/{id}/bridge-key" => routes::bridge::account_key;
        "GET" "/v1/accounts/{id}/withdrawals" => routes::bridge::list_account_withdrawals;
        "GET" "/v1/accounts/{id}/private-summary" => routes::accounts::get_private_summary;
    }
}

declare_route_registry! {
    SERVICE_ROUTE_TABLE, service_routes,
    manual {}
    documented {
        "POST" "/v1/accounts" => routes::accounts::create_account;
        "GET" "/v1/blocks/ws" => routes::blocks::ws_service_blocks;
        "POST" "/v1/orders" => routes::orders::submit_orders;
        "GET" "/v1/proofs/state/{leaf_key_hex}" => routes::proofs::get_state_proof;
        "GET" "/v1/da/{height}/payload" => routes::da::get_da_payload;
        "GET" "/v1/prover/jobs/next" => routes::prover::get_next_proof_job;
        "POST" "/v1/prover/jobs/{height}/ack" => routes::prover::acknowledge_proof_job;
        "POST" "/v1/accounts/{id}/fund" => routes::accounts::fund_account;
        "POST" "/v1/accounts/{id}/keys" => routes::accounts::register_key;
        "GET" "/v1/bridge/accounts/by-key/{key_hex}" => routes::bridge::account_by_key;
        "POST" "/v1/bridge/deposits" => routes::bridge::submit_l1_deposit;
        "GET" "/v1/bridge/withdrawals/pending" => routes::bridge::list_pending_withdrawals;
        "POST" "/v1/bridge/withdrawals" => routes::bridge::create_withdrawal;
        "POST" "/v1/bridge/withdrawals/signed" => routes::bridge::create_signed_withdrawal;
        "POST" "/v1/bridge/withdrawals/l1-events" => routes::bridge::submit_l1_withdrawal_event;
        "POST" "/v1/bridge/l1-height" => routes::bridge::observe_l1_height;
        "POST" "/v1/markets" => routes::markets::create_market;
        "POST" "/v1/markets/groups" => routes::markets::create_market_group;
        "POST" "/v1/markets/groups/{group_id}/members" => routes::markets::extend_market_group;
        "POST" "/v1/markets/{id}/resolve" => routes::markets::resolve_market;
        "PUT" "/v1/events/{event_id}/raw" => routes::events::put_event_raw;
        "POST" "/v1/feeds" => routes::feeds::register_feed;
        "POST" "/v1/markets/prices/reference" => routes::markets::set_reference_prices;
        "POST" "/v1/markets/{id}/metadata" => routes::markets::set_market_metadata;
        "POST" "/v1/admin/auto-resolutions" => routes::auto_resolution::submit_auto_resolution;
        "GET" "/v1/admin/auto-resolutions" => routes::auto_resolution::list_auto_resolutions;
        "POST" "/v1/admin/auto-resolutions/{id}/approve" => routes::auto_resolution::approve_auto_resolution;
        "POST" "/v1/admin/auto-resolutions/{id}/reject" => routes::auto_resolution::reject_auto_resolution;
    }
}

declare_route_registry! {
    DEV_ROUTE_TABLE, dev_routes,
    manual {}
    documented {
        "GET" "/v1/attestation" => routes::system::attestation;
        "POST" "/v1/simulation/pause" => routes::system::pause;
        "POST" "/v1/simulation/resume" => routes::system::resume;
        "GET" "/v1/orders/pending" => routes::orders::get_all_pending_orders;
        "GET" "/v1/markets/{id}/orderbook" => routes::orders::get_market_orderbook;
    }
}

/// Generate the same OpenAPI document that the runtime router serves. Route
/// annotations are collected from the actual handler registrations, so adding
/// a route no longer requires editing a second `ApiDoc::paths` list.
pub fn openapi_document(include_dev_routes: bool) -> utoipa::openapi::OpenApi {
    let mut routes = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(public_routes())
        .merge(owner_routes())
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
/// owner-scoped handlers call this to grant trusted service infrastructure
/// read access without moving the whole route behind `service_auth`. A
/// missing/garbage header, or an unset service token, simply returns false.
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
        .merge(owner_routes())
        .merge(
            service_routes()
                .route_layer(middleware::from_fn_with_state(state.clone(), service_auth)),
        );
    if state.dev_mode {
        app = app.merge(dev_routes());
    }
    let (app, _) = app.split_for_parts();
    let cors = cors_layer(&state);

    app.layer(middleware::from_fn_with_state(
        state.clone(),
        order_rate_limit,
    ))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        onboarding_rate_limit,
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
