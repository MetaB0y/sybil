use matching_sequencer::store::Store;
use sybil_verifier::{BlockWitness, commitments::state_schema};

use crate::{
    ProofJobError, StateTransitionProofJob, StateTransitionStateLeafProof,
    build_state_transition_guest_input,
};

#[derive(Debug, thiserror::Error)]
pub enum SequencerStoreWitgenError {
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("proof unavailable: {0}")]
    ProofUnavailable(String),
    #[error("committed state qMDB root does not match witness header state_root")]
    StateRootMismatch,
    #[error("state qMDB leaf proof at sorted leaf index {index} came from a different root")]
    ProofRootMismatch { index: usize },
    #[error("state qMDB leaf proof at sorted leaf index {index} does not match the witness leaf")]
    WitnessLeafMismatch { index: usize },
    #[error("state qMDB leaf proof at sorted leaf index {index} failed native verification")]
    NativeProofFailed { index: usize },
    #[error(transparent)]
    ProofJob(#[from] ProofJobError),
}

pub async fn collect_state_transition_proof_job(
    store: &Store,
    witness: BlockWitness,
) -> Result<StateTransitionProofJob, SequencerStoreWitgenError> {
    let leaves = state_schema::state_root_leaves(&witness.post_state, &witness.state_sidecar);
    let root = store
        .current_state_qmdb_root()
        .await
        .map_err(|error| SequencerStoreWitgenError::Persistence(error.to_string()))?
        .ok_or_else(|| {
            SequencerStoreWitgenError::ProofUnavailable("no committed state qMDB root".to_string())
        })?;

    if root.root != witness.header.state_root {
        return Err(SequencerStoreWitgenError::StateRootMismatch);
    }

    let mut state_leaf_proofs = Vec::with_capacity(leaves.len());
    for (index, (key, value)) in leaves.iter().enumerate() {
        let proof = store
            .state_qmdb_leaf_proof(root.slot, key)
            .await
            .map_err(|error| SequencerStoreWitgenError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                SequencerStoreWitgenError::ProofUnavailable(format!(
                    "missing state qMDB leaf proof at sorted leaf index {index}"
                ))
            })?;

        if proof.root != root.root || proof.slot != root.slot {
            return Err(SequencerStoreWitgenError::ProofRootMismatch { index });
        }
        if &proof.leaf_key != key || &proof.leaf_value != value {
            return Err(SequencerStoreWitgenError::WitnessLeafMismatch { index });
        }
        if !proof.verify() {
            return Err(SequencerStoreWitgenError::NativeProofFailed { index });
        }

        state_leaf_proofs.push(StateTransitionStateLeafProof {
            key: key.clone(),
            value: value.clone(),
            proof: proof.proof_parts(),
        });
    }

    let mut pre_state_leaf_proofs = Vec::new();
    if let Some(previous) = &witness.previous_header {
        let pre_slot = root.slot.inactive();
        let pre_root = store
            .state_qmdb_root(pre_slot)
            .await
            .map_err(|error| SequencerStoreWitgenError::Persistence(error.to_string()))?;
        if pre_root.root != previous.state_root {
            return Err(SequencerStoreWitgenError::StateRootMismatch);
        }
        let pre_leaves =
            state_schema::state_root_leaves(&witness.pre_state, &witness.pre_state_sidecar);
        pre_state_leaf_proofs.reserve(pre_leaves.len());
        for (index, (key, value)) in pre_leaves.iter().enumerate() {
            let proof = store
                .state_qmdb_leaf_proof(pre_slot, key)
                .await
                .map_err(|error| SequencerStoreWitgenError::Persistence(error.to_string()))?
                .ok_or_else(|| {
                    SequencerStoreWitgenError::ProofUnavailable(format!(
                        "missing pre-state qMDB leaf proof at sorted leaf index {index}"
                    ))
                })?;
            if proof.root != previous.state_root || proof.slot != pre_slot {
                return Err(SequencerStoreWitgenError::ProofRootMismatch { index });
            }
            if &proof.leaf_key != key || &proof.leaf_value != value {
                return Err(SequencerStoreWitgenError::WitnessLeafMismatch { index });
            }
            if !proof.verify() {
                return Err(SequencerStoreWitgenError::NativeProofFailed { index });
            }
            pre_state_leaf_proofs.push(StateTransitionStateLeafProof {
                key: key.clone(),
                value: value.clone(),
                proof: proof.proof_parts(),
            });
        }
    }

    Ok(StateTransitionProofJob::new(
        witness,
        state_leaf_proofs,
        pre_state_leaf_proofs,
    ))
}

pub async fn build_state_transition_guest_input_from_store(
    store: &Store,
    witness: BlockWitness,
) -> Result<sybil_zk::StateTransitionGuestInput, SequencerStoreWitgenError> {
    let job = collect_state_transition_proof_job(store, witness).await?;
    Ok(build_state_transition_guest_input(job)?)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy, outcome_sell};
    use matching_sequencer::store::Store;
    use matching_sequencer::{AccountStore, BlockSequencer, OrderSubmission, SequencerConfig};
    use sybil_oracle::AdminOracle;

    use super::*;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sybil-{prefix}-{}-{unique}.redb",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn collects_portable_job_and_builds_openvm_guest_input() {
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("ZK smoke");

        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(0);
        accounts
            .get_mut(seller)
            .expect("seller exists")
            .positions
            .insert((market_id, 0), 10);

        let oracle = Arc::new(AdminOracle::new());
        let mut sequencer = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle,
            SequencerConfig::default(),
        );
        let production = sequencer.produce_block(
            vec![
                OrderSubmission {
                    account_id: buyer,
                    orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 5)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: seller,
                    orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 5)],
                    mm_constraint: None,
                },
            ],
            1_000,
        );
        assert_eq!(production.block.fills.len(), 2);

        let path = temp_db_path("zk-proof-job");
        let store = Store::open(&path).unwrap();
        store
            .save_block_with_witness(sequencer.snapshot(), &production.witness)
            .await
            .unwrap();

        let persisted_witness = store
            .latest_block_witness()
            .unwrap()
            .expect("latest witness persisted");

        let job = collect_state_transition_proof_job(&store, persisted_witness)
            .await
            .unwrap();
        assert_eq!(job.block_height, production.block.header.height);
        assert_eq!(job.state_root, production.block.header.state_root);
        assert!(!job.state_leaf_proofs.is_empty());

        let input = build_state_transition_guest_input(job).unwrap();
        assert_eq!(
            sybil_zk::verify_state_transition_input(&input),
            Ok(sybil_zk::state_transition_public_input_hash(
                &input.public_inputs
            ))
        );

        let next = sequencer.produce_block(vec![], 2_000);
        store
            .save_block_with_witness(sequencer.snapshot(), &next.witness)
            .await
            .unwrap();
        let next_job = collect_state_transition_proof_job(&store, next.witness)
            .await
            .unwrap();
        assert!(!next_job.pre_state_leaf_proofs.is_empty());
        let next_input = build_state_transition_guest_input(next_job).unwrap();
        assert_eq!(
            sybil_zk::verify_state_transition_input(&next_input),
            Ok(sybil_zk::state_transition_public_input_hash(
                &next_input.public_inputs
            ))
        );
    }

    #[tokio::test]
    async fn store_convenience_wrapper_still_builds_guest_input() {
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("ZK smoke wrapper");

        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(0);
        accounts
            .get_mut(seller)
            .expect("seller exists")
            .positions
            .insert((market_id, 0), 10);

        let oracle = Arc::new(AdminOracle::new());
        let mut sequencer = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            oracle,
            SequencerConfig::default(),
        );
        let production = sequencer.produce_block(
            vec![
                OrderSubmission {
                    account_id: buyer,
                    orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 5)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: seller,
                    orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 5)],
                    mm_constraint: None,
                },
            ],
            1_000,
        );

        let path = temp_db_path("zk-proof-job-wrapper");
        let store = Store::open(&path).unwrap();
        store.save_block(sequencer.snapshot()).await.unwrap();

        let input = build_state_transition_guest_input_from_store(&store, production.witness)
            .await
            .unwrap();
        assert_eq!(
            sybil_zk::verify_state_transition_input(&input),
            Ok(sybil_zk::state_transition_public_input_hash(
                &input.public_inputs
            ))
        );
    }
}
