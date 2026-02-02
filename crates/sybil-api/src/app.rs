use axum::response::IntoResponse;
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
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
        routes::markets::list_markets,
        routes::markets::get_market,
        routes::markets::create_market,
        routes::markets::list_market_groups,
        routes::markets::create_market_group,
        routes::markets::get_prices,
        routes::markets::resolve_market,
        routes::orders::submit_orders,
        routes::orders::submit_signed_order,
        routes::blocks::get_latest_block,
        routes::blocks::get_block_by_height,
        routes::blocks::stream_blocks,
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
        SignedOrderData,
        OrderSpec,
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

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // OpenAPI spec
        .route("/openapi.json", axum::routing::get(openapi_json))
        // System
        .route("/v1/health", axum::routing::get(routes::system::health))
        .route(
            "/v1/state-root",
            axum::routing::get(routes::system::state_root),
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
        // Markets
        .route(
            "/v1/markets",
            axum::routing::get(routes::markets::list_markets)
                .post(routes::markets::create_market),
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
        // Orders
        .route(
            "/v1/orders",
            axum::routing::post(routes::orders::submit_orders),
        )
        .route(
            "/v1/orders/signed",
            axum::routing::post(routes::orders::submit_signed_order),
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
            "/v1/blocks/{height}",
            axum::routing::get(routes::blocks::get_block_by_height),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
