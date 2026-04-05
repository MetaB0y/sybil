use std::time::Instant;

use axum::extract::State;
use axum::http::{header, Request};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;

use crate::routes;
use crate::state::AppState;
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
        routes::markets::list_markets,
        routes::markets::get_market,
        routes::markets::create_market,
        routes::markets::list_market_groups,
        routes::markets::create_market_group,
        routes::markets::get_prices,
        routes::markets::resolve_market,
        routes::markets::get_price_history,
        routes::markets::search_markets,
        routes::markets::set_reference_prices,
        routes::markets::set_market_metadata,
        routes::orders::submit_orders,
        routes::orders::submit_signed_order,
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
        RegisterKeyRequest,
        CreateMarketRequest,
        CreateMarketGroupRequest,
        ResolveMarketRequest,
        SubmitOrderRequest,
        SubmitSignedOrderRequest,
        SetReferencePricesRequest,
        SetMarketMetadataRequest,
        SignedOrderData,
        OrderSpec,
        MarketSearchParams,
        AccountResponse,
        PositionResponse,
        MarketResponse,
        MarketGroupResponse,
        MarketPricesResponse,
        MarketPriceResponse,
        CreateMarketResponse,
        OrderAcceptedResponse,
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

    tracing::info!(
        http.method = %method,
        http.path = %path,
        http.status = status,
        http.duration_ms = format_args!("{:.1}", duration_secs * 1000.0),
        "request"
    );

    response
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
            "/v1/accounts/{id}/orders",
            axum::routing::get(routes::orders::get_account_orders),
        )
        // Markets — search MUST come before {id} to avoid path param capture
        .route(
            "/v1/markets/search",
            axum::routing::get(routes::markets::search_markets),
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
            "/v1/markets/{id}/prices/history",
            axum::routing::get(routes::markets::get_price_history),
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
        .layer(middleware::from_fn(http_metrics))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
