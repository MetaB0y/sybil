use axum::extract::{Path, State};
use axum::Json;

use matching_sequencer::{
    AccountId, BridgeWithdrawalRequest as SequencerBridgeWithdrawalRequest,
    L1Deposit as SequencerL1Deposit,
};

use crate::convert::bridge_withdrawal_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{CreateBridgeWithdrawalRequest, SubmitL1DepositRequest};
use crate::types::response::{
    BridgeAccountKeyResponse, BridgeDepositResponse, BridgeStatusResponse, BridgeWithdrawalResponse,
};

fn strip_hex_prefix(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

fn parse_hex_array<const N: usize>(value: &str, field: &str) -> Result<[u8; N], AppError> {
    let bytes = hex::decode(strip_hex_prefix(value))
        .map_err(|_| AppError::bad_request(format!("Invalid hex encoding for {field}")))?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        AppError::bad_request(format!(
            "{field} must be {N} bytes, got {} bytes",
            bytes.len()
        ))
    })
}

/// GET /v1/bridge/status
#[utoipa::path(
    get,
    path = "/v1/bridge/status",
    responses((status = 200, description = "Bridge sidecar status", body = BridgeStatusResponse))
)]
pub async fn status(State(state): State<AppState>) -> Result<Json<BridgeStatusResponse>, AppError> {
    let bridge = state.sequencer.get_bridge_state().await?;
    Ok(Json(BridgeStatusResponse {
        deposit_cursor: bridge.deposit_cursor,
        deposit_root_hex: hex::encode(bridge.deposit_root),
        next_withdrawal_id: bridge.next_withdrawal_id,
        withdrawal_count: bridge.withdrawals.len(),
    }))
}

/// GET /v1/accounts/{id}/bridge-key
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/bridge-key",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Account bridge key", body = BridgeAccountKeyResponse),
        (status = 404, description = "Account not found")
    )
)]
pub async fn account_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<BridgeAccountKeyResponse>, AppError> {
    let key = state
        .sequencer
        .get_bridge_account_key(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {id} not found")))?;
    Ok(Json(BridgeAccountKeyResponse {
        account_id: id,
        sybil_account_key_hex: hex::encode(key),
    }))
}

/// POST /v1/bridge/deposits
#[utoipa::path(
    post,
    path = "/v1/bridge/deposits",
    request_body = SubmitL1DepositRequest,
    responses(
        (status = 200, description = "L1 deposit accepted", body = BridgeDepositResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn submit_l1_deposit(
    State(state): State<AppState>,
    Json(req): Json<SubmitL1DepositRequest>,
) -> Result<Json<BridgeDepositResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let account_id = AccountId(req.account_id);
    let sybil_account_key = match req.sybil_account_key_hex {
        Some(value) => parse_hex_array::<32>(&value, "sybil_account_key_hex")?,
        None => state
            .sequencer
            .get_bridge_account_key(account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("Account {} not found", req.account_id)))?,
    };
    let deposit_root = parse_hex_array::<32>(&req.deposit_root_hex, "deposit_root_hex")?;
    let deposit = SequencerL1Deposit {
        deposit_id: req.deposit_id,
        account_id,
        chain_id: req.chain_id,
        vault_address: parse_hex_array::<20>(&req.vault_address_hex, "vault_address_hex")?,
        token_address: parse_hex_array::<20>(&req.token_address_hex, "token_address_hex")?,
        sender: parse_hex_array::<20>(&req.sender_hex, "sender_hex")?,
        sybil_account_key,
        amount_token_units: req.amount_token_units,
        deposit_root,
    };
    let account = state.sequencer.submit_l1_deposit(deposit).await?;
    Ok(Json(BridgeDepositResponse {
        account_id: account.id.0,
        balance_nanos: account.balance,
        deposit_id: req.deposit_id,
        deposit_root_hex: hex::encode(deposit_root),
    }))
}

/// POST /v1/bridge/withdrawals
#[utoipa::path(
    post,
    path = "/v1/bridge/withdrawals",
    request_body = CreateBridgeWithdrawalRequest,
    responses(
        (status = 200, description = "Withdrawal leaf created", body = BridgeWithdrawalResponse),
        (status = 403, description = "Dev mode required")
    )
)]
pub async fn create_withdrawal(
    State(state): State<AppState>,
    Json(req): Json<CreateBridgeWithdrawalRequest>,
) -> Result<Json<BridgeWithdrawalResponse>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }

    let expiry_height = match req.expiry_height {
        Some(height) => height,
        None => {
            state
                .sequencer
                .get_default_bridge_withdrawal_expiry()
                .await?
        }
    };
    let request = SequencerBridgeWithdrawalRequest {
        account_id: AccountId(req.account_id),
        chain_id: req.chain_id,
        vault_address: parse_hex_array::<20>(&req.vault_address_hex, "vault_address_hex")?,
        recipient: parse_hex_array::<20>(&req.recipient_hex, "recipient_hex")?,
        token_address: parse_hex_array::<20>(&req.token_address_hex, "token_address_hex")?,
        amount_token_units: req.amount_token_units,
        expiry_height,
    };
    let withdrawal = state.sequencer.create_bridge_withdrawal(request).await?;
    Ok(Json(bridge_withdrawal_to_response(&withdrawal)))
}

/// GET /v1/bridge/withdrawals/{id}
#[utoipa::path(
    get,
    path = "/v1/bridge/withdrawals/{id}",
    params(("id" = u64, Path, description = "Withdrawal ID")),
    responses(
        (status = 200, description = "Withdrawal leaf", body = BridgeWithdrawalResponse),
        (status = 404, description = "Withdrawal not found")
    )
)]
pub async fn get_withdrawal(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<BridgeWithdrawalResponse>, AppError> {
    let withdrawal = state
        .sequencer
        .get_bridge_withdrawal(id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("Withdrawal {id} not found")))?;
    Ok(Json(bridge_withdrawal_to_response(&withdrawal)))
}
