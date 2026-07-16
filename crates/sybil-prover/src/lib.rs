//! Host-side Sybil prover tooling.
//!
//! The default build stays sequencer-free. Store-backed proof-job export lives
//! behind the `sequencer-store` feature. The prover daemon owns both the typed
//! mock backend used by development and the STARK backend used for real proofs.

pub mod abi;
pub mod artifacts;
pub mod da;
pub mod daemon;
pub mod error;
pub mod mock_backend;
pub mod serve;

#[cfg(feature = "sequencer-store")]
pub mod sequencer_store;
#[cfg(feature = "sequencer-store")]
pub mod witgen_cli;

pub use error::ProverCliError;
pub use sybil_proof_protocol::{
    ProofJobError, STATE_TRANSITION_PROOF_JOB_VERSION, StateTransitionProofJob,
    StateTransitionProofJobId, StateTransitionStateLeafProof, build_state_transition_guest_input,
};

#[cfg(feature = "sequencer-store")]
pub use sequencer_store::{
    SequencerStoreWitgenError, build_state_transition_guest_input_from_store,
    collect_state_transition_proof_job,
};
