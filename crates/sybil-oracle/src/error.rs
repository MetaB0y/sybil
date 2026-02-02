use thiserror::Error;

/// Errors from the oracle subsystem.
#[derive(Debug, Error)]
pub enum OracleError {
    #[error("market not found")]
    MarketNotFound,
    #[error("market already resolved")]
    AlreadyResolved,
    #[error("invalid market state for this operation")]
    InvalidState,
    #[error("invalid outcome: {0}")]
    InvalidOutcome(u8),
    #[error("challenge not supported by this oracle")]
    ChallengeNotSupported,
    #[error("insufficient bond: required {required}, got {provided}")]
    InsufficientBond { required: u64, provided: u64 },
    #[error("challenge window expired")]
    ChallengeWindowExpired,
    #[error("no pending proposal for this market")]
    NoPendingProposal,
    #[error("{0}")]
    Other(String),
}
