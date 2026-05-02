use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sybil_verifier::{
    commitments::{event_schema, state_schema},
    BlockWitness,
};

use crate::ZkTransitionError;

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

pub fn verify_qmdb_state_root(
    root: &[u8; 32],
    witness: &BlockWitness,
    proof: &QmdbStateRootProof,
) -> Result<(), ZkTransitionError> {
    let leaves = state_schema::state_root_leaves(&witness.post_state, &witness.state_sidecar);
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

pub fn compute_events_root(witness: &BlockWitness) -> Option<[u8; 32]> {
    let events = event_schema::event_leaf_values(
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
        if end_chunk == complete_chunks && last_chunk_digest != sha256([chunk.as_slice()]) {
            return false;
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
