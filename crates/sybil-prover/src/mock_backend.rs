use sybil_proof_protocol::{ProofEnvelope, ProofKind, proof_payload_digest};

pub const MOCK_PROOF_PAYLOAD_DOMAIN: &[u8] = b"sybil/mock-epoch-proof/v1";

/// A native-verification result packaged exactly like a real backend result.
///
/// The payload is intentionally deterministic and non-cryptographic. Its
/// domain and typed `ProofKind::Mock` prevent it from being confused with an
/// OpenVM proof or submitted to L1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockEpochProof {
    pub envelope: ProofEnvelope,
    pub payload: Vec<u8>,
}

pub fn prove_mock_epoch(
    blocks: &[sybil_zk::StateTransitionGuestInput],
    app_exe_commit: [u8; 32],
    app_vm_commit: [u8; 32],
    created_at_ms: u64,
) -> Result<MockEpochProof, sybil_proof_protocol::EpochTransitionError> {
    let mut accumulator = sybil_proof_protocol::EpochTransitionAccumulator::new();
    for block in blocks {
        accumulator.push(block)?;
    }
    let public_inputs = accumulator.finish()?;
    let public_input_hash =
        sybil_proof_protocol::epoch_transition_public_input_hash(&public_inputs);
    let epoch_id = sybil_proof_protocol::EpochId::from_public_inputs(&public_inputs);

    let mut hasher = blake3::Hasher::new();
    hasher.update(MOCK_PROOF_PAYLOAD_DOMAIN);
    hasher.update(&epoch_id.0);
    hasher.update(&public_input_hash);
    hasher.update(&app_exe_commit);
    hasher.update(&app_vm_commit);
    let payload = hasher.finalize().as_bytes().to_vec();
    let envelope = ProofEnvelope::new(
        ProofKind::Mock,
        public_inputs,
        app_exe_commit,
        app_vm_commit,
        proof_payload_digest(&payload),
        payload.len() as u64,
        created_at_ms,
    );
    debug_assert_eq!(envelope.validate_payload(&payload), Ok(()));

    Ok(MockEpochProof { envelope, payload })
}
