//! Resolution policies: how an attestation becomes a settled market.
//!
//! Today there is one variant, `Immediate`. Future variants (`Optimistic`,
//! `Quorum`, `Predicate`, `External`) are new enum arms, not new traits — see
//! `docs/architecture/Oracle System.md` for the roadmap.

use matching_engine::{MarketId, Nanos, NANOS_PER_DOLLAR};
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

/// Decision a policy returns when given an attestation.
#[derive(Clone, Debug)]
pub enum PolicyOutcome {
    /// Market should be settled with this record.
    Settle { record: ResolutionRecord },
    /// Attestation was well-formed but the policy refused it.
    Reject { reason: String },
}

/// Evaluate `Immediate { feed_id }` against a signed attestation.
///
/// The caller is responsible for verifying the signature against `feed`'s
/// pubkey before calling this — by the time we land here, identity is already
/// proven. The policy's job is purely to construct the `ResolutionRecord`
/// and apply the state-machine checks (payout range, not-already-resolved).
pub fn evaluate_immediate(
    policy_feed_id: FeedId,
    feed: &DataFeed,
    signed: &SignedAttestation,
    current_status: &MarketStatus,
    timestamp_ms: u64,
) -> Result<PolicyOutcome, OracleError> {
    if feed.id != policy_feed_id {
        return Err(OracleError::InvalidAttestation(format!(
            "attestation signer is feed {:?}, template requires {:?}",
            feed.id, policy_feed_id
        )));
    }

    let att = &signed.attestation;

    if att.payout_nanos > NANOS_PER_DOLLAR {
        return Err(OracleError::InvalidPayout(att.payout_nanos));
    }

    match current_status {
        MarketStatus::Resolved { .. } => return Err(OracleError::AlreadyResolved),
        MarketStatus::Voided => return Err(OracleError::InvalidState),
        _ => {}
    }

    let record = ResolutionRecord {
        market_id: att.market_id,
        payout_nanos: att.payout_nanos,
        resolved_by: OracleSource::DataFeed(feed.id),
        resolved_at_ms: timestamp_ms,
        proposal: None,
        challenge: None,
    };

    Ok(PolicyOutcome::Settle { record })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::FeedPubkey;
    use matching_engine::MarketId;

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
        let signed = sample_signed(MarketId::new(5), NANOS_PER_DOLLAR);
        let outcome =
            evaluate_immediate(FeedId(7), &feed, &signed, &MarketStatus::Active, 1_000).unwrap();
        match outcome {
            PolicyOutcome::Settle { record } => {
                assert_eq!(record.market_id, MarketId::new(5));
                assert_eq!(record.payout_nanos, NANOS_PER_DOLLAR);
                assert_eq!(record.resolved_at_ms, 1_000);
                assert!(matches!(record.resolved_by, OracleSource::DataFeed(_)));
            }
            _ => panic!("expected Settle"),
        }
    }

    #[test]
    fn immediate_rejects_wrong_feed() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), NANOS_PER_DOLLAR);
        let err = evaluate_immediate(FeedId(8), &feed, &signed, &MarketStatus::Active, 1_000)
            .unwrap_err();
        assert!(matches!(err, OracleError::InvalidAttestation(_)));
    }

    #[test]
    fn immediate_rejects_invalid_payout() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), NANOS_PER_DOLLAR + 1);
        let err = evaluate_immediate(FeedId(7), &feed, &signed, &MarketStatus::Active, 1_000)
            .unwrap_err();
        assert!(matches!(err, OracleError::InvalidPayout(_)));
    }

    #[test]
    fn immediate_rejects_already_resolved() {
        let feed = sample_feed(7);
        let signed = sample_signed(MarketId::new(5), 0);
        let record = crate::ResolutionRecord {
            market_id: MarketId::new(5),
            payout_nanos: NANOS_PER_DOLLAR,
            resolved_by: OracleSource::Admin,
            resolved_at_ms: 100,
            proposal: None,
            challenge: None,
        };
        let err = evaluate_immediate(
            FeedId(7),
            &feed,
            &signed,
            &MarketStatus::Resolved { record },
            1_000,
        )
        .unwrap_err();
        assert!(matches!(err, OracleError::AlreadyResolved));
    }
}

/// Convenience wrapper used by the admin facade: reuses the `Immediate` logic
/// but records the resolution as `OracleSource::Admin` so legacy tests keep
/// asserting the same enum variant.
pub(crate) fn evaluate_admin_immediate(
    market_id: MarketId,
    payout_nanos: Nanos,
    current_status: &MarketStatus,
    timestamp_ms: u64,
) -> Result<PolicyOutcome, OracleError> {
    if payout_nanos > NANOS_PER_DOLLAR {
        return Err(OracleError::InvalidPayout(payout_nanos));
    }
    match current_status {
        MarketStatus::Resolved { .. } => return Err(OracleError::AlreadyResolved),
        MarketStatus::Voided => return Err(OracleError::InvalidState),
        _ => {}
    }
    Ok(PolicyOutcome::Settle {
        record: ResolutionRecord {
            market_id,
            payout_nanos,
            resolved_by: OracleSource::Admin,
            resolved_at_ms: timestamp_ms,
            proposal: None,
            challenge: None,
        },
    })
}
