use axum::extract::{Path, Query, State};
use axum::Json;

use matching_engine::MarketId;
use matching_sequencer::{AccountId, PublicKey};
use p256::ecdsa::VerifyingKey;
use p256::Sec1Point;

use crate::convert::account_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{CreateAccountRequest, FundAccountRequest, RegisterKeyRequest};
use crate::types::response::*;

/// POST /v1/accounts
#[utoipa::path(
    post,
    path = "/v1/accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 200, description = "Account created", body = AccountResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let balance_nanos = req.initial_balance_nanos as i64;
    let account = state.sequencer.create_account(balance_nanos).await?;
    Ok(Json(account_to_response(&account)))
}

/// POST /v1/accounts/{id}/fund
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/fund",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = FundAccountRequest,
    responses(
        (status = 200, description = "Account funded", body = AccountResponse),
        (status = 403, description = "Dev mode required"),
        (status = 404, description = "Account not found")
    )
)]
pub async fn fund_account(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<FundAccountRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let amount_nanos = req.amount_nanos as i64;
    let account = state
        .sequencer
        .fund_account(AccountId(id), amount_nanos)
        .await?;
    Ok(Json(account_to_response(&account)))
}

/// GET /v1/accounts/{id}
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Account details", body = AccountResponse),
        (status = 404, description = "Account not found")
    )
)]
pub async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<AccountResponse>, AppError> {
    let account = state
        .sequencer
        .get_account(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {} not found", id)))?;
    Ok(Json(account_to_response(&account)))
}

/// POST /v1/accounts/{id}/keys
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/keys",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = RegisterKeyRequest,
    responses(
        (status = 200, description = "Key registered"),
        (status = 400, description = "Invalid key"),
        (status = 404, description = "Account not found")
    )
)]
pub async fn register_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let key_bytes = hex::decode(&req.public_key_hex)
        .map_err(|_| AppError::bad_request("Invalid hex encoding"))?;

    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;

    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;

    let pubkey = PublicKey(verifying_key);
    state
        .sequencer
        .register_pubkey(AccountId(id), pubkey)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /v1/accounts/{id}/portfolio
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/portfolio",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Portfolio summary", body = PortfolioResponse),
        (status = 404, description = "Account not found")
    )
)]
pub async fn get_portfolio(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<PortfolioResponse>, AppError> {
    let portfolio = state.sequencer.get_portfolio(AccountId(id)).await?;

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
            current_price_nanos: p.current_price_nanos,
            value_nanos: p.value_nanos,
            avg_entry_price_nanos: p.avg_entry_price_nanos,
        })
        .collect();

    Ok(Json(PortfolioResponse {
        account_id: portfolio.account_id.0,
        balance_nanos: portfolio.balance_nanos,
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
    get,
    path = "/v1/accounts/{id}/fills",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("market_id" = Option<u32>, Query, description = "Filter by market ID"),
        ("limit" = Option<usize>, Query, description = "Result limit"),
        ("offset" = Option<usize>, Query, description = "Result offset"),
    ),
    responses(
        (status = 200, description = "Account fill history", body = Vec<AccountFillResponse>)
    )
)]
pub async fn get_account_fills(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(params): Query<AccountFillParams>,
) -> Result<Json<Vec<AccountFillResponse>>, AppError> {
    let market_id = params.market_id.map(MarketId::new);
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let fills = state
        .sequencer
        .get_account_fills(AccountId(id), market_id, limit, offset)
        .await?;

    let response: Vec<AccountFillResponse> = fills
        .into_iter()
        .map(|f| AccountFillResponse {
            order_id: f.order_id,
            fill_qty: f.fill_qty,
            fill_price_nanos: f.fill_price,
            block_height: f.block_height,
            timestamp_ms: f.timestamp_ms,
            position_deltas: f
                .position_deltas
                .into_iter()
                .map(|(mid, outcome, delta)| PositionDeltaResponse {
                    market_id: mid.0,
                    outcome: if outcome == 0 {
                        "YES".to_string()
                    } else {
                        "NO".to_string()
                    },
                    delta,
                })
                .collect(),
        })
        .collect();

    Ok(Json(response))
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
    get,
    path = "/v1/accounts/{id}/events",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("limit" = Option<usize>, Query, description = "Max events (default 50, cap 500)"),
        ("before" = Option<String>, Query, description = "Cursor \"<block>.<seq>\"; returns events strictly before it"),
        ("category" = Option<String>, Query, description = "trades | funding | settlement"),
    ),
    responses(
        (status = 200, description = "Account history feed, newest-first", body = [HistoryEventResponse])
    )
)]
pub async fn get_account_history(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<HistoryEventResponse>>, AppError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let before = params.before.as_deref().and_then(parse_cursor);
    let events = state
        .sequencer
        .get_account_events(AccountId(id), limit, before, params.category)
        .await?;
    let out: Vec<HistoryEventResponse> = events
        .into_iter()
        .map(|e| HistoryEventResponse {
            id: e.id(),
            event_type: e.kind.as_str().to_string(),
            category: e.kind.category().to_string(),
            timestamp_ms: e.timestamp_ms,
            block_height: e.block_height,
            market_id: e.market_id.map(|m| m.0),
            order_id: e.order_id,
            side: e.side.map(|s| s.to_string()),
            outcome: e.outcome.map(|o| o.to_string()),
            qty: e.qty,
            price_nanos: e.price_nanos,
            amount_nanos: e.amount_nanos,
            realized_pnl_nanos: e.realized_pnl_nanos,
            payout_outcome: e.payout_outcome.map(|p| p.to_string()),
        })
        .collect();
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
pub struct AccountFillParams {
    pub market_id: Option<u32>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EquityRangeParams {
    /// "24h" | "7d" | "30d" | "all" (default "all").
    pub range: Option<String>,
}

/// GET /v1/accounts/{id}/equity?range=
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/equity",
    params(
        ("id" = u64, Path, description = "Account ID"),
        ("range" = Option<String>, Query, description = "Time range: 24h | 7d | 30d | all (default all)"),
    ),
    responses(
        (status = 200, description = "Per-account equity series", body = EquitySeriesResponse)
    )
)]
pub async fn get_equity(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(params): Query<EquityRangeParams>,
) -> Result<Json<EquitySeriesResponse>, AppError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let since_ms = match params.range.as_deref() {
        Some("24h") => now_ms.saturating_sub(24 * 3_600_000),
        Some("7d") => now_ms.saturating_sub(7 * 24 * 3_600_000),
        Some("30d") => now_ms.saturating_sub(30 * 24 * 3_600_000),
        _ => 0,
    };
    let points = state.sequencer.get_equity_series(AccountId(id)).await?;
    let points: Vec<EquityPointResponse> = points
        .into_iter()
        .filter(|p| p.timestamp_ms >= since_ms)
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
    }))
}
