use sybil_verifier::BlockWitness;

use crate::account_storage::{
    QmdbStateKeyValueProofParts, QmdbStateOperationProofParts, QmdbStateRangeProofParts,
};
use crate::error::SequencerError;
use crate::store::Store;

pub async fn build_state_transition_guest_input(
    store: &Store,
    witness: BlockWitness,
) -> Result<sybil_zk::StateTransitionGuestInput, SequencerError> {
    let leaves = sybil_verifier::state_schema::state_root_leaves(
        &witness.post_state,
        &witness.state_sidecar,
    );
    let root = store
        .current_state_qmdb_root()
        .await
        .map_err(|error| SequencerError::Persistence(error.to_string()))?
        .ok_or_else(|| {
            SequencerError::ProofUnavailable("no committed state qMDB root".to_string())
        })?;

    if root.root != witness.header.state_root {
        return Err(SequencerError::ProofUnavailable(
            "committed state qMDB root does not match witness header state_root".to_string(),
        ));
    }

    let mut leaf_proofs = Vec::with_capacity(leaves.len());
    for (index, (key, value)) in leaves.iter().enumerate() {
        let proof = store
            .state_qmdb_leaf_proof(root.slot, key)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                SequencerError::ProofUnavailable(format!(
                    "missing state qMDB leaf proof at sorted leaf index {index}"
                ))
            })?;

        if proof.root != root.root || proof.slot != root.slot {
            return Err(SequencerError::Persistence(format!(
                "state qMDB leaf proof at sorted leaf index {index} came from a different root"
            )));
        }
        if &proof.leaf_key != key || &proof.leaf_value != value {
            return Err(SequencerError::Persistence(format!(
                "state qMDB leaf proof at sorted leaf index {index} does not match the witness leaf"
            )));
        }
        if !proof.verify() {
            return Err(SequencerError::ProofUnavailable(format!(
                "state qMDB leaf proof at sorted leaf index {index} failed native verification"
            )));
        }

        leaf_proofs.push(convert_key_value_proof(proof.proof_parts()));
    }

    Ok(sybil_zk::StateTransitionGuestInput {
        public_inputs: sybil_zk::public_inputs_from_witness(&witness),
        witness,
        state_root_proof: sybil_zk::QmdbStateRootProof { leaf_proofs },
    })
}

fn convert_key_value_proof(proof: QmdbStateKeyValueProofParts) -> sybil_zk::QmdbStateKeyValueProof {
    sybil_zk::QmdbStateKeyValueProof {
        operation: convert_operation_proof(proof.operation),
        next_key: proof.next_key,
    }
}

fn convert_operation_proof(
    proof: QmdbStateOperationProofParts,
) -> sybil_zk::QmdbStateOperationProof {
    sybil_zk::QmdbStateOperationProof {
        location: proof.location,
        activity_chunk: proof.activity_chunk,
        range: convert_range_proof(proof.range),
    }
}

fn convert_range_proof(proof: QmdbStateRangeProofParts) -> sybil_zk::QmdbStateRangeProof {
    sybil_zk::QmdbStateRangeProof {
        leaves: proof.leaves,
        digests: proof.digests,
        pre_prefix_acc: proof.pre_prefix_acc,
        unfolded_prefix_peaks: proof.unfolded_prefix_peaks,
        partial_chunk_digest: proof.partial_chunk_digest,
        ops_root: proof.ops_root,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use matching_engine::{outcome_buy, outcome_sell, MarketSet, NANOS_PER_DOLLAR};
    use sybil_oracle::AdminOracle;

    use super::*;
    use crate::account::AccountStore;
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sybil-{prefix}-{}-{unique}.redb",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn builds_openvm_guest_input_for_committed_block() {
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

        let path = temp_db_path("zk-guest-input");
        let store = Store::open(&path).unwrap();
        store.save_block(sequencer.snapshot()).await.unwrap();

        let input = build_state_transition_guest_input(&store, production.witness)
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
