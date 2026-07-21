//! API request types (DTOs).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateAccountRequest {
    /// Caller-stable retry identity. The server binds it to the current
    /// genesis and exact creation parameters.
    pub provisioning_key: String,
    /// Initial account balance. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(
        feature = "openapi",
        schema(
            value_type = String,
            pattern = r"^[0-9]+$",
            example = "100000000000"
        )
    )]
    pub initial_balance_nanos: u64,
    /// First signing key to register in the same account-creation operation.
    ///
    /// This service/dev DTO may omit the key for legacy operator tooling.
    /// Public self-service uses [`OnboardAccountRequest`] and always requires
    /// an initial key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_key: Option<RegisterKeyRequest>,
}

/// Public self-service account onboarding.
///
/// The server, not the caller, chooses the play-money grant. Keeping funding
/// out of this DTO prevents anonymous callers from turning account allocation
/// into an arbitrary minting interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OnboardAccountRequest {
    /// First signing key installed atomically with account allocation.
    pub initial_key: RegisterKeyRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FundAccountRequest {
    /// Amount to add to the account balance. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = String, pattern = r"^[0-9]+$", example = "50000000000")
    )]
    pub amount_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitL1DepositRequest {
    /// Sequential L1 vault deposit id.
    pub deposit_id: u64,
    /// Sybil account receiving the credit. Must be absent when `quarantine` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<u64>,
    /// Dispose an unresolvable raw key into the committed system quarantine ledger.
    #[serde(default)]
    pub quarantine: bool,
    /// Source chain id.
    pub chain_id: u64,
    /// Hex-encoded vault contract address (20 bytes).
    pub vault_address_hex: String,
    /// Hex-encoded token contract address (20 bytes).
    pub token_address_hex: String,
    /// Hex-encoded L1 sender address (20 bytes).
    pub sender_hex: String,
    /// Optional Sybil bridge account key. If omitted, the API derives it for the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_account_key_hex: Option<String>,
    /// Token base units accepted by the vault, e.g. USDC's 6-decimal units.
    pub amount_token_units: u64,
    /// Hex-encoded post-deposit L1 deposit tree root (32 bytes).
    pub deposit_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBridgeWithdrawalRequest {
    /// Sybil account whose available balance is burned.
    pub account_id: u64,
    /// Destination chain id.
    pub chain_id: u64,
    /// Hex-encoded vault contract address (20 bytes).
    pub vault_address_hex: String,
    /// Hex-encoded L1 recipient address (20 bytes).
    pub recipient_hex: String,
    /// Hex-encoded token contract address (20 bytes).
    pub token_address_hex: String,
    /// Token base units released by the vault.
    pub amount_token_units: u64,
    /// Last L1 block height at which this withdrawal leaf is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_height: Option<u64>,
    /// Per-account replay nonce. Required for signed bridge withdrawals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateSignedBridgeWithdrawalRequest {
    /// Withdrawal payload covered by the P256 signature.
    pub withdrawal: CreateBridgeWithdrawalRequest,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Authentication scheme for this signer. Defaults to raw P256 for SDKs and bots.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over the canonical withdrawal payload.
    /// Required when `auth_scheme` is `raw_p256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope. Required when `auth_scheme` is `webauthn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BridgeWithdrawalL1Status {
    #[default]
    NotRequested,
    Queued,
    Finalized,
    Cancelled,
    Refunded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitL1WithdrawalEventRequest {
    /// Withdrawal nullifier emitted by SybilVault.
    pub nullifier_hex: String,
    /// Queue state observed from the vault event.
    pub status: BridgeWithdrawalL1Status,
    /// Event timestamp from the vault event, in Unix seconds.
    pub event_at_unix: u64,
    /// Finalization ETA emitted by the vault, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_at_unix: Option<u64>,
    /// L1 transaction hash carrying the event, if indexed from logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash_hex: Option<String>,
    /// Confirmed L1 block number carrying the event.
    pub l1_block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ObserveL1HeightRequest {
    /// Highest fully scanned and confirmed L1 block.
    pub l1_block_height: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    /// Raw P256 ECDSA over Sybil canonical bytes.
    #[default]
    RawP256,
    /// WebAuthn assertion with challenge `base64url(sha256(canonical_bytes))`.
    ///
    /// Explicit rename: `snake_case` of `WebAuthn` would be `web_authn`, but the
    /// documented and deployed wire format — used by the entire frontend, the
    /// SDKs, and every `auth_scheme` doc string — is `webauthn`. (SYB-60 caught
    /// this as a silent wire regression when regenerating the OpenAPI client.)
    #[serde(rename = "webauthn")]
    WebAuthn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisterKeyRequest {
    /// Hex-encoded compressed P256 public key (33 bytes).
    #[cfg_attr(
        feature = "openapi",
        schema(example = "036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
    )]
    pub public_key_hex: String,
    /// Authentication scheme associated with this account key. Defaults to
    /// `raw_p256` so existing bots, SDKs, and arena clients are unchanged.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Base64url credential id for WebAuthn keys. Stored client-side today and
    /// documented here so passkey clients can round-trip the registration payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id_b64url: Option<String>,
    /// Optional WebAuthn registration payload. When present, the server parses
    /// the attestation object's COSE EC2 public key and requires it to match
    /// `public_key_hex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_registration: Option<WebAuthnRegistration>,
    /// Optional human label for this key, e.g. "agent:pricer" (SYB-60),
    /// limited to 128 UTF-8 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Scope tag describing what this key is for (SYB-60). Defaults to `primary`.
    /// Register an agent trade key by passing `agent` with its own P256 keypair —
    /// it then signs like any account key (bearer API keys are read-only and
    /// cannot trade).
    #[serde(default)]
    pub scope: KeyScope,
}

/// Signed request to register a NEW signing key on an account (SYB-229).
///
/// Required whenever the account already has at least one registered key. The
/// first key is bootstrapped over the service tier (`POST /v1/accounts/{id}/keys`);
/// every subsequent key must be authorized by a signature from an existing
/// account key. Like orders/cancels, the canonical payload is domain-separated
/// by the chain `genesis_hash` (SYB-224).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SignedRegisterKeyRequest {
    /// Hex-encoded compressed P256 public key (33 bytes) of the NEW key.
    #[cfg_attr(
        feature = "openapi",
        schema(example = "036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
    )]
    pub public_key_hex: String,
    /// Authentication scheme of the NEW key. Defaults to `raw_p256`. When
    /// `webauthn`, `webauthn_registration` must prove possession of the new key.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Base64url credential id for a WebAuthn new key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id_b64url: Option<String>,
    /// Optional WebAuthn registration payload for a WebAuthn new key. When
    /// present, the server parses the attestation object's COSE EC2 public key
    /// and requires it to match `public_key_hex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_registration: Option<WebAuthnRegistration>,
    /// Optional human label for the new key, e.g. "agent:pricer", limited to
    /// 128 UTF-8 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Scope tag for the new key. Defaults to `primary`.
    #[serde(default)]
    pub scope: KeyScope,
    /// Hex-encoded compressed P256 public key of the SIGNER — an existing active
    /// key on this account authorizing the registration.
    pub signer_pubkey_hex: String,
    /// Authentication scheme of the SIGNER. Defaults to `raw_p256`.
    #[serde(default)]
    pub signer_auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over the canonical registration
    /// payload. Required when `signer_auth_scheme` is `raw_p256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope. Required when `signer_auth_scheme` is `webauthn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
    /// Hex account key-set digest the authorization is state-bound to.
    pub bound_keys_digest_hex: String,
    /// Hex account event-chain digest the authorization is state-bound to.
    pub bound_events_digest_hex: String,
}

/// Scope tag for a registered signing key (SYB-60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum KeyScope {
    #[default]
    Primary,
    Agent,
    Custom,
}

/// Common P256/WebAuthn signature envelope shared by SYB-60 account-management
/// mutations. Mirrors the fields on `CreateSignedBridgeWithdrawalRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetProfileRequest {
    /// New display name, or `null` to clear it (SYB-60). Validated for
    /// length (1-32) and charset at the API edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// New identicon seed, or `null` to clear it. There is no image upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_seed: Option<String>,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Authentication scheme for this signer. Defaults to raw P256.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over the canonical profile payload.
    /// Required when `auth_scheme` is `raw_p256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope. Required when `auth_scheme` is `webauthn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
    /// Per-account replay nonce (strictly increasing).
    pub nonce: u64,
}

/// Signed request to revoke a registered signing key (SYB-60).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RevokeKeyRequest {
    /// Hex-encoded compressed P256 public key (33 bytes) of the key to revoke.
    pub target_pubkey_hex: String,
    /// Hex-encoded compressed P256 public key of the signer (any active key on
    /// the account may authorize revocation, including the target itself as long
    /// as another key remains).
    pub signer_pubkey_hex: String,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
    /// Hex account key-set digest the authorization is state-bound to.
    pub bound_keys_digest_hex: String,
    /// Hex account event-chain digest the authorization is state-bound to.
    pub bound_events_digest_hex: String,
}

/// Signed request to create a read-scoped bearer API key (SYB-60).
///
/// The bearer token is generated server-side, returned exactly once in the
/// response, and never recoverable afterwards (only its blake3 hash is stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateApiKeyRequest {
    /// Optional human label, e.g. "grafana".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Hex-encoded signer key. WebAuthn login bootstrap may omit this field;
    /// the server identifies the matching registered WebAuthn key by verifying
    /// the assertion against the account's active WebAuthn keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_pubkey_hex: Option<String>,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
    pub nonce: u64,
}

/// Signed request to revoke a read-scoped bearer API key by id (SYB-60).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RevokeApiKeyRequest {
    /// Id of the API key to revoke (from the API-key listing).
    pub api_key_id: u64,
    pub signer_pubkey_hex: String,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WebAuthnRegistration {
    /// Base64url-encoded WebAuthn attestationObject from `navigator.credentials.create`.
    pub attestation_object_b64url: String,
    /// Base64url-encoded WebAuthn clientDataJSON from `navigator.credentials.create`.
    pub client_data_json_b64url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WebAuthnAssertion {
    /// Base64url credential id returned by the authenticator.
    pub credential_id_b64url: String,
    /// Base64url authenticatorData bytes from `navigator.credentials.get`.
    pub authenticator_data_b64url: String,
    /// Base64url clientDataJSON bytes from `navigator.credentials.get`.
    pub client_data_json_b64url: String,
    /// Base64url DER-encoded ECDSA signature from `navigator.credentials.get`.
    pub signature_b64url: String,
    /// Optional base64url userHandle returned by the authenticator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_handle_b64url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMarketRequest {
    /// Name of the binary market.
    #[cfg_attr(feature = "openapi", schema(example = "Will it rain tomorrow?"))]
    pub name: String,
    /// Optional operator idempotency key. Repeating the same key and creation
    /// fields returns the original market; conflicting reuse is rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_key: Option<String>,
    /// Optional description of the market.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional category (e.g., "sports", "politics", "crypto").
    #[serde(default)]
    pub category: Option<String>,
    /// Optional tags for discovery.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Optional resolution criteria.
    #[serde(default)]
    pub resolution_criteria: Option<String>,
    /// Optional expiry timestamp in ms (0 = no expiry).
    #[serde(default)]
    pub expiry_timestamp_ms: Option<u64>,
    /// Resolution template id to use for this market (e.g. "admin_immediate",
    /// "polymarket_mirror"). `None` -> `admin_immediate`.
    #[serde(default)]
    pub resolution_template: Option<String>,
}

/// Full replacement of a live market's committed prose.
///
/// This is the edit path for text a creation key already owns — the catalog
/// applier reaches for it when a checked-in spec drifted from the deployed
/// market, where `POST /v1/markets` would return `MarketCreationKeyConflict`.
/// Replacement, not patch: every field is authoritative, and an omitted
/// optional clears the stored value.
///
/// Identity is not editable here. `creation_key`, `created_at_ms`, and the
/// resolution template stay as created whatever this body says — the point of
/// the op is to fix wording, not to re-key a market or re-wire how it settles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateMarketContentRequest {
    /// Replacement market name. Must not be blank.
    #[cfg_attr(feature = "openapi", schema(example = "Will it rain tomorrow?"))]
    pub name: String,
    /// Replacement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Replacement category.
    #[serde(default)]
    pub category: Option<String>,
    /// Replacement discovery tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Replacement resolution criteria.
    #[serde(default)]
    pub resolution_criteria: Option<String>,
    /// Replacement expiry timestamp in ms (0 = no expiry).
    #[serde(default)]
    pub expiry_timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMarketGroupRequest {
    /// Name for the group of mutually exclusive markets.
    #[cfg_attr(feature = "openapi", schema(example = "2024 Election"))]
    pub name: String,
    /// Optional stable operator identity. Exact retries return the original
    /// group; reuse with different creation fields is rejected.
    #[serde(default)]
    pub creation_key: Option<String>,
    /// Market IDs in the group.
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExtendMarketGroupRequest {
    /// Market ID to add to the existing group.
    pub market_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolveMarketRequest {
    /// Payout per YES share. Integer nanodollars; 1_000_000_000 = $1.
    /// Payouts are per-share probabilities in [0, 1e9].
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(
        feature = "openapi",
        schema(
            value_type = String,
            pattern = r"^0*(?:[0-9]{1,9}|1000000000)$",
            example = "1000000000"
        )
    )]
    pub payout_nanos: u64,
    /// Optional signed attestation. When provided, the market's resolution
    /// template drives verification; dev_mode is not required. When omitted,
    /// the server falls back to the legacy unsigned admin path, which
    /// requires dev_mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<SignedAttestationDto>,
}

/// Wire form of a signed resolution attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SignedAttestationDto {
    /// Hex-encoded compressed SEC1 P256 public key (33 bytes).
    pub pubkey_hex: String,
    /// Hex-encoded P256 ECDSA signature over the canonical attestation bytes.
    pub signature_hex: String,
    /// Nonce the signer chose (typically `timestamp_ms`).
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisterFeedRequest {
    /// Hex-encoded compressed P256 public key (33 bytes).
    pub pubkey_hex: String,
    /// Human-readable name (e.g. "admin", "polymarket_mirror").
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitOrderRequest {
    /// Account ID submitting the orders.
    pub account_id: u64,
    /// Orders to submit.
    #[cfg_attr(feature = "openapi", schema(min_items = 1))]
    pub orders: Vec<OrderSpec>,
    /// Time-in-force policy applied to all orders in this submission.
    #[serde(default)]
    pub time_in_force: TimeInForce,
    /// Last eligible block height for explicit-expiry orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_block: Option<u64>,
    /// If set, treat these orders as market maker orders with flash liquidity.
    /// The value is the MM's total capital budget. Integer nanodollars;
    /// 1_000_000_000 = $1.
    /// MM orders skip per-order balance validation; instead the solver enforces
    /// the portfolio-level budget constraint at clearing time.
    #[serde(default, with = "crate::wire_integer::option")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub mm_budget_nanos: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum TimeInForce {
    #[default]
    Gtc,
    Ioc,
    Gtd,
}

/// Tagged enum representing public order types.
///
/// Public submission is intentionally limited to single-market binary orders.
/// Compound payoff-vector orders remain available inside `matching-engine` for
/// research and tests, but are not accepted at the HTTP API edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type")]
pub enum OrderSpec {
    /// Buy YES share-units on a single market (`1000` units = 1 share).
    BuyYes {
        market_id: u32,
        /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
        /// Prices are per-share probabilities in [0, 1e9].
        #[serde(with = "crate::wire_integer")]
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = String, pattern = r"^0*(?:[1-9][0-9]{0,8}|1000000000)$")
        )]
        limit_price_nanos: u64,
        /// Order quantity. Integer share-units; 1000 units = 1 share.
        #[cfg_attr(feature = "openapi", schema(minimum = 1))]
        quantity: u64,
    },
    /// Buy NO share-units on a single market (`1000` units = 1 share).
    BuyNo {
        market_id: u32,
        /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
        /// Prices are per-share probabilities in [0, 1e9].
        #[serde(with = "crate::wire_integer")]
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = String, pattern = r"^0*(?:[1-9][0-9]{0,8}|1000000000)$")
        )]
        limit_price_nanos: u64,
        /// Order quantity. Integer share-units; 1000 units = 1 share.
        #[cfg_attr(feature = "openapi", schema(minimum = 1))]
        quantity: u64,
    },
    /// Sell YES share-units on a single market (`1000` units = 1 share).
    SellYes {
        market_id: u32,
        /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
        /// Prices are per-share probabilities in [0, 1e9].
        #[serde(with = "crate::wire_integer")]
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = String, pattern = r"^0*(?:[1-9][0-9]{0,8}|1000000000)$")
        )]
        limit_price_nanos: u64,
        /// Order quantity. Integer share-units; 1000 units = 1 share.
        #[cfg_attr(feature = "openapi", schema(minimum = 1))]
        quantity: u64,
    },
    /// Sell NO share-units on a single market (`1000` units = 1 share).
    SellNo {
        market_id: u32,
        /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
        /// Prices are per-share probabilities in [0, 1e9].
        #[serde(with = "crate::wire_integer")]
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = String, pattern = r"^0*(?:[1-9][0-9]{0,8}|1000000000)$")
        )]
        limit_price_nanos: u64,
        /// Order quantity. Integer share-units; 1000 units = 1 share.
        #[cfg_attr(feature = "openapi", schema(minimum = 1))]
        quantity: u64,
    },
}

/// Query parameters for market search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct MarketSearchParams {
    /// Text search (searches name + description).
    #[serde(default)]
    pub q: Option<String>,
    /// Comma-separated tags to filter by.
    #[serde(default)]
    pub tags: Option<String>,
    /// Exact category match.
    #[serde(default)]
    pub category: Option<String>,
    /// Status filter ("active" or "resolved").
    #[serde(default)]
    pub status: Option<String>,
    /// Minimum YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, with = "crate::wire_integer::option")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub min_yes_price_nanos: Option<u64>,
    /// Maximum YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, with = "crate::wire_integer::option")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub max_yes_price_nanos: Option<u64>,
    /// Minimum cumulative traded notional. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, with = "crate::wire_integer::option")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub min_volume_nanos: Option<u64>,
    /// Sort field: "volume", "created_at", "name", "price".
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitSignedOrderRequest {
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// The order to submit.
    pub order: SignedOrderData,
    /// API time-in-force policy. Signed IOC/GTD orders commit to `expires_at_block`.
    #[serde(default)]
    pub time_in_force: TimeInForce,
    /// Last eligible block height, covered by the P256 signature. Required for signed IOC/GTD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_block: Option<u64>,
    /// Per-account replay nonce covered by the P256 signature.
    pub nonce: u64,
    /// Authentication scheme for this signer. Defaults to raw P256 for SDKs and bots.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over the canonical order payload.
    /// Required when `auth_scheme` is `raw_p256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope. Required when `auth_scheme` is `webauthn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

/// Public signed submission of one all-or-nothing MM quote bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SubmitSignedMmBundleRequest {
    /// Account that owns the bundle. It must match the signer's registration.
    pub account_id: u64,
    /// Client-chosen opaque 32-byte bundle identity, hex encoded.
    pub bundle_id_hex: String,
    /// Initial submissions use revision zero.
    pub revision: u64,
    /// Every quote in the atomic bundle. All quote fields and their order are signed.
    #[cfg_attr(feature = "openapi", schema(min_items = 1))]
    pub orders: Vec<OrderSpec>,
    /// Exact next block this IOC bundle targets. The actor rejects any other height.
    pub expires_at_block: u64,
    /// Integer nanodollars: one flash-liquidity budget shared by every quote in the bundle.
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(feature = "openapi", schema(value_type = String, pattern = r"^[0-9]+$"))]
    pub mm_budget_nanos: u64,
    /// Per-account replay nonce covered by the signature.
    pub nonce: u64,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Authentication scheme for this signer.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over canonical bundle bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope over the same canonical bundle bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

/// Public signed atomic replacement of one active MM bundle revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ReplaceSignedMmBundleRequest {
    pub account_id: u64,
    pub bundle_id_hex: String,
    pub expected_revision: u64,
    pub new_revision: u64,
    #[cfg_attr(feature = "openapi", schema(min_items = 1))]
    pub orders: Vec<OrderSpec>,
    /// Exact next block this replacement targets.
    pub expires_at_block: u64,
    /// Integer nanodollars shared across every replacement quote.
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(feature = "openapi", schema(value_type = String, pattern = r"^[0-9]+$"))]
    pub mm_budget_nanos: u64,
    pub nonce: u64,
    pub signer_pubkey_hex: String,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

/// Public signed cancellation of one active MM bundle revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CancelSignedMmBundleRequest {
    pub account_id: u64,
    pub bundle_id_hex: String,
    pub expected_revision: u64,
    pub nonce: u64,
    pub signer_pubkey_hex: String,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CancelSignedOrderRequest {
    /// Account ID claiming ownership of the order being cancelled.
    pub account_id: u64,
    /// The pending order to cancel.
    pub order_id: u64,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Per-account replay nonce covered by the P256 signature.
    pub nonce: u64,
    /// Authentication scheme for this signer. Defaults to raw P256 for SDKs and bots.
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Hex-encoded raw P256 ECDSA signature over the canonical cancel payload.
    /// Required when `auth_scheme` is `raw_p256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
    /// WebAuthn assertion envelope. Required when `auth_scheme` is `webauthn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webauthn_assertion: Option<WebAuthnAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SignedOrderData {
    /// Market IDs this order spans.
    #[cfg_attr(feature = "openapi", schema(min_items = 1, max_items = 1))]
    pub market_ids: Vec<u32>,
    /// Payoff vector.
    #[cfg_attr(feature = "openapi", schema(min_items = 2, max_items = 2))]
    pub payoffs: Vec<i8>,
    /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(with = "crate::wire_integer")]
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = String, pattern = r"^0*(?:[1-9][0-9]{0,8}|1000000000)$")
    )]
    pub limit_price_nanos: u64,
    /// Maximum fill quantity. Integer share-units; 1000 units = 1 share.
    #[cfg_attr(feature = "openapi", schema(minimum = 1))]
    pub max_fill: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SetReferencePricesRequest {
    /// Map of market_id -> reference price. Integer nanodollars;
    /// 1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
    /// Zero explicitly evicts the current reference for that market.
    #[serde(with = "crate::wire_integer::map_u32_u64")]
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = std::collections::HashMap<u32, String>)
    )]
    pub prices_nanos: std::collections::HashMap<u32, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetMarketMetadataRequest {
    /// External URL (e.g., Polymarket link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    /// Polymarket parent event id — used by the frontend to group sibling
    /// markets (e.g., "Fed Decision in June" sub-questions). Distinct from the
    /// matching engine's NegRisk `MarketGroup`, which it does not affect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Polymarket parent event title — rendered as the MultiCard header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_title: Option<String>,
    /// Event-level image URL (primary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_image_url: Option<String>,
    /// Event-level icon URL (secondary; frontend uses as `onError` fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_icon_url: Option<String>,
    /// Event-level expected end date (epoch ms). Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_end_date_ms: Option<u64>,
    /// Per-market image URL (primary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_image_url: Option<String>,
    /// Per-market icon URL (secondary; frontend uses as `onError` fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_icon_url: Option<String>,
    /// Per-market expected end date (epoch ms). Display only; matching engine
    /// does not enforce trading cutoffs at this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_end_date_ms: Option<u64>,
    /// Single display category. **Legacy** — populated only for sybil-native
    /// markets at create time. Mirrored markets now use `categories` (plural)
    /// and let the frontend pick one for display via its own priority order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// All category buckets the parent event matched in the mirror's tag-to-
    /// bucket lookup (e.g. `["Sports", "Politics"]` for an NBA + Trump
    /// event). One per matched row; the frontend picks which to render
    /// using its own priority list, so reordering display priority is
    /// frontend-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Polymarket on-chain condition id — the FE join key into the event JSON
    /// snapshot (`/v1/events/{id}/raw` `markets[].conditionId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    /// Parent event start date (epoch ms). Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    /// Per-market start date (epoch ms). Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
    /// Polymarket short outcome label (`groupItemTitle`, e.g. "May 15"). The
    /// frontend renders this as the per-outcome name on multi-cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_item_title: Option<String>,
    /// Whether Polymarket has closed this market. The frontend hides closed
    /// markets from the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Wire-format pin: the frontend, SDKs, and deployed clients all send
    /// `"webauthn"` (not snake_case `"web_authn"`). A rename of the variant
    /// must never silently change the wire string again.
    #[test]
    fn auth_scheme_wire_format_is_pinned() {
        assert_eq!(
            serde_json::to_string(&AuthScheme::RawP256).unwrap(),
            "\"raw_p256\""
        );
        assert_eq!(
            serde_json::to_string(&AuthScheme::WebAuthn).unwrap(),
            "\"webauthn\""
        );
        assert_eq!(
            serde_json::from_str::<AuthScheme>("\"webauthn\"").unwrap(),
            AuthScheme::WebAuthn
        );
        assert!(serde_json::from_str::<AuthScheme>("\"web_authn\"").is_err());
    }

    #[test]
    fn custom_order_spec_is_not_part_of_public_submit_api() {
        let payload = json!({
            "account_id": 1,
            "orders": [{
                "type": "Custom",
                "market_ids": [0],
                "payoffs": [2, 0],
                "limit_price_nanos": 500_000_000u64,
                "max_fill": 1000u64
            }]
        });

        assert!(serde_json::from_value::<SubmitOrderRequest>(payload).is_err());
    }

    #[test]
    fn bundle_order_specs_are_not_part_of_public_submit_api() {
        for order_type in ["Spread", "BundleYes", "BundleSell"] {
            let order = match order_type {
                "Spread" => json!({
                    "type": order_type,
                    "market_a": 0,
                    "market_b": 1,
                    "limit_price_nanos": 500_000_000u64,
                    "quantity": 1000u64
                }),
                _ => json!({
                    "type": order_type,
                    "market_ids": [0, 1],
                    "limit_price_nanos": 500_000_000u64,
                    "quantity": 1000u64
                }),
            };
            let payload = json!({
                "account_id": 1,
                "orders": [order]
            });

            assert!(
                serde_json::from_value::<SubmitOrderRequest>(payload).is_err(),
                "{order_type} must not deserialize as a public OrderSpec"
            );
        }
    }
}
