use serde::{Deserialize, Serialize};

pub const PROOF_ENVELOPE_VERSION: u8 = 1;
pub const PROOF_PAYLOAD_DIGEST_DOMAIN: &[u8] = b"sybil/proof-payload-digest/v1";
const EPOCH_ID_DOMAIN: &[u8] = b"sybil/epoch-id/v1";

/// Trust-bearing proof kind.
///
/// This enum, rather than a caller-provided boolean, controls whether a proof
/// may cross the L1 calldata boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofKind {
    Mock,
    OpenVmStark,
    OpenVmEvm,
}

impl ProofKind {
    pub const fn is_l1_submittable(self) -> bool {
        matches!(self, Self::OpenVmEvm)
    }
}

/// Content-derived identifier for one exact epoch statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EpochId(pub [u8; 32]);

impl EpochId {
    pub fn from_public_inputs(inputs: &crate::EpochTransitionPublicInputs) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(EPOCH_ID_DOMAIN);
        hasher.update(&crate::epoch_transition_public_input_hash(inputs));
        Self(*hasher.finalize().as_bytes())
    }
}

/// Common metadata envelope for mock, root-STARK, and EVM-wrapped proofs.
///
/// The potentially large proof payload is an immutable side artifact bound by
/// `payload_digest` and `payload_len`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofEnvelope {
    pub format_version: u8,
    pub proof_kind: ProofKind,
    pub epoch_id: EpochId,
    pub public_inputs: crate::EpochTransitionPublicInputs,
    pub public_input_hash: [u8; 32],
    pub app_exe_commit: [u8; 32],
    pub app_vm_commit: [u8; 32],
    pub payload_digest: [u8; 32],
    pub payload_len: u64,
    pub created_at_ms: u64,
}

impl ProofEnvelope {
    #[allow(
        clippy::too_many_arguments,
        reason = "constructor mirrors the fixed proof-envelope wire fields"
    )]
    pub fn new(
        proof_kind: ProofKind,
        public_inputs: crate::EpochTransitionPublicInputs,
        app_exe_commit: [u8; 32],
        app_vm_commit: [u8; 32],
        payload_digest: [u8; 32],
        payload_len: u64,
        created_at_ms: u64,
    ) -> Self {
        let public_input_hash = crate::epoch_transition_public_input_hash(&public_inputs);
        let epoch_id = EpochId::from_public_inputs(&public_inputs);
        Self {
            format_version: PROOF_ENVELOPE_VERSION,
            proof_kind,
            epoch_id,
            public_inputs,
            public_input_hash,
            app_exe_commit,
            app_vm_commit,
            payload_digest,
            payload_len,
            created_at_ms,
        }
    }

    pub fn validate(&self) -> Result<(), ProofEnvelopeError> {
        if self.format_version != PROOF_ENVELOPE_VERSION {
            return Err(ProofEnvelopeError::UnsupportedVersion {
                expected: PROOF_ENVELOPE_VERSION,
                actual: self.format_version,
            });
        }
        let expected_public_input_hash =
            crate::epoch_transition_public_input_hash(&self.public_inputs);
        if self.public_input_hash != expected_public_input_hash {
            return Err(ProofEnvelopeError::PublicInputHashMismatch);
        }
        if self.epoch_id != EpochId::from_public_inputs(&self.public_inputs) {
            return Err(ProofEnvelopeError::EpochIdMismatch);
        }
        Ok(())
    }

    pub fn require_l1_submittable(&self) -> Result<(), ProofEnvelopeError> {
        self.validate()?;
        if !self.proof_kind.is_l1_submittable() {
            return Err(ProofEnvelopeError::NotL1Submittable {
                proof_kind: self.proof_kind,
            });
        }
        Ok(())
    }

    pub fn validate_payload(&self, payload: &[u8]) -> Result<(), ProofEnvelopeError> {
        self.validate()?;
        let actual_len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        if self.payload_len != actual_len {
            return Err(ProofEnvelopeError::PayloadLengthMismatch {
                expected: self.payload_len,
                actual: actual_len,
            });
        }
        if self.payload_digest != proof_payload_digest(payload) {
            return Err(ProofEnvelopeError::PayloadDigestMismatch);
        }
        Ok(())
    }
}

pub fn proof_payload_digest(payload: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PROOF_PAYLOAD_DIGEST_DOMAIN);
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    *hasher.finalize().as_bytes()
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProofEnvelopeError {
    #[error("unsupported proof envelope version: expected {expected}, got {actual}")]
    UnsupportedVersion { expected: u8, actual: u8 },
    #[error("proof envelope public-input hash does not match its epoch statement")]
    PublicInputHashMismatch,
    #[error("proof envelope epoch id does not match its epoch statement")]
    EpochIdMismatch,
    #[error("proof kind {proof_kind:?} is not eligible for L1 submission")]
    NotL1Submittable { proof_kind: ProofKind },
    #[error("proof payload length mismatch: expected {expected}, got {actual}")]
    PayloadLengthMismatch { expected: u64, actual: u64 },
    #[error("proof payload digest does not match the envelope")]
    PayloadDigestMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> crate::EpochTransitionPublicInputs {
        crate::EpochTransitionPublicInputs {
            start_height: 10,
            end_height: 12,
            start_state_root: [1; 32],
            end_state_root: [2; 32],
            block_count: 2,
            blocks_commitment: [3; 32],
            epoch_da_commitment: [4; 32],
            deposit_root: [5; 32],
            deposit_count: 7,
        }
    }

    #[test]
    fn envelope_ids_and_hashes_are_derived() {
        let envelope = ProofEnvelope::new(
            ProofKind::OpenVmStark,
            inputs(),
            [6; 32],
            [7; 32],
            [8; 32],
            99,
            123,
        );

        assert_eq!(
            envelope.public_input_hash,
            crate::epoch_transition_public_input_hash(&envelope.public_inputs)
        );
        assert_eq!(
            envelope.epoch_id,
            EpochId::from_public_inputs(&envelope.public_inputs)
        );
        assert_eq!(envelope.validate(), Ok(()));
    }

    #[test]
    fn only_evm_envelopes_are_l1_submittable() {
        for kind in [ProofKind::Mock, ProofKind::OpenVmStark] {
            let envelope = ProofEnvelope::new(kind, inputs(), [0; 32], [0; 32], [0; 32], 0, 0);
            assert!(matches!(
                envelope.require_l1_submittable(),
                Err(ProofEnvelopeError::NotL1Submittable { proof_kind }) if proof_kind == kind
            ));
        }

        let evm = ProofEnvelope::new(
            ProofKind::OpenVmEvm,
            inputs(),
            [0; 32],
            [0; 32],
            [0; 32],
            0,
            0,
        );
        assert_eq!(evm.require_l1_submittable(), Ok(()));
    }

    #[test]
    fn tampered_derived_fields_are_rejected() {
        let mut envelope =
            ProofEnvelope::new(ProofKind::Mock, inputs(), [0; 32], [0; 32], [0; 32], 0, 0);
        envelope.public_input_hash[0] ^= 1;
        assert_eq!(
            envelope.validate(),
            Err(ProofEnvelopeError::PublicInputHashMismatch)
        );
    }

    #[test]
    fn payload_bytes_are_bound_by_length_and_digest() {
        let payload = b"proof bytes";
        let envelope = ProofEnvelope::new(
            ProofKind::Mock,
            inputs(),
            [0; 32],
            [0; 32],
            proof_payload_digest(payload),
            payload.len() as u64,
            0,
        );
        assert_eq!(envelope.validate_payload(payload), Ok(()));
        assert_eq!(
            envelope.validate_payload(b"proof byte!"),
            Err(ProofEnvelopeError::PayloadDigestMismatch)
        );
        assert_eq!(
            envelope.validate_payload(b"short"),
            Err(ProofEnvelopeError::PayloadLengthMismatch {
                expected: payload.len() as u64,
                actual: 5,
            })
        );
    }
}
