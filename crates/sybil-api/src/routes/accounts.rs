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
pub struct AccountFillParams {
    pub market_id: Option<u32>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
