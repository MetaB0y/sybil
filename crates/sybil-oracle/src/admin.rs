//! Thin facade over [`crate::policy::evaluate_admin_immediate`].
//!
//! Kept so the ~30 existing call sites (sequencer.rs, tests, examples) compile
//! unchanged. New code should wire the sequencer through feeds + templates.

use matching_engine::{MarketId, Nanos};

use crate::error::OracleError;
use crate::policy::{evaluate_admin_immediate, PolicyOutcome};
use crate::traits::{Oracle, ResolutionAction};
use crate::types::MarketStatus;

pub struct AdminOracle;

impl AdminOracle {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AdminOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl Oracle for AdminOracle {
    fn resolve(
        &self,
        market_id: MarketId,
        payout_nanos: Nanos,
        current_status: &MarketStatus,
        timestamp_ms: u64,
    ) -> Result<ResolutionAction, OracleError> {
        match evaluate_admin_immediate(market_id, payout_nanos, current_status, timestamp_ms)? {
            PolicyOutcome::Settle { record } => Ok(ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            }),
            PolicyOutcome::Reject { reason } => Ok(ResolutionAction::Reject { reason }),
        }
    }

    fn name(&self) -> &str {
        "admin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OracleSource, ResolutionRecord};
    use matching_engine::{MarketId, NANOS_PER_DOLLAR};

    #[test]
    fn test_admin_resolve_yes_wins() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let action = oracle
            .resolve(MarketId::new(0), NANOS_PER_DOLLAR, &status, 1000)
            .unwrap();

        match action {
            ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            } => {
                assert_eq!(market_id, MarketId::new(0));
                assert_eq!(payout_nanos, NANOS_PER_DOLLAR);
                assert_eq!(record.resolved_at_ms, 1000);
                assert!(matches!(record.resolved_by, OracleSource::Admin));
            }
            other => panic!("Expected SettleNow, got {:?}", other),
        }
    }

    #[test]
    fn test_admin_resolve_no_wins() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let action = oracle.resolve(MarketId::new(1), 0, &status, 2000).unwrap();

        match action {
            ResolutionAction::SettleNow { payout_nanos, .. } => {
                assert_eq!(payout_nanos, 0);
            }
            other => panic!("Expected SettleNow, got {:?}", other),
        }
    }

    #[test]
    fn test_admin_resolve_fractional() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let action = oracle
            .resolve(MarketId::new(0), 700_000_000, &status, 3000)
            .unwrap();

        match action {
            ResolutionAction::SettleNow {
                payout_nanos,
                record,
                ..
            } => {
                assert_eq!(payout_nanos, 700_000_000);
                assert_eq!(record.payout_nanos, 700_000_000);
            }
            other => panic!("Expected SettleNow, got {:?}", other),
        }
    }

    #[test]
    fn test_admin_rejects_invalid_payout() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let err = oracle
            .resolve(MarketId::new(0), NANOS_PER_DOLLAR + 1, &status, 1000)
            .unwrap_err();
        assert!(matches!(err, OracleError::InvalidPayout(_)));
    }

    #[test]
    fn test_admin_rejects_already_resolved() {
        let oracle = AdminOracle::new();
        let record = ResolutionRecord {
            market_id: MarketId::new(0),
            payout_nanos: NANOS_PER_DOLLAR,
            resolved_by: OracleSource::Admin,
            resolved_at_ms: 500,
            proposal: None,
            challenge: None,
        };
        let status = MarketStatus::Resolved { record };
        let err = oracle
            .resolve(MarketId::new(0), 0, &status, 1000)
            .unwrap_err();
        assert!(matches!(err, OracleError::AlreadyResolved));
    }

    #[test]
    fn test_admin_challenge_not_supported() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let err = oracle
            .challenge(MarketId::new(0), 0, &status, 100, 1000)
            .unwrap_err();
        assert!(matches!(err, OracleError::ChallengeNotSupported));
    }

    #[test]
    fn test_admin_name() {
        let oracle = AdminOracle::new();
        assert_eq!(oracle.name(), "admin");
    }
}
