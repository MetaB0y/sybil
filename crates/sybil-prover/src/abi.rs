use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use serde::Deserialize;
use sha3::{Digest as _, Keccak256};

use crate::artifacts::{read_guest_input, write_hex_bytes};
use crate::ProverCliError;

const SUBMIT_STATE_ROOT_SIGNATURE: &str =
    "submitStateRoot((uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64),bytes)";
const STATE_TRANSITION_PUBLIC_INPUT_WORDS: usize = 10;
const ABI_WORD_BYTES: usize = 32;
const SHELL_SAFE_CALLDATA_BYTES: usize = 128 * 1024;
const OPENVM_EVM_ADAPTER_PROOF_WORDS: usize = 4;

#[derive(Args)]
pub struct SubmitStateRootArgs {
    /// MessagePack-encoded StateTransitionGuestInput produced by `prepare`.
    #[arg(long)]
    pub guest_input: PathBuf,
    /// OpenVM proof bytes to submit.
    #[arg(long)]
    pub proof: PathBuf,
    /// Proof file format. `openvm-evm-json` converts OpenVM's EVM proof JSON
    /// into the ABI payload expected by OpenVmVerifierAdapter.
    #[arg(long, value_enum, default_value_t = ProofFormat::Raw)]
    pub proof_format: ProofFormat,
    /// Deployed SybilSettlement address.
    #[arg(long)]
    pub settlement: String,
    /// Output path for hex calldata accepted by `cast send --data`.
    #[arg(long, default_value = "target/sybil-submit-state-root.calldata")]
    pub calldata: PathBuf,
    /// Optional output path for an eth_sendTransaction JSON-RPC request.
    #[arg(long)]
    pub rpc_request: Option<PathBuf>,
    /// Sender address to include in the optional eth_sendTransaction request.
    #[arg(long)]
    pub from: Option<String>,
    /// Optional gas limit to include in the eth_sendTransaction request.
    #[arg(long)]
    pub gas: Option<String>,
    /// Environment variable containing the RPC URL for the printed cast command.
    #[arg(long, default_value = "ETH_RPC_URL")]
    pub rpc_url_env: String,
    /// Environment variable containing the private key for the printed cast command.
    #[arg(long, default_value = "PRIVATE_KEY")]
    pub private_key_env: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ProofFormat {
    Raw,
    #[value(name = "openvm-evm-json")]
    OpenVmEvmJson,
}

#[derive(Deserialize)]
struct OpenVmEvmProofJson {
    app_exe_commit: String,
    app_vm_commit: String,
    user_public_values: String,
    proof_data: OpenVmEvmProofDataJson,
}

#[derive(Deserialize)]
struct OpenVmEvmProofDataJson {
    accumulator: String,
    proof: String,
}

pub fn submit_state_root(args: SubmitStateRootArgs) -> Result<(), ProverCliError> {
    let guest_input = read_guest_input(&args.guest_input)?;
    let proof = read_proof(&args.proof, args.proof_format)?;
    let calldata = submit_state_root_calldata(&guest_input.public_inputs, &proof);
    let public_input_hash =
        sybil_zk::state_transition_public_input_hash(&guest_input.public_inputs);

    write_hex_bytes(&args.calldata, &calldata)?;

    if let Some(path) = &args.rpc_request {
        let from = args
            .from
            .as_deref()
            .ok_or(ProverCliError::MissingRpcRequestFrom)?;
        write_eth_send_transaction_request(
            path,
            from,
            &args.settlement,
            args.gas.as_deref(),
            &calldata,
        )?;
    }

    let cast_command = cast_send_data_command(
        &args.settlement,
        &args.calldata,
        &args.rpc_url_env,
        &args.private_key_env,
    );

    print_public_inputs(&guest_input.public_inputs);
    println!("public_input_hash=0x{}", hex::encode(public_input_hash));
    println!("proof={}", args.proof.display());
    println!("proof_bytes={}", proof.len());
    println!("calldata={}", args.calldata.display());
    println!("cast_send={cast_command}");
    if calldata.len() > SHELL_SAFE_CALLDATA_BYTES {
        println!("cast_send_warning=calldata is large; prefer --rpc-request and curl_send instead");
    }
    if let Some(path) = args.rpc_request {
        println!("rpc_request={}", path.display());
        println!(
            "curl_send={}",
            curl_rpc_request_command(&path, &args.rpc_url_env)
        );
    }
    Ok(())
}

fn read_proof(path: &Path, format: ProofFormat) -> Result<Vec<u8>, ProverCliError> {
    match format {
        ProofFormat::Raw => read_raw_proof(path),
        ProofFormat::OpenVmEvmJson => read_openvm_evm_adapter_proof(path),
    }
}

fn read_raw_proof(path: &Path) -> Result<Vec<u8>, ProverCliError> {
    let proof = std::fs::read(path).map_err(|source| ProverCliError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if proof.is_empty() {
        return Err(ProverCliError::EmptyProof {
            path: path.to_path_buf(),
        });
    }
    Ok(proof)
}

fn read_openvm_evm_adapter_proof(path: &Path) -> Result<Vec<u8>, ProverCliError> {
    let file = File::open(path).map_err(|source| ProverCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    let proof: OpenVmEvmProofJson =
        serde_json::from_reader(reader).map_err(|source| ProverCliError::DecodeOpenVmEvmProof {
            path: path.to_path_buf(),
            source,
        })?;

    let public_values = decode_hex_field("user_public_values", &proof.user_public_values)?;
    let mut proof_data = decode_hex_field("proof_data.accumulator", &proof.proof_data.accumulator)?;
    proof_data.extend(decode_hex_field(
        "proof_data.proof",
        &proof.proof_data.proof,
    )?);
    let app_exe_commit = decode_bytes32_field("app_exe_commit", &proof.app_exe_commit)?;
    let app_vm_commit = decode_bytes32_field("app_vm_commit", &proof.app_vm_commit)?;

    Ok(openvm_evm_adapter_proof(
        &public_values,
        &proof_data,
        &app_exe_commit,
        &app_vm_commit,
    ))
}

fn openvm_evm_adapter_proof(
    public_values: &[u8],
    proof_data: &[u8],
    app_exe_commit: &[u8; ABI_WORD_BYTES],
    app_vm_commit: &[u8; ABI_WORD_BYTES],
) -> Vec<u8> {
    let public_values_offset = (OPENVM_EVM_ADAPTER_PROOF_WORDS * ABI_WORD_BYTES) as u64;
    let proof_data_offset =
        public_values_offset + ABI_WORD_BYTES as u64 + padded_abi_len(public_values.len()) as u64;

    let mut encoded = Vec::with_capacity(
        (OPENVM_EVM_ADAPTER_PROOF_WORDS * ABI_WORD_BYTES)
            + ABI_WORD_BYTES
            + padded_abi_len(public_values.len())
            + ABI_WORD_BYTES
            + padded_abi_len(proof_data.len()),
    );
    append_abi_word_u64(&mut encoded, public_values_offset);
    append_abi_word_u64(&mut encoded, proof_data_offset);
    append_abi_word_bytes32(&mut encoded, app_exe_commit);
    append_abi_word_bytes32(&mut encoded, app_vm_commit);
    append_abi_word_u64(&mut encoded, public_values.len() as u64);
    encoded.extend_from_slice(public_values);
    encoded.resize(encoded.len() + abi_padding_len(public_values.len()), 0);
    append_abi_word_u64(&mut encoded, proof_data.len() as u64);
    encoded.extend_from_slice(proof_data);
    encoded.resize(encoded.len() + abi_padding_len(proof_data.len()), 0);
    encoded
}

fn decode_bytes32_field(
    field: &'static str,
    hex_value: &str,
) -> Result<[u8; ABI_WORD_BYTES], ProverCliError> {
    let bytes = decode_hex_field(field, hex_value)?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| ProverCliError::InvalidBytes32Field {
            field,
            actual: bytes.len(),
        })
}

fn decode_hex_field(field: &'static str, hex_value: &str) -> Result<Vec<u8>, ProverCliError> {
    let normalized = hex_value
        .strip_prefix("0x")
        .or_else(|| hex_value.strip_prefix("0X"))
        .unwrap_or(hex_value);
    hex::decode(normalized).map_err(|source| ProverCliError::DecodeHex { field, source })
}

fn write_eth_send_transaction_request(
    path: &Path,
    from: &str,
    to: &str,
    gas: Option<&str>,
    calldata: &[u8],
) -> Result<(), ProverCliError> {
    let mut tx = serde_json::Map::new();
    tx.insert(
        "from".to_string(),
        serde_json::Value::String(from.to_string()),
    );
    tx.insert("to".to_string(), serde_json::Value::String(to.to_string()));
    tx.insert(
        "data".to_string(),
        serde_json::Value::String(format!("0x{}", hex::encode(calldata))),
    );
    if let Some(gas) = gas {
        tx.insert(
            "gas".to_string(),
            serde_json::Value::String(gas.to_string()),
        );
    }

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_sendTransaction",
        "params": [serde_json::Value::Object(tx)],
    });
    let json =
        serde_json::to_vec_pretty(&request).map_err(|source| ProverCliError::EncodeJson {
            path: path.to_path_buf(),
            source,
        })?;
    std::fs::write(path, json).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn print_public_inputs(inputs: &sybil_zk::StateTransitionPublicInputs) {
    println!("previous_height={}", inputs.previous_height);
    println!("new_height={}", inputs.new_height);
    println!(
        "previous_state_root=0x{}",
        hex::encode(inputs.previous_state_root)
    );
    println!("new_state_root=0x{}", hex::encode(inputs.new_state_root));
    println!("block_hash=0x{}", hex::encode(inputs.block_hash));
    println!("events_root=0x{}", hex::encode(inputs.events_root));
    println!("witness_root=0x{}", hex::encode(inputs.witness_root));
    println!("da_commitment=0x{}", hex::encode(inputs.da_commitment));
    println!("deposit_root=0x{}", hex::encode(inputs.deposit_root));
    println!("deposit_count={}", inputs.deposit_count);
}

fn submit_state_root_calldata(
    inputs: &sybil_zk::StateTransitionPublicInputs,
    proof: &[u8],
) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(
        4 + ((STATE_TRANSITION_PUBLIC_INPUT_WORDS + 2) * ABI_WORD_BYTES)
            + padded_abi_len(proof.len()),
    );
    encoded.extend_from_slice(&function_selector(SUBMIT_STATE_ROOT_SIGNATURE));
    append_public_inputs(&mut encoded, inputs);
    append_abi_word_u64(
        &mut encoded,
        ((STATE_TRANSITION_PUBLIC_INPUT_WORDS + 1) * ABI_WORD_BYTES) as u64,
    );
    append_abi_word_u64(&mut encoded, proof.len() as u64);
    encoded.extend_from_slice(proof);
    encoded.resize(encoded.len() + abi_padding_len(proof.len()), 0);
    encoded
}

fn append_public_inputs(out: &mut Vec<u8>, inputs: &sybil_zk::StateTransitionPublicInputs) {
    append_abi_word_u64(out, inputs.previous_height);
    append_abi_word_u64(out, inputs.new_height);
    append_abi_word_bytes32(out, &inputs.previous_state_root);
    append_abi_word_bytes32(out, &inputs.new_state_root);
    append_abi_word_bytes32(out, &inputs.block_hash);
    append_abi_word_bytes32(out, &inputs.events_root);
    append_abi_word_bytes32(out, &inputs.witness_root);
    append_abi_word_bytes32(out, &inputs.da_commitment);
    append_abi_word_bytes32(out, &inputs.deposit_root);
    append_abi_word_u64(out, inputs.deposit_count);
}

fn append_abi_word_u64(out: &mut Vec<u8>, value: u64) {
    let mut word = [0u8; ABI_WORD_BYTES];
    word[ABI_WORD_BYTES - std::mem::size_of::<u64>()..].copy_from_slice(&value.to_be_bytes());
    out.extend_from_slice(&word);
}

fn append_abi_word_bytes32(out: &mut Vec<u8>, value: &[u8; ABI_WORD_BYTES]) {
    out.extend_from_slice(value);
}

fn function_selector(signature: &str) -> [u8; 4] {
    let hash = Keccak256::digest(signature.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

fn abi_padding_len(len: usize) -> usize {
    (ABI_WORD_BYTES - (len % ABI_WORD_BYTES)) % ABI_WORD_BYTES
}

fn padded_abi_len(len: usize) -> usize {
    len + abi_padding_len(len)
}

fn cast_send_data_command(
    settlement: &str,
    calldata: &Path,
    rpc_url_env: &str,
    private_key_env: &str,
) -> String {
    format!(
        "cast send {} --data \"$(cat {})\" --rpc-url \"${}\" --private-key \"${}\"",
        shell_quote(settlement),
        shell_quote(&calldata.display().to_string()),
        rpc_url_env,
        private_key_env,
    )
}

fn curl_rpc_request_command(path: &Path, rpc_url_env: &str) -> String {
    format!(
        "curl -sS -H 'content-type: application/json' --data-binary @{} \"${}\"",
        shell_quote(&path.display().to_string()),
        rpc_url_env,
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
