use serde::{Deserialize, Serialize};
use sybil_verifier::{commitments::state_schema, BlockWitness};

pub const STATE_TRANSITION_PROOF_JOB_VERSION: u8 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateTransitionProofJob {
    pub format_version: u8,
    pub block_height: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub witness: BlockWitness,
    pub state_leaf_proofs: Vec<StateTransitionStateLeafProof>,
    pub pre_state_leaf_proofs: Vec<StateTransitionStateLeafProof>,
}

impl StateTransitionProofJob {
    pub fn new(
        witness: BlockWitness,
        state_leaf_proofs: Vec<StateTransitionStateLeafProof>,
        pre_state_leaf_proofs: Vec<StateTransitionStateLeafProof>,
    ) -> Self {
        let block_height = witness.header.height;
        let block_hash = sybil_zk::hash_header(&witness.header);
        let state_root = witness.header.state_root;
        Self {
            format_version: STATE_TRANSITION_PROOF_JOB_VERSION,
            block_height,
            block_hash,
            state_root,
            witness,
            state_leaf_proofs,
            pre_state_leaf_proofs,
        }
    }

    pub fn id(&self) -> StateTransitionProofJobId {
        StateTransitionProofJobId {
            block_height: self.block_height,
            block_hash: self.block_hash,
            state_root: self.state_root,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransitionProofJobId {
    pub block_height: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransitionStateLeafProof {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub proof: sybil_zk::QmdbStateKeyValueProof,
}

#[derive(Debug, thiserror::Error)]
pub enum ProofJobError {
    #[error("unsupported state-transition proof job version: expected {expected}, got {actual}")]
    UnsupportedProofJobVersion { expected: u8, actual: u8 },
    #[error("proof job {field} does not match witness")]
    ProofJobMetadataMismatch { field: &'static str },
    #[error("committed state qMDB root does not match witness header state_root")]
    StateRootMismatch,
    #[error("proof job has {actual} state leaf proofs, but witness derives {expected} leaves")]
    ProofJobLeafCountMismatch { expected: usize, actual: usize },
    #[error("state qMDB leaf proof at sorted leaf index {index} does not match the witness leaf")]
    WitnessLeafMismatch { index: usize },
    #[error("proof job state-root proof failed: {0}")]
    StateRootProofFailed(#[source] sybil_zk::ZkTransitionError),
}

pub fn build_state_transition_guest_input(
    job: StateTransitionProofJob,
) -> Result<sybil_zk::StateTransitionGuestInput, ProofJobError> {
    validate_job_metadata(&job)?;

    let leaves =
        state_schema::state_root_leaves(&job.witness.post_state, &job.witness.state_sidecar);
    if job.state_leaf_proofs.len() != leaves.len() {
        return Err(ProofJobError::ProofJobLeafCountMismatch {
            expected: leaves.len(),
            actual: job.state_leaf_proofs.len(),
        });
    }

    for (index, ((expected_key, expected_value), proof)) in
        leaves.iter().zip(&job.state_leaf_proofs).enumerate()
    {
        if &proof.key != expected_key || &proof.value != expected_value {
            return Err(ProofJobError::WitnessLeafMismatch { index });
        }
    }

    let state_root_proof = sybil_zk::QmdbStateRootProof {
        leaf_proofs: job
            .state_leaf_proofs
            .iter()
            .map(|leaf| leaf.proof.clone())
            .collect(),
    };

    sybil_zk::verify_qmdb_state_root(&job.state_root, &job.witness, &state_root_proof)
        .map_err(ProofJobError::StateRootProofFailed)?;

    let pre_state_root_proof = if let Some(previous) = &job.witness.previous_header {
        let pre_leaves =
            state_schema::state_root_leaves(&job.witness.pre_state, &job.witness.pre_state_sidecar);
        if job.pre_state_leaf_proofs.len() != pre_leaves.len() {
            return Err(ProofJobError::ProofJobLeafCountMismatch {
                expected: pre_leaves.len(),
                actual: job.pre_state_leaf_proofs.len(),
            });
        }
        for (index, ((expected_key, expected_value), proof)) in pre_leaves
            .iter()
            .zip(&job.pre_state_leaf_proofs)
            .enumerate()
        {
            if &proof.key != expected_key || &proof.value != expected_value {
                return Err(ProofJobError::WitnessLeafMismatch { index });
            }
        }
        let proof = sybil_zk::QmdbStateRootProof {
            leaf_proofs: job
                .pre_state_leaf_proofs
                .iter()
                .map(|leaf| leaf.proof.clone())
                .collect(),
        };
        sybil_zk::verify_qmdb_state_root_for(&previous.state_root, &pre_leaves, &proof)
            .map_err(ProofJobError::StateRootProofFailed)?;
        proof
    } else {
        if !job.pre_state_leaf_proofs.is_empty() {
            return Err(ProofJobError::ProofJobLeafCountMismatch {
                expected: 0,
                actual: job.pre_state_leaf_proofs.len(),
            });
        }
        sybil_zk::QmdbStateRootProof::default()
    };

    Ok(sybil_zk::StateTransitionGuestInput {
        public_inputs: sybil_zk::public_inputs_from_witness(&job.witness),
        witness: job.witness,
        da_provider_refs: vec![],
        state_root_proof,
        pre_state_root_proof,
    })
}

fn validate_job_metadata(job: &StateTransitionProofJob) -> Result<(), ProofJobError> {
    if job.format_version != STATE_TRANSITION_PROOF_JOB_VERSION {
        return Err(ProofJobError::UnsupportedProofJobVersion {
            expected: STATE_TRANSITION_PROOF_JOB_VERSION,
            actual: job.format_version,
        });
    }
    if job.block_height != job.witness.header.height {
        return Err(ProofJobError::ProofJobMetadataMismatch {
            field: "block_height",
        });
    }
    if job.block_hash != sybil_zk::hash_header(&job.witness.header) {
        return Err(ProofJobError::ProofJobMetadataMismatch {
            field: "block_hash",
        });
    }
    if job.state_root != job.witness.header.state_root {
        return Err(ProofJobError::StateRootMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use sybil_verifier::{BlockWitness, StateSidecarSnapshot, WitnessBlockHeader};

    use super::*;

    fn minimal_job() -> StateTransitionProofJob {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 7,
                parent_hash: [1u8; 32],
                state_root: [2u8; 32],
                events_root: [3u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1_000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: sybil_verifier::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            account_keys: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };
        StateTransitionProofJob::new(witness, vec![], vec![])
    }

    #[test]
    fn job_id_is_derived_from_witness_header() {
        let job = minimal_job();

        assert_eq!(
            job.id(),
            StateTransitionProofJobId {
                block_height: job.witness.header.height,
                block_hash: sybil_zk::hash_header(&job.witness.header),
                state_root: job.witness.header.state_root,
            }
        );
    }

    #[test]
    fn unsupported_job_version_is_rejected_before_proof_validation() {
        let mut job = minimal_job();
        job.format_version = 0;

        assert!(matches!(
            build_state_transition_guest_input(job),
            Err(ProofJobError::UnsupportedProofJobVersion {
                expected: STATE_TRANSITION_PROOF_JOB_VERSION,
                actual: 0,
            })
        ));
    }

    #[test]
    fn metadata_mismatch_is_rejected_before_proof_validation() {
        let mut job = minimal_job();
        job.block_hash = [9u8; 32];

        assert!(matches!(
            build_state_transition_guest_input(job),
            Err(ProofJobError::ProofJobMetadataMismatch {
                field: "block_hash"
            })
        ));
    }
}
