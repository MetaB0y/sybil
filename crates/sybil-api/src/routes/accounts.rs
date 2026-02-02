use axum::extract::{Path, State};
use axum::Json;

use matching_sequencer::{AccountId, PublicKey};
use p256::ecdsa::VerifyingKey;
use p256::EncodedPoint;

use crate::convert::account_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{CreateAccountRequest, FundAccountRequest, RegisterKeyRequest};
use crate::types::response::AccountResponse;

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
    let key_bytes =
        hex::decode(&req.public_key_hex).map_err(|_| AppError::bad_request("Invalid hex encoding"))?;

    let encoded_point = EncodedPoint::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;

    let verifying_key = VerifyingKey::from_encoded_point(&encoded_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;

    let pubkey = PublicKey(verifying_key);
    state
        .sequencer
        .register_pubkey(AccountId(id), pubkey)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
