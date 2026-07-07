use std::fmt;

use serde::{Deserialize, Serialize};
use sha3::{Digest as _, Keccak256};
use sybil_l1_protocol::DepositLeaf;
use sybil_verifier::{
    commitments::witness_schema, BlockWitness, L1DepositWitness, SystemEventWitness,
    VerificationResult,
};

mod guest_commitments;
mod header_hash {
    use sybil_verifier::WitnessBlockHeader;

    include!("header_hash_impl.rs");
}

pub use guest_commitments::{
    compute_events_root, events_root_from_event_bytes, verify_qmdb_key_value_proof,
    verify_qmdb_state_root, QmdbStateExclusionProof, QmdbStateKeyValueProof,
    QmdbStateOperationProof, QmdbStateRangeProof, QmdbStateRootProof, QMDB_STATE_CHUNK_SIZE,
};
pub use header_hash::hash_header;

pub const STATE_TRANSITION_DOMAIN: &[u8] = b"sybil/openvm/state-transition/v1";
pub const WITNESS_ROOT_DOMAIN: &[u8] = b"sybil/witness";
pub const DA_COMMITMENT_DOMAIN: &[u8] = b"sybil/da-commitment/v1";
pub const DA_WITNESS_PAYLOAD_DOMAIN: &[u8] = b"sybil/da/witness-payload/v1";
pub const DA_EMPTY_PROVIDER_REFS_DOMAIN: &[u8] = b"sybil/da/provider-refs/empty/v1";
pub const DA_PROVIDER_REFS_DOMAIN: &[u8] = b"sybil/da/provider-refs/v1";
pub const BRIDGE_ACCOUNT_KEY_DOMAIN: &[u8] = b"sybil/bridge/account-key/v1";
pub const NANOS_PER_TOKEN_UNIT: u64 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaCommitmentComponents {
    pub block_height: u64,
    pub state_root: [u8; 32],
    pub witness_root: [u8; 32],
    pub payload_root: [u8; 32],
    pub payload_len: u64,
    pub provider_refs_hash: [u8; 32],
    pub da_commitment: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransitionPublicInputs {
    pub previous_height: u64,
    pub new_height: u64,
    pub previous_state_root: [u8; 32],
    pub new_state_root: [u8; 32],
    pub block_hash: [u8; 32],
    pub events_root: [u8; 32],
    pub witness_root: [u8; 32],
    pub da_commitment: [u8; 32],
    pub deposit_root: [u8; 32],
    pub deposit_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateTransitionGuestInput {
    pub public_inputs: StateTransitionPublicInputs,
    pub witness: BlockWitness,
    pub da_provider_refs: Vec<Vec<u8>>,
    pub state_root_proof: QmdbStateRootProof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZkTransitionError {
    HeaderHeightMismatch {
        expected: u64,
        actual: u64,
    },
    StateRootMismatch,
    EventsRootMismatch,
    EventsRootComputationFailed,
    BlockHashMismatch,
    PreviousHeightMismatch {
        expected: u64,
        actual: u64,
    },
    PreviousStateRootMismatch,
    ParentHashMismatch,
    NonMonotonicHeight {
        previous: u64,
        new: u64,
    },
    OrderCountMismatch {
        expected: u32,
        actual: u32,
    },
    FillCountMismatch {
        expected: u32,
        actual: u32,
    },
    DepositRootMismatch,
    DepositCountMismatch {
        expected: u64,
        actual: u64,
    },
    DepositDeltaLengthMismatch {
        expected: u64,
        actual: usize,
    },
    DepositDeltaIdMismatch {
        index: usize,
        expected: u64,
        actual: u64,
    },
    DepositFrontierRootMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    DepositEventCountExceedsCursor {
        cursor: u64,
        events: usize,
    },
    DepositEventIdMismatch {
        expected: u64,
        actual: u64,
    },
    DepositEventMissingFromLog {
        deposit_id: u64,
    },
    DepositEventRootMismatch {
        deposit_id: u64,
        expected: [u8; 32],
        actual: [u8; 32],
    },
    DepositEventAccountKeyMismatch {
        account_id: u64,
        expected: [u8; 32],
        actual: [u8; 32],
    },
    DepositEventAmountMismatch {
        deposit_id: u64,
        expected: i64,
        actual: i64,
    },
    StateRootProofCountMismatch {
        expected: usize,
        actual: usize,
    },
    DuplicateStateLeafKey {
        index: usize,
    },
    StateRootProofVerificationFailed {
        index: usize,
    },
    StateRootNextKeyMismatch {
        index: usize,
    },
    WitnessRootMismatch,
    DaCommitmentMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    VerificationLayerFailed {
        layer: &'static str,
        violations: usize,
    },
}

impl fmt::Display for ZkTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZkTransitionError::HeaderHeightMismatch { expected, actual } => {
                write!(
                    f,
                    "header height mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::StateRootMismatch => write!(f, "state root mismatch"),
            ZkTransitionError::EventsRootMismatch => write!(f, "events root mismatch"),
            ZkTransitionError::EventsRootComputationFailed => {
                write!(f, "events root computation failed")
            }
            ZkTransitionError::BlockHashMismatch => write!(f, "block hash mismatch"),
            ZkTransitionError::PreviousHeightMismatch { expected, actual } => {
                write!(
                    f,
                    "previous height mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::PreviousStateRootMismatch => {
                write!(f, "previous state root mismatch")
            }
            ZkTransitionError::ParentHashMismatch => write!(f, "parent hash mismatch"),
            ZkTransitionError::NonMonotonicHeight { previous, new } => {
                write!(f, "non-monotonic height: previous {previous}, new {new}")
            }
            ZkTransitionError::OrderCountMismatch { expected, actual } => {
                write!(f, "order count mismatch: expected {expected}, got {actual}")
            }
            ZkTransitionError::FillCountMismatch { expected, actual } => {
                write!(f, "fill count mismatch: expected {expected}, got {actual}")
            }
            ZkTransitionError::DepositRootMismatch => write!(f, "deposit root mismatch"),
            ZkTransitionError::DepositCountMismatch { expected, actual } => {
                write!(
                    f,
                    "deposit count mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::DepositDeltaLengthMismatch { expected, actual } => {
                write!(
                    f,
                    "deposit delta length mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::DepositDeltaIdMismatch {
                index,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "deposit delta id mismatch at index {index}: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::DepositFrontierRootMismatch { expected, actual } => {
                write!(
                    f,
                    "deposit frontier root mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            ZkTransitionError::DepositEventCountExceedsCursor { cursor, events } => {
                write!(
                    f,
                    "deposit event count {events} exceeds committed cursor {cursor}"
                )
            }
            ZkTransitionError::DepositEventIdMismatch { expected, actual } => {
                write!(
                    f,
                    "deposit event id mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::DepositEventMissingFromLog { deposit_id } => {
                write!(f, "deposit event id {deposit_id} missing from log witness")
            }
            ZkTransitionError::DepositEventRootMismatch {
                deposit_id,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "deposit event root mismatch for id {deposit_id}: expected {expected:?}, got {actual:?}"
                )
            }
            ZkTransitionError::DepositEventAccountKeyMismatch {
                account_id,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "deposit account key mismatch for account {account_id}: expected {expected:?}, got {actual:?}"
                )
            }
            ZkTransitionError::DepositEventAmountMismatch {
                deposit_id,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "deposit event amount mismatch for id {deposit_id}: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::StateRootProofCountMismatch { expected, actual } => {
                write!(
                    f,
                    "state root proof count mismatch: expected {expected}, got {actual}"
                )
            }
            ZkTransitionError::DuplicateStateLeafKey { index } => {
                write!(f, "duplicate state leaf key at sorted index {index}")
            }
            ZkTransitionError::StateRootProofVerificationFailed { index } => {
                write!(f, "state root qMDB proof failed at leaf index {index}")
            }
            ZkTransitionError::StateRootNextKeyMismatch { index } => {
                write!(f, "state root qMDB next-key mismatch at leaf index {index}")
            }
            ZkTransitionError::WitnessRootMismatch => write!(f, "witness root mismatch"),
            ZkTransitionError::DaCommitmentMismatch { expected, actual } => {
                write!(
                    f,
                    "DA commitment mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            ZkTransitionError::VerificationLayerFailed { layer, violations } => {
                write!(
                    f,
                    "{layer} verification failed with {violations} violations"
                )
            }
        }
    }
}

impl std::error::Error for ZkTransitionError {}

pub fn verify_state_transition_input(
    input: &StateTransitionGuestInput,
) -> Result<[u8; 32], ZkTransitionError> {
    verify_public_input_binding(
        &input.public_inputs,
        &input.witness,
        &input.da_provider_refs,
    )?;
    verify_qmdb_state_root(
        &input.public_inputs.new_state_root,
        &input.witness,
        &input.state_root_proof,
    )?;
    ensure_valid("match", sybil_verifier::verify_match(&input.witness, false))?;
    ensure_valid(
        "settlement",
        sybil_verifier::verify_settlement(&input.witness),
    )?;
    ensure_valid("orders", sybil_verifier::verify_orders(&input.witness))?;
    ensure_valid("sidecar", sybil_verifier::verify_sidecar(&input.witness))?;
    Ok(state_transition_public_input_hash(&input.public_inputs))
}

pub fn state_transition_public_input_hash(inputs: &StateTransitionPublicInputs) -> [u8; 32] {
    keccak256(&abi_encode_domain_and_words(
        STATE_TRANSITION_DOMAIN,
        &[
            AbiWord::Uint(inputs.previous_height),
            AbiWord::Uint(inputs.new_height),
            AbiWord::Bytes32(inputs.previous_state_root),
            AbiWord::Bytes32(inputs.new_state_root),
            AbiWord::Bytes32(inputs.block_hash),
            AbiWord::Bytes32(inputs.events_root),
            AbiWord::Bytes32(inputs.witness_root),
            AbiWord::Bytes32(inputs.da_commitment),
            AbiWord::Bytes32(inputs.deposit_root),
            AbiWord::Uint(inputs.deposit_count),
        ],
    ))
}

pub fn witness_root(witness: &BlockWitness) -> [u8; 32] {
    let witness_bytes = da_witness_payload_bytes(witness);
    witness_root_from_bytes(&witness_bytes)
}

fn witness_root_from_bytes(witness_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(WITNESS_ROOT_DOMAIN);
    hasher.update(witness_bytes);
    *hasher.finalize().as_bytes()
}

pub fn da_commitment(witness: &BlockWitness) -> [u8; 32] {
    da_commitment_components(witness).da_commitment
}

pub fn da_commitment_with_provider_refs(
    witness: &BlockWitness,
    provider_refs: &[Vec<u8>],
) -> [u8; 32] {
    da_commitment_components_with_provider_refs(witness, provider_refs).da_commitment
}

pub fn da_witness_payload_bytes(witness: &BlockWitness) -> Vec<u8> {
    witness_schema::canonical_witness_bytes(witness)
}

pub fn da_commitment_components(witness: &BlockWitness) -> DaCommitmentComponents {
    da_commitment_components_with_provider_refs(witness, &[])
}

pub fn da_commitment_components_with_provider_refs(
    witness: &BlockWitness,
    provider_refs: &[Vec<u8>],
) -> DaCommitmentComponents {
    let witness_bytes = da_witness_payload_bytes(witness);
    da_commitment_components_from_payload_and_provider_refs(witness, &witness_bytes, provider_refs)
}

pub fn da_commitment_components_from_payload(
    witness: &BlockWitness,
    witness_bytes: &[u8],
) -> DaCommitmentComponents {
    da_commitment_components_from_payload_and_provider_refs(witness, witness_bytes, &[])
}

pub fn da_commitment_components_from_payload_and_provider_refs(
    witness: &BlockWitness,
    witness_bytes: &[u8],
    provider_refs: &[Vec<u8>],
) -> DaCommitmentComponents {
    let witness_root = witness_root_from_bytes(witness_bytes);
    let payload_root = da_witness_payload_root(witness_bytes);
    let payload_len = witness_bytes.len() as u64;
    let provider_refs_hash = da_provider_refs_hash(provider_refs);
    let da_commitment = da_commitment_from_parts(
        witness.header.height,
        witness.header.state_root,
        witness_root,
        payload_root,
        payload_len,
        provider_refs_hash,
    );
    DaCommitmentComponents {
        block_height: witness.header.height,
        state_root: witness.header.state_root,
        witness_root,
        payload_root,
        payload_len,
        provider_refs_hash,
        da_commitment,
    }
}

pub fn da_witness_payload_root(witness_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_WITNESS_PAYLOAD_DOMAIN);
    hasher.update(&(witness_bytes.len() as u64).to_le_bytes());
    hasher.update(witness_bytes);
    *hasher.finalize().as_bytes()
}

pub fn empty_da_provider_refs_hash() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_EMPTY_PROVIDER_REFS_DOMAIN);
    *hasher.finalize().as_bytes()
}

pub fn da_provider_refs_hash(provider_refs: &[Vec<u8>]) -> [u8; 32] {
    if provider_refs.is_empty() {
        return empty_da_provider_refs_hash();
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_PROVIDER_REFS_DOMAIN);
    hasher.update(&(provider_refs.len() as u64).to_le_bytes());
    for provider_ref in provider_refs {
        hasher.update(&(provider_ref.len() as u64).to_le_bytes());
        hasher.update(provider_ref);
    }
    *hasher.finalize().as_bytes()
}

pub fn da_commitment_from_parts(
    block_height: u64,
    state_root: [u8; 32],
    witness_root: [u8; 32],
    payload_root: [u8; 32],
    payload_len: u64,
    provider_refs_hash: [u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_COMMITMENT_DOMAIN);
    hasher.update(&block_height.to_le_bytes());
    hasher.update(&state_root);
    hasher.update(&witness_root);
    hasher.update(&payload_root);
    hasher.update(&payload_len.to_le_bytes());
    hasher.update(&provider_refs_hash);
    *hasher.finalize().as_bytes()
}

pub fn public_inputs_from_witness(witness: &BlockWitness) -> StateTransitionPublicInputs {
    public_inputs_from_witness_and_provider_refs(witness, &[])
}

pub fn public_inputs_from_witness_and_provider_refs(
    witness: &BlockWitness,
    provider_refs: &[Vec<u8>],
) -> StateTransitionPublicInputs {
    let (previous_height, previous_state_root) = match &witness.previous_header {
        Some(previous) => (previous.height, previous.state_root),
        None => (0, [0u8; 32]),
    };
    let components = da_commitment_components_with_provider_refs(witness, provider_refs);

    StateTransitionPublicInputs {
        previous_height,
        new_height: witness.header.height,
        previous_state_root,
        new_state_root: witness.header.state_root,
        block_hash: hash_header(&witness.header),
        events_root: witness.header.events_root,
        witness_root: components.witness_root,
        da_commitment: components.da_commitment,
        deposit_root: witness.state_sidecar.bridge.deposit_root,
        deposit_count: witness.state_sidecar.bridge.deposit_cursor,
    }
}

fn verify_public_input_binding(
    inputs: &StateTransitionPublicInputs,
    witness: &BlockWitness,
    provider_refs: &[Vec<u8>],
) -> Result<(), ZkTransitionError> {
    if inputs.new_height != witness.header.height {
        return Err(ZkTransitionError::HeaderHeightMismatch {
            expected: inputs.new_height,
            actual: witness.header.height,
        });
    }
    if inputs.new_state_root != witness.header.state_root {
        return Err(ZkTransitionError::StateRootMismatch);
    }
    if inputs.events_root != witness.header.events_root {
        return Err(ZkTransitionError::EventsRootMismatch);
    }
    let expected_events_root =
        compute_events_root(witness).ok_or(ZkTransitionError::EventsRootComputationFailed)?;
    if witness.header.events_root != expected_events_root {
        return Err(ZkTransitionError::EventsRootMismatch);
    }
    if inputs.block_hash != hash_header(&witness.header) {
        return Err(ZkTransitionError::BlockHashMismatch);
    }
    if inputs.witness_root != witness_root(witness) {
        return Err(ZkTransitionError::WitnessRootMismatch);
    }
    let expected_da_commitment = da_commitment_with_provider_refs(witness, provider_refs);
    if inputs.da_commitment != expected_da_commitment {
        return Err(ZkTransitionError::DaCommitmentMismatch {
            expected: expected_da_commitment,
            actual: inputs.da_commitment,
        });
    }

    let (previous_height, previous_state_root, expected_parent_hash) =
        match &witness.previous_header {
            Some(previous) => (previous.height, previous.state_root, hash_header(previous)),
            None => (0, [0u8; 32], [0u8; 32]),
        };
    if inputs.previous_height != previous_height {
        return Err(ZkTransitionError::PreviousHeightMismatch {
            expected: inputs.previous_height,
            actual: previous_height,
        });
    }
    if inputs.previous_state_root != previous_state_root {
        return Err(ZkTransitionError::PreviousStateRootMismatch);
    }
    if witness.header.parent_hash != expected_parent_hash {
        return Err(ZkTransitionError::ParentHashMismatch);
    }
    if inputs.new_height <= previous_height {
        return Err(ZkTransitionError::NonMonotonicHeight {
            previous: previous_height,
            new: inputs.new_height,
        });
    }

    let expected_order_count = (witness.orders.len() + witness.rejections.len()) as u32;
    if witness.header.order_count != expected_order_count {
        return Err(ZkTransitionError::OrderCountMismatch {
            expected: expected_order_count,
            actual: witness.header.order_count,
        });
    }
    let expected_fill_count = witness.fills.len() as u32;
    if witness.header.fill_count != expected_fill_count {
        return Err(ZkTransitionError::FillCountMismatch {
            expected: expected_fill_count,
            actual: witness.header.fill_count,
        });
    }

    if inputs.deposit_root != witness.state_sidecar.bridge.deposit_root {
        return Err(ZkTransitionError::DepositRootMismatch);
    }
    if inputs.deposit_count != witness.state_sidecar.bridge.deposit_cursor {
        return Err(ZkTransitionError::DepositCountMismatch {
            expected: witness.state_sidecar.bridge.deposit_cursor,
            actual: inputs.deposit_count,
        });
    }
    verify_l1_deposit_checkpoint(inputs, witness)?;

    Ok(())
}

fn verify_l1_deposit_checkpoint(
    inputs: &StateTransitionPublicInputs,
    witness: &BlockWitness,
) -> Result<(), ZkTransitionError> {
    let accumulator = &witness.deposit_accumulator;
    if accumulator.pre_count != witness.pre_state_sidecar.bridge.deposit_cursor {
        return Err(ZkTransitionError::DepositCountMismatch {
            expected: witness.pre_state_sidecar.bridge.deposit_cursor,
            actual: accumulator.pre_count,
        });
    }
    let pre_root = sybil_l1_protocol::deposit_root_from_frontier(
        &accumulator.pre_frontier,
        accumulator.pre_count,
    )
    .ok_or(ZkTransitionError::DepositCountMismatch {
        expected: sybil_l1_protocol::deposit_tree_capacity(),
        actual: accumulator.pre_count,
    })?;
    if pre_root != witness.pre_state_sidecar.bridge.deposit_root {
        return Err(ZkTransitionError::DepositFrontierRootMismatch {
            expected: witness.pre_state_sidecar.bridge.deposit_root,
            actual: pre_root,
        });
    }

    let expected_post_count = accumulator
        .pre_count
        .checked_add(accumulator.new_deposits.len() as u64)
        .ok_or(ZkTransitionError::DepositCountMismatch {
            expected: inputs.deposit_count,
            actual: u64::MAX,
        })?;
    if expected_post_count != inputs.deposit_count {
        return Err(ZkTransitionError::DepositCountMismatch {
            expected: expected_post_count,
            actual: inputs.deposit_count,
        });
    }

    for (index, deposit) in accumulator.new_deposits.iter().enumerate() {
        let expected = accumulator.pre_count + index as u64 + 1;
        if deposit.deposit_id != expected {
            return Err(ZkTransitionError::DepositDeltaIdMismatch {
                index,
                expected,
                actual: deposit.deposit_id,
            });
        }
    }

    let leaves = witness
        .deposit_accumulator
        .new_deposits
        .iter()
        .map(deposit_leaf_from_witness)
        .collect::<Vec<_>>();
    let prefix_roots = sybil_l1_protocol::deposit_frontier_prefix_roots(
        &accumulator.pre_frontier,
        accumulator.pre_count,
        &leaves,
    )
    .ok_or(ZkTransitionError::DepositCountMismatch {
        expected: sybil_l1_protocol::deposit_tree_capacity(),
        actual: inputs.deposit_count,
    })?;
    let computed_root = prefix_roots.last().copied().unwrap_or(pre_root);
    if computed_root != inputs.deposit_root {
        return Err(ZkTransitionError::DepositFrontierRootMismatch {
            expected: inputs.deposit_root,
            actual: computed_root,
        });
    }

    for (index, deposit) in accumulator.new_deposits.iter().enumerate() {
        let expected = prefix_roots[index];
        if deposit.deposit_root != expected {
            return Err(ZkTransitionError::DepositEventRootMismatch {
                deposit_id: deposit.deposit_id,
                expected,
                actual: deposit.deposit_root,
            });
        }
    }

    let credited_events = witness
        .system_events
        .iter()
        .filter_map(|event| match event {
            SystemEventWitness::L1Deposit {
                account_id,
                amount,
                deposit_id,
                deposit_root,
                sybil_account_key,
            } => Some((
                *account_id,
                *amount,
                *deposit_id,
                *deposit_root,
                *sybil_account_key,
            )),
            SystemEventWitness::CreateAccount { .. }
            | SystemEventWitness::Deposit { .. }
            | SystemEventWitness::WithdrawalCreated { .. }
            | SystemEventWitness::MarketResolved { .. }
            | SystemEventWitness::OrderCancelled { .. }
            | SystemEventWitness::MarketGroupExtended { .. } => None,
        })
        .collect::<Vec<_>>();

    if credited_events.len() != accumulator.new_deposits.len() {
        return Err(ZkTransitionError::DepositDeltaLengthMismatch {
            expected: accumulator.new_deposits.len() as u64,
            actual: credited_events.len(),
        });
    }

    for (event_index, (account_id, amount, deposit_id, deposit_root, sybil_account_key)) in
        credited_events.into_iter().enumerate()
    {
        let expected_id = accumulator.pre_count + event_index as u64 + 1;
        if deposit_id != expected_id {
            return Err(ZkTransitionError::DepositEventIdMismatch {
                expected: expected_id,
                actual: deposit_id,
            });
        }
        let Some(deposit) = accumulator.new_deposits.get(event_index) else {
            return Err(ZkTransitionError::DepositEventMissingFromLog { deposit_id });
        };

        let expected_root = prefix_roots[event_index];
        if deposit_root != expected_root {
            return Err(ZkTransitionError::DepositEventRootMismatch {
                deposit_id,
                expected: expected_root,
                actual: deposit_root,
            });
        }

        let expected_key = bridge_account_key(account_id);
        if sybil_account_key != expected_key {
            return Err(ZkTransitionError::DepositEventAccountKeyMismatch {
                account_id,
                expected: expected_key,
                actual: sybil_account_key,
            });
        }
        if deposit.sybil_account_key != expected_key {
            return Err(ZkTransitionError::DepositEventAccountKeyMismatch {
                account_id,
                expected: expected_key,
                actual: deposit.sybil_account_key,
            });
        }

        let expected_amount =
            deposit_amount_nanos(deposit).ok_or(ZkTransitionError::DepositEventAmountMismatch {
                deposit_id,
                expected: i64::MAX,
                actual: amount,
            })?;
        if amount != expected_amount {
            return Err(ZkTransitionError::DepositEventAmountMismatch {
                deposit_id,
                expected: expected_amount,
                actual: amount,
            });
        }
    }

    Ok(())
}

fn deposit_leaf_from_witness(deposit: &L1DepositWitness) -> DepositLeaf {
    DepositLeaf {
        chain_id: deposit.chain_id,
        vault_address: deposit.vault_address,
        deposit_id: deposit.deposit_id,
        token_address: deposit.token_address,
        sender: deposit.sender,
        sybil_account_key: deposit.sybil_account_key,
        amount_token_units: deposit.amount_token_units,
    }
}

fn deposit_amount_nanos(deposit: &L1DepositWitness) -> Option<i64> {
    let amount = deposit
        .amount_token_units
        .checked_mul(NANOS_PER_TOKEN_UNIT)?;
    i64::try_from(amount).ok()
}

fn bridge_account_key(account_id: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(BRIDGE_ACCOUNT_KEY_DOMAIN);
    hasher.update(&account_id.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn ensure_valid(layer: &'static str, result: VerificationResult) -> Result<(), ZkTransitionError> {
    if result.valid {
        Ok(())
    } else {
        Err(ZkTransitionError::VerificationLayerFailed {
            layer,
            violations: result.violations.len(),
        })
    }
}

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

enum AbiWord {
    Uint(u64),
    Bytes32([u8; 32]),
}

fn abi_encode_domain_and_words(domain: &[u8], words: &[AbiWord]) -> Vec<u8> {
    let head_words = 1 + words.len();
    let mut out = Vec::with_capacity(head_words * 32 + 32 + padded_len(domain.len()));
    out.extend_from_slice(&abi_usize_word(head_words * 32));
    for word in words {
        match word {
            AbiWord::Uint(value) => out.extend_from_slice(&abi_u64_word(*value)),
            AbiWord::Bytes32(bytes) => out.extend_from_slice(bytes),
        }
    }

    out.extend_from_slice(&abi_usize_word(domain.len()));
    out.extend_from_slice(domain);
    out.resize(out.len() + padded_len(domain.len()) - domain.len(), 0);
    out
}

fn abi_u64_word(value: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

fn abi_usize_word(value: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&(value as u64).to_be_bytes());
    out
}

fn padded_len(len: usize) -> usize {
    len.div_ceil(32) * 32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};

    use commonware_codec::RangeCfg;
    use commonware_cryptography::{
        sha256::Digest as QmdbDigest, Hasher as _, Sha256 as QmdbSha256,
    };
    use commonware_runtime::buffer::paged::CacheRef;
    use commonware_runtime::{deterministic, Runner as _};
    use commonware_storage::journal::contiguous::variable::Config as VConfig;
    use commonware_storage::merkle::mmr::journaled::Config as MmrConfig;
    use commonware_storage::merkle::mmr::Family as MmrFamily;
    use commonware_storage::qmdb::current::ordered::variable::{
        Db as OrderedVariableDb, KeyValueProof,
    };
    use commonware_storage::qmdb::current::VariableConfig;
    use commonware_storage::translator::OneCap;
    use matching_engine::{MarketId, Order};
    use sybil_verifier::{
        commitments::{event_schema, state_schema},
        AccountReservationSnapshot, AccountSnapshot, BridgeStateSnapshot,
        DepositAccumulatorWitness, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
        StateSidecarSnapshot, SystemEventWitness, WithdrawalSnapshot, WitnessBlockHeader,
    };

    const PAGE_SIZE: u16 = 4096;
    const PAGE_CACHE_PAGES: usize = 128;
    const ITEMS_PER_BLOB: u64 = 1024;
    const WRITE_BUFFER_BYTES: usize = 64 * 1024;
    const MAX_KEY_BYTES: usize = 64;
    const MAX_VALUE_BYTES: usize = 1 << 20;

    type TestStateDb = OrderedVariableDb<
        MmrFamily,
        deterministic::Context,
        Vec<u8>,
        Vec<u8>,
        QmdbSha256,
        OneCap,
        QMDB_STATE_CHUNK_SIZE,
    >;
    type NativeKeyValueProof = KeyValueProof<MmrFamily, Vec<u8>, QmdbDigest, QMDB_STATE_CHUNK_SIZE>;

    fn empty_bridge_snapshot() -> BridgeStateSnapshot {
        BridgeStateSnapshot {
            deposit_root: sybil_l1_protocol::empty_deposit_root(),
            ..BridgeStateSnapshot::default()
        }
    }

    fn empty_state_sidecar() -> StateSidecarSnapshot {
        StateSidecarSnapshot {
            bridge: empty_bridge_snapshot(),
            ..StateSidecarSnapshot::default()
        }
    }

    fn address(byte: u8) -> [u8; 20] {
        [byte; 20]
    }

    fn l1_deposit_prefix(count: u64, account_id: u64) -> Vec<L1DepositWitness> {
        let mut deposits = (1..=count)
            .map(|deposit_id| L1DepositWitness {
                deposit_id,
                chain_id: 31_337,
                vault_address: address(0x10),
                token_address: address(0x20),
                sender: address(0x30 + deposit_id as u8),
                sybil_account_key: bridge_account_key(account_id),
                amount_token_units: 1_000 + deposit_id,
                deposit_root: [0u8; 32],
            })
            .collect::<Vec<_>>();
        let leaves = deposits
            .iter()
            .map(deposit_leaf_from_witness)
            .collect::<Vec<_>>();
        let roots = sybil_l1_protocol::deposit_prefix_roots(&leaves);
        for (deposit, root) in deposits.iter_mut().zip(roots) {
            deposit.deposit_root = root;
        }
        deposits
    }

    fn deposit_accumulator_from_prefix(
        new_deposits: Vec<L1DepositWitness>,
    ) -> DepositAccumulatorWitness {
        DepositAccumulatorWitness {
            pre_frontier: sybil_l1_protocol::empty_deposit_frontier(),
            pre_count: 0,
            new_deposits,
        }
    }

    fn deposit_accumulator_after_prefix(prefix: &[L1DepositWitness]) -> DepositAccumulatorWitness {
        let leaves = prefix
            .iter()
            .map(deposit_leaf_from_witness)
            .collect::<Vec<_>>();
        DepositAccumulatorWitness {
            pre_frontier: sybil_l1_protocol::deposit_frontier_after_prefix(
                &sybil_l1_protocol::empty_deposit_frontier(),
                0,
                &leaves,
            )
            .expect("test prefix fits deposit tree"),
            pre_count: prefix.len() as u64,
            new_deposits: vec![],
        }
    }

    #[test]
    fn hash_header_golden_vector() {
        let header = WitnessBlockHeader {
            height: 11,
            parent_hash: [4u8; 32],
            state_root: [5u8; 32],
            events_root: [6u8; 32],
            order_count: 2,
            fill_count: 1,
            timestamp_ms: 1_700_000_001_234,
        };

        assert_eq!(
            hash_header(&header),
            [
                237, 2, 52, 82, 23, 11, 241, 36, 196, 211, 229, 155, 159, 99, 198, 162, 76, 210,
                68, 96, 104, 0, 3, 235, 39, 53, 0, 15, 146, 163, 93, 242,
            ],
        );
    }

    fn empty_guest_input() -> StateTransitionGuestInput {
        let state_sidecar = empty_state_sidecar();
        let pre_state_sidecar = empty_state_sidecar();
        let leaves = state_schema::state_root_leaves(&[], &state_sidecar);
        let (state_root, state_root_proof) = state_root_and_proof(&leaves);
        let events_root = events_root_from_event_bytes(&[]).expect("empty events root");
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            events_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1000,
        };
        let witness = BlockWitness {
            header,
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: Default::default(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);
        StateTransitionGuestInput {
            public_inputs,
            witness,
            da_provider_refs: vec![],
            state_root_proof,
        }
    }

    fn non_empty_guest_input() -> StateTransitionGuestInput {
        let account = AccountSnapshot {
            id: 7,
            balance: 2_500_000_000,
            total_deposited: 3_000_000_000,
            positions: vec![(MarketId::new(3), 0, 11), (MarketId::new(3), 1, 11)],
            events_digest: [9u8; 32],
            keys_digest: sybil_verifier::empty_account_keys_digest(7),
        };
        let market = MarketSnapshot {
            market_id: MarketId::new(3),
            name: "Election test".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: state_schema::market_metadata_digest(b"metadata"),
            resolution_template: "yes/no".to_string(),
        };
        let market_group = MarketGroupSnapshot {
            group_id: 2,
            name: "Group".to_string(),
            markets: vec![MarketId::new(3)],
        };
        let withdrawal = WithdrawalSnapshot {
            withdrawal_id: 4,
            account_id: account.id,
            recipient: [1u8; 20],
            token: [2u8; 20],
            amount_token_units: 50,
            amount_nanos: 50_000_000,
            expiry_height: 20,
            nullifier: [3u8; 32],
        };
        let mut order = Order::new(44);
        order.markets[0] = MarketId::new(3);
        order.num_markets = 1;
        order.num_states = 2;
        order.payoffs[0] = 1;
        order.limit_price = matching_engine::Nanos(600_000_000);
        order.max_fill = matching_engine::Qty(12);
        order.expires_at_block = Some(10);

        let l1_deposits = l1_deposit_prefix(5, account.id);
        let state_sidecar = StateSidecarSnapshot {
            bridge: BridgeStateSnapshot {
                deposit_cursor: 5,
                deposit_root: l1_deposits.last().expect("non-empty prefix").deposit_root,
                next_withdrawal_id: 5,
                withdrawals: vec![withdrawal],
            },
            markets: vec![market],
            market_groups: vec![market_group],
            resting_orders: vec![sybil_verifier::RestingOrderSnapshot {
                order,
                account_id: account.id,
                created_at: 1,
                expires_at_block: 10,
                reserved_balance: 120_000_000,
                reserved_positions: vec![(MarketId::new(3), 0, 2)],
            }],
            account_reservations: vec![AccountReservationSnapshot {
                account_id: account.id,
                reserved_balance: 120_000_000,
                reserved_positions: vec![(MarketId::new(3), 0, 2)],
            }],
        };
        let pre_state_sidecar = state_sidecar.clone();
        let deposit_accumulator = deposit_accumulator_after_prefix(&l1_deposits);

        let post_state = vec![account];
        let leaves = state_schema::state_root_leaves(&post_state, &state_sidecar);
        let (state_root, state_root_proof) = state_root_and_proof(&leaves);
        let events_root = events_root_from_event_bytes(&[]).expect("empty events root");
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            events_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1000,
        };
        let witness = BlockWitness {
            header,
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator,
            fills: vec![],
            clearing_prices: Default::default(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: post_state.clone(),
            post_system_state: post_state.clone(),
            post_state,
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);
        StateTransitionGuestInput {
            public_inputs,
            witness,
            da_provider_refs: vec![],
            state_root_proof,
        }
    }

    fn many_account_guest_input(account_count: u64) -> StateTransitionGuestInput {
        let state_sidecar = empty_state_sidecar();
        let pre_state_sidecar = empty_state_sidecar();
        let post_state = (0..account_count)
            .map(|id| AccountSnapshot {
                id,
                balance: 1_000_000_000 + id as i64,
                total_deposited: 1_000_000_000 + id as i64,
                positions: vec![],
                events_digest: [id as u8; 32],
                keys_digest: sybil_verifier::empty_account_keys_digest(id),
            })
            .collect::<Vec<_>>();
        let leaves = state_schema::state_root_leaves(&post_state, &state_sidecar);
        let (state_root, state_root_proof) = state_root_and_proof(&leaves);
        let events_root = events_root_from_event_bytes(&[]).expect("empty events root");
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            events_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1000,
        };
        let witness = BlockWitness {
            header,
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: Default::default(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: post_state.clone(),
            post_system_state: post_state.clone(),
            post_state,
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);

        StateTransitionGuestInput {
            public_inputs,
            witness,
            da_provider_refs: vec![],
            state_root_proof,
        }
    }

    fn recompute_roots_and_public_inputs(input: &mut StateTransitionGuestInput) {
        let leaves = state_schema::state_root_leaves(
            &input.witness.post_state,
            &input.witness.state_sidecar,
        );
        let (state_root, state_root_proof) = state_root_and_proof(&leaves);
        input.witness.header.state_root = state_root;
        input.state_root_proof = state_root_proof;

        let events = event_schema::event_leaf_values(
            &input.witness.system_events,
            &input.witness.orders,
            &input.witness.rejections,
            &input.witness.fills,
        );
        input.witness.header.events_root =
            events_root_from_event_bytes(&events).expect("event root");
        input.public_inputs = public_inputs_from_witness(&input.witness);
    }

    fn l1_deposit_guest_input() -> StateTransitionGuestInput {
        let mut input = empty_guest_input();
        let account_id = 7;
        let mut deposits = l1_deposit_prefix(1, account_id);
        let deposit = deposits.pop().expect("one deposit");
        let amount = deposit_amount_nanos(&deposit).expect("small deposit amount");
        input.witness.system_events = vec![SystemEventWitness::L1Deposit {
            account_id,
            amount,
            deposit_id: deposit.deposit_id,
            deposit_root: deposit.deposit_root,
            sybil_account_key: deposit.sybil_account_key,
        }];
        input.witness.deposit_accumulator = deposit_accumulator_from_prefix(vec![deposit.clone()]);
        input.witness.state_sidecar.bridge.deposit_cursor = deposit.deposit_id;
        input.witness.state_sidecar.bridge.deposit_root = deposit.deposit_root;
        recompute_roots_and_public_inputs(&mut input);
        input
    }

    fn split_frontier_l1_deposit_guest_input() -> StateTransitionGuestInput {
        let mut input = empty_guest_input();
        let account_id = 7;
        let deposits = l1_deposit_prefix(3, account_id);
        let leaves = deposits
            .iter()
            .map(deposit_leaf_from_witness)
            .collect::<Vec<_>>();
        let prefix_roots = sybil_l1_protocol::deposit_prefix_roots(&leaves);
        let pre_frontier = sybil_l1_protocol::deposit_frontier_after_prefix(
            &sybil_l1_protocol::empty_deposit_frontier(),
            0,
            &leaves[..2],
        )
        .expect("test prefix fits deposit tree");
        assert_eq!(
            sybil_l1_protocol::deposit_frontier_prefix_roots(&pre_frontier, 2, &leaves[2..])
                .expect("test delta fits deposit tree"),
            vec![prefix_roots[2]]
        );

        let deposit = deposits[2].clone();
        let amount = deposit_amount_nanos(&deposit).expect("small deposit amount");
        input.witness.pre_state_sidecar.bridge.deposit_cursor = 2;
        input.witness.pre_state_sidecar.bridge.deposit_root = prefix_roots[1];
        input.witness.system_events = vec![SystemEventWitness::L1Deposit {
            account_id,
            amount,
            deposit_id: deposit.deposit_id,
            deposit_root: deposit.deposit_root,
            sybil_account_key: deposit.sybil_account_key,
        }];
        input.witness.deposit_accumulator = DepositAccumulatorWitness {
            pre_frontier,
            pre_count: 2,
            new_deposits: vec![deposit.clone()],
        };
        input.witness.state_sidecar.bridge.deposit_cursor = deposit.deposit_id;
        input.witness.state_sidecar.bridge.deposit_root = deposit.deposit_root;
        recompute_roots_and_public_inputs(&mut input);
        input
    }

    fn state_root_and_proof(leaves: &[(Vec<u8>, Vec<u8>)]) -> ([u8; 32], QmdbStateRootProof) {
        let proof_keys = leaves
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        state_root_and_selected_proof(leaves, &proof_keys)
    }

    fn state_root_and_selected_proof(
        leaves: &[(Vec<u8>, Vec<u8>)],
        proof_keys: &[Vec<u8>],
    ) -> ([u8; 32], QmdbStateRootProof) {
        let leaves = leaves.to_vec();
        let proof_keys = proof_keys.to_vec();
        deterministic::Runner::default().start(|context| async move {
            let mut db = open_test_state_db(context).await;
            if !leaves.is_empty() {
                let mut batch = db.new_batch();
                for (key, value) in leaves.iter().cloned() {
                    batch = batch.write(key, Some(value));
                }
                let merkleized = batch.merkleize(&db, None).await.unwrap();
                db.apply_batch(merkleized).await.unwrap();
            }

            let root = db.root().0;
            let mut leaf_proofs = Vec::with_capacity(proof_keys.len());
            for key in &proof_keys {
                let mut hasher = QmdbSha256::new();
                let proof = db.key_value_proof(&mut hasher, key.clone()).await.unwrap();
                leaf_proofs.push(qmdb_proof_parts(&proof));
            }
            (root, QmdbStateRootProof { leaf_proofs })
        })
    }

    async fn open_test_state_db(context: deterministic::Context) -> TestStateDb {
        let page_cache = CacheRef::from_pooler(
            &context,
            NonZeroU16::new(PAGE_SIZE).unwrap(),
            NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
        );
        let config = VariableConfig {
            merkle_config: MmrConfig {
                journal_partition: "test-state-mmr-journal".to_string(),
                items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
                write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
                metadata_partition: "test-state-mmr-metadata".to_string(),
                thread_pool: None,
                page_cache: page_cache.clone(),
            },
            journal_config: VConfig {
                partition: "test-state-log".to_string(),
                write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
                compression: None,
                codec_config: (
                    (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                    (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
                ),
                items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
                page_cache,
            },
            grafted_metadata_partition: "test-state-grafted-mmr-metadata".to_string(),
            translator: OneCap,
        };

        TestStateDb::init(context, config).await.unwrap()
    }

    fn qmdb_proof_parts(proof: &NativeKeyValueProof) -> QmdbStateKeyValueProof {
        QmdbStateKeyValueProof {
            operation: QmdbStateOperationProof {
                location: u64::from(proof.proof.loc),
                activity_chunk: proof.proof.chunk,
                range: QmdbStateRangeProof {
                    leaves: u64::from(proof.proof.range_proof.proof.leaves),
                    digests: proof
                        .proof
                        .range_proof
                        .proof
                        .digests
                        .iter()
                        .copied()
                        .map(digest_bytes)
                        .collect(),
                    pre_prefix_acc: proof.proof.range_proof.pre_prefix_acc.map(digest_bytes),
                    unfolded_prefix_peaks: proof
                        .proof
                        .range_proof
                        .unfolded_prefix_peaks
                        .iter()
                        .copied()
                        .map(digest_bytes)
                        .collect(),
                    partial_chunk_digest: proof
                        .proof
                        .range_proof
                        .partial_chunk_digest
                        .map(digest_bytes),
                    ops_root: digest_bytes(proof.proof.range_proof.ops_root),
                },
            },
            next_key: proof.next_key.clone(),
        }
    }

    fn digest_bytes(digest: QmdbDigest) -> [u8; 32] {
        digest.0
    }

    #[test]
    fn empty_transition_verifies() {
        let input = empty_guest_input();
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn non_empty_transition_verifies_state_root_keyspace() {
        let input = non_empty_guest_input();
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn l1_deposit_transition_verifies_reconstructed_root() {
        let input = l1_deposit_guest_input();
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn l1_deposit_transition_verifies_split_frontier_delta() {
        let input = split_frontier_l1_deposit_guest_input();
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn forged_l1_deposit_credit_not_in_reconstructed_root_fails() {
        let mut input = l1_deposit_guest_input();
        let SystemEventWitness::L1Deposit { amount, .. } =
            input.witness.system_events.first_mut().expect("l1 deposit")
        else {
            panic!("expected l1 deposit event");
        };
        *amount += NANOS_PER_TOKEN_UNIT as i64;
        recompute_roots_and_public_inputs(&mut input);

        assert!(matches!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::DepositEventAmountMismatch { .. })
        ));
    }

    #[test]
    fn large_transition_verifies_complete_bitmap_chunk() {
        let input = many_account_guest_input(300);
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn public_input_hash_golden() {
        let input = empty_guest_input();
        assert_eq!(
            state_transition_public_input_hash(&input.public_inputs),
            [
                64, 57, 123, 28, 177, 17, 224, 212, 38, 229, 155, 66, 90, 206, 225, 133, 166, 21,
                63, 166, 234, 180, 190, 245, 236, 115, 36, 141, 224, 191, 63, 80,
            ]
        );
    }

    #[test]
    fn state_transition_public_input_hash_solidity_golden_vector() {
        // Twin: contracts/test/SybilGoldenVectors.t.sol. Keep these constants
        // byte-for-byte aligned with the Solidity suite.
        let inputs = StateTransitionPublicInputs {
            previous_height: 41,
            new_height: 42,
            previous_state_root: [0x10; 32],
            new_state_root: [0x20; 32],
            block_hash: [0x30; 32],
            events_root: [0x40; 32],
            witness_root: [0x50; 32],
            da_commitment: [0x60; 32],
            deposit_root: [
                93, 155, 73, 65, 157, 237, 20, 180, 127, 175, 15, 148, 49, 152, 195, 54, 71, 192,
                22, 189, 55, 249, 152, 177, 217, 25, 107, 16, 58, 207, 236, 218,
            ],
            deposit_count: 3,
        };

        assert_eq!(
            state_transition_public_input_hash(&inputs),
            [
                66, 25, 125, 13, 255, 123, 194, 248, 106, 110, 53, 159, 24, 122, 221, 161, 99, 252,
                155, 79, 250, 160, 231, 207, 185, 132, 85, 97, 187, 116, 72, 48,
            ]
        );
    }

    #[test]
    fn guest_events_root_matches_native_golden_deposit() {
        let system_events = vec![sybil_verifier::SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = event_schema::event_leaf_values(&system_events, &[], &[], &[]);

        assert_eq!(
            events_root_from_event_bytes(&events),
            Some([
                192, 49, 15, 127, 205, 199, 131, 164, 175, 240, 21, 115, 173, 61, 247, 113, 35,
                129, 44, 150, 211, 36, 13, 167, 222, 164, 46, 216, 180, 50, 124, 160,
            ])
        );
    }

    #[test]
    fn mismatched_event_root_fails_before_guest_hash() {
        let mut input = empty_guest_input();
        input.public_inputs.events_root = [9u8; 32];
        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::EventsRootMismatch)
        );
    }

    #[test]
    fn zero_witness_root_is_rejected() {
        let mut input = empty_guest_input();
        input.public_inputs.witness_root = [0u8; 32];

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::WitnessRootMismatch)
        );
    }

    #[test]
    fn public_inputs_include_nonzero_da_commitment() {
        let input = empty_guest_input();

        assert_ne!(input.public_inputs.da_commitment, [0u8; 32]);
        assert_eq!(
            input.public_inputs.da_commitment,
            da_commitment(&input.witness)
        );
    }

    #[test]
    fn da_components_match_public_inputs() {
        let input = empty_guest_input();
        let payload = da_witness_payload_bytes(&input.witness);
        let components = da_commitment_components_from_payload_and_provider_refs(
            &input.witness,
            &payload,
            &input.da_provider_refs,
        );

        assert_eq!(components.block_height, input.public_inputs.new_height);
        assert_eq!(components.state_root, input.public_inputs.new_state_root);
        assert_eq!(components.witness_root, input.public_inputs.witness_root);
        assert_eq!(components.payload_len, payload.len() as u64);
        assert_eq!(components.payload_root, da_witness_payload_root(&payload));
        assert_eq!(components.provider_refs_hash, empty_da_provider_refs_hash());
        assert_eq!(components.da_commitment, input.public_inputs.da_commitment);
    }

    #[test]
    fn provider_refs_are_bound_into_da_commitment() {
        let mut input = empty_guest_input();
        input.da_provider_refs = vec![b"file://payload".to_vec()];
        input.public_inputs =
            public_inputs_from_witness_and_provider_refs(&input.witness, &input.da_provider_refs);

        assert_ne!(
            input.public_inputs.da_commitment,
            da_commitment(&input.witness)
        );
        assert_eq!(
            verify_state_transition_input(&input),
            Ok(state_transition_public_input_hash(&input.public_inputs))
        );
    }

    #[test]
    fn provider_ref_mutation_is_rejected() {
        let mut input = empty_guest_input();
        input.da_provider_refs = vec![b"file://payload-a".to_vec()];
        input.public_inputs =
            public_inputs_from_witness_and_provider_refs(&input.witness, &input.da_provider_refs);
        input.da_provider_refs = vec![b"file://payload-b".to_vec()];

        assert!(matches!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::DaCommitmentMismatch { .. })
        ));
    }

    #[test]
    fn tampered_da_commitment_is_rejected() {
        let mut input = empty_guest_input();
        input.public_inputs.da_commitment = [7u8; 32];

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::DaCommitmentMismatch {
                expected: da_commitment(&input.witness),
                actual: [7u8; 32],
            })
        );
    }

    #[test]
    fn witness_mutation_after_public_binding_is_rejected() {
        let mut input = empty_guest_input();
        input.witness.total_welfare = 1;

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::WitnessRootMismatch)
        );
    }

    #[test]
    fn witness_event_root_mismatch_fails_even_when_public_input_matches() {
        let mut input = empty_guest_input();
        input.witness.header.events_root = [9u8; 32];
        input.public_inputs.events_root = input.witness.header.events_root;
        input.public_inputs.block_hash = hash_header(&input.witness.header);

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::EventsRootMismatch)
        );
    }

    #[test]
    fn tampered_state_root_proof_fails() {
        let mut input = empty_guest_input();
        input.state_root_proof.leaf_proofs[0].next_key.push(0);
        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofVerificationFailed { index: 0 })
        );
    }

    #[test]
    fn missing_state_root_proof_fails_count_check() {
        let mut input = empty_guest_input();
        input.state_root_proof.leaf_proofs.pop();
        let expected = state_schema::state_root_leaves(
            &input.witness.post_state,
            &input.witness.state_sidecar,
        )
        .len();

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofCountMismatch {
                expected,
                actual: expected - 1,
            })
        );
    }

    #[test]
    fn duplicate_state_leaf_key_fails_before_proof_verification() {
        let mut input = non_empty_guest_input();
        input
            .witness
            .post_state
            .push(input.witness.post_state[0].clone());
        input
            .state_root_proof
            .leaf_proofs
            .push(input.state_root_proof.leaf_proofs[0].clone());
        input.public_inputs = public_inputs_from_witness(&input.witness);

        assert!(matches!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::DuplicateStateLeafKey { .. })
        ));
    }

    #[test]
    fn reordered_state_root_proofs_fail() {
        let mut input = non_empty_guest_input();
        input.state_root_proof.leaf_proofs.swap(0, 1);

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofVerificationFailed { index: 0 })
        );
    }

    #[test]
    fn corrupted_activity_chunk_fails() {
        let mut input = non_empty_guest_input();
        input.state_root_proof.leaf_proofs[0]
            .operation
            .activity_chunk = [0u8; QMDB_STATE_CHUNK_SIZE];

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofVerificationFailed { index: 0 })
        );
    }

    #[test]
    fn wrong_committed_state_root_fails_proof_verification() {
        let mut input = non_empty_guest_input();
        input.witness.header.state_root = [0x42; 32];
        input.public_inputs = public_inputs_from_witness(&input.witness);

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofVerificationFailed { index: 0 })
        );
    }

    #[test]
    fn hidden_state_leaf_fails_next_key_ring() {
        let mut input = empty_guest_input();
        let witness_leaves = state_schema::state_root_leaves(
            &input.witness.post_state,
            &input.witness.state_sidecar,
        );
        let proof_keys = witness_leaves
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut committed_leaves = witness_leaves;
        committed_leaves.push((b"hidden/state".to_vec(), b"extra".to_vec()));

        let (root, state_root_proof) =
            state_root_and_selected_proof(&committed_leaves, &proof_keys);
        input.witness.header.state_root = root;
        input.public_inputs = public_inputs_from_witness(&input.witness);
        input.state_root_proof = state_root_proof;

        assert!(matches!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootNextKeyMismatch { .. })
        ));
    }
}
