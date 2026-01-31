use std::hash::{Hash, Hasher};

use matching_engine::{ConditionDir, Order, MAX_MARKETS_PER_ORDER, MAX_STATES};
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use crate::error::SequencerError;

/// A P256 public key (secp256r1 / passkey-compatible).
#[derive(Clone, Debug)]
pub struct PublicKey(pub VerifyingKey);

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_encoded_point(true) == other.0.to_encoded_point(true)
    }
}

impl Eq for PublicKey {}

impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_encoded_point(true).as_bytes().hash(state);
    }
}

/// An order with a P256 ECDSA signature.
pub struct SignedOrder {
    pub order: Order,
    pub signer: PublicKey,
    pub signature: Signature,
}

/// Deterministic canonical byte encoding of an Order for signing.
///
/// Layout (all integers little-endian):
/// - markets: 5 × u32 (MarketId.0)
/// - num_markets: u8
/// - payoffs: 32 × i8
/// - num_states: u8
/// - limit_price: u64
/// - min_fill: u64
/// - max_fill: u64
/// - condition present: u8 (0 or 1)
///   if present:
///   - condition.market: u32
///   - condition.threshold: u64
///   - condition.direction: u8 (0=Above, 1=Below)
///
/// NOTE: `id` is excluded because the sequencer assigns IDs after submission.
pub fn canonical_order_bytes(order: &Order) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    // Markets (fixed-size array)
    for i in 0..MAX_MARKETS_PER_ORDER {
        buf.extend_from_slice(&order.markets[i].0.to_le_bytes());
    }
    buf.push(order.num_markets);

    // Payoffs (fixed-size array)
    for i in 0..MAX_STATES {
        buf.push(order.payoffs[i] as u8);
    }
    buf.push(order.num_states);

    // Price and fill
    buf.extend_from_slice(&order.limit_price.to_le_bytes());
    buf.extend_from_slice(&order.min_fill.to_le_bytes());
    buf.extend_from_slice(&order.max_fill.to_le_bytes());

    // Condition
    match &order.condition {
        None => buf.push(0),
        Some(cond) => {
            buf.push(1);
            buf.extend_from_slice(&cond.market.0.to_le_bytes());
            buf.extend_from_slice(&cond.threshold.to_le_bytes());
            buf.push(match cond.direction {
                ConditionDir::Above => 0,
                ConditionDir::Below => 1,
            });
        }
    }

    buf
}

/// Verify a signed order's P256 ECDSA signature.
pub fn verify_signed_order(signed: &SignedOrder) -> Result<(), SequencerError> {
    let msg = canonical_order_bytes(&signed.order);
    signed
        .signer
        .0
        .verify(&msg, &signed.signature)
        .map_err(|_| SequencerError::InvalidSignature)
}

/// Sign an order with a P256 signing key (for testing / client use).
pub fn sign_order(order: &Order, key: &SigningKey) -> SignedOrder {
    let msg = canonical_order_bytes(order);
    let signature: Signature = key.sign(&msg);
    SignedOrder {
        order: order.clone(),
        signer: PublicKey(key.verifying_key().clone()),
        signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::{outcome_buy, MarketSet};
    use p256::ecdsa::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_sign_verify_roundtrip() {
        let key = SigningKey::random(&mut OsRng);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let signed = sign_order(&order, &key);

        assert!(verify_signed_order(&signed).is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let key1 = SigningKey::random(&mut OsRng);
        let key2 = SigningKey::random(&mut OsRng);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);

        // Sign with key1 but claim key2
        let msg = canonical_order_bytes(&order);
        let sig: Signature = key1.sign(&msg);

        let signed = SignedOrder {
            order,
            signer: PublicKey(key2.verifying_key().clone()),
            signature: sig,
        };

        assert!(matches!(
            verify_signed_order(&signed),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_tampered_order_rejected() {
        let key = SigningKey::random(&mut OsRng);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let mut signed = sign_order(&order, &key);

        // Tamper with the order after signing
        signed.order.limit_price = 999_999_999;

        assert!(matches!(
            verify_signed_order(&signed),
            Err(SequencerError::InvalidSignature)
        ));
    }

    #[test]
    fn test_canonical_encoding_deterministic() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let bytes1 = canonical_order_bytes(&order);
        let bytes2 = canonical_order_bytes(&order);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_canonical_encoding_differs_for_different_orders() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");

        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 2, m0, 0, 600_000_000, 10);

        assert_ne!(canonical_order_bytes(&order1), canonical_order_bytes(&order2));
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
        assert_eq!(canonical_order_bytes(&order1), canonical_order_bytes(&order2));
    }
}
