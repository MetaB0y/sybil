//! Comprehensive block verification for prediction market matching.
//!
//! This crate verifies every aspect of a block produced by the sequencer,
//! designed so a future ZK circuit can implement the same checks.
//!
//! # Verification Layers
//!
//! 1. **Match verification** — per-fill checks + market-level invariants
//! 2. **Settlement verification** — re-derive post-state from post-system state + fills
//! 3. **Block verification** — state root, events root, parent hash, height, counts
//! 4. **Order verification** — post-system balance/position checks, rejection correctness
//!
//! # Usage
//!
//! ```ignore
//! use sybil_verifier::{verify_full, BlockWitness};
//!
//! let result = verify_full(&witness, /* diagnostics */ true);
//! assert!(result.valid, "Violations: {:?}", result.violations);
//! ```

pub mod arithmetic;
#[cfg(feature = "qmdb")]
pub mod block;
mod canonical;
#[cfg(feature = "qmdb")]
pub mod event_commitment;
pub mod event_schema;
#[cfg(feature = "qmdb")]
mod header_hash {
    use crate::WitnessBlockHeader;

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sybil-zk/src/header_hash_impl.rs"
    ));
}
pub mod match_verifier;
pub mod orders;
pub mod settlement;
mod snapshot_schema;
pub mod state_schema;
pub mod types;
pub mod violations;
pub mod witness_schema;

#[cfg(all(test, feature = "qmdb"))]
mod byte_identity;

/// Canonical byte schemas used as inputs to state, event, and witness commitments.
///
/// The verifier crate owns these schemas so native verification, witness
/// generation, and guest verification all serialize committed data the same way.
pub mod commitments {
    #[cfg(feature = "qmdb")]
    pub use crate::header_hash::hash_header;
    pub use crate::{event_schema, state_schema, witness_schema};
}

pub use types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, L1DepositWitness, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
    OracleSourceSnapshot, RejectionReason, ResolutionProposalSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, SystemEventWitness, WithdrawalSnapshot,
    WitnessBlockHeader, WitnessOrder, WitnessRejection,
};
pub use violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify fill-level and market-level invariants.
///
/// Core checks (ZK invariants) always run. Diagnostic checks (quality metrics
/// like zero-fill rejection and market group sum constraints) only run when
/// `diagnostics` is true.
pub fn verify_match(witness: &BlockWitness, diagnostics: bool) -> VerificationResult {
    match_verifier::verify_match(witness, diagnostics)
}

/// Verify that `post_system_state + fills → post_state`.
pub fn verify_settlement(witness: &BlockWitness) -> VerificationResult {
    settlement::verify_settlement(witness)
}

/// Verify block header integrity (state root, parent hash, height, counts).
#[cfg(feature = "qmdb")]
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    block::verify_block(witness)
}

/// Verify order validation (post-system balance/position checks, rejection correctness).
pub fn verify_orders(witness: &BlockWitness) -> VerificationResult {
    orders::verify_orders(witness)
}

/// Run all 4 verification layers and merge results.
#[cfg(feature = "qmdb")]
pub fn verify_full(witness: &BlockWitness, diagnostics: bool) -> VerificationResult {
    let mut result = verify_match(witness, diagnostics);
    result.merge(verify_settlement(witness));
    result.merge(verify_block(witness));
    result.merge(verify_orders(witness));
    result
}
