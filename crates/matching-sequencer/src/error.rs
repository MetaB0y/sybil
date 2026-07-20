use matching_engine::MarketId;

use crate::account::AccountId;

/// Reason an order was rejected.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Buy limits fund a complete set within a market group (self-trade via minting).
    CompleteSetFormation,
    /// Order shape or quantity is not supported by production admission.
    InvalidOrder(String),
    /// Order time-in-force made it ineligible for the target batch.
    Expired {
        current_block: u64,
        expires_at_block: u64,
    },
}

impl RejectionReason {
    /// Stable wire code for the per-account history feed (`HistoryKind::Rejected`).
    pub fn code(&self) -> &'static str {
        match self {
            RejectionReason::InsufficientBalance { .. } => "insufficient_balance",
            RejectionReason::InsufficientPosition { .. } => "insufficient_position",
            RejectionReason::AccountNotFound => "account_not_found",
            RejectionReason::CompleteSetFormation => "complete_set",
            RejectionReason::InvalidOrder(_) => "invalid_order",
            RejectionReason::Expired { .. } => "expired",
        }
    }

    /// `(required, available)` nanos, when the reason carries them.
    pub fn amounts(&self) -> (Option<i64>, Option<i64>) {
        match self {
            RejectionReason::InsufficientBalance {
                required,
                available,
            }
            | RejectionReason::InsufficientPosition {
                required,
                available,
                ..
            } => (Some(*required), Some(*available)),
            _ => (None, None),
        }
    }
}

/// A rejected order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Rejection {
    pub order_id: u64,
    pub account_id: AccountId,
    pub reason: RejectionReason,
}

/// Verifier violation carried across the sequencer error boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierFailure {
    pub kind: String,
    pub details: String,
}

/// Why a non-zero fill could not be applied to host account state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsettleableFillReason {
    MissingOrder,
    MissingAccount,
    SettlementOverflow,
}

/// Hard block-production invariant that must hold before a prepared block is committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockInvariantFailure {
    UnsettleableFill {
        order_id: u64,
        account_id: u64,
        reason: UnsettleableFillReason,
    },
    NegativeBalance {
        account_id: AccountId,
        balance: i64,
    },
    BalanceDeltaMismatch {
        balance_delta: i64,
        expected_balance_delta: i64,
    },
    PositionImbalance {
        market_id: MarketId,
        total_yes: i64,
        total_no: i64,
    },
    PreparedStateRootMismatch {
        block_state_root: [u8; 32],
        prepared_state_root: [u8; 32],
    },
    FullVerificationFailed {
        violations: Vec<VerifierFailure>,
    },
}

/// Errors from the sequencer subsystem.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SequencerError {
    /// Order validation failure.
    #[error("order {} rejected: {:?}", .0.order_id, .0.reason)]
    Rejected(Rejection),
    /// Resting-order reservation aggregates failed a fail-closed invariant.
    #[error(transparent)]
    ReservationInvariant(#[from] crate::order_book::ReservationError),
    /// P256 signature check failed.
    #[error("invalid P256 signature")]
    InvalidSignature,
    /// No account registered for this public key.
    #[error("unknown signer public key")]
    UnknownSigner,
    /// The signed account_id does not match the signer registry mapping.
    #[error("signed account does not match signer public key")]
    SignerAccountMismatch,
    /// The signed action nonce is not strictly greater than the last accepted nonce.
    #[error(
        "stale replay nonce for account {}: nonce {} must be greater than last accepted nonce {}",
        .account_id.0,
        nonce,
        last_nonce
    )]
    ReplayNonceStale {
        account_id: AccountId,
        nonce: u64,
        last_nonce: u64,
    },
    /// A key operation was signed against account digests that are no longer current.
    #[error("stale key-operation state binding for account {}", .account_id.0)]
    KeyOpStateStale { account_id: AccountId },
    /// Order/cancel signatures are chain-instance scoped; no instance hash exists yet.
    #[error("genesis hash unavailable until the genesis block is committed")]
    GenesisHashUnavailable,
    /// A service provisioning key must be a small, non-empty caller identity.
    #[error("account provisioning key must contain 1..={max_bytes} UTF-8 bytes")]
    InvalidAccountProvisioningKey { max_bytes: usize },
    /// An idempotency key was already committed for different account parameters.
    #[error("account provisioning key is already bound to different parameters")]
    AccountProvisioningConflict,
    /// The durable public grant stock has reached its configured lifetime cap.
    #[error("public account capacity {capacity} is exhausted")]
    PublicAccountCapacityExhausted { capacity: u64 },
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
    /// The bounded external RPC class has no capacity before actor enqueue.
    #[error("sequencer {class} admission is at capacity")]
    ActorOverloaded { class: &'static str },
    /// A public key is already registered to an account.
    #[error("public key already registered to an account")]
    AccountAlreadyRegistered,
    /// First keys are validity-bound to the account-creation event.
    #[error("the first signing key must be attached before the account's creation block is sealed")]
    FirstKeyMustBeInitial,
    /// Per-account validity key cap reached.
    #[error("account signing-key limit reached")]
    SigningKeyLimit,
    /// The signing key targeted for revocation is not registered (SYB-60).
    #[error("signing key not found")]
    KeyNotFound,
    /// Refused to revoke an account's last remaining signing key (SYB-60).
    /// Doing so would permanently lock the account out of all signed actions.
    #[error("cannot revoke the account's last remaining signing key")]
    LastSigningKey,
    /// The requested read API key was not found (SYB-60).
    #[error("api key not found")]
    ApiKeyNotFound,
    /// Lifetime read API-key record cap reached (active and revoked records).
    #[error("account API-key limit reached: {count} >= {limit}")]
    ApiKeyLimit { count: usize, limit: usize },
    /// A read API-key label exceeds its persisted byte limit.
    #[error("API-key label is too long: {bytes} bytes > {limit}")]
    ApiKeyLabelTooLong { bytes: usize, limit: usize },
    /// A signing-key label exceeds its persisted byte limit.
    #[error("signing-key label is too long: {bytes} bytes > {limit}")]
    SigningKeyLabelTooLong { bytes: usize, limit: usize },
    /// A control-plane mutation would make the recovery account too large for
    /// its deliberately conservative qMDB admission budget.
    #[error("serialized account exceeds storage budget: {bytes} bytes > {limit}")]
    AccountStorageBudgetExceeded { bytes: usize, limit: usize },
    /// The next signing-key operation would exceed the witness validity cap.
    #[error("block signing-key operation limit reached: {count} >= {limit}")]
    KeyOpLimit { count: usize, limit: usize },
    /// Profile field failed length/charset validation (SYB-60).
    #[error("invalid profile: {0}")]
    ProfileInvalid(String),
    /// The requested market was not found.
    #[error("market {market_id:?} not found")]
    MarketNotFound { market_id: MarketId },
    /// An operator creation key is malformed at the canonical boundary.
    #[error("invalid market creation key: {0}")]
    InvalidMarketCreationKey(String),
    /// A creation key already identifies a different immutable market spec.
    #[error(
        "market creation key {key:?} already identifies market {existing_market_id:?} with different creation fields"
    )]
    MarketCreationKeyConflict {
        key: String,
        existing_market_id: MarketId,
    },
    /// An operator market-group creation key is malformed.
    #[error("invalid market group creation key: {0}")]
    InvalidMarketGroupCreationKey(String),
    /// A creation key already identifies a different immutable group spec.
    #[error(
        "market group creation key {key:?} already identifies group {existing_group_id} with different creation fields"
    )]
    MarketGroupCreationKeyConflict { key: String, existing_group_id: u64 },
    /// The requested market group was not found.
    #[error("market group not found")]
    MarketGroupNotFound,
    /// The requested market already belongs to another market group.
    #[error("market already belongs to group {group_id}")]
    MarketAlreadyGrouped { group_id: u64 },
    /// The requested block was not found.
    #[error("block not found")]
    BlockNotFound,
    /// The requested block is older than the retained canonical replay archive.
    #[error("block {requested_height} is older than canonical archive min {retention_min_height}")]
    BlockPruned {
        requested_height: u64,
        retention_min_height: u64,
    },
    /// The requested pending order was not found.
    #[error("pending order not found")]
    OrderNotFound,
    /// The requested pending order does not belong to the caller.
    #[error("pending order does not belong to account")]
    OrderOwnershipMismatch,
    /// Oracle error during resolution.
    #[error("oracle error: {0}")]
    OracleError(String),
    /// The requested market exists but is not open for trading.
    #[error("market {market_id:?} is not tradeable ({status})")]
    MarketNotTradeable { market_id: MarketId, status: String },
    /// Bridge deposit or withdrawal validation failed.
    #[error("bridge error: {0}")]
    Bridge(String),
    /// Requested proof cannot be served by this sequencer configuration.
    #[error("proof unavailable: {0}")]
    ProofUnavailable(String),
    /// A hard block invariant failed and the actor has entered fail-stop mode.
    /// Reads remain available, but every canonical write is rejected until an
    /// operator recovers from the last committed block.
    #[error("sequencer integrity halted; writes are unavailable until operator recovery")]
    IntegrityHalted,
    /// Block production is intentionally paused.
    #[error("block production paused")]
    BlockProductionPaused,
    /// Block persistence failed before the prepared block could be committed.
    #[error("block persistence failed: {0}")]
    Persistence(String),
    /// A prepared block failed hard invariant or verifier checks before commit.
    #[error("block {height} failed hard invariant verification: {failures:?}")]
    BlockInvariantFailure {
        height: u64,
        failures: Vec<BlockInvariantFailure>,
    },
}
