use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;

use matching_sequencer::crypto::{
    AccountAuthScheme, AuthenticatedBridgeWithdrawal, PublicKey, SignedBridgeWithdrawal,
    canonical_bridge_withdrawal_bytes,
};
use matching_sequencer::{
    AccountId, BridgeWithdrawalL1Event,
    BridgeWithdrawalRequest as SequencerBridgeWithdrawalRequest, L1Deposit as SequencerL1Deposit,
    L1WithdrawalStatus,
};
use p256::Sec1Point;
use p256::ecdsa::{Signature, VerifyingKey};

use crate::convert::bridge_withdrawal_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    AuthScheme, BridgeWithdrawalL1Status as ApiBridgeWithdrawalL1Status,
    CreateBridgeWithdrawalRequest, CreateSignedBridgeWithdrawalRequest, ObserveL1HeightRequest,
    SubmitL1DepositRequest, SubmitL1WithdrawalEventRequest,
};
use crate::types::response::{
    BridgeAccountKeyResponse, BridgeDepositResponse, BridgeDomainResponse, BridgeStatusResponse,
    BridgeWithdrawalL1EventResponse, BridgeWithdrawalResponse, ObserveL1HeightResponse,
};
use crate::webauthn;

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

fn parse_required_signature(signature_hex: Option<&str>) -> Result<Signature, AppError> {
    parse_signature(signature_hex.ok_or_else(|| {
        AppError::bad_request("signature_hex is required for raw_p256 signed requests")
    })?)
}

fn sequencer_auth_scheme(scheme: AuthScheme) -> AccountAuthScheme {
    match scheme {
        AuthScheme::RawP256 => AccountAuthScheme::RawP256,
        AuthScheme::WebAuthn => AccountAuthScheme::WebAuthn,
    }
}

fn sequencer_l1_withdrawal_status(status: ApiBridgeWithdrawalL1Status) -> L1WithdrawalStatus {
    match status {
        ApiBridgeWithdrawalL1Status::NotRequested => L1WithdrawalStatus::NotRequested,
        ApiBridgeWithdrawalL1Status::Queued => L1WithdrawalStatus::Queued,
        ApiBridgeWithdrawalL1Status::Finalized => L1WithdrawalStatus::Finalized,
        ApiBridgeWithdrawalL1Status::Cancelled => L1WithdrawalStatus::Cancelled,
        ApiBridgeWithdrawalL1Status::Refunded => L1WithdrawalStatus::Refunded,
    }
}

async fn ensure_registered_scheme(
    state: &AppState,
    signer: &PublicKey,
    expected: AccountAuthScheme,
) -> Result<(), AppError> {
    let registered = state
        .sequencer
        .lookup_registered_pubkey(signer.clone())
        .await?
        .ok_or_else(|| AppError::not_found("No account registered for this public key"))?;
    if registered.auth_scheme != expected {
        return Err(AppError::forbidden(
            "Signer key is not registered for this auth scheme",
        ));
    }
    Ok(())
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

fn require_configured_domain(
    state: &AppState,
    chain_id: u64,
    vault_address: [u8; 20],
    token_address: [u8; 20],
) -> Result<(), AppError> {
    let domain = state
        .bridge_domain
        .ok_or_else(AppError::bridge_unavailable)?;
    if domain.chain_id != chain_id
        || domain.vault_address != vault_address
        || domain.token_address != token_address
    {
        return Err(AppError::bridge_domain_mismatch());
    }
    Ok(())
}

/// GET /v1/bridge/status
#[utoipa::path(
    tag = "routesbridge",
    get,
    path = "/v1/bridge/status",
    responses((status = 200, description = "Bridge sidecar status", body = BridgeStatusResponse))
)]
pub async fn status(State(state): State<AppState>) -> Result<Json<BridgeStatusResponse>, AppError> {
    let bridge = state.sequencer.get_bridge_state().await?;
    let queued_withdrawal_count = bridge
        .withdrawals
        .values()
        .filter(|withdrawal| withdrawal.l1_status == L1WithdrawalStatus::Queued)
        .count();
    let finalized_withdrawal_count = bridge
        .withdrawals
        .values()
        .filter(|withdrawal| withdrawal.l1_status == L1WithdrawalStatus::Finalized)
        .count();
    let cancelled_withdrawal_count = bridge
        .withdrawals
        .values()
        .filter(|withdrawal| withdrawal.l1_status == L1WithdrawalStatus::Cancelled)
        .count();
    let refunded_withdrawal_count = bridge
        .withdrawals
        .values()
        .filter(|withdrawal| withdrawal.l1_status == L1WithdrawalStatus::Refunded)
        .count();
    Ok(Json(BridgeStatusResponse {
        configured_domain: state.bridge_domain.map(|domain| BridgeDomainResponse {
            chain_id: domain.chain_id,
            vault_address_hex: hex::encode(domain.vault_address),
            token_address_hex: hex::encode(domain.token_address),
        }),
        deposit_cursor: bridge.deposit_cursor,
        deposit_root_hex: hex::encode(bridge.deposit_root),
        observed_l1_height: bridge.observed_l1_height,
        next_withdrawal_id: bridge.next_withdrawal_id,
        withdrawal_count: bridge.withdrawals.len(),
        queued_withdrawal_count,
        finalized_withdrawal_count,
        cancelled_withdrawal_count,
        refunded_withdrawal_count,
        quarantine_ledger_size: bridge.quarantine.len(),
        total_quarantined_nanos: bridge
            .quarantine
            .values()
            .copied()
            .fold(0i64, i64::saturating_add),
    }))
}

/// GET /v1/accounts/{id}/bridge-key
#[utoipa::path(
    tag = "routesbridge",
    get,
    path = "/v1/accounts/{id}/bridge-key",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Account bridge key", body = BridgeAccountKeyResponse),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 404, description = "Account not found")
    ),
    security(("bearer_read" = []))
)]
pub async fn account_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<BridgeAccountKeyResponse>, AppError> {
    crate::routes::accounts::authorize_account_read(&state, &headers, AccountId(id)).await?;
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

/// GET /v1/accounts/{id}/withdrawals
///
/// Returns the account's currently active withdrawal leaves. Terminal leaves
/// are visible with their terminal status until the next committed block
/// retires them, then disappear from this collection. Historical creation
/// blocks remain immutable and must not be used as a current-status view.
#[utoipa::path(
    tag = "routesbridge",
    get,
    path = "/v1/accounts/{id}/withdrawals",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Active withdrawals for this account", body = [BridgeWithdrawalResponse]),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account")
    ),
    security(("bearer_read" = []))
)]
pub async fn list_account_withdrawals(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<Vec<BridgeWithdrawalResponse>>, AppError> {
    let account_id = AccountId(id);
    crate::routes::accounts::authorize_account_read(&state, &headers, account_id).await?;
    let bridge = state.sequencer.get_bridge_state().await?;
    Ok(Json(
        bridge
            .withdrawals
            .values()
            .filter(|withdrawal| withdrawal.account_id == account_id)
            .map(bridge_withdrawal_to_response)
            .collect(),
    ))
}

/// GET /v1/bridge/withdrawals/pending
///
/// Operator relay feed for active leaves that have not yet produced a
/// confirmed `WithdrawalQueued` event. This is service-authenticated because
/// rows contain account-attributed recipients and amounts.
#[utoipa::path(
    tag = "routesbridge",
    get,
    path = "/v1/bridge/withdrawals/pending",
    responses(
        (status = 200, description = "Active withdrawals awaiting an L1 queue event", body = [BridgeWithdrawalResponse]),
        (status = 401, description = "Missing or invalid service token")
    ),
    security(("bearer_service" = []))
)]
pub async fn list_pending_withdrawals(
    State(state): State<AppState>,
) -> Result<Json<Vec<BridgeWithdrawalResponse>>, AppError> {
    let bridge = state.sequencer.get_bridge_state().await?;
    Ok(Json(
        bridge
            .withdrawals
            .values()
            .filter(|withdrawal| withdrawal.l1_status == L1WithdrawalStatus::NotRequested)
            .map(bridge_withdrawal_to_response)
            .collect(),
    ))
}

/// GET /v1/bridge/accounts/by-key/{key_hex}
#[utoipa::path(
    tag = "routesbridge",
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
    tag = "routesbridge",
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
    // This service-gated route accepts only the authenticated indexer delivery
    // path in production. The indexer establishes finalized-provider unanimity
    // and exact block-hash log/state reads; the sequencer independently
    // reconstructs the submitted leaf/root, and L1 settlement matches the
    // proven checkpoint to the vault. Bearer authorization identifies the
    // ingress service but is not itself the L1 inclusion proof.
    if req.quarantine == req.account_id.is_some() {
        return Err(AppError::bad_request(
            "exactly one deposit disposition is required: account_id or quarantine=true",
        ));
    }
    let vault_address = parse_hex_array::<20>(&req.vault_address_hex, "vault_address_hex")?;
    let token_address = parse_hex_array::<20>(&req.token_address_hex, "token_address_hex")?;
    require_configured_domain(&state, req.chain_id, vault_address, token_address)?;
    let account_id = req.account_id.map(AccountId);
    let sybil_account_key = match req.sybil_account_key_hex {
        Some(value) => parse_hex_array::<32>(&value, "sybil_account_key_hex")?,
        None => match account_id {
            Some(account_id) => state
                .sequencer
                .get_bridge_account_key(account_id)
                .await?
                .ok_or_else(|| {
                    AppError::not_found(format!("Account {} not found", account_id.0))
                })?,
            None => {
                return Err(AppError::bad_request(
                    "quarantined deposits require sybil_account_key_hex",
                ));
            }
        },
    };
    let deposit_root = parse_hex_array::<32>(&req.deposit_root_hex, "deposit_root_hex")?;
    let deposit = SequencerL1Deposit {
        deposit_id: req.deposit_id,
        account_id,
        chain_id: req.chain_id,
        vault_address,
        token_address,
        sender: parse_hex_array::<20>(&req.sender_hex, "sender_hex")?,
        sybil_account_key,
        amount_token_units: req.amount_token_units,
        deposit_root,
    };
    let disposition = state.sequencer.submit_l1_deposit(deposit).await?;
    let (account_id, balance_nanos, disposition) = match disposition {
        matching_sequencer::DepositDisposition::Credited(account) => {
            (Some(account.id.0), Some(account.balance), "credited")
        }
        matching_sequencer::DepositDisposition::Quarantined { .. } => (None, None, "quarantined"),
    };
    Ok(Json(BridgeDepositResponse {
        account_id,
        balance_nanos,
        disposition: disposition.to_string(),
        deposit_id: req.deposit_id,
        deposit_root_hex: hex::encode(deposit_root),
    }))
}

/// POST /v1/bridge/withdrawals
#[utoipa::path(
    tag = "routesbridge",
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
    require_configured_domain(
        &state,
        request.chain_id,
        request.vault_address,
        request.token_address,
    )?;
    let withdrawal = state.sequencer.create_bridge_withdrawal(request).await?;
    Ok(Json(bridge_withdrawal_to_response(&withdrawal)))
}

/// POST /v1/bridge/withdrawals/signed
#[utoipa::path(
    tag = "routesbridge",
    post,
    path = "/v1/bridge/withdrawals/signed",
    request_body = CreateSignedBridgeWithdrawalRequest,
    responses(
        (status = 200, description = "Signed withdrawal leaf created", body = BridgeWithdrawalResponse),
        (status = 400, description = "Invalid withdrawal or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 409, description = "Replay nonce is stale or duplicate"),
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
    let nonce = req
        .withdrawal
        .nonce
        .ok_or_else(|| AppError::bad_request("nonce is required for signed bridge withdrawals"))?;
    let request = withdrawal_request_from_api(&req.withdrawal, expiry_height)?;
    require_configured_domain(
        &state,
        request.chain_id,
        request.vault_address,
        request.token_address,
    )?;
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let genesis_hash = state
        .sequencer
        .get_genesis_hash()
        .await?
        .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;

    // TODO(SYB-188/SYB-178): this authenticates the Sybil account intent and
    // burns off-chain balance, but L1 release still needs proof-backed vault
    // authorization before this is production-complete.
    let withdrawal = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedBridgeWithdrawal {
                request,
                nonce,
                signer,
                signature: parse_required_signature(req.signature_hex.as_deref())?,
            };
            state
                .sequencer
                .create_signed_bridge_withdrawal(signed)
                .await?
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let canonical = canonical_bridge_withdrawal_bytes(&request, nonce, genesis_hash);
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            state
                .sequencer
                .create_authenticated_bridge_withdrawal(AuthenticatedBridgeWithdrawal {
                    request,
                    nonce,
                    signer,
                })
                .await?
        }
    };
    Ok(Json(bridge_withdrawal_to_response(&withdrawal)))
}

/// POST /v1/bridge/withdrawals/l1-events
#[utoipa::path(
    tag = "routesbridge",
    post,
    path = "/v1/bridge/withdrawals/l1-events",
    request_body = SubmitL1WithdrawalEventRequest,
    responses(
        (status = 200, description = "L1 withdrawal queue status applied or idempotently ignored", body = BridgeWithdrawalL1EventResponse),
        (status = 400, description = "Invalid L1 withdrawal event"),
        (status = 404, description = "Withdrawal not found")
    )
)]
pub async fn submit_l1_withdrawal_event(
    State(state): State<AppState>,
    Json(req): Json<SubmitL1WithdrawalEventRequest>,
) -> Result<Json<BridgeWithdrawalL1EventResponse>, AppError> {
    if req.status == ApiBridgeWithdrawalL1Status::Refunded {
        return Err(AppError::bad_request(
            "refunded is a Sybil terminal state, not an L1 event status",
        ));
    }
    let event = BridgeWithdrawalL1Event {
        nullifier: parse_hex_array::<32>(&req.nullifier_hex, "nullifier_hex")?,
        status: sequencer_l1_withdrawal_status(req.status),
        event_at_unix: req.event_at_unix,
        executable_at_unix: req.executable_at_unix,
        tx_hash: req
            .tx_hash_hex
            .as_deref()
            .map(|value| parse_hex_array::<32>(value, "tx_hash_hex"))
            .transpose()?,
        l1_block_height: req.l1_block_height,
    };
    let withdrawal = state
        .sequencer
        .apply_bridge_withdrawal_l1_event(event)
        .await?;
    Ok(Json(BridgeWithdrawalL1EventResponse {
        active_withdrawal_found: withdrawal.is_some(),
        withdrawal: withdrawal.as_ref().map(bridge_withdrawal_to_response),
    }))
}

/// POST /v1/bridge/l1-height
#[utoipa::path(
    tag = "routesbridge",
    post,
    path = "/v1/bridge/l1-height",
    request_body = ObserveL1HeightRequest,
    responses((status = 200, description = "Confirmed L1 height applied", body = ObserveL1HeightResponse))
)]
pub async fn observe_l1_height(
    State(state): State<AppState>,
    Json(req): Json<ObserveL1HeightRequest>,
) -> Result<Json<ObserveL1HeightResponse>, AppError> {
    let refunded = state
        .sequencer
        .observe_bridge_l1_height(req.l1_block_height)
        .await?;
    Ok(Json(ObserveL1HeightResponse {
        observed_l1_height: req.l1_block_height,
        refunded_withdrawal_ids: refunded
            .into_iter()
            .map(|withdrawal| withdrawal.withdrawal_id)
            .collect(),
    }))
}
