use matching_engine::MarketId;

use crate::error::OracleError;
use crate::types::{MarketStatus, ResolutionProposal, ResolutionRecord};

/// Action the sequencer should take after consulting the oracle about resolution.
#[derive(Debug)]
pub enum ResolutionAction {
    /// Settle the market immediately (no challenge window).
    SettleNow {
        market_id: MarketId,
        winning_outcome: u8,
        record: ResolutionRecord,
    },
    /// Propose a resolution with a challenge window.
    Propose {
        proposal: ResolutionProposal,
        challenge_window_ms: u64,
    },
    /// Reject the resolution request.
    Reject { reason: String },
}

/// Action the sequencer should take after consulting the oracle about a challenge.
#[derive(Debug)]
pub enum ChallengeAction {
    /// Escalate to L1 adjudication.
    Escalate,
    /// Reject the challenge.
    Reject { reason: String },
}

/// Pluggable oracle trait for market resolution decisions.
///
/// The oracle does NOT perform settlement, fetch external data, or handle bond
/// escrow. It only makes authorization/lifecycle decisions. The sequencer acts
/// on the returned actions.
pub trait Oracle: Send + Sync {
    /// Process a resolution request. Returns an action for the sequencer to execute.
    fn resolve(
        &self,
        market_id: MarketId,
        winning_outcome: u8,
        current_status: &MarketStatus,
        timestamp_ms: u64,
    ) -> Result<ResolutionAction, OracleError>;

    /// Process a challenge against a pending proposal.
    ///
    /// Default implementation rejects all challenges.
    fn challenge(
        &self,
        _market_id: MarketId,
        _proposed_outcome: u8,
        _current_status: &MarketStatus,
        _bond_amount: u64,
        _timestamp_ms: u64,
    ) -> Result<ChallengeAction, OracleError> {
        Err(OracleError::ChallengeNotSupported)
    }

    /// Check if a Proposed market should auto-finalize.
    ///
    /// Called periodically by the sequencer. Returns `Some(SettleNow)` if the
    /// challenge window has elapsed with no challenges.
    fn check_finalization(
        &self,
        _market_id: MarketId,
        _current_status: &MarketStatus,
        _timestamp_ms: u64,
    ) -> Option<ResolutionAction> {
        None
    }

    /// Human-readable name for this oracle implementation.
    fn name(&self) -> &str;
}
