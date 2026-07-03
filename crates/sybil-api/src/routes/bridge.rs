use axum::extract::{Path, State};
use axum::Json;

use matching_sequencer::crypto::{PublicKey, SignedBridgeWithdrawal};
use matching_sequencer::{
    AccountId, BridgeWithdrawalRequest as SequencerBridgeWithdrawalRequest,
    L1Deposit as SequencerL1Deposit,
};
use p256::ecdsa::{Signature, VerifyingKey};
use p256::Sec1Point;

use crate::convert::bridge_withdrawal_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    CreateBridgeWithdrawalRequest, CreateSignedBridgeWithdrawalRequest, SubmitL1DepositRequest,
};
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

fn parse_signer_public_key(public_key_hex: &str) -> Result<PublicKey, AppError> {
    let key_bytes = hex::decode(strip_hex_prefix(public_key_hex))
        .map_err(|_| AppError::bad_request("Invalid hex encoding for public key"))?;
    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;
    Ok(PublicKey(verifying_key))
}

fn parse_signature(signature_hex: &str) -> Result<Signature, AppError> {
    let sig_bytes = hex::decode(strip_hex_prefix(signature_hex))
        .map_err(|_| AppError::bad_request("Invalid hex encoding for signature"))?;
    Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 ECDSA signature"))
}

fn withdrawal_request_from_api(
    req: &CreateBridgeWithdrawalRequest,
    expiry_height: u64,
) -> Result<SequencerBridgeWithdrawalRequest, AppError> {
    Ok(SequencerBridgeWithdrawalRequest {
        account_id: AccountId(req.account_id),
        chain_id: req.chain_id,
        vault_address: parse_hex_array::<20>(&req.vault_address_hex, "vault_address_hex")?,
        recipient: parse_hex_array::<20>(&req.recipient_hex, "recipient_hex")?,
        token_address: parse_hex_array::<20>(&req.token_address_hex, "token_address_hex")?,
        amount_token_units: req.amount_token_units,
        expiry_height,
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

/// GET /v1/bridge/accounts/by-key/{key_hex}
#[utoipa::path(
    get,
    path = "/v1/bridge/accounts/by-key/{key_hex}",
    params(("key_hex" = String, Path, description = "Hex-encoded Sybil bridge account key")),
    responses(
        (status = 200, description = "Account bridge key mapping", body = BridgeAccountKeyResponse),
        (status = 404, description = "Bridge key not found")
    )
)]
pub async fn account_by_key(
    State(state): State<AppState>,
    Path(key_hex): Path<String>,
) -> Result<Json<BridgeAccountKeyResponse>, AppError> {
    let key = parse_hex_array::<32>(&key_hex, "key_hex")?;
    let account_id = state
        .sequencer
        .get_bridge_account_id_by_key(key)
        .await?
        .ok_or_else(|| AppError::not_found("Bridge account key not found"))?;
    Ok(Json(BridgeAccountKeyResponse {
        account_id: account_id.0,
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
    // TODO(SYB-188/SYB-178): this service-gated scaffold trusts the operator's
    // submitted L1 event fields. Production deposit soundness must verify L1
    // inclusion/finality against the vault deposit root before crediting.
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
    // TODO(SYB-188/SYB-178): keep this unsigned path service-only. End-user
    // bridge withdrawals should use the signed route once the L1 authorization
    // and withdrawal-proof trust story is complete.
    let expiry_height = match req.expiry_height {
        Some(height) => height,
        None => {
            state
                .sequencer
                .get_default_bridge_withdrawal_expiry()
                .await?
        }
    };
    let request = withdrawal_request_from_api(&req, expiry_height)?;
    let withdrawal = state.sequencer.create_bridge_withdrawal(request).await?;
    Ok(Json(bridge_withdrawal_to_response(&withdrawal)))
}

/// POST /v1/bridge/withdrawals/signed
#[utoipa::path(
    post,
    path = "/v1/bridge/withdrawals/signed",
    request_body = CreateSignedBridgeWithdrawalRequest,
    responses(
        (status = 200, description = "Signed withdrawal leaf created", body = BridgeWithdrawalResponse),
        (status = 400, description = "Invalid withdrawal or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer")
    )
)]
pub async fn create_signed_withdrawal(
    State(state): State<AppState>,
    Json(req): Json<CreateSignedBridgeWithdrawalRequest>,
) -> Result<Json<BridgeWithdrawalResponse>, AppError> {
    let expiry_height = req.withdrawal.expiry_height.ok_or_else(|| {
        AppError::bad_request("expiry_height is required for signed bridge withdrawals")
    })?;
    let request = withdrawal_request_from_api(&req.withdrawal, expiry_height)?;
    let signed = SignedBridgeWithdrawal {
        request,
        signer: parse_signer_public_key(&req.signer_pubkey_hex)?,
        signature: parse_signature(&req.signature_hex)?,
    };

    // TODO(SYB-188/SYB-178): this authenticates the Sybil account intent and
    // burns off-chain balance, but L1 release still needs proof-backed vault
    // authorization before this is production-complete.
    let withdrawal = state
        .sequencer
        .create_signed_bridge_withdrawal(signed)
        .await?;
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
