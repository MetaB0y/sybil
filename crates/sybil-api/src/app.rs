use std::time::Instant;

use axum::extract::State;
use axum::http::{header, Request};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use utoipa::OpenApi;

use crate::routes;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::*;
use crate::types::response::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::system::health,
        routes::system::state_root,
        routes::accounts::create_account,
        routes::accounts::fund_account,
        routes::accounts::get_account,
        routes::accounts::register_key,
        routes::accounts::get_portfolio,
        routes::accounts::get_account_fills,
        routes::bridge::status,
        routes::bridge::account_key,
        routes::bridge::submit_l1_deposit,
        routes::bridge::create_withdrawal,
        routes::bridge::get_withdrawal,
        routes::markets::list_markets,
        routes::markets::list_markets_summary,
        routes::markets::get_market,
        routes::markets::create_market,
        routes::markets::list_market_groups,
        routes::markets::create_market_group,
        routes::markets::get_prices,
        routes::markets::resolve_market,
        routes::markets::get_resolution,
        routes::markets::get_price_history,
        routes::markets::search_markets,
        routes::markets::set_reference_prices,
        routes::markets::set_market_metadata,
        routes::feeds::register_feed,
        routes::feeds::list_feeds,
        routes::orders::submit_orders,
        routes::orders::submit_signed_order,
        routes::orders::cancel_signed_order,
        routes::orders::get_account_orders,
        routes::orders::get_market_orderbook,
        routes::orders::get_all_pending_orders,
        routes::blocks::get_latest_block,
        routes::blocks::get_block_by_height,
        routes::blocks::stream_blocks,
        routes::blocks::ws_blocks,
    ),
    components(schemas(
        CreateAccountRequest,
        FundAccountRequest,
        SubmitL1DepositRequest,
        CreateBridgeWithdrawalRequest,
        RegisterKeyRequest,
        CreateMarketRequest,
        CreateMarketGroupRequest,
        ResolveMarketRequest,
        SignedAttestationDto,
        RegisterFeedRequest,
        RegisteredFeedResponse,
        ResolutionResponse,
        SubmitOrderRequest,
        SubmitSignedOrderRequest,
        CancelSignedOrderRequest,
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
        HealthResponse,
        StateRootResponse,
        ResolveMarketResponse,
        PortfolioResponse,
        PositionValueResponse,
        PriceHistoryResponse,
        PricePointResponse,
        AccountFillResponse,
        PendingOrderResponse,
        PositionDeltaResponse,
    )),
    info(
        title = "Sybil API",
        description = "HTTP API for AI agent trading on Sybil prediction markets",
        version = "0.1.0"
    )
)]
pub struct ApiDoc;

async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

async fn dashboard() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../static/index.html"),
    )
}

async fn trade() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../static/trade.html"),
    )
}

async fn prometheus_metrics(State(state): State<AppState>) -> impl IntoResponse {
    state.prometheus.render()
}

async fn http_metrics(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(req).await;

    let duration_secs = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    metrics::counter!("sybil_http_requests_total", "method" => method.to_string(), "path" => path.clone(), "status" => status.to_string()).increment(1);
    metrics::histogram!("sybil_http_request_duration_seconds", "method" => method.to_string(), "path" => path.clone()).record(duration_secs);

    response
}

fn order_rate_limit_client_key(req: &Request<axum::body::Body>) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("direct")
        .to_string()
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
        let client_key = order_rate_limit_client_key(&req);
        let allowed = state
            .http_order_limiter
            .lock()
            .map(|mut limiter| limiter.allow(&client_key))
            .unwrap_or(Err(1));
        if let Err(retry_after_secs) = allowed {
            metrics::counter!("sybil_http_order_rate_limited_total").increment(1);
            return AppError::rate_limited(retry_after_secs).into_response();
        }
    }
    next.run(req).await
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Dashboard
        .route("/", axum::routing::get(dashboard))
        .route("/trade", axum::routing::get(trade))
        // OpenAPI spec
        .route("/openapi.json", axum::routing::get(openapi_json))
        // Metrics (outside http_metrics middleware to avoid self-scraping noise)
        .route("/metrics", axum::routing::get(prometheus_metrics))
        // Native arena / bot analytics
        .route(
            "/v1/bots/decisions",
            axum::routing::get(routes::bots::get_bot_decisions),
        )
        // System
        .route("/v1/health", axum::routing::get(routes::system::health))
        .route(
            "/v1/state-root",
            axum::routing::get(routes::system::state_root),
        )
        // Simulation control
        .route(
            "/v1/simulation/pause",
            axum::routing::post(routes::system::pause),
        )
        .route(
            "/v1/simulation/resume",
            axum::routing::post(routes::system::resume),
        )
        // Accounts
        .route(
            "/v1/accounts",
            axum::routing::post(routes::accounts::create_account),
        )
        .route(
            "/v1/accounts/{id}",
            axum::routing::get(routes::accounts::get_account),
        )
        .route(
            "/v1/accounts/{id}/fund",
            axum::routing::post(routes::accounts::fund_account),
        )
        .route(
            "/v1/accounts/{id}/keys",
            axum::routing::post(routes::accounts::register_key),
        )
        .route(
            "/v1/accounts/{id}/portfolio",
            axum::routing::get(routes::accounts::get_portfolio),
        )
        .route(
            "/v1/accounts/{id}/fills",
            axum::routing::get(routes::accounts::get_account_fills),
        )
        .route(
            "/v1/accounts/{id}/bridge-key",
            axum::routing::get(routes::bridge::account_key),
        )
        .route(
            "/v1/accounts/{id}/orders",
            axum::routing::get(routes::orders::get_account_orders),
        )
        // Bridge sidecar
        .route(
            "/v1/bridge/status",
            axum::routing::get(routes::bridge::status),
        )
        .route(
            "/v1/bridge/deposits",
            axum::routing::post(routes::bridge::submit_l1_deposit),
        )
        .route(
            "/v1/bridge/withdrawals",
            axum::routing::post(routes::bridge::create_withdrawal),
        )
        .route(
            "/v1/bridge/withdrawals/{id}",
            axum::routing::get(routes::bridge::get_withdrawal),
        )
        // Markets — search & summary MUST come before {id} to avoid path param capture
        .route(
            "/v1/markets/search",
            axum::routing::get(routes::markets::search_markets),
        )
        .route(
            "/v1/markets/summary",
            axum::routing::get(routes::markets::list_markets_summary),
        )
        .route(
            "/v1/markets",
            axum::routing::get(routes::markets::list_markets).post(routes::markets::create_market),
        )
        .route(
            "/v1/markets/groups",
            axum::routing::get(routes::markets::list_market_groups)
                .post(routes::markets::create_market_group),
        )
        .route(
            "/v1/markets/prices",
            axum::routing::get(routes::markets::get_prices),
        )
        .route(
            "/v1/markets/{id}",
            axum::routing::get(routes::markets::get_market),
        )
        .route(
            "/v1/markets/{id}/resolve",
            axum::routing::post(routes::markets::resolve_market),
        )
        .route(
            "/v1/markets/{id}/resolution",
            axum::routing::get(routes::markets::get_resolution),
        )
        .route(
            "/v1/markets/{id}/prices/history",
            axum::routing::get(routes::markets::get_price_history),
        )
        // Feeds
        .route(
            "/v1/feeds",
            axum::routing::get(routes::feeds::list_feeds).post(routes::feeds::register_feed),
        )
        // Orders
        .route(
            "/v1/orders",
            axum::routing::post(routes::orders::submit_orders),
        )
        .route(
            "/v1/orders/signed",
            axum::routing::post(routes::orders::submit_signed_order),
        )
        .route(
            "/v1/orders/cancel/signed",
            axum::routing::post(routes::orders::cancel_signed_order),
        )
        .route(
            "/v1/orders/pending",
            axum::routing::get(routes::orders::get_all_pending_orders),
        )
        .route(
            "/v1/markets/{id}/orderbook",
            axum::routing::get(routes::orders::get_market_orderbook),
        )
        .route(
            "/v1/markets/prices/reference",
            axum::routing::post(routes::markets::set_reference_prices),
        )
        .route(
            "/v1/markets/{id}/metadata",
            axum::routing::post(routes::markets::set_market_metadata),
        )
        // Blocks
        .route(
            "/v1/blocks/latest",
            axum::routing::get(routes::blocks::get_latest_block),
        )
        .route(
            "/v1/blocks/stream",
            axum::routing::get(routes::blocks::stream_blocks),
        )
        .route(
            "/v1/blocks/ws",
            axum::routing::get(routes::blocks::ws_blocks),
        )
        .route(
            "/v1/blocks/{height}",
            axum::routing::get(routes::blocks::get_block_by_height),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            order_rate_limit,
        ))
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
        .layer(CorsLayer::permissive())
        .with_state(state)
}
