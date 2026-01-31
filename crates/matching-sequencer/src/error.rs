use matching_engine::MarketId;

use crate::account::AccountId;

/// Reason an order was rejected.
#[derive(Debug, Clone)]
pub enum RejectionReason {
    InsufficientBalance {
        required: i64,
        available: i64,
    },
    InsufficientPosition {
        market: MarketId,
        outcome: u8,
        required: i64,
        available: i64,
    },
    AccountNotFound,
}

/// A rejected order.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub order_id: u64,
    pub account_id: AccountId,
    pub reason: RejectionReason,
}

/// Errors from the sequencer subsystem.
#[derive(Debug)]
pub enum SequencerError {
    /// Order validation failure.
    Rejected(Rejection),
    /// P256 signature check failed.
    InvalidSignature,
    /// No account registered for this public key.
    UnknownSigner,
    /// Mempool capacity exceeded.
    MempoolFull,
    /// All handles dropped; the actor has shut down.
    ActorGone,
}

impl std::fmt::Display for SequencerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SequencerError::Rejected(r) => write!(f, "order {} rejected: {:?}", r.order_id, r.reason),
            SequencerError::InvalidSignature => write!(f, "invalid P256 signature"),
            SequencerError::UnknownSigner => write!(f, "unknown signer public key"),
            SequencerError::MempoolFull => write!(f, "mempool full"),
            SequencerError::ActorGone => write!(f, "sequencer actor shut down"),
        }
    }
}

impl std::error::Error for SequencerError {}
