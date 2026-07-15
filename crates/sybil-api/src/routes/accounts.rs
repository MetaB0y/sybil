use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;

use matching_sequencer::crypto::{
    canonical_api_key_create_bytes, canonical_api_key_revoke_bytes,
    canonical_key_registration_bytes, canonical_key_revocation_bytes,
    canonical_profile_update_bytes,
};
use matching_sequencer::{
    AccountAuthScheme, AccountId, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedKeyRegistration, AuthenticatedKeyRevocation, AuthenticatedProfileUpdate, KeyScope,
    MAX_API_KEY_LABEL_BYTES, PublicKey, RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke,
    SignedKeyRegistration, SignedKeyRevocation, SignedProfileUpdate, api_key_hash,
};
use p256::Sec1Point;
use p256::ecdsa::{Signature, VerifyingKey};

use crate::convert::{account_balance_breakdown, account_to_response};
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    AuthScheme, CreateAccountRequest, CreateApiKeyRequest, FundAccountRequest,
    KeyScope as KeyScopeDto, OnboardAccountRequest, RegisterKeyRequest, RevokeApiKeyRequest,
    RevokeKeyRequest, SetProfileRequest, SignedRegisterKeyRequest, WebAuthnAssertion,
};
use crate::types::response::*;
use crate::util::now_ms;
use crate::webauthn;

pub(crate) mod history;

#[cfg(test)]
use history::{
    DEFAULT_ACCOUNT_FILL_QUERY_LIMIT, MAX_ACCOUNT_FILL_QUERY_LIMIT, account_fill_query_limit,
    fill_cursor_has_gap,
};
pub use history::{get_account_fills, get_account_history, get_equity, get_portfolio};
#[cfg(test)]
use matching_sequencer::AccountFillCursor;

fn sequencer_auth_scheme(scheme: AuthScheme) -> AccountAuthScheme {
    match scheme {
        AuthScheme::RawP256 => AccountAuthScheme::RawP256,
        AuthScheme::WebAuthn => AccountAuthScheme::WebAuthn,
    }
}

fn validate_signing_key_label(label: Option<&str>) -> Result<(), AppError> {
    RegisteredPubkey::validate_label(label).map_err(AppError::from)
}

/// GET /v1/onboarding — public account stock and fixed grant policy.
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/onboarding",
    responses(
        (status = 200, description = "Public onboarding stock policy", body = OnboardingPolicyResponse),
        (status = 500, description = "Sequencer unavailable")
    )
)]
pub async fn get_onboarding_policy(
    State(state): State<AppState>,
) -> Result<Json<OnboardingPolicyResponse>, AppError> {
    let accounts_allocated = state.sequencer.account_stock().await?;
    state.record_public_account_stock(accounts_allocated);
    let accounts_remaining = state
        .public_account_capacity
        .saturating_sub(accounts_allocated);
    Ok(Json(OnboardingPolicyResponse {
        enabled: accounts_remaining > 0,
        account_capacity: state.public_account_capacity,
        accounts_allocated,
        accounts_remaining,
        grant_nanos: state.public_account_grant_nanos,
    }))
}

/// POST /v1/onboarding/accounts — allocate one capped public account.
///
/// The server supplies the fixed grant. The API lock covers the durable-stock
/// read and atomic account/key command, so concurrent callers cannot overshoot
/// the lifetime ceiling.
#[utoipa::path(
    tag = "routesaccounts",
    post,
    path = "/v1/onboarding/accounts",
    request_body = OnboardAccountRequest,
    responses(
        (status = 200, description = "Public account and initial key created", body = AccountResponse),
        (status = 400, description = "Invalid initial key"),
        (status = 409, description = "Key conflict or public account capacity exhausted"),
        (status = 429, description = "Onboarding request rate exceeded")
    )
)]
pub async fn onboard_account(
    State(state): State<AppState>,
    Json(req): Json<OnboardAccountRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    validate_signing_key_label(req.initial_key.label.as_deref())?;
    let balance_nanos = i64::try_from(state.public_account_grant_nanos).map_err(|_| {
        AppError::internal("SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS exceeds the signed-balance range")
    })?;
    // Parse caller-controlled key material before occupying the allocation
    // lock or sequencer mailbox.
    let pubkey = parse_new_key(
        &state,
        &req.initial_key.public_key_hex,
        req.initial_key.auth_scheme,
        req.initial_key.webauthn_registration.as_ref(),
    )?;

    let _bootstrap_guard = state.account_bootstrap_lock.lock().await;
    if state
        .sequencer
        .lookup_registered_pubkey(pubkey.clone())
        .await?
        .is_some()
    {
        return Err(AppError::conflict(
            "Initial signing key is already registered",
        ));
    }
    let accounts_allocated = state.sequencer.account_stock().await?;
    state.record_public_account_stock(accounts_allocated);
    if accounts_allocated >= state.public_account_capacity {
        metrics::counter!(
            "sybil_public_account_creation_total",
            "result" => "capacity_exhausted"
        )
        .increment(1);
        return Err(AppError::public_account_capacity_exhausted(
            state.public_account_capacity,
        ));
    }

    let key = req.initial_key;
    let account = state
        .sequencer
        .create_account_with_initial_key(
            balance_nanos,
            pubkey,
            RegisteredPubkey {
                account_id: AccountId(0),
                auth_scheme: sequencer_auth_scheme(key.auth_scheme),
                label: key.label,
                scope: sequencer_key_scope(key.scope),
                created_at_ms: now_ms(),
            },
        )
        .await?;
    let stock = account.id.0.saturating_add(1);
    state.record_public_account_stock(stock);
    metrics::counter!("sybil_public_account_creation_total", "result" => "created").increment(1);
    Ok(Json(account_to_response(&account, 0)))
}

/// POST /v1/accounts — service/dev account creation with explicit funding.
///
/// This operator surface may install an initial key atomically or create the
/// deprecated bare account used by local tooling. It is never public.
#[utoipa::path(
    tag = "routesaccounts",
    post,
    path = "/v1/accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 200, description = "Service account created", body = AccountResponse),
        (status = 400, description = "Invalid initial balance or key"),
        (status = 401, description = "Service token required"),
        (status = 403, description = "Invalid service token")
    )
)]
pub async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    validate_signing_key_label(
        req.initial_key
            .as_ref()
            .and_then(|key| key.label.as_deref()),
    )?;
    let balance_nanos = i64::try_from(req.initial_balance_nanos).map_err(|_| {
        AppError::bad_request(format!(
            "initial_balance_nanos {} exceeds the maximum signed-balance range",
            req.initial_balance_nanos
        ))
    })?;
    // Validate all caller-controlled key material before allocating an account.
    let initial_key = req
        .initial_key
        .as_ref()
        .map(|key| {
            parse_new_key(
                &state,
                &key.public_key_hex,
                key.auth_scheme,
                key.webauthn_registration.as_ref(),
            )
            .map(|pubkey| (key, pubkey))
        })
        .transpose()?;

    let _bootstrap_guard = state.account_bootstrap_lock.lock().await;
    if let Some((_, pubkey)) = &initial_key
        && state
            .sequencer
            .lookup_registered_pubkey(pubkey.clone())
            .await?
            .is_some()
    {
        return Err(AppError::conflict(
            "Initial signing key is already registered",
        ));
    }

    let account = match initial_key {
        Some((key, pubkey)) => {
            state
                .sequencer
                .create_account_with_initial_key(
                    balance_nanos,
                    pubkey,
                    RegisteredPubkey {
                        // The sequencer overwrites this placeholder with the
                        // atomically allocated account id.
                        account_id: AccountId(0),
                        auth_scheme: sequencer_auth_scheme(key.auth_scheme),
                        label: key.label.clone(),
                        scope: sequencer_key_scope(key.scope),
                        created_at_ms: now_ms(),
                    },
                )
                .await?
        }
        None => state.sequencer.create_account(balance_nanos).await?,
    };
    // Service allocations consume the same monotonic id space. Keep the public
    // remaining-stock gauge honest even though this trusted path bypasses the
    // anonymous admission ceiling.
    state.record_public_account_stock(account.id.0.saturating_add(1));
    Ok(Json(account_to_response(&account, 0)))
}

/// POST /v1/accounts/{id}/fund
#[utoipa::path(
    tag = "routesaccounts",
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
    state
        .sequencer
        .fund_account(AccountId(id), amount_nanos)
        .await?;
    let (account, reserved_balance) = state
        .sequencer
        .get_account_with_reserved_balance(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {} not found", id)))?;
    Ok(Json(account_to_response(&account, reserved_balance)))
}

/// GET /v1/accounts/{id}
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Account details", body = AccountResponse),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
        (status = 404, description = "Account not found")
    ),
    security(("bearer_read" = []))
)]
pub async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<AccountResponse>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    let (account, reserved_balance) = state
        .sequencer
        .get_account_with_reserved_balance(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {} not found", id)))?;
    Ok(Json(account_to_response(&account, reserved_balance)))
}

/// GET /v1/accounts/{id}/keyop-state — public signing state for key operations.
///
/// These digests are already committed validity state and reveal no key or
/// portfolio data. A client must fetch them immediately before signing a
/// registration or revocation; admission rejects stale values with 409.
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/keyop-state",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Current key-operation signing state", body = KeyOpStateResponse),
        (status = 404, description = "Account not found")
    )
)]
pub async fn get_keyop_state(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<KeyOpStateResponse>, AppError> {
    let account = state
        .sequencer
        .get_account(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {id} not found")))?;
    Ok(Json(KeyOpStateResponse {
        account_id: id,
        keys_digest_hex: hex::encode(account.keys_digest),
        events_digest_hex: hex::encode(account.events_digest),
    }))
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
    tag = "routesaccounts",
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
    headers: HeaderMap,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    crate::app::require_service_token(&state, &headers)?;
    validate_signing_key_label(req.label.as_deref())?;
    let _bootstrap_guard = state.account_bootstrap_lock.lock().await;
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
/// Canonical bytes cover the full new key record and the account's current
/// key/event digests, domain-separated by `genesis_hash`. The raw-P256 path is
/// re-verified by the sequencer; the WebAuthn path is verified at the edge and
/// again by the shared verifier before the authenticated intent is forwarded.
#[utoipa::path(
    tag = "routesaccounts",
    post,
    path = "/v1/accounts/{id}/keys/register",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = SignedRegisterKeyRequest,
    responses(
        (status = 200, description = "Key registered"),
        (status = 400, description = "Invalid key or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer or account"),
        (status = 409, description = "Key already registered, or stale state binding")
    )
)]
pub async fn register_signed_key(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<SignedRegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_signing_key_label(req.label.as_deref())?;
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
    let bound_keys_digest = parse_digest32("bound_keys_digest_hex", &req.bound_keys_digest_hex)?;
    let bound_events_digest =
        parse_digest32("bound_events_digest_hex", &req.bound_events_digest_hex)?;
    let key_record = sybil_verifier::KeyRecord {
        auth_scheme: new_auth_scheme.canonical_byte(),
        pubkey_sec1: new_pubkey
            .compressed_bytes()
            .try_into()
            .expect("compressed P-256 key is 33 bytes"),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };

    match req.signer_auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedKeyRegistration {
                account_id,
                new_pubkey,
                new_auth_scheme,
                label,
                scope,
                bound_keys_digest,
                bound_events_digest,
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
                genesis_hash,
                account_id,
                &key_record,
                bound_keys_digest,
                bound_events_digest,
            );
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            let authorization = webauthn::key_op_authorization(
                &signer.0,
                req.webauthn_assertion.as_ref().expect("verified above"),
            )
            .map_err(|err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")))?;
            let signer_record = sybil_verifier::KeyRecord {
                auth_scheme: AccountAuthScheme::WebAuthn.canonical_byte(),
                pubkey_sec1: signer
                    .compressed_bytes()
                    .try_into()
                    .expect("compressed P-256 key is 33 bytes"),
                capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
            };
            sybil_verifier::verify_keyop_auth(&authorization, [&signer_record], &canonical)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion: {err}"))
                })?;
            state
                .sequencer
                .register_key_authenticated(AuthenticatedKeyRegistration {
                    account_id,
                    new_pubkey,
                    new_auth_scheme,
                    label,
                    scope,
                    bound_keys_digest,
                    bound_events_digest,
                    signer,
                    authorization,
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
// Bearer-vs-signing boundary: every mutation below is P256-signed. Key ops bind
// current state digests; the others use replay nonces. Bearer API
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

fn parse_digest32(field: &str, value: &str) -> Result<[u8; 32], AppError> {
    let bytes = hex::decode(value.trim_start_matches("0x"))
        .map_err(|_| AppError::bad_request(format!("Invalid hex encoding for {field}")))?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        AppError::bad_request(format!("{field} must be 32 bytes, got {}", bytes.len()))
    })
}

/// For the WebAuthn path, confirm the signer is registered as a WebAuthn key and
/// verify the assertion over the canonical bytes. Returns Ok once the intent is
/// authenticated (the caller then hands an `Authenticated*` value to the
/// sequencer, which re-checks signer↔account and the relevant replay binding).
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

/// POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)
#[utoipa::path(
    tag = "routesaccounts",
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

    match req.auth_scheme {
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
    let (account, reserved_balance) = state
        .sequencer
        .get_account_with_reserved_balance(account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {} not found", id)))?;
    Ok(Json(account_to_response(&account, reserved_balance)))
}

/// GET /v1/accounts/{id}/keys — list registered signing keys with metadata
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/keys",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Registered signing keys", body = [AccountKeyResponse]),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account")
    ),
    security(("bearer_read" = []))
)]
pub async fn list_account_keys(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccountKeyResponse>>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
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
    tag = "routesaccounts",
    post,
    path = "/v1/accounts/{id}/keys/revoke",
    params(("id" = u64, Path, description = "Account ID")),
    request_body = RevokeKeyRequest,
    responses(
        (status = 200, description = "Key revoked"),
        (status = 400, description = "Invalid request or signature"),
        (status = 403, description = "Signer/account mismatch"),
        (status = 404, description = "Unknown signer or key"),
        (status = 409, description = "Cannot revoke the last key, or stale key state")
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
    let target_registered = state
        .sequencer
        .lookup_registered_pubkey(target.clone())
        .await?
        .ok_or(matching_sequencer::SequencerError::KeyNotFound)?;
    if target_registered.account_id != account_id {
        return Err(matching_sequencer::SequencerError::SignerAccountMismatch.into());
    }
    let target_key = sybil_verifier::KeyRecord {
        auth_scheme: target_registered.auth_scheme.canonical_byte(),
        pubkey_sec1: target_bytes
            .as_slice()
            .try_into()
            .expect("compressed P-256 key is 33 bytes"),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let bound_keys_digest = parse_digest32("bound_keys_digest_hex", &req.bound_keys_digest_hex)?;
    let bound_events_digest =
        parse_digest32("bound_events_digest_hex", &req.bound_events_digest_hex)?;

    match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedKeyRevocation {
                account_id,
                target_key,
                bound_keys_digest,
                bound_events_digest,
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
            let canonical = canonical_key_revocation_bytes(
                genesis_hash,
                account_id,
                &target_key,
                bound_keys_digest,
                bound_events_digest,
            );
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            let authorization = webauthn::key_op_authorization(
                &signer.0,
                req.webauthn_assertion.as_ref().expect("verified above"),
            )
            .map_err(|err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")))?;
            let signer_record = sybil_verifier::KeyRecord {
                auth_scheme: AccountAuthScheme::WebAuthn.canonical_byte(),
                pubkey_sec1: signer
                    .compressed_bytes()
                    .try_into()
                    .expect("compressed P-256 key is 33 bytes"),
                capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
            };
            sybil_verifier::verify_keyop_auth(&authorization, [&signer_record], &canonical)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion: {err}"))
                })?;
            state
                .sequencer
                .revoke_signing_key_authenticated(AuthenticatedKeyRevocation {
                    account_id,
                    target_key,
                    bound_keys_digest,
                    bound_events_digest,
                    signer,
                    authorization,
                })
                .await?;
        }
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /v1/accounts/{id}/api-keys — list read API keys (metadata only) (SYB-60)
#[utoipa::path(
    tag = "routesaccounts",
    get,
    path = "/v1/accounts/{id}/api-keys",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Read API keys", body = [ApiKeyResponse]),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account")
    ),
    security(("bearer_read" = []))
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeyResponse>>, AppError> {
    authorize_account_read(&state, &headers, AccountId(id)).await?;
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
    tag = "routesaccounts",
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
    if let Some(label) = label.as_deref()
        && label.len() > MAX_API_KEY_LABEL_BYTES
    {
        return Err(AppError::bad_request(format!(
            "API-key label exceeds {MAX_API_KEY_LABEL_BYTES} bytes"
        )));
    }
    let canonical = canonical_api_key_create_bytes(account_id, label.as_deref(), req.nonce);

    let signer = match req.auth_scheme {
        AuthScheme::RawP256 => {
            parse_signer(req.signer_pubkey_hex.as_deref().ok_or_else(|| {
                AppError::bad_request("signer_pubkey_hex is required for raw_p256 requests")
            })?)?
        }
        AuthScheme::WebAuthn => {
            if let Some(signer_hex) = req.signer_pubkey_hex.as_deref() {
                let signer = parse_signer(signer_hex)?;
                verify_webauthn_intent(
                    &state,
                    &signer,
                    &canonical,
                    req.webauthn_assertion.as_ref(),
                )
                .await?;
                signer
            } else {
                resolve_webauthn_login_signer(
                    &state,
                    account_id,
                    &canonical,
                    req.webauthn_assertion.as_ref(),
                )
                .await?
            }
        }
    };

    // Generate the token server-side (256 bits of CSPRNG entropy). The plaintext
    // is returned once below and never persisted; only blake3(token) is stored.
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw)
        .map_err(|_| AppError::internal("Failed to generate API key entropy"))?;
    let token = format!("sybk_{}", hex::encode(raw));
    let token_hash = api_key_hash(token.as_bytes());
    let signer_pubkey_hex = hex::encode(signer.compressed_bytes());

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
    state
        .insert_read_api_key_owner(token_hash, account_id)
        .await;
    Ok(Json(CreateApiKeyResponse {
        id: key_id,
        token,
        label,
        created_at_ms: now_ms(),
        signer_pubkey_hex,
    }))
}

/// Resolve the signer during discoverable passkey login without exposing the
/// owner-gated key list. The API-key canonical payload does not include the
/// signer key, so one assertion can be checked against each active WebAuthn key
/// on the claimed account. Only the matching key is returned to the caller.
async fn resolve_webauthn_login_signer(
    state: &AppState,
    account_id: AccountId,
    canonical: &[u8],
    assertion: Option<&WebAuthnAssertion>,
) -> Result<PublicKey, AppError> {
    let assertion = assertion.ok_or_else(|| {
        AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
    })?;
    let keys = state.sequencer.signing_keys_for_account(account_id).await?;
    for (compressed, meta) in keys {
        if meta.auth_scheme != AccountAuthScheme::WebAuthn {
            continue;
        }
        let signer = parse_signer(&hex::encode(compressed))?;
        if webauthn::verify_assertion(&state.webauthn, &signer.0, canonical, assertion).is_ok() {
            return Ok(signer);
        }
    }
    Err(AppError::unauthorized(
        "Passkey is not registered for this account",
    ))
}

/// POST /v1/accounts/{id}/api-keys/revoke — revoke a read API key (signed)
#[utoipa::path(
    tag = "routesaccounts",
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

    let revoked_hash = state
        .sequencer
        .api_keys_for_account(account_id)
        .await?
        .into_iter()
        .find(|key| key.id == req.api_key_id && key.is_active())
        .map(|key| key.hash);

    let revoke_result = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signed = SignedApiKeyRevoke {
                account_id,
                api_key_id: req.api_key_id,
                nonce: req.nonce,
                signer,
                signature: parse_raw_signature(req.signature_hex.as_deref())?,
            };
            if let Some(hash) = revoked_hash {
                state.remove_read_api_key(&hash).await;
            }
            state.sequencer.revoke_api_key_signed(signed).await
        }
        AuthScheme::WebAuthn => {
            verify_webauthn_intent(&state, &signer, &canonical, req.webauthn_assertion.as_ref())
                .await?;
            if let Some(hash) = revoked_hash {
                state.remove_read_api_key(&hash).await;
            }
            state
                .sequencer
                .revoke_api_key_authenticated(AuthenticatedApiKeyRevoke {
                    account_id,
                    api_key_id: req.api_key_id,
                    nonce: req.nonce,
                    signer,
                })
                .await
        }
    };
    if let Err(error) = revoke_result {
        if let Some(hash) = revoked_hash {
            state.insert_read_api_key_owner(hash, account_id).await;
        }
        return Err(error.into());
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Extract and authenticate a read-scoped bearer token from `Authorization`.
/// Returns the account the active token belongs to.
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
        .read_api_key_owner(token_hash)
        .await?
        .ok_or_else(|| AppError::unauthorized("Invalid or revoked API key"))
}

/// Authorize a per-account read for either the account owner or trusted service
/// infrastructure. Owner mismatches are deliberately 403; absent, malformed,
/// invalid, and revoked read keys are 401.
pub(crate) async fn authorize_account_read(
    state: &AppState,
    headers: &HeaderMap,
    requested: AccountId,
) -> Result<(), AppError> {
    // Dev mode mirrors the service-route middleware: local tools and legacy
    // integration flows run as the trusted operator without a bearer token.
    if state.dev_mode {
        return Ok(());
    }
    if crate::app::request_has_valid_service_token(state, headers) {
        return Ok(());
    }
    let authed = bearer_account(state, headers).await?;
    if authed != requested {
        return Err(AppError::forbidden(
            "Bearer token does not grant access to this account",
        ));
    }
    Ok(())
}

/// GET /v1/accounts/{id}/private-summary — bearer-gated private read (SYB-60)
///
/// Template endpoint demonstrating `Authorization: Bearer` gating. It returns
/// the same account data the public endpoints already expose, but only to a
/// read key that belongs to the requested account.
#[utoipa::path(
    tag = "routesaccounts",
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
    authorize_account_read(&state, &headers, AccountId(id)).await?;
    let (account, portfolio, reserved_balance) = state
        .sequencer
        .get_account_summary_with_reserved_balance(AccountId(id))
        .await?
        .ok_or_else(|| AppError::not_found(format!("Account {id} not found")))?;
    let positions: Vec<PositionResponse> = account
        .positions
        .iter()
        .filter(|&(_, &qty)| qty != 0)
        .map(|(&(market_id, outcome), &qty)| PositionResponse {
            market_id: market_id.0,
            outcome: if outcome == 0 { "YES" } else { "NO" }.to_string(),
            quantity: qty,
        })
        .collect();
    let (available_balance_nanos, reserved_balance_nanos) =
        account_balance_breakdown(account.balance, reserved_balance);
    Ok(Json(PrivateAccountSummaryResponse {
        account_id: id,
        balance_nanos: account.balance,
        available_balance_nanos,
        reserved_balance_nanos,
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

    #[test]
    fn fill_cursor_gap_includes_start_sentinel_after_pruning() {
        assert!(fill_cursor_has_gap(Some(AccountFillCursor::MIN), Some(1)));
        assert!(fill_cursor_has_gap(
            Some(AccountFillCursor::new(4, 9)),
            Some(4)
        ));
        assert!(!fill_cursor_has_gap(
            Some(AccountFillCursor::new(5, 1)),
            Some(4)
        ));
        assert!(!fill_cursor_has_gap(None, Some(4)));
    }
}
