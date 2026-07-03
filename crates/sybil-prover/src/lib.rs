//! Host-side Sybil prover tooling.
//!
//! The default build stays sequencer-free. Store-backed proof-job export lives
//! behind the `sequencer-store` feature, and the dev mock artifact producer
//! lives behind the `mock-live` feature.

pub mod abi;
pub mod artifacts;
pub mod da;
pub mod error;
pub mod job;
pub mod serve;

#[cfg(feature = "mock-live")]
pub mod mock_live;
#[cfg(feature = "sequencer-store")]
pub mod sequencer_store;
#[cfg(feature = "sequencer-store")]
pub mod witgen_cli;

pub use error::ProverCliError;
pub use job::{
    build_state_transition_guest_input, ProofJobError, StateTransitionProofJob,
    StateTransitionProofJobId, StateTransitionStateLeafProof, STATE_TRANSITION_PROOF_JOB_VERSION,
};

#[cfg(feature = "sequencer-store")]
pub use sequencer_store::{
    build_state_transition_guest_input_from_store, collect_state_transition_proof_job,
    SequencerStoreWitgenError,
};
