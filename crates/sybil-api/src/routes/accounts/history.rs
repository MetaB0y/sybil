use axum::extract::State;
use axum::http::HeaderMap;

use matching_engine::MarketId;
use matching_sequencer::AccountId;
use sybil_history_types::{
    AccountEventQuery, EquityQuery, FillCursor, FillQuery, ProjectionStatus,
};

use crate::extract::{Json, Path, Query};

use crate::convert::account_balance_breakdown;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::*;
use crate::util::now_ms;

use super::authorize_account_read;

pub(super) const DEFAULT_ACCOUNT_FILL_QUERY_LIMIT: usize = 100;
pub(super) const MAX_ACCOUNT_FILL_QUERY_LIMIT: usize = 500;

/// GET /v1/accounts/{id}/portfolio
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/portfolio",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Portfolio summary", body = PortfolioResponse),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 404, description = "Account not found")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_portfolio(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<PortfolioResponse>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    let (portfolio, reserved_balance) = state
        .sequencer
        .get_portfolio_with_reserved_balance(AccountId(id))
        .await?;
    let (available_balance_nanos, reserved_balance_nanos) =
        account_balance_breakdown(portfolio.balance_nanos, reserved_balance);

    let positions: Vec<PositionValueResponse> = portfolio
        .positions
        .iter()
        .map(|p| PositionValueResponse {
            market_id: p.market_id.0,
            outcome: if p.outcome == 0 {
                "YES".to_string()
            } else {
                "NO".to_string()
            },
            quantity: p.quantity,
            current_price_nanos: p.current_price_nanos.0,
            value_nanos: p.value_nanos,
            avg_entry_price_nanos: p.avg_entry_price_nanos,
        })
        .collect();

    Ok(Json(PortfolioResponse {
        account_id: portfolio.account_id.0,
        balance_nanos: portfolio.balance_nanos,
        available_balance_nanos,
        reserved_balance_nanos,
        total_deposited_nanos: portfolio.total_deposited_nanos,
        positions,
        total_position_value_nanos: portfolio.total_position_value_nanos,
        portfolio_value_nanos: portfolio.portfolio_value_nanos,
        pnl_nanos: portfolio.pnl_nanos,
        first_deposit_ms: portfolio.first_deposit_ms,
        total_fill_count: portfolio.total_fill_count,
        realized_pnl_nanos: portfolio.realized_pnl_nanos,
        unrealized_pnl_nanos: portfolio.unrealized_pnl_nanos,
    }))
}

/// GET /v1/accounts/{id}/fills
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/fills",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("market_id" = Option<u32>, Query, description = "Filter by market ID"),
        ("after" = Option<String>, Query, description = "Stable cursor returned as `cursor` on each fill. When present, returns fills strictly after this cursor in ascending order. Use `0.0` to start from the beginning."),
        ("limit" = Option<usize>, Query, maximum = 500, description = "Result limit (default 100, cap 500)"),
    ),
    responses(
        (status = 200, description = "Retained account fill history", body = AccountFillPageResponse),
        (status = 400, description = "Invalid cursor"),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 503, description = "Private history service unavailable")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_account_fills(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
    Query(params): Query<AccountFillParams>,
) -> Result<Json<AccountFillPageResponse>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    if params
        .limit
        .is_some_and(|limit| limit > MAX_ACCOUNT_FILL_QUERY_LIMIT)
    {
        return Err(AppError::bad_request(format!(
            "limit must be at most {MAX_ACCOUNT_FILL_QUERY_LIMIT}"
        )));
    }
    let market_id = params.market_id.map(MarketId::new);
    let limit = account_fill_query_limit(params.limit);
    let request_limit = limit.saturating_add(1);
    let forward = params.after.is_some();
    let mut requested_cursor = None;
    let history = state.history.as_ref().ok_or_else(|| {
        AppError::history_unavailable("Historical data service is not configured")
    })?;
    let page = if let Some(after) = params.after.as_deref() {
        let cursor =
            FillCursor::parse(after).ok_or_else(|| AppError::bad_request("Invalid fill cursor"))?;
        requested_cursor = Some(cursor);
        history
            .fills(&FillQuery {
                account_id: id,
                market_id: market_id.map(|market_id| market_id.0),
                after: Some(FillCursor {
                    block_height: cursor.block_height,
                    order_id: cursor.order_id,
                }),
                limit: request_limit,
            })
            .await?
    } else {
        history
            .fills(&FillQuery {
                account_id: id,
                market_id: market_id.map(|market_id| market_id.0),
                after: None,
                limit: request_limit,
            })
            .await?
    };

    let (retention_min_timestamp_ms, pruned_through_height, history_truncated) =
        projection_floor(&page.status);
    let indexed_through_height = page.status.indexed_through_height;
    let history_complete_from_height = page.status.first_height;
    let mut fills: Vec<AccountFillResponse> = page
        .items
        .into_iter()
        .map(account_fill_fact_response)
        .collect();
    let has_more = fills.len() > limit;
    fills.truncate(limit);
    let next_after = (forward && has_more)
        .then(|| fills.last().map(|fill| fill.cursor.clone()))
        .flatten();

    Ok(Json(AccountFillPageResponse {
        fills,
        next_after,
        retention_min_timestamp_ms,
        pruned_through_height,
        cursor_gap: fill_cursor_has_gap(requested_cursor, pruned_through_height),
        history_truncated,
        history_scope: "remote".to_string(),
        indexed_through_height,
        history_complete_from_height,
    }))
}

fn projection_floor(status: &ProjectionStatus) -> (Option<u64>, Option<u64>, bool) {
    match status.first_height {
        Some(first_height) if first_height > 1 => (
            status.first_timestamp_ms,
            Some(first_height.saturating_sub(1)),
            true,
        ),
        _ => (None, None, false),
    }
}

fn account_fill_fact_response(f: sybil_history_types::AccountFillFact) -> AccountFillResponse {
    AccountFillResponse {
        cursor: format!("{}.{}", f.block_height, f.order_id),
        order_id: f.order_id,
        fill_qty: f.fill_qty,
        fill_price_nanos: f.fill_price_nanos,
        block_height: f.block_height,
        timestamp_ms: f.timestamp_ms,
        position_deltas: f
            .position_deltas
            .into_iter()
            .map(|delta| PositionDeltaResponse {
                market_id: delta.market_id,
                outcome: if delta.outcome == 0 {
                    "YES".to_string()
                } else {
                    "NO".to_string()
                },
                delta: delta.delta,
            })
            .collect(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct HistoryParams {
    pub limit: Option<usize>,
    /// Cursor "<block>.<seq>" — return events strictly before it.
    pub before: Option<String>,
    /// "trades" | "funding" | "settlement".
    pub category: Option<String>,
}

fn parse_cursor(s: &str) -> Option<(u64, u64)> {
    let (b, q) = s.split_once('.')?;
    Some((b.parse().ok()?, q.parse().ok()?))
}

/// GET /v1/accounts/{id}/events?limit&before&category
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/events",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("limit" = Option<usize>, Query, maximum = 500, description = "Max events (default 50, cap 500)"),
        ("before" = Option<String>, Query, description = "Cursor \"<block>.<seq>\"; returns events strictly before it"),
        ("category" = Option<String>, Query, description = "trades | funding | settlement"),
    ),
    responses(
        (status = 200, description = "Retained account history feed, newest-first", body = AccountHistoryPageResponse),
        (status = 400, description = "Invalid cursor"),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 503, description = "Private history service unavailable")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_account_history(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
    Query(params): Query<HistoryParams>,
) -> Result<Json<AccountHistoryPageResponse>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    if params.limit.is_some_and(|limit| limit > 500) {
        return Err(AppError::bad_request("limit must be at most 500"));
    }
    let limit = params.limit.unwrap_or(50).min(500);
    let before = params
        .before
        .as_deref()
        .map(|cursor| {
            parse_cursor(cursor).ok_or_else(|| AppError::bad_request("Invalid event cursor"))
        })
        .transpose()?;
    let history = state.history.as_ref().ok_or_else(|| {
        AppError::history_unavailable("Historical data service is not configured")
    })?;
    let page = history
        .events(&AccountEventQuery {
            account_id: id,
            limit: limit.saturating_add(1),
            before,
            category: params.category,
        })
        .await?;
    let (retention_min_timestamp_ms, _, history_truncated) = projection_floor(&page.status);
    let indexed_through_height = page.status.indexed_through_height;
    let history_complete_from_height = page.status.first_height;
    let mut events: Vec<HistoryEventResponse> = page
        .items
        .into_iter()
        .map(|e| HistoryEventResponse {
            id: format!("{}.{}", e.block_height, e.seq),
            event_type: e.kind.as_str().to_string(),
            category: e.kind.category().to_string(),
            timestamp_ms: e.timestamp_ms,
            block_height: e.block_height,
            market_id: e.market_id,
            order_id: e.order_id,
            side: e.side,
            outcome: e.outcome,
            qty: e.qty,
            price_nanos: e.price_nanos,
            amount_nanos: e.amount_nanos,
            realized_pnl_nanos: e.realized_pnl_nanos,
            payout_outcome: e.payout_outcome,
            reason: e.reason,
            required_nanos: e.required_nanos,
            available_nanos: e.available_nanos,
        })
        .collect();
    let has_more = events.len() > limit;
    events.truncate(limit);
    let next_before = has_more
        .then(|| events.last().map(|event| event.id.clone()))
        .flatten();
    Ok(Json(AccountHistoryPageResponse {
        events,
        next_before,
        retention_min_timestamp_ms,
        history_truncated,
        history_scope: "remote".to_string(),
        indexed_through_height,
        history_complete_from_height,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountFillParams {
    pub market_id: Option<u32>,
    pub after: Option<String>,
    pub limit: Option<usize>,
}

pub(super) fn account_fill_query_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_ACCOUNT_FILL_QUERY_LIMIT)
        .min(MAX_ACCOUNT_FILL_QUERY_LIMIT)
}

pub(super) fn fill_cursor_has_gap(
    cursor: Option<FillCursor>,
    pruned_through_height: Option<u64>,
) -> bool {
    cursor.is_some_and(|cursor| {
        pruned_through_height.is_some_and(|height| cursor.block_height <= height)
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct EquityRangeParams {
    /// "24h" | "7d" | "30d" | "all" (default "all").
    pub range: Option<String>,
}

/// GET /v1/accounts/{id}/equity?range=
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/equity",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("range" = Option<String>, Query, description = "Time range: 24h | 7d | 30d | all (default all)"),
    ),
    responses(
        (status = 200, description = "Per-account equity series", body = EquitySeriesResponse),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 503, description = "Private history service unavailable")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_equity(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
    Query(params): Query<EquityRangeParams>,
) -> Result<Json<EquitySeriesResponse>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    let now_ms = now_ms();
    let since_ms = match params.range.as_deref() {
        Some("24h") => now_ms.saturating_sub(24 * 3_600_000),
        Some("7d") => now_ms.saturating_sub(7 * 24 * 3_600_000),
        Some("30d") => now_ms.saturating_sub(30 * 24 * 3_600_000),
        _ => 0,
    };
    let history = state.history.as_ref().ok_or_else(|| {
        AppError::history_unavailable("Historical data service is not configured")
    })?;
    let page = history
        .equity(&EquityQuery {
            account_id: id,
            since_ms,
        })
        .await?;
    let (retention_min_timestamp_ms, _, projection_truncated) = projection_floor(&page.status);
    let indexed_through_height = page.status.indexed_through_height;
    let history_complete_from_height = page.status.first_height;
    let source_points = page.source_points;
    let downsampled = page.downsampled;
    let points: Vec<EquityPointResponse> = page
        .items
        .into_iter()
        .map(|p| EquityPointResponse {
            timestamp_ms: p.timestamp_ms,
            height: p.height,
            portfolio_value_nanos: p.portfolio_value_nanos,
            deposited_nanos: p.deposited_nanos,
        })
        .collect();
    Ok(Json(EquitySeriesResponse {
        account_id: id,
        points,
        retention_min_timestamp_ms,
        history_truncated: projection_truncated
            || retention_min_timestamp_ms.is_some_and(|floor| since_ms < floor),
        history_scope: "remote".to_string(),
        source_points,
        downsampled,
        indexed_through_height,
        history_complete_from_height,
    }))
}
