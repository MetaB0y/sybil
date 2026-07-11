use crate::ZkTransitionError;
#[cfg(target_os = "zkvm")]
use openvm_sha2::Sha256;
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "zkvm"))]
use sha2::{Digest as _, Sha256};
use sybil_verifier::{
    commitments::{event_schema, state_schema},
    BlockWitness,
};

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
pub enum QmdbStateExclusionProof {
    KeyValue {
        operation: QmdbStateOperationProof,
        span_key: Vec<u8>,
        span_value: Vec<u8>,
        span_next_key: Vec<u8>,
    },
    Commit {
        operation: QmdbStateOperationProof,
        metadata: Option<Vec<u8>>,
    },
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
    pub inactive_peaks: u64,
    pub digests: Vec<[u8; 32]>,
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
    verify_qmdb_state_root_for(root, &leaves, proof)
}

pub fn verify_qmdb_state_root_for(
    root: &[u8; 32],
    leaves: &[(Vec<u8>, Vec<u8>)],
    proof: &QmdbStateRootProof,
) -> Result<(), ZkTransitionError> {
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

/// Verify that `key` is absent from the ordered current-qMDB at `root`.
///
/// This mirrors commonware's ordered exclusion verifier while retaining the
/// guest-safe proof representation used by Sybil. A non-empty database proves
/// absence by authenticating the adjacent cyclic key span; an empty database
/// proves it with the active commit whose inactivity floor equals its location.
pub fn verify_qmdb_exclusion_proof(
    root: &[u8; 32],
    key: &[u8],
    proof: &QmdbStateExclusionProof,
) -> bool {
    let (operation, operation_proof) = match proof {
        QmdbStateExclusionProof::KeyValue {
            operation,
            span_key,
            span_value,
            span_next_key,
        } => {
            if span_key.as_slice() == key || !qmdb_span_contains(span_key, span_next_key, key) {
                return false;
            }
            let Some(encoded) = encode_qmdb_update_operation(span_key, span_value, span_next_key)
            else {
                return false;
            };
            (encoded, operation)
        }
        QmdbStateExclusionProof::Commit {
            operation,
            metadata,
        } => {
            let Some(encoded) =
                encode_qmdb_commit_floor_operation(metadata.as_deref(), operation.location)
            else {
                return false;
            };
            (encoded, operation)
        }
    };
    verify_qmdb_operation_proof(root, &operation, operation_proof)
}

fn qmdb_span_contains(span_start: &[u8], span_end: &[u8], key: &[u8]) -> bool {
    if span_start >= span_end {
        key >= span_start || key < span_end
    } else {
        key >= span_start && key < span_end
    }
}

fn encode_qmdb_commit_floor_operation(metadata: Option<&[u8]>, floor: u64) -> Option<Vec<u8>> {
    const COMMIT_CONTEXT: u8 = 0xD3;
    let metadata_len = match metadata {
        Some(bytes) => encoded_len_size(bytes.len())?.checked_add(bytes.len())?,
        None => 0,
    };
    let mut out = Vec::with_capacity(2usize.checked_add(metadata_len)?.checked_add(10)?);
    out.push(COMMIT_CONTEXT);
    match metadata {
        Some(bytes) => {
            out.push(1);
            append_len_prefixed_bytes(&mut out, bytes)?;
        }
        None => out.push(0),
    }
    append_u64_varint(&mut out, floor);
    Some(out)
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

// The borrowed hasher args below are REQUIRED on the guest target
// (riscv32im-risc0-zkvm-elf), whose minimal Sha256::update takes &[u8]; host
// sha2's update takes impl AsRef<[u8]>, so host clippy calls the borrows
// needless. Removing them breaks the zkVM guest build (SYB-208, fixed after
// SYB-170 introduced owned args that only compiled on host).
#[allow(clippy::needless_borrows_for_generic_args)]
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

    let Some(merkle_root) = reconstruct_qmdb_mmr_root(
        proof.leaves,
        match usize::try_from(proof.inactive_peaks) {
            Ok(inactive_peaks) => inactive_peaks,
            Err(_) => return false,
        },
        &proof.digests,
        start_loc,
        operation,
        chunk,
    ) else {
        return false;
    };

    let mut hasher = Sha256::new();
    hasher.update(&proof.ops_root);
    hasher.update(&merkle_root);
    if has_partial_chunk {
        hasher.update(&next_bit.to_be_bytes());
        hasher.update(&proof.partial_chunk_digest.expect("checked above"));
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
    // Commonware 2026.5 commits the application-declared inactivity floor.
    // Sybil retains the complete per-block event log, so the floor is always 0.
    append_u64_varint(&mut out, 0);
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

fn append_u64_varint(out: &mut Vec<u8>, mut value: u64) {
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
    inactive_peaks: usize,
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
    let mut fold_prefix_count = 0usize;
    let mut prefix_active_count = 0usize;
    let mut after_inactive_count = 0usize;
    let mut suffix_active_count = 0usize;
    let mut range_peaks = Vec::new();
    let mut leaf_cursor = 0u64;
    let mut peak_count = 0usize;

    for (peak_pos, height) in mmr_peaks(size) {
        let width = 1u64.checked_shl(height)?;
        let leaf_start = leaf_cursor;
        let leaf_end = leaf_start.checked_add(width)?;

        if leaf_end <= range.start {
            if peak_count < inactive_peaks {
                fold_prefix_count = fold_prefix_count.checked_add(1)?;
            } else {
                prefix_active_count = prefix_active_count.checked_add(1)?;
            }
        } else if leaf_start >= range.end {
            if peak_count >= inactive_peaks {
                suffix_active_count = suffix_active_count.checked_add(1)?;
            } else {
                after_inactive_count = after_inactive_count.checked_add(1)?;
            }
        } else {
            range_peaks.push(MmrSubtree {
                pos: peak_pos,
                height,
                leaf_start,
            });
        }
        leaf_cursor = leaf_end;
        peak_count = peak_count.checked_add(1)?;
    }

    if range_peaks.is_empty() || leaf_cursor != leaves || inactive_peaks > peak_count {
        return None;
    }

    // Commonware's backward-bagged range-proof layout is:
    // [folded inactive prefix? | active prefix peaks | inactive after peaks |
    //  backward-folded active suffix? | DFS siblings].
    let fold_count = usize::from(fold_prefix_count != 0);
    let prefix_start = fold_count;
    let after_start = prefix_start.checked_add(prefix_active_count)?;
    let suffix_start = after_start.checked_add(after_inactive_count)?;
    let suffix_count = usize::from(suffix_active_count != 0);
    let siblings_start = suffix_start.checked_add(suffix_count)?;
    if proof_digests.len() < siblings_start {
        return None;
    }

    let mut peak_digests = Vec::new();
    if fold_count != 0 {
        peak_digests.push(proof_digests[0]);
    }
    peak_digests.extend_from_slice(&proof_digests[prefix_start..after_start]);

    let siblings = &proof_digests[siblings_start..];
    let mut sibling_cursor = 0usize;
    let mut element = Some(operation);
    let mut leaf_consumed = false;
    let start_chunk = start_loc / qmdb_chunk_bits();
    let mut reconstruction = QmdbPeakReconstruction {
        range: &range,
        element: &mut element,
        leaf_consumed: &mut leaf_consumed,
        siblings,
        sibling_cursor: &mut sibling_cursor,
        start_chunk,
        chunk,
    };
    for peak in range_peaks {
        let peak_digest = reconstruction.reconstruct_peak(peak)?;
        peak_digests.push(peak_digest);
    }

    peak_digests.extend_from_slice(&proof_digests[after_start..suffix_start]);
    if suffix_count != 0 {
        peak_digests.push(proof_digests[suffix_start]);
    }

    if !leaf_consumed || element.is_some() || sibling_cursor != siblings.len() {
        return None;
    }

    let inactive_peaks_to_fold = if fold_prefix_count == 0 {
        inactive_peaks
    } else {
        inactive_peaks
            .checked_sub(fold_prefix_count)?
            .checked_add(1)?
    };
    mmr_root(
        leaves,
        inactive_peaks_to_fold,
        inactive_peaks,
        &peak_digests,
    )
}

struct QmdbPeakReconstruction<'a, 'b> {
    range: &'a std::ops::Range<u64>,
    element: &'a mut Option<&'b [u8]>,
    leaf_consumed: &'a mut bool,
    siblings: &'a [[u8; 32]],
    sibling_cursor: &'a mut usize,
    start_chunk: u64,
    chunk: &'a [u8; QMDB_STATE_CHUNK_SIZE],
}

impl QmdbPeakReconstruction<'_, '_> {
    fn reconstruct_peak(&mut self, node: MmrSubtree) -> Option<[u8; 32]> {
        if node.leaf_end()? <= self.range.start || node.leaf_start >= self.range.end {
            let digest = *self.siblings.get(*self.sibling_cursor)?;
            *self.sibling_cursor = self.sibling_cursor.checked_add(1)?;
            return Some(digest);
        }

        if node.height == 0 {
            let operation = self.element.take()?;
            *self.leaf_consumed = true;
            return Some(mmr_leaf_digest(node.pos, operation));
        }

        let (left, right) = node.children()?;
        let left_digest = self.reconstruct_peak(left)?;
        let right_digest = self.reconstruct_peak(right)?;

        Some(grafted_node_digest(
            node.pos,
            node.height,
            &left_digest,
            &right_digest,
            self.start_chunk,
            self.chunk,
        ))
    }
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
    mmr_root(leaves, 0, 0, &peak_digests)
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

fn mmr_root(
    leaves: u64,
    inactive_peaks_to_fold: usize,
    committed_inactive_peaks: usize,
    peak_digests: &[[u8; 32]],
) -> Option<[u8; 32]> {
    let Some((first, rest)) = peak_digests.split_first() else {
        return (inactive_peaks_to_fold == 0 && committed_inactive_peaks == 0)
            .then(|| sha256([leaves.to_be_bytes().as_slice()]));
    };
    if inactive_peaks_to_fold > peak_digests.len() {
        return None;
    }

    let mut acc = *first;
    let mut active = Vec::with_capacity(peak_digests.len());
    let mut rest = rest.iter();
    for _ in 0..inactive_peaks_to_fold.saturating_sub(1) {
        let peak = rest.next()?;
        acc = sha256([acc.as_slice(), peak.as_slice()]);
    }
    active.push(acc);
    active.extend(rest.copied());

    let mut folded = *active.last()?;
    for peak in active.iter().rev().skip(1) {
        folded = sha256([peak.as_slice(), folded.as_slice()]);
    }

    if committed_inactive_peaks == 0 {
        Some(sha256([leaves.to_be_bytes().as_slice(), folded.as_slice()]))
    } else {
        Some(sha256([
            leaves.to_be_bytes().as_slice(),
            (committed_inactive_peaks as u64).to_be_bytes().as_slice(),
            folded.as_slice(),
        ]))
    }
}

fn sha256<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}
