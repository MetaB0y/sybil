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
    /// MM orders form a complete set within a market group (self-trade via minting).
    CompleteSetFormation,
    /// Order time-in-force made it ineligible for the target batch.
    Expired {
        current_block: u64,
        expires_at_block: u64,
    },
}

/// A rejected order.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub order_id: u64,
    pub account_id: AccountId,
    pub reason: RejectionReason,
}

/// Errors from the sequencer subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SequencerError {
    /// Order validation failure.
    #[error("order {} rejected: {:?}", .0.order_id, .0.reason)]
    Rejected(Rejection),
    /// P256 signature check failed.
    #[error("invalid P256 signature")]
    InvalidSignature,
    /// No account registered for this public key.
    #[error("unknown signer public key")]
    UnknownSigner,
    /// The signed account_id does not match the signer registry mapping.
    #[error("signed account does not match signer public key")]
    SignerAccountMismatch,
    /// Mempool capacity exceeded.
    #[error("mempool full")]
    MempoolFull,
    /// Submission rate limit exceeded.
    #[error("rate limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    /// Submission contains too many orders.
    #[error("too many orders in submission: {count} > {limit}")]
    TooManyOrdersInSubmission { count: usize, limit: usize },
    /// Account has too many resting or staged orders.
    #[error("account {} has too many open orders: limit {}", .account_id.0, .limit)]
    TooManyOpenOrders { account_id: AccountId, limit: usize },
    /// Account has too many deferred bundles.
    #[error("account {} has too many pending bundles: limit {}", .account_id.0, .limit)]
    TooManyPendingBundles { account_id: AccountId, limit: usize },
    /// All handles dropped; the actor has shut down.
    #[error("sequencer actor shut down")]
    ActorGone,
    /// A public key is already registered to an account.
    #[error("public key already registered to an account")]
    AccountAlreadyRegistered,
    /// The requested market was not found.
    #[error("market not found")]
    MarketNotFound,
    /// The requested block was not found.
    #[error("block not found")]
    BlockNotFound,
    /// The requested pending order was not found.
    #[error("pending order not found")]
    OrderNotFound,
    /// The requested pending order does not belong to the caller.
    #[error("pending order does not belong to account")]
    OrderOwnershipMismatch,
    /// Oracle error during resolution.
    #[error("oracle error: {0}")]
    OracleError(String),
    /// Market lifecycle transition is not valid in the current state.
    #[error("invalid market state: {0}")]
    InvalidMarketState(String),
    /// Bridge deposit or withdrawal validation failed.
    #[error("bridge error: {0}")]
    Bridge(String),
    /// Requested proof cannot be served by this sequencer configuration.
    #[error("proof unavailable: {0}")]
    ProofUnavailable(String),
    /// Block persistence failed before the prepared block could be committed.
    #[error("block persistence failed: {0}")]
    Persistence(String),
}
