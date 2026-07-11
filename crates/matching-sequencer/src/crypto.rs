// TODO: Consider commonware-cryptography's secp256r1 module for namespace-scoped
// signatures (prevents cross-deployment replay) and batch verification.
// See: https://commonware.xyz/ — same P256/secp256r1 curve, adds context string
// to signing so a signature from deployment A can't be replayed on deployment B.
// Not urgent but a real security improvement for multi-environment setups.

use std::hash::{Hash, Hasher};

use crate::error::SequencerError;
use matching_engine::Order;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sybil_oracle::{ResolutionAttestation, SignedAttestation};
use sybil_signing::{
    BridgeWithdrawalRequest as CanonicalBridgeWithdrawalRequest,
    ConditionDir as CanonicalConditionDir, MarketId as CanonicalMarketId, Order as CanonicalOrder,
    PriceCondition as CanonicalPriceCondition, ResolutionAttestation as CanonicalAttestation,
};

/// Registered authentication scheme for an account public key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccountAuthScheme {
    /// Raw P256 ECDSA over Sybil canonical bytes.
    #[default]
    RawP256,
    /// WebAuthn assertion whose challenge is the hash of Sybil canonical bytes.
    WebAuthn,
}

impl AccountAuthScheme {
    /// Stable byte tag used in the SYB-229 key-registration canonical form.
    /// MUST stay in sync with the TS canonical encoder.
    pub fn canonical_byte(self) -> u8 {
        match self {
            AccountAuthScheme::RawP256 => 0,
            AccountAuthScheme::WebAuthn => 1,
        }
    }
}

/// Scope tag for a registered signing key (SYB-60).
///
/// This describes *what the key is for*, not what it may sign: every registered
/// key can sign every mutation for its account. The tag is metadata that lets a
/// UI/operator reason about keys (e.g. hide agent keys, warn before revoking the
/// primary) and feeds the planned `keys_digest` (SYB-225).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyScope {
    /// The account's own primary key (typically the first key registered).
    #[default]
    Primary,
    /// A delegated agent/trade key registered under the account. An agent gets
    /// its own P256 keypair and signs like any key — this is the mechanism for
    /// "agent trade keys" (as opposed to a read-only bearer token).
    Agent,
    /// Any other operator-defined key.
    Custom,
}

impl KeyScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyScope::Primary => "primary",
            KeyScope::Agent => "agent",
            KeyScope::Custom => "custom",
        }
    }
}

/// Account registration metadata attached to a public key (SYB-60).
///
/// Extends the original `{account_id, auth_scheme}` with a label, scope tag, and
/// creation timestamp so keys can be listed and managed. All new fields are
/// `#[serde(default)]` so previously persisted rows deserialize as a labelless
/// primary key created at time 0.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegisteredPubkey {
    pub account_id: crate::account::AccountId,
    #[serde(default)]
    pub auth_scheme: AccountAuthScheme,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub scope: KeyScope,
    #[serde(default)]
    pub created_at_ms: u64,
}

impl RegisteredPubkey {
    /// Construct a primary, labelless registration (back-compat constructor).
    pub fn primary(account_id: crate::account::AccountId, auth_scheme: AccountAuthScheme) -> Self {
        Self {
            account_id,
            auth_scheme,
            label: None,
            scope: KeyScope::Primary,
            created_at_ms: 0,
        }
    }
}

/// A signed request to set/clear an account profile (SYB-60).
pub struct SignedProfileUpdate {
    pub account_id: crate::account::AccountId,
    pub display_name: Option<String>,
    pub avatar_seed: Option<String>,
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// A profile update whose signature was verified before entering the sequencer.
pub struct AuthenticatedProfileUpdate {
    pub account_id: crate::account::AccountId,
    pub display_name: Option<String>,
    pub avatar_seed: Option<String>,
    pub nonce: u64,
    pub signer: PublicKey,
}

/// A signed request to revoke a registered signing key (SYB-60).
pub struct SignedKeyRevocation {
    pub account_id: crate::account::AccountId,
    /// Full validity record being revoked (the canonical payload covers it).
    pub target_key: sybil_verifier::KeyRecord,
    pub bound_keys_digest: [u8; 32],
    pub bound_events_digest: [u8; 32],
    pub signer: PublicKey,
    pub signature: Signature,
}

/// A key revocation whose signature was verified before entering the sequencer.
pub struct AuthenticatedKeyRevocation {
    pub account_id: crate::account::AccountId,
    pub target_key: sybil_verifier::KeyRecord,
    pub bound_keys_digest: [u8; 32],
    pub bound_events_digest: [u8; 32],
    pub signer: PublicKey,
    pub authorization: sybil_verifier::KeyOpAuth,
}

/// A signed request to register a NEW signing key on an account (SYB-229).
///
/// Required whenever the account already has at least one registered key; the
/// first key is bootstrapped over the service tier instead. The signature (by an
/// existing account key) authorizes attaching `new_pubkey`, and — like orders —
/// is domain-separated by `genesis_hash`.
pub struct SignedKeyRegistration {
    pub account_id: crate::account::AccountId,
    /// The key being registered.
    pub new_pubkey: PublicKey,
    pub new_auth_scheme: AccountAuthScheme,
    pub label: Option<String>,
    pub scope: KeyScope,
    pub bound_keys_digest: [u8; 32],
    pub bound_events_digest: [u8; 32],
    pub signer: PublicKey,
    pub signature: Signature,
}

/// A key registration whose signature was verified before the sequencer.
pub struct AuthenticatedKeyRegistration {
    pub account_id: crate::account::AccountId,
    pub new_pubkey: PublicKey,
    pub new_auth_scheme: AccountAuthScheme,
    pub label: Option<String>,
    pub scope: KeyScope,
    pub bound_keys_digest: [u8; 32],
    pub bound_events_digest: [u8; 32],
    pub signer: PublicKey,
    pub authorization: sybil_verifier::KeyOpAuth,
}

/// A signed request to create a read-scoped bearer API key (SYB-60).
///
/// `token_hash` is blake3(token); the plaintext token is generated at the API
/// edge, returned to the caller once, and never signed or persisted.
pub struct SignedApiKeyCreate {
    pub account_id: crate::account::AccountId,
    pub label: Option<String>,
    pub token_hash: [u8; 32],
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// An API-key creation whose signature was verified before the sequencer.
pub struct AuthenticatedApiKeyCreate {
    pub account_id: crate::account::AccountId,
    pub label: Option<String>,
    pub token_hash: [u8; 32],
    pub nonce: u64,
    pub signer: PublicKey,
}

/// A signed request to revoke a read-scoped bearer API key (SYB-60).
pub struct SignedApiKeyRevoke {
    pub account_id: crate::account::AccountId,
    pub api_key_id: u64,
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// An API-key revocation whose signature was verified before the sequencer.
pub struct AuthenticatedApiKeyRevoke {
    pub account_id: crate::account::AccountId,
    pub api_key_id: u64,
    pub nonce: u64,
    pub signer: PublicKey,
}

/// A P256 public key (secp256r1 / passkey-compatible).
#[derive(Clone, Debug)]
pub struct PublicKey(pub VerifyingKey);

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_sec1_point(true) == other.0.to_sec1_point(true)
    }
}

impl Eq for PublicKey {}

impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_sec1_point(true).as_bytes().hash(state);
    }
}

impl PublicKey {
    /// Serialize to compressed SEC1 bytes (33 bytes).
    pub fn compressed_bytes(&self) -> Vec<u8> {
        self.0.to_sec1_point(true).as_bytes().to_vec()
    }

    /// Deserialize from compressed SEC1 bytes.
    pub fn from_compressed_bytes(bytes: &[u8]) -> Option<Self> {
        VerifyingKey::from_sec1_bytes(bytes).ok().map(PublicKey)
    }
}

/// An order with a P256 ECDSA signature.
pub struct SignedOrder {
    pub order: Order,
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// A resting-order cancellation authenticated by a P256 signature.
pub struct SignedCancel {
    pub account_id: crate::account::AccountId,
    pub order_id: u64,
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// A bridge withdrawal request authenticated by a P256 signature.
pub struct SignedBridgeWithdrawal {
    pub request: crate::bridge::BridgeWithdrawalRequest,
    pub nonce: u64,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// An order whose account signature was verified before entering the sequencer.
pub struct AuthenticatedOrder {
    pub order: Order,
    pub nonce: u64,
    pub signer: PublicKey,
}

/// A cancellation whose account signature was verified before entering the sequencer.
pub struct AuthenticatedCancel {
    pub account_id: crate::account::AccountId,
    pub order_id: u64,
    pub nonce: u64,
    pub signer: PublicKey,
}

/// A bridge withdrawal whose account signature was verified before entering the sequencer.
pub struct AuthenticatedBridgeWithdrawal {
    pub request: crate::bridge::BridgeWithdrawalRequest,
    pub nonce: u64,
    pub signer: PublicKey,
}

fn to_canonical_order(order: &Order, nonce: u64) -> CanonicalOrder {
    let mut markets = [CanonicalMarketId::NONE; sybil_signing::MAX_MARKETS_PER_ORDER];
    for (dst, src) in markets.iter_mut().zip(order.markets.iter()) {
        *dst = CanonicalMarketId(src.0);
    }

    let condition = order
        .condition
        .as_ref()
        .map(|condition| CanonicalPriceCondition {
            market: CanonicalMarketId(condition.market.0),
            threshold: condition.threshold.0,
            direction: match condition.direction {
                matching_engine::ConditionDir::Above => CanonicalConditionDir::Above,
                matching_engine::ConditionDir::Below => CanonicalConditionDir::Below,
            },
        });

    CanonicalOrder {
        markets,
        num_markets: order.num_markets,
        payoffs: order.payoffs,
        num_states: order.num_states,
        limit_price: order.limit_price.0,
        max_fill: order.max_fill.0,
        condition,
        expires_at_block: order.expires_at_block,
        nonce,
    }
}

/// Deterministic canonical byte encoding of an Order for signing.
///
/// NOTE: `id` is excluded because the sequencer assigns IDs after submission.
pub fn canonical_order_bytes(order: &Order, nonce: u64, genesis_hash: [u8; 32]) -> Vec<u8> {
    sybil_signing::canonical_order_bytes(&to_canonical_order(order, nonce), genesis_hash)
}

/// Deterministic canonical byte encoding of a cancel request for signing.
///
/// Layout (all integers little-endian):
/// - genesis_hash: [u8; 32]
/// - account_id: u64
/// - order_id: u64
/// - nonce: u64
pub fn canonical_cancel_bytes(
    account_id: crate::account::AccountId,
    order_id: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
) -> Vec<u8> {
    sybil_signing::canonical_cancel_bytes(account_id.0, order_id, nonce, genesis_hash)
}

fn to_canonical_bridge_withdrawal(
    request: &crate::bridge::BridgeWithdrawalRequest,
    nonce: u64,
) -> CanonicalBridgeWithdrawalRequest {
    CanonicalBridgeWithdrawalRequest {
        account_id: request.account_id.0,
        chain_id: request.chain_id,
        vault_address: request.vault_address,
        recipient: request.recipient,
        token_address: request.token_address,
        amount_token_units: request.amount_token_units,
        expiry_height: request.expiry_height,
        nonce,
    }
}

/// Deterministic canonical byte encoding of a bridge withdrawal request for signing.
pub fn canonical_bridge_withdrawal_bytes(
    request: &crate::bridge::BridgeWithdrawalRequest,
    nonce: u64,
) -> Vec<u8> {
    sybil_signing::canonical_bridge_withdrawal_bytes(&to_canonical_bridge_withdrawal(
        request, nonce,
    ))
}

/// Canonical bytes for a signed account-profile update (SYB-60).
pub fn canonical_profile_update_bytes(
    account_id: crate::account::AccountId,
    display_name: Option<&str>,
    avatar_seed: Option<&str>,
    nonce: u64,
) -> Vec<u8> {
    sybil_signing::canonical_profile_update_bytes(account_id.0, display_name, avatar_seed, nonce)
}

/// Canonical bytes for a signed signing-key revocation (SYB-60).
///
/// Domain-separated by `genesis_hash` (SYB-231), mirroring orders/cancels
/// (SYB-224) and key registrations (SYB-229).
pub fn canonical_key_revocation_bytes(
    genesis_hash: [u8; 32],
    account_id: crate::account::AccountId,
    target_key: &sybil_verifier::KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
) -> Vec<u8> {
    sybil_verifier::canonical_key_revocation_bytes(
        genesis_hash,
        account_id.0,
        target_key,
        bound_keys_digest,
        bound_events_digest,
    )
}

/// Canonical bytes for a signed read API-key creation (SYB-60).
pub fn canonical_api_key_create_bytes(
    account_id: crate::account::AccountId,
    label: Option<&str>,
    nonce: u64,
) -> Vec<u8> {
    sybil_signing::canonical_api_key_create_bytes(account_id.0, label, nonce)
}

/// Canonical bytes for a signed signing-key registration (SYB-229).
///
/// Domain-separated by `genesis_hash`, mirroring orders/cancels (SYB-224).
pub fn canonical_key_registration_bytes(
    genesis_hash: [u8; 32],
    account_id: crate::account::AccountId,
    key: &sybil_verifier::KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
) -> Vec<u8> {
    sybil_verifier::canonical_key_registration_bytes(
        genesis_hash,
        account_id.0,
        key,
        bound_keys_digest,
        bound_events_digest,
    )
}

/// Canonical bytes for a signed read API-key revocation (SYB-60).
pub fn canonical_api_key_revoke_bytes(
    account_id: crate::account::AccountId,
    api_key_id: u64,
    nonce: u64,
) -> Vec<u8> {
    sybil_signing::canonical_api_key_revoke_bytes(account_id.0, api_key_id, nonce)
}

/// Verify a signed profile update's P256 ECDSA signature (SYB-60).
pub fn verify_signed_profile_update(signed: &SignedProfileUpdate) -> Result<(), SequencerError> {
    let msg = canonical_profile_update_bytes(
        signed.account_id,
        signed.display_name.as_deref(),
        signed.avatar_seed.as_deref(),
        signed.nonce,
    );
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed signing-key revocation's P256 ECDSA signature (SYB-60).
///
/// Domain-separated by `genesis_hash` (SYB-231) so a captured revocation cannot
/// replay against a fresh-genesis redeploy.
pub fn verify_signed_key_revocation(
    signed: &SignedKeyRevocation,
    genesis_hash: [u8; 32],
) -> Result<(), SequencerError> {
    let msg = canonical_key_revocation_bytes(
        genesis_hash,
        signed.account_id,
        &signed.target_key,
        signed.bound_keys_digest,
        signed.bound_events_digest,
    );
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed signing-key registration's P256 ECDSA signature (SYB-229).
pub fn verify_signed_key_registration(
    signed: &SignedKeyRegistration,
    genesis_hash: [u8; 32],
) -> Result<(), SequencerError> {
    let msg = canonical_key_registration_bytes(
        genesis_hash,
        signed.account_id,
        &sybil_verifier::KeyRecord {
            auth_scheme: signed.new_auth_scheme.canonical_byte(),
            pubkey_sec1: signed
                .new_pubkey
                .compressed_bytes()
                .try_into()
                .expect("compressed P-256 key is 33 bytes"),
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        },
        signed.bound_keys_digest,
        signed.bound_events_digest,
    );
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed API-key creation's P256 ECDSA signature (SYB-60).
pub fn verify_signed_api_key_create(signed: &SignedApiKeyCreate) -> Result<(), SequencerError> {
    let msg =
        canonical_api_key_create_bytes(signed.account_id, signed.label.as_deref(), signed.nonce);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed API-key revocation's P256 ECDSA signature (SYB-60).
pub fn verify_signed_api_key_revoke(signed: &SignedApiKeyRevoke) -> Result<(), SequencerError> {
    let msg = canonical_api_key_revoke_bytes(signed.account_id, signed.api_key_id, signed.nonce);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed order's P256 ECDSA signature.
pub fn verify_signed_order(
    signed: &SignedOrder,
    genesis_hash: [u8; 32],
) -> Result<(), SequencerError> {
    let msg = canonical_order_bytes(&signed.order, signed.nonce, genesis_hash);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed cancel request's P256 ECDSA signature.
pub fn verify_signed_cancel(
    signed: &SignedCancel,
    genesis_hash: [u8; 32],
) -> Result<(), SequencerError> {
    let msg = canonical_cancel_bytes(
        signed.account_id,
        signed.order_id,
        signed.nonce,
        genesis_hash,
    );
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed bridge withdrawal request's P256 ECDSA signature.
pub fn verify_signed_bridge_withdrawal(
    signed: &SignedBridgeWithdrawal,
) -> Result<(), SequencerError> {
    let msg = canonical_bridge_withdrawal_bytes(&signed.request, signed.nonce);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Sign an order with a P256 signing key (for testing / client use).
pub fn sign_order(
    order: &Order,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> SignedOrder {
    let msg = canonical_order_bytes(order, nonce, genesis_hash);
    let signature: Signature = key.sign(&msg);
    SignedOrder {
        order: order.clone(),
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

fn to_canonical_attestation(att: &ResolutionAttestation) -> CanonicalAttestation {
    CanonicalAttestation {
        market_id: CanonicalMarketId(att.market_id.0),
        payout_nanos: att.payout_nanos.0,
        nonce: att.nonce,
    }
}

/// Deterministic canonical byte encoding of a `ResolutionAttestation` for signing.
pub fn canonical_attestation_bytes(att: &ResolutionAttestation) -> Vec<u8> {
    sybil_signing::canonical_attestation_bytes(&to_canonical_attestation(att))
}

/// Verify the signature on a [`SignedAttestation`]. Does NOT check that the
/// signer is a registered feed — callers do that via the feed registry.
pub fn verify_signed_attestation(signed: &SignedAttestation) -> Result<PublicKey, SequencerError> {
    let pubkey = PublicKey::from_compressed_bytes(&signed.signer.0)
        .ok_or(SequencerError::InvalidSignature)?;
    let signature =
        Signature::from_der(&signed.signature_der).map_err(|_| SequencerError::InvalidSignature)?;
    let msg = canonical_attestation_bytes(&signed.attestation);
    pubkey
        .0
        .verify(&msg, &signature)
        .map_err(|_| SequencerError::InvalidSignature)?;
    Ok(pubkey)
}

/// Sign a `ResolutionAttestation` with a P256 signing key (testing / signer use).
pub fn sign_attestation(attestation: ResolutionAttestation, key: &SigningKey) -> SignedAttestation {
    let msg = canonical_attestation_bytes(&attestation);
    let signature: Signature = key.sign(&msg);
    let pubkey = PublicKey(*key.verifying_key());
    SignedAttestation {
        attestation,
        signer: sybil_oracle::FeedPubkey(pubkey.compressed_bytes()),
        signature_der: signature.to_der().as_bytes().to_vec(),
    }
}

/// Sign a cancel request with a P256 signing key (for testing / client use).
pub fn sign_cancel(
    account_id: crate::account::AccountId,
    order_id: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> SignedCancel {
    let msg = canonical_cancel_bytes(account_id, order_id, nonce, genesis_hash);
    let signature: Signature = key.sign(&msg);
    SignedCancel {
        account_id,
        order_id,
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

/// Sign a bridge withdrawal request with a P256 signing key (testing / client use).
pub fn sign_bridge_withdrawal(
    request: crate::bridge::BridgeWithdrawalRequest,
    nonce: u64,
    key: &SigningKey,
) -> SignedBridgeWithdrawal {
    let msg = canonical_bridge_withdrawal_bytes(&request, nonce);
    let signature: Signature = key.sign(&msg);
    SignedBridgeWithdrawal {
        request,
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

/// blake3 hash of a bearer API-key token (SYB-60).
///
/// blake3 (not argon2) is the correct choice here: API tokens are 256 bits of
/// CSPRNG entropy, so there is no low-entropy password to brute-force. A slow
/// memory-hard KDF only buys resistance to guessing weak secrets; for
/// high-entropy tokens a fast cryptographic hash gives the same at-rest safety
/// (a hash leak can't be inverted) while keeping bearer auth O(1) per request.
pub fn api_key_hash(token: &[u8]) -> [u8; 32] {
    *blake3::hash(token).as_bytes()
}

/// Sign a profile update with a P256 signing key (testing / client use).
pub fn sign_profile_update(
    account_id: crate::account::AccountId,
    display_name: Option<String>,
    avatar_seed: Option<String>,
    nonce: u64,
    key: &SigningKey,
) -> SignedProfileUpdate {
    let msg = canonical_profile_update_bytes(
        account_id,
        display_name.as_deref(),
        avatar_seed.as_deref(),
        nonce,
    );
    let signature: Signature = key.sign(&msg);
    SignedProfileUpdate {
        account_id,
        display_name,
        avatar_seed,
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

/// Sign a signing-key revocation with a P256 signing key (testing / client use).
pub fn sign_key_revocation(
    account_id: crate::account::AccountId,
    target_key: sybil_verifier::KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> SignedKeyRevocation {
    let msg = canonical_key_revocation_bytes(
        genesis_hash,
        account_id,
        &target_key,
        bound_keys_digest,
        bound_events_digest,
    );
    let signature: Signature = key.sign(&msg);
    SignedKeyRevocation {
        account_id,
        target_key,
        bound_keys_digest,
        bound_events_digest,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

/// Sign a signing-key registration with a P256 signing key (testing / client use).
#[allow(clippy::too_many_arguments)]
pub fn sign_key_registration(
    account_id: crate::account::AccountId,
    new_pubkey: PublicKey,
    new_auth_scheme: AccountAuthScheme,
    label: Option<String>,
    scope: KeyScope,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> SignedKeyRegistration {
    let signer = PublicKey(*key.verifying_key());
    let key_record = sybil_verifier::KeyRecord {
        auth_scheme: new_auth_scheme.canonical_byte(),
        pubkey_sec1: new_pubkey
            .compressed_bytes()
            .try_into()
            .expect("compressed P-256 key is 33 bytes"),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let msg = canonical_key_registration_bytes(
        genesis_hash,
        account_id,
        &key_record,
        bound_keys_digest,
        bound_events_digest,
    );
    let signature: Signature = key.sign(&msg);
    SignedKeyRegistration {
        account_id,
        new_pubkey,
        new_auth_scheme,
        label,
        scope,
        bound_keys_digest,
        bound_events_digest,
        signer,
        signature,
    }
}

/// Sign an API-key creation with a P256 signing key (testing / client use).
pub fn sign_api_key_create(
    account_id: crate::account::AccountId,
    label: Option<String>,
    token_hash: [u8; 32],
    nonce: u64,
    key: &SigningKey,
) -> SignedApiKeyCreate {
    let msg = canonical_api_key_create_bytes(account_id, label.as_deref(), nonce);
    let signature: Signature = key.sign(&msg);
    SignedApiKeyCreate {
        account_id,
        label,
        token_hash,
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

/// Sign an API-key revocation with a P256 signing key (testing / client use).
pub fn sign_api_key_revoke(
    account_id: crate::account::AccountId,
    api_key_id: u64,
    nonce: u64,
    key: &SigningKey,
) -> SignedApiKeyRevoke {
    let msg = canonical_api_key_revoke_bytes(account_id, api_key_id, nonce);
    let signature: Signature = key.sign(&msg);
    SignedApiKeyRevoke {
        account_id,
        api_key_id,
        nonce,
        signer: PublicKey(*key.verifying_key()),
        signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use getrandom::SysRng;
    use matching_engine::{MarketSet, outcome_buy};
    use p256::ecdsa::SigningKey;
    use p256::elliptic_curve::rand_core::UnwrapErr;

    const GENESIS_HASH: [u8; 32] = [0xab; 32];
    const OTHER_GENESIS_HASH: [u8; 32] = [0xcd; 32];

    fn crypto_rng() -> UnwrapErr<SysRng> {
        UnwrapErr(SysRng)
    }

    #[test]
    fn test_sign_verify_roundtrip() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let signed = sign_order(&order, 1, GENESIS_HASH, &key);

        assert!(verify_signed_order(&signed, GENESIS_HASH).is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let key1 =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let key2 =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);

        // Sign with key1 but claim key2
        let msg = canonical_order_bytes(&order, 1, GENESIS_HASH);
        let sig: Signature = key1.sign(&msg);

        let signed = SignedOrder {
            order,
            nonce: 1,
            signer: PublicKey(*key2.verifying_key()),
            signature: sig,
        };

        assert!(matches!(
            verify_signed_order(&signed, GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_tampered_order_rejected() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let mut signed = sign_order(&order, 1, GENESIS_HASH, &key);

        // Tamper with the order after signing
        signed.order.limit_price = matching_engine::Nanos(999_999_999);

        assert!(matches!(
            verify_signed_order(&signed, GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_expires_at_block_is_signature_covered() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let mut signed = sign_order(&order, 1, GENESIS_HASH, &key);
        signed.order.expires_at_block = Some(1);

        assert!(matches!(
            verify_signed_order(&signed, GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_sign_verify_cancel_roundtrip() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let signed = sign_cancel(crate::account::AccountId(7), 42, 1, GENESIS_HASH, &key);

        assert!(verify_signed_cancel(&signed, GENESIS_HASH).is_ok());
    }

    #[test]
    fn test_tampered_cancel_rejected() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut signed = sign_cancel(crate::account::AccountId(7), 42, 1, GENESIS_HASH, &key);
        signed.order_id = 99;

        assert!(matches!(
            verify_signed_cancel(&signed, GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_sign_verify_bridge_withdrawal_roundtrip() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let request = crate::bridge::BridgeWithdrawalRequest {
            account_id: crate::account::AccountId(7),
            chain_id: 31_337,
            vault_address: [0x11; 20],
            recipient: [0x22; 20],
            token_address: [0x33; 20],
            amount_token_units: 42_000_000,
            expiry_height: 123_456,
        };
        let signed = sign_bridge_withdrawal(request, 1, &key);

        assert!(verify_signed_bridge_withdrawal(&signed).is_ok());
    }

    #[test]
    fn test_tampered_bridge_withdrawal_rejected() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let request = crate::bridge::BridgeWithdrawalRequest {
            account_id: crate::account::AccountId(7),
            chain_id: 31_337,
            vault_address: [0x11; 20],
            recipient: [0x22; 20],
            token_address: [0x33; 20],
            amount_token_units: 42_000_000,
            expiry_height: 123_456,
        };
        let mut signed = sign_bridge_withdrawal(request, 1, &key);
        signed.request.amount_token_units = 43_000_000;

        assert!(matches!(
            verify_signed_bridge_withdrawal(&signed),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_canonical_encoding_deterministic() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let bytes1 = canonical_order_bytes(&order, 1, GENESIS_HASH);
        let bytes2 = canonical_order_bytes(&order, 1, GENESIS_HASH);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_canonical_encoding_differs_for_different_orders() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 2, m0, 0, 600_000_000, 10);

        assert_ne!(
            canonical_order_bytes(&order1, 1, GENESIS_HASH),
            canonical_order_bytes(&order2, 1, GENESIS_HASH)
        );
    }

    #[test]
    fn test_canonical_encoding_excludes_id() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let mut order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let mut order2 = order1.clone();
        order1.id = 100;
        order2.id = 200;

        // Same order content but different IDs should produce same canonical bytes
        assert_eq!(
            canonical_order_bytes(&order1, 1, GENESIS_HASH),
            canonical_order_bytes(&order2, 1, GENESIS_HASH)
        );
    }

    #[test]
    fn test_canonical_cancel_encoding_deterministic() {
        let bytes1 = canonical_cancel_bytes(crate::account::AccountId(3), 17, 1, GENESIS_HASH);
        let bytes2 = canonical_cancel_bytes(crate::account::AccountId(3), 17, 1, GENESIS_HASH);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_canonical_bridge_withdrawal_encoding_deterministic() {
        let request = crate::bridge::BridgeWithdrawalRequest {
            account_id: crate::account::AccountId(7),
            chain_id: 31_337,
            vault_address: [0x11; 20],
            recipient: [0x22; 20],
            token_address: [0x33; 20],
            amount_token_units: 42_000_000,
            expiry_height: 123_456,
        };
        let bytes1 = canonical_bridge_withdrawal_bytes(&request, 1);
        let bytes2 = canonical_bridge_withdrawal_bytes(&request, 1);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_nonce_is_signature_covered() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let mut signed = sign_order(&order, 1, GENESIS_HASH, &key);
        signed.nonce = 2;

        assert!(matches!(
            verify_signed_order(&signed, GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_genesis_hash_is_signature_covered() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let signed = sign_order(&order, 1, GENESIS_HASH, &key);

        assert!(matches!(
            verify_signed_order(&signed, OTHER_GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_cancel_genesis_hash_is_signature_covered() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let signed = sign_cancel(crate::account::AccountId(7), 42, 1, GENESIS_HASH, &key);

        assert!(matches!(
            verify_signed_cancel(&signed, OTHER_GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_sign_verify_key_revocation_roundtrip() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let target = sybil_verifier::KeyRecord {
            auth_scheme: 0,
            pubkey_sec1: [0x02u8; 33],
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        let signed = sign_key_revocation(
            crate::account::AccountId(7),
            target,
            [3; 32],
            [4; 32],
            GENESIS_HASH,
            &key,
        );
        assert!(verify_signed_key_revocation(&signed, GENESIS_HASH).is_ok());
    }

    #[test]
    fn test_key_revocation_genesis_hash_is_signature_covered() {
        // SYB-231: a revocation signed under one genesis must not verify under
        // another, so a captured revocation cannot replay across a redeploy.
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let target = sybil_verifier::KeyRecord {
            auth_scheme: 0,
            pubkey_sec1: [0x02u8; 33],
            capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
        };
        let signed = sign_key_revocation(
            crate::account::AccountId(7),
            target,
            [3; 32],
            [4; 32],
            GENESIS_HASH,
            &key,
        );

        assert!(matches!(
            verify_signed_key_revocation(&signed, OTHER_GENESIS_HASH),
            Err(SequencerError::InvalidSignature)
        ));
    }
}
