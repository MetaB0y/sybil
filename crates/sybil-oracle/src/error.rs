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
    #[error("invalid payout: {0} nanos (must be 0 to 1_000_000_000)")]
    InvalidPayout(u64),
    #[error("challenge not supported by this oracle")]
    ChallengeNotSupported,
    #[error("insufficient bond: required {required}, got {provided}")]
    InsufficientBond { required: u64, provided: u64 },
    #[error("challenge window expired")]
    ChallengeWindowExpired,
    #[error("no pending proposal for this market")]
    NoPendingProposal,
    #[error("unknown feed")]
    UnknownFeed,
    #[error("unknown template: {0}")]
    UnknownTemplate(String),
    #[error("invalid attestation: {0}")]
    InvalidAttestation(String),
    #[error("{0}")]
    Other(String),
}
