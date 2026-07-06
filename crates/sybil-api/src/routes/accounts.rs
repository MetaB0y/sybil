use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;

use matching_engine::MarketId;
use matching_sequencer::crypto::{
    canonical_api_key_create_bytes, canonical_api_key_revoke_bytes,
    canonical_key_registration_bytes, canonical_key_revocation_bytes,
    canonical_profile_update_bytes,
};
use matching_sequencer::{
    api_key_hash, AccountAuthScheme, AccountFillCursor, AccountFillRecord, AccountId,
    AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke, AuthenticatedKeyRegistration,
    AuthenticatedKeyRevocation, AuthenticatedProfileUpdate, KeyScope, PublicKey, RegisteredPubkey,
    SignedApiKeyCreate, SignedApiKeyRevoke, SignedKeyRegistration, SignedKeyRevocation,
    SignedProfileUpdate,
};
use p256::ecdsa::{Signature, VerifyingKey};
use p256::Sec1Point;

use crate::convert::account_to_response;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    AuthScheme, CreateAccountRequest, CreateApiKeyRequest, FundAccountRequest,
    KeyScope as KeyScopeDto, RegisterKeyRequest, RevokeApiKeyRequest, RevokeKeyRequest,
    SetProfileRequest, SignedRegisterKeyRequest, WebAuthnAssertion,
};
use crate::types::response::*;
use crate::util::now_ms;
use crate::webauthn;

const DEFAULT_ACCOUNT_FILL_QUERY_LIMIT: usize = 100;
const MAX_ACCOUNT_FILL_QUERY_LIMIT: usize = 500;

fn sequencer_auth_scheme(scheme: AuthScheme) -> AccountAuthScheme {
    match scheme {
        AuthScheme::RawP256 => AccountAuthScheme::RawP256,
        AuthScheme::WebAuthn => AccountAuthScheme::WebAuthn,
    }
}

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
    let balance_nanos = i64::try_from(req.initial_balance_nanos).map_err(|_| {
        AppError::bad_request(format!(
            "initial_balance_nanos {} exceeds the maximum signed-balance range",
            req.initial_balance_nanos
        ))
    })?;
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
    let amount_nanos = i64::try_from(req.amount_nanos).map_err(|_| {
        AppError::bad_request(format!(
            "amount_nanos {} exceeds the maximum signed-balance range",
            req.amount_nanos
        ))
    })?;
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

/// Parse + validate the NEW key material shared by the first-key (service) and
/// signed (public) registration paths. Returns the compressed public key. For
/// WebAuthn keys, `webauthn_registration` must prove possession of the new key.
fn parse_new_key(
    state: &AppState,
    public_key_hex: &str,
    auth_scheme: AuthScheme,
    webauthn_registration: Option<&crate::types::request::WebAuthnRegistration>,
) -> Result<PublicKey, AppError> {
    let key_bytes =
        hex::decode(public_key_hex).map_err(|_| AppError::bad_request("Invalid hex encoding"))?;
    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;
    let pubkey = PublicKey(verifying_key);
    if auth_scheme == AuthScheme::WebAuthn {
        let registration = webauthn_registration.ok_or_else(|| {
            AppError::bad_request("webauthn_registration is required for webauthn keys")
        })?;
        let extracted = webauthn::public_key_from_registration(&state.webauthn, registration)
            .map_err(|err| {
                AppError::bad_request(format!("Invalid WebAuthn registration: {err}"))
            })?;
        if extracted != pubkey.compressed_bytes() {
            return Err(AppError::bad_request(
                "WebAuthn registration public key does not match public_key_hex",
            ));
        }
    }
    Ok(pubkey)
}

/// POST /v1/accounts/{id}/keys — bootstrap the FIRST signing key (service tier).
///
/// SYB-229: public unsigned key registration is a critical auth hole — anyone
/// could attach their own key to any account and then sign as it. This endpoint
/// is service-token gated (like account creation) and accepts an UNSIGNED
/// registration ONLY when the account has zero registered keys. Once an account
/// has a key, every subsequent key must be added via the SIGNED path
/// (`POST /v1/accounts/{id}/keys/register`), authorized by an existing key.
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/keys",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = RegisterKeyRequest,
    responses(
        (status = 200, description = "First key registered"),
        (status = 400, description = "Invalid key"),
        (status = 404, description = "Account not found"),
        (status = 409, description = "Account already has a key; use the signed register path")
    )
)]
pub async fn register_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account_id = AccountId(id);
    // First-key bootstrap only: refuse if the account already has a key so this
    // unsigned service endpoint can never overwrite/append to an established
    // account. Additional keys go through the signed path.
    let existing = state.sequencer.signing_keys_for_account(account_id).await?;
    if !existing.is_empty() {
        return Err(AppError::conflict(
            "Account already has a signing key; register additional keys via \
             POST /v1/accounts/{id}/keys/register (signed by an existing key)",
        ));
    }

    let pubkey = parse_new_key(
        &state,
        &req.public_key_hex,
        req.auth_scheme,
        req.webauthn_registration.as_ref(),
    )?;

    state
        .sequencer
        .register_pubkey_with_meta(
            account_id,
            pubkey,
            RegisteredPubkey {
                account_id,
                auth_scheme: sequencer_auth_scheme(req.auth_scheme),
                label: req.label.clone(),
                scope: sequencer_key_scope(req.scope),
                created_at_ms: now_ms(),
            },
        )
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /v1/accounts/{id}/keys/register — register an additional signing key,
/// authorized by a signature from an existing account key (SYB-229).
///
/// Mirrors the SYB-60 revoke shape: canonical bytes cover the account, the new
/// key (scheme + compressed SEC1), the signer, and a replay nonce, and are
/// domain-separated by `genesis_hash` (SYB-224). The `raw_p256` signer path
/// hands a `SignedKeyRegistration` to the sequencer (which re-verifies and burns
/// the nonce); the `webauthn` signer path verifies the assertion at the edge and
/// hands an already-authenticated intent.
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/keys/register",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = SignedRegisterKeyRequest,
    responses(
        (status = 200, description = "Key registered"),
        (status = 400, description = "Invalid key or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer or account"),
        (status = 409, description = "Key already registered, or stale nonce")
    )
)]
pub async fn register_signed_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<SignedRegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account_id = AccountId(id);
    let new_pubkey = parse_new_key(
        &state,
        &req.public_key_hex,
        req.auth_scheme,
        req.webauthn_registration.as_ref(),
    )?;
    let signer = parse_signer(&req.signer_pubkey_hex)?;
    let new_auth_scheme = sequencer_auth_scheme(req.auth_scheme);
    let scope = sequencer_key_scope(req.scope);
    let label = req.label.clone();

    match req.signer_auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedKeyRegistration {
                account_id,
                new_pubkey,
                new_auth_scheme,
                label,
                scope,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            state.sequencer.register_key_signed(signed).await?;
        }
        AuthScheme::WebAuthn => {
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_key_registration_bytes(
                account_id,
                new_auth_scheme,
                &new_pubkey.compressed_bytes(),
                &signer.compressed_bytes(),
                req.nonce,
                genesis_hash,
            );
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            state
                .sequencer
                .register_key_authenticated(AuthenticatedKeyRegistration {
                    account_id,
                    new_pubkey,
                    new_auth_scheme,
                    label,
                    scope,
                    nonce: req.nonce,
                    signer,
                })
                .await?;
        }
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

fn sequencer_key_scope(scope: KeyScopeDto) -> KeyScope {
    match scope {
        KeyScopeDto::Primary => KeyScope::Primary,
        KeyScopeDto::Agent => KeyScope::Agent,
        KeyScopeDto::Custom => KeyScope::Custom,
    }
}

// --- SYB-60 shared signed-mutation helpers ---
//
// Bearer-vs-signing boundary: every mutation below is P256-signed over canonical
// bytes plus a replay nonce, exactly like orders/cancels/withdrawals. Bearer API
// keys (created here) are READ-ONLY and can never authorize these mutations —
// that is what preserves the signing model's replay protection. To grant an
// agent trade authority, register an additional P256 key (`scope: agent`), which
// signs like any other key.

fn parse_signer(public_key_hex: &str) -> Result<PublicKey, AppError> {
    let key_bytes = hex::decode(public_key_hex.trim_start_matches("0x"))
        .map_err(|_| AppError::bad_request("Invalid hex encoding for signer public key"))?;
    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;
    Ok(PublicKey(verifying_key))
}

fn parse_raw_signature(signature_hex: Option<&str>) -> Result<Signature, AppError> {
    let hex_str = signature_hex.ok_or_else(|| {
        AppError::bad_request("signature_hex is required for raw_p256 signed requests")
    })?;
    let sig_bytes = hex::decode(hex_str.trim_start_matches("0x"))
        .map_err(|_| AppError::bad_request("Invalid hex encoding for signature"))?;
    Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 ECDSA signature"))
}

/// For the WebAuthn path, confirm the signer is registered as a WebAuthn key and
/// verify the assertion over the canonical bytes. Returns Ok once the intent is
/// authenticated (the caller then hands an `Authenticated*` value to the
/// sequencer, which re-checks signer↔account and burns the nonce).
async fn verify_webauthn_intent(
    state: &AppState,
    signer: &PublicKey,
    canonical: &[u8],
    assertion: Option<&WebAuthnAssertion>,
) -> Result<(), AppError> {
    let registered = state
        .sequencer
        .lookup_registered_pubkey(signer.clone())
        .await?
        .ok_or_else(|| AppError::not_found("No account registered for this public key"))?;
    if registered.auth_scheme != AccountAuthScheme::WebAuthn {
        return Err(AppError::forbidden(
            "Signer key is not registered for the webauthn auth scheme",
        ));
    }
    let assertion = assertion.ok_or_else(|| {
        AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
    })?;
    webauthn::verify_assertion(&state.webauthn, &signer.0, canonical, assertion)
        .map_err(|err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")))
}

const DISPLAY_NAME_MAX: usize = 32;
const AVATAR_SEED_MAX: usize = 64;

/// Validate optional profile fields (length + charset). Empty strings are
/// treated as "clear" (mapped to None) so a UI can clear via `""`.
fn validate_profile_fields(
    display_name: Option<String>,
    avatar_seed: Option<String>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let display_name = match display_name.map(|s| s.trim().to_string()) {
        Some(s) if s.is_empty() => None,
        Some(s) => {
            if s.chars().count() > DISPLAY_NAME_MAX {
                return Err(AppError::bad_request(format!(
                    "display_name must be at most {DISPLAY_NAME_MAX} characters"
                )));
            }
            if !s.chars().all(|c| c.is_alphanumeric() || " _-.".contains(c)) {
                return Err(AppError::bad_request(
                    "display_name may only contain letters, digits, spaces, and _-.",
                ));
            }
            Some(s)
        }
        None => None,
    };
    let avatar_seed = match avatar_seed {
        Some(s) if s.is_empty() => None,
        Some(s) => {
            if s.len() > AVATAR_SEED_MAX {
                return Err(AppError::bad_request(format!(
                    "avatar_seed must be at most {AVATAR_SEED_MAX} characters"
                )));
            }
            if !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            {
                return Err(AppError::bad_request(
                    "avatar_seed may only contain ASCII letters, digits, and _-.",
                ));
            }
            Some(s)
        }
        None => None,
    };
    Ok((display_name, avatar_seed))
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
            current_price_nanos: p.current_price_nanos.0,
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
        ("after" = Option<String>, Query, description = "Stable cursor returned as `cursor` on each fill. When present, returns fills strictly after this cursor in ascending order. Use `0.0` to start from the beginning."),
        ("limit" = Option<usize>, Query, description = "Result limit (default 100, cap 500)"),
        ("offset" = Option<usize>, Query, deprecated, description = "Deprecated offset-from-newest pagination. Ignored when `after` is present."),
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
    let limit = account_fill_query_limit(params.limit);
    let fills = if let Some(after) = params.after.as_deref() {
        let cursor = AccountFillCursor::parse(after)
            .ok_or_else(|| AppError::bad_request("Invalid fill cursor"))?;
        state
            .sequencer
            .get_account_fills_after(AccountId(id), market_id, Some(cursor), limit)
            .await?
    } else {
        let offset = params.offset.unwrap_or(0);
        state
            .sequencer
            .get_account_fills(AccountId(id), market_id, limit, offset)
            .await?
    };

    let response: Vec<AccountFillResponse> = fills.into_iter().map(account_fill_response).collect();

    Ok(Json(response))
}

fn account_fill_response(f: AccountFillRecord) -> AccountFillResponse {
    AccountFillResponse {
        cursor: AccountFillCursor::from_record(&f).to_string(),
        order_id: f.order_id,
        fill_qty: f.fill_qty,
        fill_price_nanos: f.fill_price.0,
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
            reason: e.reason.map(|r| r.to_string()),
            required_nanos: e.required_nanos,
            available_nanos: e.available_nanos,
        })
        .collect();
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
pub struct AccountFillParams {
    pub market_id: Option<u32>,
    pub after: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

fn account_fill_query_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_ACCOUNT_FILL_QUERY_LIMIT)
        .min(MAX_ACCOUNT_FILL_QUERY_LIMIT)
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
    let now_ms = now_ms();
    let since_ms = match params.range.as_deref() {
        Some("24h") => now_ms.saturating_sub(24 * 3_600_000),
        Some("7d") => now_ms.saturating_sub(7 * 24 * 3_600_000),
        Some("30d") => now_ms.saturating_sub(30 * 24 * 3_600_000),
        _ => 0,
    };
    let points = state
        .sequencer
        .get_equity_series(AccountId(id), since_ms)
        .await?;
    let points: Vec<EquityPointResponse> = points
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
    }))
}

/// POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/profile",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = SetProfileRequest,
    responses(
        (status = 200, description = "Profile updated", body = AccountResponse),
        (status = 400, description = "Invalid profile or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer"),
        (status = 409, description = "Replay nonce is stale or duplicate")
    )
)]
pub async fn set_profile(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<SetProfileRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    let account_id = AccountId(id);
    let (display_name, avatar_seed) = validate_profile_fields(req.display_name, req.avatar_seed)?;
    let signer = parse_signer(&req.signer_pubkey_hex)?;
    let canonical = canonical_profile_update_bytes(
        account_id,
        display_name.as_deref(),
        avatar_seed.as_deref(),
        req.nonce,
    );

    let account = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedProfileUpdate {
                account_id,
                display_name,
                avatar_seed,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            state.sequencer.set_profile_signed(signed).await?
        }
        AuthScheme::WebAuthn => {
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            state
                .sequencer
                .set_profile_authenticated(AuthenticatedProfileUpdate {
                    account_id,
                    display_name,
                    avatar_seed,
                    nonce: req.nonce,
                    signer,
                })
                .await?
        }
    };
    Ok(Json(account_to_response(&account)))
}

/// GET /v1/accounts/{id}/keys — list registered signing keys with metadata
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/keys",
    params(("id" = u64, Path, description = "Account ID")),
    responses((status = 200, description = "Registered signing keys", body = [AccountKeyResponse]))
)]
pub async fn list_account_keys(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Vec<AccountKeyResponse>>, AppError> {
    let keys = state
        .sequencer
        .signing_keys_for_account(AccountId(id))
        .await?;
    let out = keys
        .into_iter()
        .map(|(pubkey, meta)| AccountKeyResponse {
            public_key_hex: hex::encode(pubkey),
            auth_scheme: match meta.auth_scheme {
                AccountAuthScheme::RawP256 => "raw_p256".to_string(),
                AccountAuthScheme::WebAuthn => "webauthn".to_string(),
            },
            scope: meta.scope.as_str().to_string(),
            label: meta.label,
            created_at_ms: meta.created_at_ms,
        })
        .collect();
    Ok(Json(out))
}

/// POST /v1/accounts/{id}/keys/revoke — revoke a signing key (signed) (SYB-60)
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/keys/revoke",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = RevokeKeyRequest,
    responses(
        (status = 200, description = "Key revoked"),
        (status = 400, description = "Invalid request or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer or key"),
        (status = 409, description = "Cannot revoke the last key, or stale nonce")
    )
)]
pub async fn revoke_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<RevokeKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account_id = AccountId(id);
    // Normalize the target to compressed SEC1 bytes so the signed canonical form
    // is stable regardless of the caller's hex casing / prefix.
    let target = parse_signer(&req.target_pubkey_hex)?;
    let target_bytes = target.compressed_bytes();
    let signer = parse_signer(&req.signer_pubkey_hex)?;

    match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedKeyRevocation {
                account_id,
                target_pubkey: target_bytes,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            state.sequencer.revoke_signing_key_signed(signed).await?;
        }
        AuthScheme::WebAuthn => {
            // Revocation canonical bytes are domain-separated by genesis_hash
            // (SYB-231); the signed (raw P256) path checks this in the actor,
            // the WebAuthn path binds it here at the API edge.
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical =
                canonical_key_revocation_bytes(account_id, &target_bytes, req.nonce, genesis_hash);
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            state
                .sequencer
                .revoke_signing_key_authenticated(AuthenticatedKeyRevocation {
                    account_id,
                    target_pubkey: target_bytes,
                    nonce: req.nonce,
                    signer,
                })
                .await?;
        }
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /v1/accounts/{id}/api-keys — list read API keys (metadata only) (SYB-60)
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/api-keys",
    params(("id" = u64, Path, description = "Account ID")),
    responses((status = 200, description = "Read API keys", body = [ApiKeyResponse]))
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Vec<ApiKeyResponse>>, AppError> {
    let keys = state.sequencer.api_keys_for_account(AccountId(id)).await?;
    let out = keys
        .into_iter()
        .map(|k| ApiKeyResponse {
            id: k.id,
            label: k.label,
            created_at_ms: k.created_at_ms,
            revoked_at_ms: k.revoked_at_ms,
        })
        .collect();
    Ok(Json(out))
}

/// POST /v1/accounts/{id}/api-keys — create a read API key (signed) (SYB-60)
///
/// The bearer token is returned exactly once; only its blake3 hash is stored.
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/api-keys",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "API key created (token shown once)", body = CreateApiKeyResponse),
        (status = 400, description = "Invalid request or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer"),
        (status = 409, description = "Replay nonce is stale or duplicate")
    )
)]
pub async fn create_api_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, AppError> {
    let account_id = AccountId(id);
    let label = req.label.clone().filter(|s| !s.is_empty());
    let signer = parse_signer(&req.signer_pubkey_hex)?;
    let canonical = canonical_api_key_create_bytes(account_id, label.as_deref(), req.nonce);

    // Generate the token server-side (256 bits of CSPRNG entropy). The plaintext
    // is returned once below and never persisted; only blake3(token) is stored.
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw)
        .map_err(|_| AppError::internal("Failed to generate API key entropy"))?;
    let token = format!("sybk_{}", hex::encode(raw));
    let token_hash = api_key_hash(token.as_bytes());

    let key_id = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedApiKeyCreate {
                account_id,
                label: label.clone(),
                token_hash,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            state.sequencer.create_api_key_signed(signed).await?
        }
        AuthScheme::WebAuthn => {
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            state
                .sequencer
                .create_api_key_authenticated(AuthenticatedApiKeyCreate {
                    account_id,
                    label: label.clone(),
                    token_hash,
                    nonce: req.nonce,
                    signer,
                })
                .await?
        }
    };
    Ok(Json(CreateApiKeyResponse {
        id: key_id,
        token,
        label,
        created_at_ms: now_ms(),
    }))
}

/// POST /v1/accounts/{id}/api-keys/revoke — revoke a read API key (signed)
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/api-keys/revoke",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = RevokeApiKeyRequest,
    responses(
        (status = 200, description = "API key revoked"),
        (status = 400, description = "Invalid request or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer or API key"),
        (status = 409, description = "Replay nonce is stale or duplicate")
    )
)]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<RevokeApiKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account_id = AccountId(id);
    let signer = parse_signer(&req.signer_pubkey_hex)?;
    let canonical = canonical_api_key_revoke_bytes(account_id, req.api_key_id, req.nonce);

    match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedApiKeyRevoke {
                account_id,
                api_key_id: req.api_key_id,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            state.sequencer.revoke_api_key_signed(signed).await?;
        }
        AuthScheme::WebAuthn => {
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            state
                .sequencer
                .revoke_api_key_authenticated(AuthenticatedApiKeyRevoke {
                    account_id,
                    api_key_id: req.api_key_id,
                    nonce: req.nonce,
                    signer,
                })
                .await?;
        }
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Extract and authenticate a read-scoped bearer token from `Authorization`.
///
/// Returns the account the token belongs to (active keys only). This is the
/// reusable gating primitive for private read endpoints. It is applied to ONE
/// new endpoint (`private-summary`) as the template — existing public endpoints
/// are intentionally left ungated, because gating them is a breaking change and
/// a deliberate future step.
async fn bearer_account(state: &AppState, headers: &HeaderMap) -> Result<AccountId, AppError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("Missing bearer token"))?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or_else(|| AppError::unauthorized("Malformed Authorization header"))?
        .trim();
    let token_hash = api_key_hash(token.as_bytes());
    state
        .sequencer
        .lookup_api_key(token_hash)
        .await?
        .ok_or_else(|| AppError::unauthorized("Invalid or revoked API key"))
}

/// GET /v1/accounts/{id}/private-summary — bearer-gated private read (SYB-60)
///
/// Template endpoint demonstrating `Authorization: Bearer` gating. It returns
/// the same account data the public endpoints already expose, but only to a
/// read key that belongs to the requested account.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/private-summary",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Private account summary", body = PrivateAccountSummaryResponse),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 404, description = "Account not found")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_private_summary(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<PrivateAccountSummaryResponse>, AppError> {
    let authed = bearer_account(&state, &headers).await?;
    if authed != AccountId(id) {
        return Err(AppError::forbidden(
            "Bearer token does not grant access to this account",
        ));
    }
    let account = state
        .sequencer
        .get_account(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {id} not found")))?;
    let portfolio = state.sequencer.get_portfolio(AccountId(id)).await?;
    let positions: Vec<PositionResponse> = account
        .positions
        .iter()
        .filter(|(_, &qty)| qty != 0)
        .map(|(&(market_id, outcome), &qty)| PositionResponse {
            market_id: market_id.0,
            outcome: if outcome == 0 { "YES" } else { "NO" }.to_string(),
            quantity: qty,
        })
        .collect();
    Ok(Json(PrivateAccountSummaryResponse {
        account_id: id,
        balance_nanos: account.balance,
        total_deposited_nanos: portfolio.total_deposited_nanos,
        portfolio_value_nanos: portfolio.portfolio_value_nanos,
        pnl_nanos: portfolio.pnl_nanos,
        positions,
        display_name: account.profile.display_name.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_fill_query_limit_defaults_and_clamps() {
        assert_eq!(
            account_fill_query_limit(None),
            DEFAULT_ACCOUNT_FILL_QUERY_LIMIT
        );
        assert_eq!(account_fill_query_limit(Some(0)), 0);
        assert_eq!(account_fill_query_limit(Some(42)), 42);
        assert_eq!(
            account_fill_query_limit(Some(MAX_ACCOUNT_FILL_QUERY_LIMIT + 1)),
            MAX_ACCOUNT_FILL_QUERY_LIMIT
        );
    }
}
