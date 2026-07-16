//! Resolution policies: how an attestation becomes a settled market.
//!
//! Today there is one variant, `Immediate`. Future variants (`Optimistic`,
//! `Quorum`, `Predicate`, `External`) are new enum arms, not new traits — see
//! `docs/architecture/05-interfaces/Market Resolution.md` for the current
//! trust boundary and future-policy discussion.

use matching_engine::{MarketId, NANOS_PER_DOLLAR, Nanos};
use serde::{Deserialize, Serialize};

use crate::attestation::SignedAttestation;
use crate::error::OracleError;
use crate::feed::{DataFeed, FeedId};
use crate::types::{MarketStatus, OracleSource, ResolutionRecord};

/// How a market resolves. Keyed off a feed (or set of feeds) by id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionPolicy {
    /// One attestation from the named feed settles the market with no
    /// challenge window. Used for the admin and Polymarket-mirror templates.
    Immediate { feed_id: FeedId },
}

/// Evaluate `Immediate { feed_id }` against a signed attestation.
///
/// The caller is responsible for verifying the signature against `feed`'s
/// pubkey before calling this — by the time we land here, identity is already
/// proven. `market_id` is the market actually being resolved; the policy's job
/// is to bind the attestation to it, construct the `ResolutionRecord`, and
/// apply the state-machine checks (payout range, not-already-resolved).
pub fn evaluate_immediate(
    policy_feed_id: FeedId,
    feed: &DataFeed,
    market_id: MarketId,
    signed: &SignedAttestation,
    current_status: &MarketStatus,
    timestamp_ms: u64,
) -> Result<ResolutionRecord, OracleError> {
    if feed.id != policy_feed_id {
        return Err(OracleError::InvalidAttestation(format!(
            "attestation signer is feed {:?}, template requires {:?}",
            feed.id, policy_feed_id
        )));
    }

    let att = &signed.attestation;

    // Bind the signed attestation to the market being resolved. Without this,
    // an otherwise-valid signature for one market could settle another — the
    // caller's target `market_id` must match what the signer attested to.
    if att.market_id != market_id {
        return Err(OracleError::InvalidAttestation(format!(
            "attestation targets market {:?}, but resolving market {:?}",
            att.market_id, market_id
        )));
    }

    if att.payout_nanos.0 > NANOS_PER_DOLLAR {
        return Err(OracleError::InvalidPayout(att.payout_nanos.0));
    }

    if matches!(current_status, MarketStatus::Resolved { .. }) {
        return Err(OracleError::AlreadyResolved);
    }

    Ok(ResolutionRecord {
        payout_nanos: att.payout_nanos,
        resolved_by: OracleSource::DataFeed(feed.id),
        resolved_at_ms: timestamp_ms,
    })
}

/// Apply the trusted-admin immediate policy.
pub fn evaluate_admin_immediate(
    payout_nanos: Nanos,
    current_status: &MarketStatus,
    timestamp_ms: u64,
) -> Result<ResolutionRecord, OracleError> {
    if payout_nanos.0 > NANOS_PER_DOLLAR {
        return Err(OracleError::InvalidPayout(payout_nanos.0));
    }
    if matches!(current_status, MarketStatus::Resolved { .. }) {
        return Err(OracleError::AlreadyResolved);
    }
    Ok(ResolutionRecord {
        payout_nanos,
        resolved_by: OracleSource::Admin,
        resolved_at_ms: timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::FeedPubkey;
    use matching_engine::{MarketId, NANOS_PER_DOLLAR, Nanos};

    fn sample_feed(id: u64) -> DataFeed {
        DataFeed {
            id: FeedId(id),
            pubkey: FeedPubkey(vec![1u8; 33]),
            name: "test".into(),
            created_at_ms: 0,
        }
    }

    fn sample_signed(market_id: MarketId, payout_nanos: Nanos) -> SignedAttestation {
        SignedAttestation {
            attestation: crate::ResolutionAttestation {
                market_id,
                payout_nanos,
                nonce: 0,
            },
            signer: FeedPubkey(vec![1u8; 33]),
            signature_der: Vec::new(),
        }
    }

    #[test]
    fn immediate_happy_path_yields_settle() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), Nanos(NANOS_PER_DOLLAR));
        let outcome = evaluate_immediate(
            FeedId(7),
            &feed,
            MarketId::new(5),
            &signed,
            &MarketStatus::Active,
            1_000,
        )
        .unwrap();
        assert_eq!(outcome.payout_nanos, Nanos(NANOS_PER_DOLLAR));
        assert_eq!(outcome.resolved_at_ms, 1_000);
        assert!(matches!(outcome.resolved_by, OracleSource::DataFeed(_)));
    }

    #[test]
    fn immediate_rejects_wrong_feed() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), Nanos(NANOS_PER_DOLLAR));
        let err = evaluate_immediate(
            FeedId(8),
            &feed,
            MarketId::new(5),
            &signed,
            &MarketStatus::Active,
            1_000,
        )
        .unwrap_err();
        assert!(matches!(err, OracleError::InvalidAttestation(_)));
    }

    #[test]
    fn immediate_rejects_market_id_mismatch() {
        // Valid signer and feed, but the attestation was signed for market 5
        // while the caller is resolving market 6. This must be rejected so a
        // valid signature can never settle a market it did not authorize.
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), Nanos(NANOS_PER_DOLLAR));
        let err = evaluate_immediate(
            FeedId(7),
            &feed,
            MarketId::new(6),
            &signed,
            &MarketStatus::Active,
            1_000,
        )
        .unwrap_err();
        assert!(matches!(err, OracleError::InvalidAttestation(_)));
    }

    #[test]
    fn immediate_rejects_invalid_payout() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), Nanos(NANOS_PER_DOLLAR + 1));
        let err = evaluate_immediate(
            FeedId(7),
            &feed,
            MarketId::new(5),
            &signed,
            &MarketStatus::Active,
            1_000,
        )
        .unwrap_err();
        assert!(matches!(err, OracleError::InvalidPayout(_)));
    }

    #[test]
    fn immediate_rejects_already_resolved() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), Nanos(0));
        let record = crate::ResolutionRecord {
            payout_nanos: Nanos(NANOS_PER_DOLLAR),
            resolved_by: OracleSource::Admin,
            resolved_at_ms: 100,
        };
        let err = evaluate_immediate(
            FeedId(7),
            &feed,
            MarketId::new(5),
            &signed,
            &MarketStatus::Resolved { record },
            1_000,
        )
        .unwrap_err();
        assert!(matches!(err, OracleError::AlreadyResolved));
    }
}
