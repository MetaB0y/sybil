use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;
use sybil_verifier::{BlockWitness, VerificationResult, WitnessBlockHeader};

pub const STATE_TRANSITION_DOMAIN: &[u8] = b"sybil/openvm/state-transition/v1";
pub const WITNESS_ROOT_DOMAIN: &[u8] = b"sybil/witness";
pub const UNIMPLEMENTED_DA_COMMITMENT: [u8; 32] = [0u8; 32];

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
    pub state_root_proof: QmdbStateRootProof,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QmdbStateRootProof {
    pub leaf_proofs: Vec<QmdbStateKeyValueProof>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QmdbStateKeyValueProof {
    pub operation: QmdbStateOperationProof,
    pub next_key: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QmdbStateOperationProof {
    pub location: u64,
    pub activity_chunk: [u8; QMDB_STATE_CHUNK_SIZE],
    pub range: QmdbStateRangeProof,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QmdbStateRangeProof {
    pub leaves: u64,
    pub digests: Vec<[u8; 32]>,
    pub pre_prefix_acc: Option<[u8; 32]>,
    pub unfolded_prefix_peaks: Vec<[u8; 32]>,
    pub partial_chunk_digest: Option<[u8; 32]>,
    pub ops_root: [u8; 32],
}

pub const QMDB_STATE_CHUNK_SIZE: usize = 32;

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
    DaCommitmentUnsupported,
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
            ZkTransitionError::DaCommitmentUnsupported => {
                write!(f, "DA commitment is not implemented yet")
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
    verify_public_input_binding(&input.public_inputs, &input.witness)?;
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
    Ok(state_transition_public_input_hash(&input.public_inputs))
}

pub fn verify_qmdb_state_root(
    root: &[u8; 32],
    witness: &BlockWitness,
    proof: &QmdbStateRootProof,
) -> Result<(), ZkTransitionError> {
    let leaves = sybil_verifier::state_schema::state_root_leaves(
        &witness.post_state,
        &witness.state_sidecar,
    );
    if proof.leaf_proofs.len() != leaves.len() {
        return Err(ZkTransitionError::StateRootProofCountMismatch {
            expected: leaves.len(),
            actual: proof.leaf_proofs.len(),
        });
    }

    for (index, window) in leaves.windows(2).enumerate() {
        if window[0].0 == window[1].0 {
            return Err(ZkTransitionError::DuplicateStateLeafKey { index });
        }
    }

    for (index, ((key, value), leaf_proof)) in leaves.iter().zip(&proof.leaf_proofs).enumerate() {
        if !verify_qmdb_key_value_proof(root, key, value, leaf_proof) {
            return Err(ZkTransitionError::StateRootProofVerificationFailed { index });
        }

        let expected_next_key = &leaves[(index + 1) % leaves.len()].0;
        if leaf_proof.next_key != *expected_next_key {
            return Err(ZkTransitionError::StateRootNextKeyMismatch { index });
        }
    }

    Ok(())
}

pub fn verify_qmdb_key_value_proof(
    root: &[u8; 32],
    key: &[u8],
    value: &[u8],
    proof: &QmdbStateKeyValueProof,
) -> bool {
    let Some(operation) = encode_qmdb_update_operation(key, value, &proof.next_key) else {
        return false;
    };
    verify_qmdb_operation_proof(root, &operation, &proof.operation)
}

fn verify_qmdb_operation_proof(
    root: &[u8; 32],
    operation: &[u8],
    proof: &QmdbStateOperationProof,
) -> bool {
    if !get_bit_from_chunk(&proof.activity_chunk, proof.location) {
        return false;
    }
    verify_qmdb_range_proof(
        root,
        &proof.range,
        proof.location,
        operation,
        &proof.activity_chunk,
    )
}

fn verify_qmdb_range_proof(
    root: &[u8; 32],
    proof: &QmdbStateRangeProof,
    start_loc: u64,
    operation: &[u8],
    chunk: &[u8; QMDB_STATE_CHUNK_SIZE],
) -> bool {
    let Some(end_loc) = start_loc.checked_add(1) else {
        return false;
    };
    if end_loc > proof.leaves {
        return false;
    }

    let chunk_bits = qmdb_chunk_bits();
    let start_chunk = start_loc / chunk_bits;
    let end_chunk = (end_loc - 1) / chunk_bits;
    let complete_chunks = proof.leaves / chunk_bits;
    if start_chunk != end_chunk {
        return false;
    }

    let next_bit = proof.leaves % chunk_bits;
    let has_partial_chunk = next_bit != 0;
    if has_partial_chunk {
        let Some(last_chunk_digest) = proof.partial_chunk_digest else {
            return false;
        };
        if end_chunk == complete_chunks {
            if last_chunk_digest != sha256([chunk.as_slice()]) {
                return false;
            }
        }
    } else if proof.partial_chunk_digest.is_some() {
        return false;
    }

    let Some(merkle_root) =
        reconstruct_qmdb_mmr_root(proof.leaves, &proof.digests, start_loc, operation, chunk)
    else {
        return false;
    };

    let mut hasher = Sha256::new();
    hasher.update(proof.ops_root);
    hasher.update(merkle_root);
    if has_partial_chunk {
        hasher.update(next_bit.to_be_bytes());
        hasher.update(proof.partial_chunk_digest.expect("checked above"));
    }
    hasher.finalize().as_slice() == root
}

fn encode_qmdb_update_operation(key: &[u8], value: &[u8], next_key: &[u8]) -> Option<Vec<u8>> {
    const UPDATE_CONTEXT: u8 = 0xD2;

    let mut out = Vec::with_capacity(
        1 + encoded_len_size(key.len())?
            + key.len()
            + encoded_len_size(value.len())?
            + value.len()
            + encoded_len_size(next_key.len())?
            + next_key.len(),
    );
    out.push(UPDATE_CONTEXT);
    append_len_prefixed_bytes(&mut out, key)?;
    append_len_prefixed_bytes(&mut out, value)?;
    append_len_prefixed_bytes(&mut out, next_key)?;
    Some(out)
}

fn append_len_prefixed_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Option<()> {
    append_u32_varint(out, u32::try_from(bytes.len()).ok()?);
    out.extend_from_slice(bytes);
    Some(())
}

fn encoded_len_size(len: usize) -> Option<usize> {
    let mut value = u32::try_from(len).ok()?;
    let mut size = 1;
    while value >= 0x80 {
        value >>= 7;
        size += 1;
    }
    Some(size)
}

fn append_u32_varint(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn get_bit_from_chunk(chunk: &[u8; QMDB_STATE_CHUNK_SIZE], bit: u64) -> bool {
    let byte = ((bit / 8) % QMDB_STATE_CHUNK_SIZE as u64) as usize;
    let mask = 1u8 << (bit % 8);
    (chunk[byte] & mask) != 0
}

const fn qmdb_chunk_bits() -> u64 {
    (QMDB_STATE_CHUNK_SIZE * 8) as u64
}

const fn grafting_height() -> u32 {
    qmdb_chunk_bits().trailing_zeros()
}

// Guest-side subset of commonware's MMR verifier. Keeping this local avoids
// pulling native commonware cryptography dependencies into the OpenVM build.
fn reconstruct_qmdb_mmr_root(
    leaves: u64,
    proof_digests: &[[u8; 32]],
    start_loc: u64,
    operation: &[u8],
    chunk: &[u8; QMDB_STATE_CHUNK_SIZE],
) -> Option<[u8; 32]> {
    if leaves > MAX_MMR_LEAVES || start_loc >= leaves {
        return None;
    }

    let end_loc = start_loc.checked_add(1)?;
    if end_loc > leaves {
        return None;
    }

    let size = mmr_location_to_position(leaves)?;
    let range = start_loc..end_loc;
    let mut fold_prefix = Vec::new();
    let mut after_peaks = Vec::new();
    let mut range_peaks = Vec::new();
    let mut leaf_cursor = 0u64;

    for (peak_pos, height) in mmr_peaks(size) {
        let width = 1u64.checked_shl(height)?;
        let leaf_start = leaf_cursor;
        let leaf_end = leaf_start.checked_add(width)?;

        if leaf_end <= range.start {
            fold_prefix.push(peak_pos);
        } else if leaf_start >= range.end {
            after_peaks.push(peak_pos);
        } else {
            range_peaks.push(MmrSubtree {
                pos: peak_pos,
                height,
                leaf_start,
            });
        }
        leaf_cursor = leaf_end;
    }

    if range_peaks.is_empty() || leaf_cursor != leaves {
        return None;
    }

    let prefix_digests = usize::from(!fold_prefix.is_empty());
    let after_start = prefix_digests;
    let after_end = after_start.checked_add(after_peaks.len())?;
    if proof_digests.len() < after_end {
        return None;
    }

    let mut peak_digests = Vec::new();
    if !fold_prefix.is_empty() {
        peak_digests.push(proof_digests[0]);
    }

    let siblings = &proof_digests[after_end..];
    let mut sibling_cursor = 0usize;
    let mut element = Some(operation);
    let mut leaf_consumed = false;
    let start_chunk = start_loc / qmdb_chunk_bits();
    for peak in range_peaks {
        let peak_digest = reconstruct_qmdb_peak(
            peak,
            &range,
            &mut element,
            &mut leaf_consumed,
            siblings,
            &mut sibling_cursor,
            start_chunk,
            chunk,
        )?;
        peak_digests.push(peak_digest);
    }

    for digest in &proof_digests[after_start..after_end] {
        peak_digests.push(*digest);
    }

    if !leaf_consumed || element.is_some() || sibling_cursor != siblings.len() {
        return None;
    }

    Some(mmr_root(leaves, &peak_digests))
}

fn reconstruct_qmdb_peak(
    node: MmrSubtree,
    range: &std::ops::Range<u64>,
    element: &mut Option<&[u8]>,
    leaf_consumed: &mut bool,
    siblings: &[[u8; 32]],
    sibling_cursor: &mut usize,
    start_chunk: u64,
    chunk: &[u8; QMDB_STATE_CHUNK_SIZE],
) -> Option<[u8; 32]> {
    if node.leaf_end()? <= range.start || node.leaf_start >= range.end {
        let digest = *siblings.get(*sibling_cursor)?;
        *sibling_cursor = sibling_cursor.checked_add(1)?;
        return Some(digest);
    }

    if node.height == 0 {
        let operation = element.take()?;
        *leaf_consumed = true;
        return Some(mmr_leaf_digest(node.pos, operation));
    }

    let (left, right) = node.children()?;
    let left_digest = reconstruct_qmdb_peak(
        left,
        range,
        element,
        leaf_consumed,
        siblings,
        sibling_cursor,
        start_chunk,
        chunk,
    )?;
    let right_digest = reconstruct_qmdb_peak(
        right,
        range,
        element,
        leaf_consumed,
        siblings,
        sibling_cursor,
        start_chunk,
        chunk,
    )?;

    Some(grafted_node_digest(
        node.pos,
        node.height,
        &left_digest,
        &right_digest,
        start_chunk,
        chunk,
    ))
}

fn grafted_node_digest(
    pos: u64,
    height: u32,
    left_digest: &[u8; 32],
    right_digest: &[u8; 32],
    start_chunk: u64,
    chunk: &[u8; QMDB_STATE_CHUNK_SIZE],
) -> [u8; 32] {
    let ops_subtree_root = mmr_node_digest(pos, left_digest, right_digest);
    if height != grafting_height() {
        return ops_subtree_root;
    }

    let Some(loc) = mmr_leftmost_leaf(pos, height) else {
        return ops_subtree_root;
    };
    let chunk_idx = loc >> height;
    if chunk_idx != start_chunk || chunk.iter().all(|&byte| byte == 0) {
        ops_subtree_root
    } else {
        sha256([chunk.as_slice(), ops_subtree_root.as_slice()])
    }
}

#[derive(Clone, Copy)]
struct MmrSubtree {
    pos: u64,
    height: u32,
    leaf_start: u64,
}

impl MmrSubtree {
    fn leaf_end(self) -> Option<u64> {
        self.leaf_start.checked_add(1u64.checked_shl(self.height)?)
    }

    fn children(self) -> Option<(Self, Self)> {
        if self.height == 0 {
            return None;
        }
        let child_height = self.height - 1;
        let left_pos = self.pos.checked_sub(1u64.checked_shl(self.height)?)?;
        let right_pos = self.pos.checked_sub(1)?;
        let mid = self
            .leaf_start
            .checked_add(1u64.checked_shl(child_height)?)?;
        Some((
            Self {
                pos: left_pos,
                height: child_height,
                leaf_start: self.leaf_start,
            },
            Self {
                pos: right_pos,
                height: child_height,
                leaf_start: mid,
            },
        ))
    }
}

const MAX_MMR_LEAVES: u64 = 0x4000_0000_0000_0000;
const MAX_MMR_NODES: u64 = 0x7fff_ffff_ffff_ffff;

fn mmr_location_to_position(loc: u64) -> Option<u64> {
    if loc > MAX_MMR_LEAVES {
        return None;
    }
    loc.checked_mul(2)?.checked_sub(loc.count_ones() as u64)
}

fn mmr_peaks(size: u64) -> Vec<(u64, u32)> {
    if size == 0 {
        return Vec::new();
    }

    let mut peaks = Vec::new();
    let start = u64::MAX >> size.leading_zeros();
    if start == u64::MAX {
        return peaks;
    }
    let mut two_h = 1u64 << start.trailing_ones();
    let Some(mut node_pos) = start.checked_sub(1) else {
        return peaks;
    };

    while two_h > 1 {
        if node_pos < size {
            peaks.push((node_pos, two_h.trailing_zeros() - 1));
            let Some(next_pos) = node_pos.checked_add(two_h - 1) else {
                return Vec::new();
            };
            node_pos = next_pos;
        } else {
            two_h >>= 1;
            let Some(next_pos) = node_pos.checked_sub(two_h) else {
                return Vec::new();
            };
            node_pos = next_pos;
        }
    }

    peaks
}

fn mmr_leftmost_leaf(pos: u64, height: u32) -> Option<u64> {
    let shift = 1u64.checked_shl(height.checked_add(1)?)?;
    let leftmost_pos = pos.checked_add(2)?.checked_sub(shift)?;
    mmr_position_to_location(leftmost_pos)
}

fn mmr_position_to_location(pos: u64) -> Option<u64> {
    if pos > MAX_MMR_NODES {
        return None;
    }
    if pos == 0 {
        return Some(0);
    }

    let start = u64::MAX >> (pos + 1).leading_zeros();
    let height = start.trailing_ones();
    if height == 0 {
        return None;
    }

    let mut two_h = 1u64 << (height - 1);
    let mut cur_node = start.checked_sub(1)?;
    let mut leaf_loc_floor = 0u64;

    while two_h > 1 {
        if cur_node == pos {
            return None;
        }
        let left_pos = cur_node.checked_sub(two_h)?;
        two_h >>= 1;
        if pos > left_pos {
            leaf_loc_floor = leaf_loc_floor.checked_add(two_h)?;
            cur_node = cur_node.checked_sub(1)?;
        } else {
            cur_node = left_pos;
        }
    }

    Some(leaf_loc_floor)
}

fn mmr_leaf_digest(pos: u64, element: &[u8]) -> [u8; 32] {
    sha256([pos.to_be_bytes().as_slice(), element])
}

fn mmr_node_digest(pos: u64, left_digest: &[u8; 32], right_digest: &[u8; 32]) -> [u8; 32] {
    sha256([
        pos.to_be_bytes().as_slice(),
        left_digest.as_slice(),
        right_digest.as_slice(),
    ])
}

fn mmr_root(leaves: u64, peak_digests: &[[u8; 32]]) -> [u8; 32] {
    let Some((first, rest)) = peak_digests.split_first() else {
        return sha256([leaves.to_be_bytes().as_slice()]);
    };
    let acc = rest.iter().fold(*first, |acc, digest| {
        sha256([acc.as_slice(), digest.as_slice()])
    });
    sha256([leaves.to_be_bytes().as_slice(), acc.as_slice()])
}

fn sha256<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
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

pub fn hash_header(header: &WitnessBlockHeader) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.height.to_le_bytes());
    hasher.update(&header.parent_hash);
    hasher.update(&header.state_root);
    hasher.update(&header.events_root);
    hasher.update(&header.order_count.to_le_bytes());
    hasher.update(&header.fill_count.to_le_bytes());
    hasher.update(&header.timestamp_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub fn witness_root(witness: &BlockWitness) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(WITNESS_ROOT_DOMAIN);
    hasher.update(&sybil_verifier::witness_schema::canonical_witness_bytes(
        witness,
    ));
    *hasher.finalize().as_bytes()
}

pub fn compute_events_root(witness: &BlockWitness) -> Option<[u8; 32]> {
    let events = sybil_verifier::event_schema::event_leaf_values(
        &witness.system_events,
        &witness.orders,
        &witness.rejections,
        &witness.fills,
    );
    events_root_from_event_bytes(&events)
}

pub fn events_root_from_event_bytes(events: &[Vec<u8>]) -> Option<[u8; 32]> {
    let mut operations = Vec::with_capacity(events.len().checked_add(2)?);
    operations.push(encode_keyless_commit_operation(None)?);
    for event in events {
        operations.push(encode_keyless_append_operation(event)?);
    }
    operations.push(encode_keyless_commit_operation(Some(
        &(u64::try_from(events.len()).ok()?).to_le_bytes(),
    ))?);
    compute_mmr_root_from_elements(&operations)
}

fn encode_keyless_append_operation(value: &[u8]) -> Option<Vec<u8>> {
    const APPEND_CONTEXT: u8 = 1;

    let mut out = Vec::with_capacity(1 + encoded_len_size(value.len())? + value.len());
    out.push(APPEND_CONTEXT);
    append_len_prefixed_bytes(&mut out, value)?;
    Some(out)
}

fn encode_keyless_commit_operation(metadata: Option<&[u8]>) -> Option<Vec<u8>> {
    const COMMIT_CONTEXT: u8 = 0;

    let metadata_len = match metadata {
        Some(bytes) => encoded_len_size(bytes.len())?.checked_add(bytes.len())?,
        None => 0,
    };
    let mut out = Vec::with_capacity(2 + metadata_len);
    out.push(COMMIT_CONTEXT);
    match metadata {
        Some(bytes) => {
            out.push(1);
            append_len_prefixed_bytes(&mut out, bytes)?;
        }
        None => out.push(0),
    }
    Some(out)
}

fn compute_mmr_root_from_elements(elements: &[Vec<u8>]) -> Option<[u8; 32]> {
    let leaves = u64::try_from(elements.len()).ok()?;
    if leaves > MAX_MMR_LEAVES {
        return None;
    }
    let size = mmr_location_to_position(leaves)?;
    let mut peak_digests = Vec::new();
    let mut leaf_cursor = 0usize;

    for (peak_pos, height) in mmr_peaks(size) {
        let width = 1usize.checked_shl(height)?;
        let end = leaf_cursor.checked_add(width)?;
        let peak = elements.get(leaf_cursor..end)?;
        peak_digests.push(compute_mmr_peak_from_elements(peak_pos, height, peak)?);
        leaf_cursor = end;
    }

    if leaf_cursor != elements.len() {
        return None;
    }
    Some(mmr_root(leaves, &peak_digests))
}

fn compute_mmr_peak_from_elements(pos: u64, height: u32, elements: &[Vec<u8>]) -> Option<[u8; 32]> {
    if height == 0 {
        let [element] = elements else {
            return None;
        };
        return Some(mmr_leaf_digest(pos, element));
    }

    let child_height = height - 1;
    let left_width = 1usize.checked_shl(child_height)?;
    let (left_elements, right_elements) = elements.split_at(left_width);
    let left_pos = pos.checked_sub(1u64.checked_shl(height)?)?;
    let right_pos = pos.checked_sub(1)?;
    let left = compute_mmr_peak_from_elements(left_pos, child_height, left_elements)?;
    let right = compute_mmr_peak_from_elements(right_pos, child_height, right_elements)?;
    Some(mmr_node_digest(pos, &left, &right))
}

pub fn public_inputs_from_witness(witness: &BlockWitness) -> StateTransitionPublicInputs {
    let (previous_height, previous_state_root) = match &witness.previous_header {
        Some(previous) => (previous.height, previous.state_root),
        None => (0, [0u8; 32]),
    };

    StateTransitionPublicInputs {
        previous_height,
        new_height: witness.header.height,
        previous_state_root,
        new_state_root: witness.header.state_root,
        block_hash: hash_header(&witness.header),
        events_root: witness.header.events_root,
        witness_root: witness_root(witness),
        da_commitment: UNIMPLEMENTED_DA_COMMITMENT,
        deposit_root: witness.state_sidecar.bridge.deposit_root,
        deposit_count: witness.state_sidecar.bridge.deposit_cursor,
    }
}

fn verify_public_input_binding(
    inputs: &StateTransitionPublicInputs,
    witness: &BlockWitness,
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
    if inputs.da_commitment != UNIMPLEMENTED_DA_COMMITMENT {
        return Err(ZkTransitionError::DaCommitmentUnsupported);
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

    Ok(())
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
        AccountReservationSnapshot, AccountSnapshot, BridgeStateSnapshot, MarketGroupSnapshot,
        MarketSnapshot, MarketStatusSnapshot, StateSidecarSnapshot, WithdrawalSnapshot,
        WitnessBlockHeader,
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

    fn empty_guest_input() -> StateTransitionGuestInput {
        let state_sidecar = StateSidecarSnapshot::default();
        let leaves = sybil_verifier::state_schema::state_root_leaves(&[], &state_sidecar);
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
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);
        StateTransitionGuestInput {
            public_inputs,
            witness,
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
        };
        let market = MarketSnapshot {
            market_id: MarketId::new(3),
            name: "Election test".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: sybil_verifier::state_schema::market_metadata_digest(b"metadata"),
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
        order.limit_price = 600_000_000;
        order.max_fill = 12;
        order.expires_at_block = Some(10);

        let state_sidecar = StateSidecarSnapshot {
            bridge: BridgeStateSnapshot {
                deposit_cursor: 5,
                deposit_root: [8u8; 32],
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

        let post_state = vec![account];
        let leaves = sybil_verifier::state_schema::state_root_leaves(&post_state, &state_sidecar);
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
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);
        StateTransitionGuestInput {
            public_inputs,
            witness,
            state_root_proof,
        }
    }

    fn many_account_guest_input(account_count: u64) -> StateTransitionGuestInput {
        let state_sidecar = StateSidecarSnapshot::default();
        let post_state = (0..account_count)
            .map(|id| AccountSnapshot {
                id,
                balance: 1_000_000_000 + id as i64,
                total_deposited: 1_000_000_000 + id as i64,
                positions: vec![],
                events_digest: [id as u8; 32],
            })
            .collect::<Vec<_>>();
        let leaves = sybil_verifier::state_schema::state_root_leaves(&post_state, &state_sidecar);
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
            resolved_markets: vec![],
        };
        let public_inputs = public_inputs_from_witness(&witness);

        StateTransitionGuestInput {
            public_inputs,
            witness,
            state_root_proof,
        }
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
                126, 206, 84, 228, 251, 129, 163, 143, 167, 114, 107, 195, 102, 217, 75, 46, 5,
                169, 103, 108, 187, 48, 143, 93, 124, 99, 38, 78, 184, 208, 197, 48,
            ]
        );
    }

    #[test]
    fn guest_events_root_matches_native_golden_deposit() {
        let system_events = vec![sybil_verifier::SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = sybil_verifier::event_schema::event_leaf_values(&system_events, &[], &[], &[]);

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
        let expected = sybil_verifier::state_schema::state_root_leaves(
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
        input.public_inputs.witness_root = witness_root(&input.witness);

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
        input.public_inputs.new_state_root = input.witness.header.state_root;
        input.public_inputs.block_hash = hash_header(&input.witness.header);
        input.public_inputs.witness_root = witness_root(&input.witness);

        assert_eq!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootProofVerificationFailed { index: 0 })
        );
    }

    #[test]
    fn hidden_state_leaf_fails_next_key_ring() {
        let mut input = empty_guest_input();
        let witness_leaves = sybil_verifier::state_schema::state_root_leaves(
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
        input.public_inputs.new_state_root = root;
        input.public_inputs.block_hash = hash_header(&input.witness.header);
        input.public_inputs.witness_root = witness_root(&input.witness);
        input.state_root_proof = state_root_proof;

        assert!(matches!(
            verify_state_transition_input(&input),
            Err(ZkTransitionError::StateRootNextKeyMismatch { .. })
        ));
    }
}
