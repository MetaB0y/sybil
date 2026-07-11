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

mod account_keys;
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
pub mod key_op_auth;
pub mod key_transition;
pub mod match_verifier;
pub mod orders;
pub mod quarantine;
pub mod settlement;
pub mod sidecar;
mod snapshot_schema;
pub mod state_schema;
pub mod system;
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

pub use account_keys::{
    account_keys_digest, canonical_escape_claim_bytes, canonical_key_registration_bytes,
    canonical_key_revocation_bytes, empty_account_keys_digest, AccountKeyDigestRecord,
    ACCOUNT_KEYS_DIGEST_DOMAIN, MAX_KEYS_PER_ACCOUNT, MAX_KEY_OPS_PER_BLOCK,
    MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES, MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES,
};
pub use key_op_auth::{verify_keyop_auth, EXPECTED_RP_ID_HASH, EXPECTED_WEBAUTHN_RP_ID};
pub use types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, DepositAccumulatorWitness, KeyOpAuth, KeyRecord, L1DepositWitness,
    MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot, OracleSourceSnapshot,
    QuarantineEntrySnapshot, RejectionReason, ResolutionProposalSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, SystemEventWitness, WithdrawalRefundReasonWitness,
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

/// Verify that authenticated pre-state plus system events reproduces the
/// account-value fields in post-system state.
pub fn verify_system(witness: &BlockWitness) -> VerificationResult {
    let mut result = system::verify_system_transition(witness);
    result.merge(key_transition::verify_key_transitions(witness));
    result
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

/// Verify derivable non-account sidecar facts.
pub fn verify_sidecar(witness: &BlockWitness) -> VerificationResult {
    sidecar::verify_sidecar(witness)
}

/// Run all 4 verification layers and merge results.
#[cfg(feature = "qmdb")]
pub fn verify_full(witness: &BlockWitness, diagnostics: bool) -> VerificationResult {
    let mut result = verify_match(witness, diagnostics);
    result.merge(verify_system(witness));
    result.merge(verify_settlement(witness));
    result.merge(verify_block(witness));
    result.merge(verify_orders(witness));
    result.merge(verify_sidecar(witness));
    result
}
