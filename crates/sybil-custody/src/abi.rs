use alloy::primitives::{B256, Bytes, U256};
use alloy::sol_types::{SolCall, SolValue};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use sybil_escape_claim::EscapeClaimPublicInputs;
use sybil_l1_abi::{EscapeClaimPublicInputs as AbiEscapeClaimPublicInputs, SybilVault};

#[cfg(test)]
const WORD: usize = 32;

#[derive(Debug, Deserialize)]
pub struct OpenVmEvmProof {
    pub app_exe_commit: String,
    pub app_vm_commit: String,
    pub user_public_values: String,
    pub proof_data: OpenVmProofData,
}

#[derive(Debug, Deserialize)]
pub struct OpenVmProofData {
    pub accumulator: String,
    pub proof: String,
}

pub fn adapter_proof_from_openvm_json(bytes: &[u8]) -> Result<Vec<u8>> {
    let proof: OpenVmEvmProof = serde_json::from_slice(bytes).context("decode OpenVM EVM proof")?;
    let public_values = decode_hex("user_public_values", &proof.user_public_values)?;
    let mut proof_data = decode_hex("proof_data.accumulator", &proof.proof_data.accumulator)?;
    proof_data.extend(decode_hex("proof_data.proof", &proof.proof_data.proof)?);
    let app_exe_commit = bytes32("app_exe_commit", &proof.app_exe_commit)?;
    let app_vm_commit = bytes32("app_vm_commit", &proof.app_vm_commit)?;
    Ok(encode_adapter_proof(
        &public_values,
        &proof_data,
        app_exe_commit,
        app_vm_commit,
    ))
}

/// Encode the exact tuple decoded by `OpenVmVerifierAdapter.decodeProof`:
/// `(bytes publicValues, bytes proofData, bytes32 appExeCommit, bytes32 appVmCommit)`.
pub fn encode_adapter_proof(
    public_values: &[u8],
    proof_data: &[u8],
    app_exe_commit: [u8; 32],
    app_vm_commit: [u8; 32],
) -> Vec<u8> {
    (
        Bytes::copy_from_slice(public_values),
        Bytes::copy_from_slice(proof_data),
        B256::from(app_exe_commit),
        B256::from(app_vm_commit),
    )
        .abi_encode_params()
}

/// Six ABI words for the Solidity `EscapeClaimPublicInputs` struct. This is
/// exposed independently so the Rust/Solidity field layout has a direct test.
pub fn encode_escape_public_inputs(inputs: &EscapeClaimPublicInputs) -> Vec<u8> {
    abi_escape_inputs(inputs).abi_encode_params()
}

pub fn escape_claim_calldata(inputs: &EscapeClaimPublicInputs, proof: &[u8]) -> Vec<u8> {
    SybilVault::escapeClaimCall {
        inputs: abi_escape_inputs(inputs),
        proof: Bytes::copy_from_slice(proof),
    }
    .abi_encode()
}

fn abi_escape_inputs(inputs: &EscapeClaimPublicInputs) -> AbiEscapeClaimPublicInputs {
    AbiEscapeClaimPublicInputs {
        stateRoot: inputs.state_root.into(),
        height: inputs.height,
        accountId: inputs.account_id,
        recipient: inputs.recipient.into(),
        amount: U256::from_limbs([inputs.amount, 0, 0, 0]),
        nullifier: inputs.nullifier.into(),
    }
}

fn decode_hex(field: &'static str, value: &str) -> Result<Vec<u8>> {
    let raw = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    hex::decode(raw).with_context(|| format!("decode {field}"))
}

fn bytes32(field: &'static str, value: &str) -> Result<[u8; 32]> {
    let bytes = decode_hex(field, value)?;
    if bytes.len() != 32 {
        bail!("{field} must be 32 bytes, got {}", bytes.len());
    }
    bytes
        .try_into()
        .map_err(|_| anyhow!("{field} must be 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sybil_escape_claim::escape_claim_public_input_hash;

    fn golden_inputs() -> (EscapeClaimPublicInputs, [u8; 32]) {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../golden/golden-vectors.json"
        );
        let json: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let value = &json["escape_claim_public_inputs"];
        let inputs = EscapeClaimPublicInputs {
            state_root: bytes32("state_root", value["state_root"].as_str().unwrap()).unwrap(),
            height: value["height"].as_u64().unwrap(),
            account_id: value["account_id"].as_u64().unwrap(),
            recipient: decode_hex("recipient", value["recipient"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
            amount: value["amount"].as_u64().unwrap(),
            nullifier: bytes32("nullifier", value["nullifier"].as_str().unwrap()).unwrap(),
        };
        let expected_hash = bytes32("hash", value["hash"].as_str().unwrap()).unwrap();
        (inputs, expected_hash)
    }

    #[test]
    fn escape_public_input_struct_words_match_solidity_twin_golden() {
        let (inputs, expected_hash) = golden_inputs();
        let encoded = encode_escape_public_inputs(&inputs);
        assert_eq!(encoded.len(), 6 * WORD);
        assert_eq!(&encoded[0..32], &inputs.state_root);
        assert_eq!(&encoded[56..64], &inputs.height.to_be_bytes());
        assert_eq!(&encoded[88..96], &inputs.account_id.to_be_bytes());
        assert_eq!(&encoded[108..128], &inputs.recipient);
        assert_eq!(&encoded[152..160], &inputs.amount.to_be_bytes());
        assert_eq!(&encoded[160..192], &inputs.nullifier);
        assert_eq!(escape_claim_public_input_hash(&inputs), expected_hash);
    }

    #[test]
    fn adapter_blob_offsets_and_fixture_bytes_match_solidity_decoder_layout() {
        let (inputs, _) = golden_inputs();
        let public_values = escape_claim_public_input_hash(&inputs);
        let proof = encode_adapter_proof(&public_values, &[1, 2, 3, 4], [0xaa; 32], [0xbb; 32]);
        assert_eq!(&proof[24..32], &(128u64).to_be_bytes());
        assert_eq!(&proof[56..64], &(192u64).to_be_bytes());
        assert_eq!(&proof[64..96], &[0xaa; 32]);
        assert_eq!(&proof[96..128], &[0xbb; 32]);
        assert_eq!(&proof[152..160], &(32u64).to_be_bytes());
        assert_eq!(&proof[160..192], &public_values);
        assert_eq!(&proof[216..224], &(4u64).to_be_bytes());
        assert_eq!(&proof[224..228], &[1, 2, 3, 4]);
    }

    #[test]
    fn escape_calldata_uses_static_tuple_then_dynamic_proof() {
        let (inputs, _) = golden_inputs();
        let proof = vec![0xcc; 33];
        let calldata = escape_claim_calldata(&inputs, &proof);
        assert_eq!(&calldata[..4], &SybilVault::escapeClaimCall::SELECTOR);
        assert_eq!(&calldata[4..196], encode_escape_public_inputs(&inputs));
        assert_eq!(&calldata[220..228], &(224u64).to_be_bytes());
        assert_eq!(&calldata[252..260], &(33u64).to_be_bytes());
        assert_eq!(&calldata[260..293], proof);
    }
}
