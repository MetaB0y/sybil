//! Portable proof jobs and proof-result envelopes.
//!
//! This crate deliberately has no store, filesystem, process, network, clock,
//! sequencer, or OpenVM SDK dependency. It is the versioned handoff shared by
//! the sequencer-side outbox and the standalone prover service.

mod envelope;
mod job;

pub use envelope::{
    EpochId, PROOF_ENVELOPE_VERSION, PROOF_PAYLOAD_DIGEST_DOMAIN, ProofEnvelope,
    ProofEnvelopeError, ProofKind, proof_payload_digest,
};
pub use job::{
    PROOF_JOB_TRANSPORT_DIGEST_DOMAIN, ProofJobError, STATE_TRANSITION_PROOF_JOB_VERSION,
    StateTransitionProofJob, StateTransitionProofJobId, StateTransitionStateLeafProof,
    build_state_transition_guest_input, proof_job_transport_digest,
};
pub use sybil_zk::{
    EPOCH_BLOCKS_DOMAIN, EPOCH_BLOCKS_FOLD_DOMAIN, EPOCH_DA_DOMAIN, EPOCH_DA_FOLD_DOMAIN,
    EPOCH_TRANSITION_DOMAIN, EPOCH_TRANSITION_INPUT_VERSION, EpochTransitionAccumulator,
    EpochTransitionError, EpochTransitionHeader, EpochTransitionPublicInputs, MAX_EPOCH_BLOCKS,
    epoch_transition_public_input_hash, verify_epoch_transition_inputs,
};
