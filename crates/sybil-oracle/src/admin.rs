use matching_engine::MarketId;

use crate::error::OracleError;
use crate::traits::{Oracle, ResolutionAction};
use crate::types::{MarketStatus, OracleSource, ResolutionRecord};

/// Trivial admin oracle: resolves markets immediately with no challenge window.
///
/// Validates that the outcome is 0 or 1 (binary) and that the market is not
/// already resolved. Returns `SettleNow` on success.
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
        winning_outcome: u8,
        current_status: &MarketStatus,
        timestamp_ms: u64,
    ) -> Result<ResolutionAction, OracleError> {
        // Validate outcome (binary markets only)
        if winning_outcome > 1 {
            return Err(OracleError::InvalidOutcome(winning_outcome));
        }

        // Check current state
        match current_status {
            MarketStatus::Resolved { .. } => return Err(OracleError::AlreadyResolved),
            MarketStatus::Voided => return Err(OracleError::InvalidState),
            _ => {}
        }

        let record = ResolutionRecord {
            market_id,
            winning_outcome,
            resolved_by: OracleSource::Admin,
            resolved_at_ms: timestamp_ms,
            proposal: None,
            challenge: None,
        };

        Ok(ResolutionAction::SettleNow {
            market_id,
            winning_outcome,
            record,
        })
    }

    fn name(&self) -> &str {
        "admin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::MarketId;

    #[test]
    fn test_admin_resolve_yes() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let action = oracle
            .resolve(MarketId::new(0), 0, &status, 1000)
            .unwrap();

        match action {
            ResolutionAction::SettleNow {
                market_id,
                winning_outcome,
                record,
            } => {
                assert_eq!(market_id, MarketId::new(0));
                assert_eq!(winning_outcome, 0);
                assert_eq!(record.resolved_at_ms, 1000);
                assert!(matches!(record.resolved_by, OracleSource::Admin));
            }
            other => panic!("Expected SettleNow, got {:?}", other),
        }
    }

    #[test]
    fn test_admin_resolve_no() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let action = oracle
            .resolve(MarketId::new(1), 1, &status, 2000)
            .unwrap();

        match action {
            ResolutionAction::SettleNow {
                winning_outcome, ..
            } => {
                assert_eq!(winning_outcome, 1);
            }
            other => panic!("Expected SettleNow, got {:?}", other),
        }
    }

    #[test]
    fn test_admin_rejects_invalid_outcome() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let err = oracle.resolve(MarketId::new(0), 2, &status, 1000).unwrap_err();
        assert!(matches!(err, OracleError::InvalidOutcome(2)));
    }

    #[test]
    fn test_admin_rejects_already_resolved() {
        let oracle = AdminOracle::new();
        let record = ResolutionRecord {
            market_id: MarketId::new(0),
            winning_outcome: 0,
            resolved_by: OracleSource::Admin,
            resolved_at_ms: 500,
            proposal: None,
            challenge: None,
        };
        let status = MarketStatus::Resolved { record };
        let err = oracle.resolve(MarketId::new(0), 1, &status, 1000).unwrap_err();
        assert!(matches!(err, OracleError::AlreadyResolved));
    }

    #[test]
    fn test_admin_challenge_not_supported() {
        let oracle = AdminOracle::new();
        let status = MarketStatus::Active;
        let err = oracle
            .challenge(MarketId::new(0), 1, &status, 100, 1000)
            .unwrap_err();
        assert!(matches!(err, OracleError::ChallengeNotSupported));
    }

    #[test]
    fn test_admin_name() {
        let oracle = AdminOracle::new();
        assert_eq!(oracle.name(), "admin");
    }
}
