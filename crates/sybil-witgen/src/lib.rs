//! Host-side construction of prover inputs from persisted proof jobs.
//!
//! The sequencer owns block production and persistence. This crate owns the
//! witgen/prover boundary: a portable proof-job type plus conversion into the
//! guest input consumed by `sybil-zk`. With the default `sequencer-store`
//! feature, it also includes the adapter that collects a job from sequencer
//! storage.

mod job;
#[cfg(feature = "sequencer-store")]
mod sequencer_store;

pub use job::{
    build_state_transition_guest_input, ProofJobError, StateTransitionProofJob,
    StateTransitionProofJobId, StateTransitionStateLeafProof, STATE_TRANSITION_PROOF_JOB_VERSION,
};

#[cfg(feature = "sequencer-store")]
pub use sequencer_store::{
    build_state_transition_guest_input_from_store, collect_state_transition_proof_job,
    SequencerStoreWitgenError,
};
