use thiserror::Error;

/// Errors from the oracle subsystem.
#[derive(Debug, Error)]
pub enum OracleError {
    #[error("market already resolved")]
    AlreadyResolved,
    #[error("invalid payout: {0} nanos (must be 0 to 1_000_000_000)")]
    InvalidPayout(u64),
    #[error("unknown feed")]
    UnknownFeed,
    #[error("unknown template: {0}")]
    UnknownTemplate(String),
    #[error("invalid attestation: {0}")]
    InvalidAttestation(String),
}
