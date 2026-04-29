//! Comprehensive block verification for prediction market matching.
//!
//! This crate verifies every aspect of a block produced by the sequencer,
//! designed so a future ZK circuit can implement the same checks.
//!
//! # Verification Layers
//!
//! 1. **Match verification** — per-fill checks + market-level invariants
//! 2. **Settlement verification** — re-derive post-state from post-system state + fills
//! 3. **Block verification** — state root, parent hash, height, counts
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
pub mod block;
pub mod match_verifier;
pub mod orders;
pub mod settlement;
pub mod types;
pub mod violations;

pub use types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    RejectionReason, RestingOrderSnapshot, StateSidecarSnapshot, SystemEventWitness,
    WithdrawalSnapshot, WitnessBlockHeader, WitnessOrder, WitnessRejection,
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
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    block::verify_block(witness)
}

/// Verify order validation (post-system balance/position checks, rejection correctness).
pub fn verify_orders(witness: &BlockWitness) -> VerificationResult {
    orders::verify_orders(witness)
}

/// Run all 4 verification layers and merge results.
pub fn verify_full(witness: &BlockWitness, diagnostics: bool) -> VerificationResult {
    let mut result = verify_match(witness, diagnostics);
    result.merge(verify_settlement(witness));
    result.merge(verify_block(witness));
    result.merge(verify_orders(witness));
    result
}
