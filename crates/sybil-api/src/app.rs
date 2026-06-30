use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{header, Request};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use rusqlite::{Connection, OpenFlags};
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
        routes::proofs::get_state_proof,
        routes::accounts::create_account,
        routes::accounts::fund_account,
        routes::accounts::get_account,
        routes::accounts::register_key,
        routes::accounts::get_portfolio,
        routes::accounts::get_account_fills,
        routes::accounts::get_equity,
        routes::accounts::get_account_history,
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
        routes::blocks::get_recent_blocks,
        routes::blocks::get_latest_block,
        routes::blocks::get_block_by_height,
        routes::blocks::stream_blocks,
        routes::blocks::ws_blocks,
        routes::aggregates::get_activity_overview,
        routes::aggregates::get_open_batch,
        routes::aggregates::get_event_traders,
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
        StateProofResponse,
        QmdbStateInclusionProofResponse,
        QmdbStateExclusionProofResponse,
        QmdbStateOperationProofResponse,
        QmdbStateRangeProofResponse,
        ResolveMarketResponse,
        PortfolioResponse,
        PositionValueResponse,
        PriceHistoryResponse,
        PricePointResponse,
        AccountFillResponse,
        EquityPointResponse,
        EquitySeriesResponse,
        PendingOrderResponse,
        PositionDeltaResponse,
        BlockMarketStats,
        ActivityOverviewResponse,
        OverviewBucketResponse,
        OverviewOrderStatsResponse,
        OpenBatchResponse,
        EventTradersResponse,
        HistoryEventResponse,
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
    record_live_market_metrics(&state).await;
    record_bot_metrics(&state).await;
    state.prometheus.render()
}

async fn record_live_market_metrics(state: &AppState) {
    let (markets, prices, statuses, volumes) = match tokio::try_join!(
        state.sequencer.list_markets(),
        state.sequencer.get_market_prices(),
        state.sequencer.get_all_market_statuses(),
        state.sequencer.get_all_market_volumes(),
    ) {
        Ok(values) => values,
        Err(error) => {
            tracing::warn!(error = %error, "failed to collect live market metrics");
            return;
        }
    };

    let ref_prices = state.reference_prices.read().await;
    let updated_at_ms = *state.reference_prices_updated_at_ms.read().await;
    let mut active_markets = 0u64;
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
        if status.as_str() == "active" {
            active_markets += 1;
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
            let diff = yes_price.abs_diff(reference_price);
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
}

async fn record_bot_metrics(state: &AppState) {
    let path = state.arena_db_path.clone();
    let snapshot = match tokio::task::spawn_blocking(move || load_bot_metrics_snapshot(&path)).await
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(error = %error, "bot metrics task failed");
            BotMetricsSnapshot::unavailable()
        }
    };

    metrics::gauge!("sybil_bot_db_available").set(if snapshot.db_available { 1.0 } else { 0.0 });
    metrics::gauge!("sybil_bot_decisions_total").set(snapshot.decisions as f64);
    metrics::gauge!("sybil_bot_traders_total").set(snapshot.traders.len() as f64);
    metrics::gauge!("sybil_bot_latest_decision_age_seconds")
        .set(snapshot.latest_decision_age_seconds.unwrap_or(0) as f64);

    for trader in snapshot.traders {
        metrics::gauge!("sybil_bot_latest_decision_age_seconds", "trader" => trader.name.clone())
            .set(trader.latest_decision_age_seconds.unwrap_or(0) as f64);
        metrics::gauge!("sybil_bot_decisions_total", "trader" => trader.name.clone())
            .set(trader.decisions as f64);
        metrics::gauge!("sybil_bot_total_fills", "trader" => trader.name.clone())
            .set(trader.total_fills.unwrap_or(0) as f64);
        metrics::gauge!("sybil_bot_total_orders", "trader" => trader.name)
            .set(trader.total_orders.unwrap_or(0) as f64);
    }
}

#[derive(Debug, Default)]
struct BotMetricsSnapshot {
    db_available: bool,
    decisions: i64,
    latest_decision_age_seconds: Option<u64>,
    traders: Vec<TraderMetricsSnapshot>,
}

impl BotMetricsSnapshot {
    fn unavailable() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
struct TraderMetricsSnapshot {
    name: String,
    decisions: i64,
    latest_decision_age_seconds: Option<u64>,
    total_fills: Option<i64>,
    total_orders: Option<i64>,
}

fn load_bot_metrics_snapshot(path: &str) -> BotMetricsSnapshot {
    if path.trim().is_empty() || !Path::new(path).exists() {
        return BotMetricsSnapshot::unavailable();
    }
    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(path, error = %error, "failed to open arena bot db for metrics");
            return BotMetricsSnapshot::unavailable();
        }
    };
    if !sqlite_table_exists(&conn, "decisions") {
        return BotMetricsSnapshot::unavailable();
    }

    let now = now_secs();
    let decisions = sqlite_count_rows(&conn, "decisions");
    let latest_decision_age_seconds = latest_timestamp_seconds(
        &conn,
        "SELECT MAX(strftime('%s', timestamp)) FROM decisions",
    )
    .map(|ts| now.saturating_sub(ts));
    let mut traders = load_trader_decision_metrics(&conn, now);
    load_trader_snapshot_metrics(&conn, &mut traders);

    BotMetricsSnapshot {
        db_available: true,
        decisions,
        latest_decision_age_seconds,
        traders,
    }
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

fn sqlite_count_rows(conn: &Connection, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0)).unwrap_or(0)
}

fn latest_timestamp_seconds(conn: &Connection, sql: &str) -> Option<u64> {
    conn.query_row(sql, [], |row| row.get::<_, Option<String>>(0))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
}

fn load_trader_decision_metrics(conn: &Connection, now: u64) -> Vec<TraderMetricsSnapshot> {
    let mut stmt = match conn.prepare(
        "SELECT trader_name, COUNT(*), MAX(strftime('%s', timestamp))
         FROM decisions GROUP BY trader_name",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            tracing::warn!(error = %error, "failed to prepare trader decision metrics query");
            return Vec::new();
        }
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let latest: Option<String> = row.get(2)?;
        Ok(TraderMetricsSnapshot {
            name: row.get(0)?,
            decisions: row.get(1)?,
            latest_decision_age_seconds: latest
                .and_then(|value| value.parse::<u64>().ok())
                .map(|ts| now.saturating_sub(ts)),
            total_fills: None,
            total_orders: None,
        })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn load_trader_snapshot_metrics(conn: &Connection, traders: &mut [TraderMetricsSnapshot]) {
    if !sqlite_table_exists(conn, "portfolio_snapshots") {
        return;
    }
    let mut stmt = match conn.prepare(
        "SELECT p.trader_name, p.total_fills, p.total_orders
         FROM portfolio_snapshots p
         JOIN (
           SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots GROUP BY trader_name
         ) latest ON p.trader_name = latest.trader_name AND p.id = latest.id",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            tracing::warn!(error = %error, "failed to prepare trader snapshot metrics query");
            return;
        }
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    }) else {
        return;
    };
    let snapshots: std::collections::HashMap<_, _> = rows
        .filter_map(Result::ok)
        .map(|(name, fills, orders)| (name, (fills, orders)))
        .collect();
    for trader in traders {
        if let Some((fills, orders)) = snapshots.get(&trader.name) {
            trader.total_fills = *fills;
            trader.total_orders = *orders;
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_secs() -> u64 {
    now_ms() / 1000
}

async fn http_metrics(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = metric_path_label(req.uri().path());
    let start = Instant::now();

    let response = next.run(req).await;

    let duration_secs = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    metrics::counter!("sybil_http_requests_total", "method" => method.to_string(), "path" => path, "status" => status.to_string()).increment(1);
    metrics::histogram!("sybil_http_request_duration_seconds", "method" => method.to_string(), "path" => path)
        .record(duration_secs);

    response
}

fn metric_path_label(path: &str) -> &'static str {
    let trimmed = path.trim_matches('/');
    let segments: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('/').collect()
    };

    match segments.as_slice() {
        [] => "/",
        ["trade"] => "/trade",
        ["openapi.json"] => "/openapi.json",
        ["metrics"] => "/metrics",
        ["v1", "activity", "overview"] => "/v1/activity/overview",
        ["v1", "blocks"] => "/v1/blocks",
        ["v1", "blocks", "latest"] => "/v1/blocks/latest",
        ["v1", "blocks", "stream"] => "/v1/blocks/stream",
        ["v1", "blocks", "ws"] => "/v1/blocks/ws",
        ["v1", "blocks", _] => "/v1/blocks/{height}",
        ["v1", "bots", "decisions"] => "/v1/bots/decisions",
        ["v1", "bridge", "deposits"] => "/v1/bridge/deposits",
        ["v1", "bridge", "status"] => "/v1/bridge/status",
        ["v1", "bridge", "withdrawals"] => "/v1/bridge/withdrawals",
        ["v1", "bridge", "withdrawals", _] => "/v1/bridge/withdrawals/{id}",
        ["v1", "events", _, "raw"] => "/v1/events/{event_id}/raw",
        ["v1", "events", _, "traders"] => "/v1/events/{event_id}/traders",
        ["v1", "feeds"] => "/v1/feeds",
        ["v1", "health"] => "/v1/health",
        ["v1", "orders"] => "/v1/orders",
        ["v1", "orders", "cancel", "signed"] => "/v1/orders/cancel/signed",
        ["v1", "orders", "pending"] => "/v1/orders/pending",
        ["v1", "orders", "signed"] => "/v1/orders/signed",
        ["v1", "proofs", "state", _] => "/v1/proofs/state/{leaf_key_hex}",
        ["v1", "simulation", "pause"] => "/v1/simulation/pause",
        ["v1", "simulation", "resume"] => "/v1/simulation/resume",
        ["v1", "state-root"] => "/v1/state-root",
        ["v1", "accounts"] => "/v1/accounts",
        ["v1", "accounts", _] => "/v1/accounts/{id}",
        ["v1", "accounts", _, "bridge-key"] => "/v1/accounts/{id}/bridge-key",
        ["v1", "accounts", _, "equity"] => "/v1/accounts/{id}/equity",
        ["v1", "accounts", _, "events"] => "/v1/accounts/{id}/events",
        ["v1", "accounts", _, "fills"] => "/v1/accounts/{id}/fills",
        ["v1", "accounts", _, "fund"] => "/v1/accounts/{id}/fund",
        ["v1", "accounts", _, "keys"] => "/v1/accounts/{id}/keys",
        ["v1", "accounts", _, "orders"] => "/v1/accounts/{id}/orders",
        ["v1", "accounts", _, "portfolio"] => "/v1/accounts/{id}/portfolio",
        ["v1", "markets"] => "/v1/markets",
        ["v1", "markets", "groups"] => "/v1/markets/groups",
        ["v1", "markets", "prices"] => "/v1/markets/prices",
        ["v1", "markets", "prices", "reference"] => "/v1/markets/prices/reference",
        ["v1", "markets", "search"] => "/v1/markets/search",
        ["v1", "markets", "summary"] => "/v1/markets/summary",
        ["v1", "markets", _] => "/v1/markets/{id}",
        ["v1", "markets", _, "metadata"] => "/v1/markets/{id}/metadata",
        ["v1", "markets", _, "open-batch"] => "/v1/markets/{id}/open-batch",
        ["v1", "markets", _, "orderbook"] => "/v1/markets/{id}/orderbook",
        ["v1", "markets", _, "prices", "history"] => "/v1/markets/{id}/prices/history",
        ["v1", "markets", _, "resolution"] => "/v1/markets/{id}/resolution",
        ["v1", "markets", _, "resolve"] => "/v1/markets/{id}/resolve",
        ["v1", ..] => "/v1/{unmatched}",
        _ => "/{unmatched}",
    }
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
        .route(
            "/v1/proofs/state/{leaf_key_hex}",
            axum::routing::get(routes::proofs::get_state_proof),
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
            "/v1/accounts/{id}/equity",
            axum::routing::get(routes::accounts::get_equity),
        )
        .route(
            "/v1/accounts/{id}/events",
            axum::routing::get(routes::accounts::get_account_history),
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
        .route(
            "/v1/markets/{id}/open-batch",
            axum::routing::get(routes::aggregates::get_open_batch),
        )
        .route(
            "/v1/activity/overview",
            axum::routing::get(routes::aggregates::get_activity_overview),
        )
        .route(
            "/v1/events/{event_id}/traders",
            axum::routing::get(routes::aggregates::get_event_traders),
        )
        .route(
            "/v1/events/{event_id}/raw",
            axum::routing::get(routes::events::get_event_raw).put(routes::events::put_event_raw),
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
            "/v1/blocks",
            axum::routing::get(routes::blocks::get_recent_blocks),
        )
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

#[cfg(test)]
mod tests {
    use super::metric_path_label;

    #[test]
    fn metric_path_label_normalizes_dynamic_routes() {
        assert_eq!(
            metric_path_label("/v1/accounts/112/fills"),
            "/v1/accounts/{id}/fills"
        );
        assert_eq!(
            metric_path_label("/v1/accounts/112/orders"),
            "/v1/accounts/{id}/orders"
        );
        assert_eq!(
            metric_path_label("/v1/markets/42/prices/history"),
            "/v1/markets/{id}/prices/history"
        );
        assert_eq!(
            metric_path_label("/v1/events/polymarket-abc/raw"),
            "/v1/events/{event_id}/raw"
        );
        assert_eq!(
            metric_path_label("/v1/proofs/state/abcdef"),
            "/v1/proofs/state/{leaf_key_hex}"
        );
        assert_eq!(metric_path_label("/v1/blocks/123"), "/v1/blocks/{height}");
    }

    #[test]
    fn metric_path_label_buckets_unmatched_routes() {
        assert_eq!(
            metric_path_label("/v1/accounts/1/fills/extra"),
            "/v1/{unmatched}"
        );
        assert_eq!(metric_path_label("/wp-login.php"), "/{unmatched}");
    }
}
