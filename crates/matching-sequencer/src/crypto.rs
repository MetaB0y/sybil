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

/// Account registration metadata attached to a public key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegisteredPubkey {
    pub account_id: crate::account::AccountId,
    #[serde(default)]
    pub auth_scheme: AccountAuthScheme,
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
pub fn canonical_order_bytes(order: &Order, nonce: u64) -> Vec<u8> {
    sybil_signing::canonical_order_bytes(&to_canonical_order(order, nonce))
}

/// Deterministic canonical byte encoding of a cancel request for signing.
///
/// Layout (all integers little-endian):
/// - account_id: u64
/// - order_id: u64
/// - nonce: u64
pub fn canonical_cancel_bytes(
    account_id: crate::account::AccountId,
    order_id: u64,
    nonce: u64,
) -> Vec<u8> {
    sybil_signing::canonical_cancel_bytes(account_id.0, order_id, nonce)
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

/// Verify a signed order's P256 ECDSA signature.
pub fn verify_signed_order(signed: &SignedOrder) -> Result<(), SequencerError> {
    let msg = canonical_order_bytes(&signed.order, signed.nonce);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Verify a signed cancel request's P256 ECDSA signature.
pub fn verify_signed_cancel(signed: &SignedCancel) -> Result<(), SequencerError> {
    let msg = canonical_cancel_bytes(signed.account_id, signed.order_id, signed.nonce);
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
pub fn sign_order(order: &Order, nonce: u64, key: &SigningKey) -> SignedOrder {
    let msg = canonical_order_bytes(order, nonce);
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
    key: &SigningKey,
) -> SignedCancel {
    let msg = canonical_cancel_bytes(account_id, order_id, nonce);
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

#[cfg(test)]
mod tests {
    use super::*;
    use getrandom::SysRng;
    use matching_engine::{outcome_buy, MarketSet};
    use p256::ecdsa::SigningKey;
    use p256::elliptic_curve::rand_core::UnwrapErr;

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
        let signed = sign_order(&order, 1, &key);

        assert!(verify_signed_order(&signed).is_ok());
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
        let msg = canonical_order_bytes(&order, 1);
        let sig: Signature = key1.sign(&msg);

        let signed = SignedOrder {
            order,
            nonce: 1,
            signer: PublicKey(*key2.verifying_key()),
            signature: sig,
        };

        assert!(matches!(
            verify_signed_order(&signed),
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
        let mut signed = sign_order(&order, 1, &key);

        // Tamper with the order after signing
        signed.order.limit_price = matching_engine::Nanos(999_999_999);

        assert!(matches!(
            verify_signed_order(&signed),
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
        let mut signed = sign_order(&order, 1, &key);
        signed.order.expires_at_block = Some(1);

        assert!(matches!(
            verify_signed_order(&signed),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_sign_verify_cancel_roundtrip() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let signed = sign_cancel(crate::account::AccountId(7), 42, 1, &key);

        assert!(verify_signed_cancel(&signed).is_ok());
    }

    #[test]
    fn test_tampered_cancel_rejected() {
        let key =
            <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut crypto_rng());
        let mut signed = sign_cancel(crate::account::AccountId(7), 42, 1, &key);
        signed.order_id = 99;

        assert!(matches!(
            verify_signed_cancel(&signed),
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
        let bytes1 = canonical_order_bytes(&order, 1);
        let bytes2 = canonical_order_bytes(&order, 1);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_canonical_encoding_differs_for_different_orders() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 2, m0, 0, 600_000_000, 10);

        assert_ne!(
            canonical_order_bytes(&order1, 1),
            canonical_order_bytes(&order2, 1)
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
            canonical_order_bytes(&order1, 1),
            canonical_order_bytes(&order2, 1)
        );
    }

    #[test]
    fn test_canonical_cancel_encoding_deterministic() {
        let bytes1 = canonical_cancel_bytes(crate::account::AccountId(3), 17, 1);
        let bytes2 = canonical_cancel_bytes(crate::account::AccountId(3), 17, 1);

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
        let mut signed = sign_order(&order, 1, &key);
        signed.nonce = 2;

        assert!(matches!(
            verify_signed_order(&signed),
            Err(SequencerError::InvalidSignature)
        ));
    }
}
