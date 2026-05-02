use std::fs::File;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use sha3::{Digest as _, Keccak256};
use sybil_witgen::{
    build_state_transition_guest_input, StateTransitionProofJob, StateTransitionProofJobId,
};

const SUBMIT_STATE_ROOT_SIGNATURE: &str =
    "submitStateRoot((uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64),bytes)";
const STATE_TRANSITION_PUBLIC_INPUT_WORDS: usize = 10;
const ABI_WORD_BYTES: usize = 32;

#[derive(Parser)]
#[command(name = "sybil-prover")]
#[command(about = "Sybil proof-job tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect a serialized state-transition proof job.
    Inspect(JobPathArgs),
    /// Validate a proof job and write the OpenVM guest input artifact.
    Prepare(PrepareArgs),
    /// Encode a state-root submission for SybilSettlement.
    SubmitStateRoot(SubmitStateRootArgs),
}

#[derive(Args)]
struct JobPathArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
}

#[derive(Args)]
struct PrepareArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
    /// Output path for MessagePack-encoded StateTransitionGuestInput.
    #[arg(long)]
    guest_input: PathBuf,
    /// Optional output path for the hex public input hash.
    #[arg(long)]
    public_input_hash: Option<PathBuf>,
}

#[derive(Args)]
struct SubmitStateRootArgs {
    /// MessagePack-encoded StateTransitionGuestInput produced by `prepare`.
    #[arg(long)]
    guest_input: PathBuf,
    /// OpenVM proof bytes to submit.
    #[arg(long)]
    proof: PathBuf,
    /// Deployed SybilSettlement address.
    #[arg(long)]
    settlement: String,
    /// Output path for hex calldata accepted by `cast send --data`.
    #[arg(long, default_value = "/tmp/sybil-submit-state-root.calldata")]
    calldata: PathBuf,
    /// Environment variable containing the RPC URL for the printed cast command.
    #[arg(long, default_value = "ETH_RPC_URL")]
    rpc_url_env: String,
    /// Environment variable containing the private key for the printed cast command.
    #[arg(long, default_value = "PRIVATE_KEY")]
    private_key_env: String,
}

#[derive(Debug, thiserror::Error)]
enum ProverCliError {
    #[error("open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read MessagePack proof job from {path}: {source}")]
    DecodeJob {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("read MessagePack guest input from {path}: {source}")]
    DecodeGuestInput {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("encode MessagePack artifact for {path}: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: rmp_serde::encode::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("proof file is empty: {path}")]
    EmptyProof { path: PathBuf },
    #[error(transparent)]
    ProofJob(#[from] sybil_witgen::ProofJobError),
    #[error("verify prepared guest input: {0}")]
    ZkTransition(#[from] sybil_zk::ZkTransitionError),
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), ProverCliError> {
    match cli.command {
        Command::Inspect(args) => inspect(args),
        Command::Prepare(args) => prepare(args),
        Command::SubmitStateRoot(args) => submit_state_root(args),
    }
}

fn inspect(args: JobPathArgs) -> Result<(), ProverCliError> {
    let job = read_job(&args.job)?;
    print_job_summary(&job);
    Ok(())
}

fn prepare(args: PrepareArgs) -> Result<(), ProverCliError> {
    let job = read_job(&args.job)?;
    let job_id = job.id();
    let guest_input = build_state_transition_guest_input(job)?;
    let public_input_hash = sybil_zk::verify_state_transition_input(&guest_input)?;

    write_msgpack_named(&args.guest_input, &guest_input)?;
    if let Some(path) = args.public_input_hash {
        write_hex_hash(&path, public_input_hash)?;
    }

    print_job_id(&job_id);
    println!("public_input_hash=0x{}", hex::encode(public_input_hash));
    println!("guest_input={}", args.guest_input.display());
    Ok(())
}

fn submit_state_root(args: SubmitStateRootArgs) -> Result<(), ProverCliError> {
    let guest_input = read_guest_input(&args.guest_input)?;
    let proof = read_proof(&args.proof)?;
    let calldata = submit_state_root_calldata(&guest_input.public_inputs, &proof);
    let public_input_hash =
        sybil_zk::state_transition_public_input_hash(&guest_input.public_inputs);

    write_hex_bytes(&args.calldata, &calldata)?;

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
    Ok(())
}

fn read_job(path: &Path) -> Result<StateTransitionProofJob, ProverCliError> {
    let file = File::open(path).map_err(|source| ProverCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    rmp_serde::from_read(reader).map_err(|source| ProverCliError::DecodeJob {
        path: path.to_path_buf(),
        source,
    })
}

fn read_guest_input(path: &Path) -> Result<sybil_zk::StateTransitionGuestInput, ProverCliError> {
    let file = File::open(path).map_err(|source| ProverCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    rmp_serde::from_read(reader).map_err(|source| ProverCliError::DecodeGuestInput {
        path: path.to_path_buf(),
        source,
    })
}

fn read_proof(path: &Path) -> Result<Vec<u8>, ProverCliError> {
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

fn write_msgpack_named<T: Serialize>(path: &Path, value: &T) -> Result<(), ProverCliError> {
    let bytes = rmp_serde::to_vec_named(value).map_err(|source| ProverCliError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, bytes).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn write_hex_bytes(path: &Path, bytes: &[u8]) -> Result<(), ProverCliError> {
    std::fs::write(path, format!("0x{}\n", hex::encode(bytes))).map_err(|source| {
        ProverCliError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn write_hex_hash(path: &Path, hash: [u8; 32]) -> Result<(), ProverCliError> {
    let mut file = File::create(path).map_err(|source| ProverCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    writeln!(file, "0x{}", hex::encode(hash)).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn print_job_summary(job: &StateTransitionProofJob) {
    print_job_id(&job.id());
    println!("format_version={}", job.format_version);
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("orders={}", job.witness.orders.len());
    println!("rejections={}", job.witness.rejections.len());
    println!("fills={}", job.witness.fills.len());
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

fn print_job_id(job_id: &StateTransitionProofJobId) {
    println!("block_height={}", job_id.block_height);
    println!("block_hash=0x{}", hex::encode(job_id.block_hash));
    println!("state_root=0x{}", hex::encode(job_id.state_root));
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sybil_verifier::{BlockWitness, StateSidecarSnapshot, WitnessBlockHeader};

    use super::*;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sybil-prover-{prefix}-{}-{unique}.msgpack",
            std::process::id()
        ))
    }

    fn minimal_job() -> StateTransitionProofJob {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 3,
                parent_hash: [1u8; 32],
                state_root: [2u8; 32],
                events_root: [3u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1_000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };
        StateTransitionProofJob::new(witness, vec![])
    }

    fn minimal_guest_input() -> sybil_zk::StateTransitionGuestInput {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 3,
                parent_hash: [1u8; 32],
                state_root: [2u8; 32],
                events_root: [3u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1_000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };
        sybil_zk::StateTransitionGuestInput {
            public_inputs: sybil_zk::StateTransitionPublicInputs {
                previous_height: 1,
                new_height: 3,
                previous_state_root: [4u8; 32],
                new_state_root: [5u8; 32],
                block_hash: [6u8; 32],
                events_root: [7u8; 32],
                witness_root: [8u8; 32],
                da_commitment: [0u8; 32],
                deposit_root: [9u8; 32],
                deposit_count: 11,
            },
            witness,
            state_root_proof: sybil_zk::QmdbStateRootProof {
                leaf_proofs: vec![],
            },
        }
    }

    #[test]
    fn reads_named_messagepack_proof_job() {
        let path = temp_path("job");
        let job = minimal_job();
        std::fs::write(&path, rmp_serde::to_vec_named(&job).unwrap()).unwrap();

        let decoded = read_job(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(decoded.id(), job.id());
        assert_eq!(decoded.state_leaf_proofs.len(), 0);
    }

    #[test]
    fn reads_named_messagepack_guest_input() {
        let path = temp_path("guest-input");
        let input = minimal_guest_input();
        std::fs::write(&path, rmp_serde::to_vec_named(&input).unwrap()).unwrap();

        let decoded = read_guest_input(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(decoded.public_inputs, input.public_inputs);
        assert_eq!(decoded.witness.header.height, input.witness.header.height);
        assert_eq!(
            decoded.witness.header.state_root,
            input.witness.header.state_root
        );
    }

    #[test]
    fn submit_state_root_calldata_uses_solidity_abi_layout() {
        let input = minimal_guest_input();
        let proof = b"proof";
        let calldata = submit_state_root_calldata(&input.public_inputs, proof);
        let expected_offset = ((STATE_TRANSITION_PUBLIC_INPUT_WORDS + 1) * ABI_WORD_BYTES) as u64;

        assert_eq!(
            function_selector(SUBMIT_STATE_ROOT_SIGNATURE),
            [0xf2, 0x33, 0x91, 0xb1]
        );
        assert_eq!(
            &calldata[..4],
            &function_selector(SUBMIT_STATE_ROOT_SIGNATURE)
        );
        assert_eq!(calldata.len(), 4 + 13 * ABI_WORD_BYTES);
        assert_eq!(
            &calldata[4 + 9 * ABI_WORD_BYTES + 24..4 + 10 * ABI_WORD_BYTES],
            &input.public_inputs.deposit_count.to_be_bytes()
        );
        assert_eq!(
            &calldata[4 + 10 * ABI_WORD_BYTES + 24..4 + 11 * ABI_WORD_BYTES],
            &expected_offset.to_be_bytes()
        );
        assert_eq!(
            &calldata[4 + 11 * ABI_WORD_BYTES + 24..4 + 12 * ABI_WORD_BYTES],
            &(proof.len() as u64).to_be_bytes()
        );
        assert_eq!(
            &calldata[4 + 12 * ABI_WORD_BYTES..4 + 12 * ABI_WORD_BYTES + proof.len()],
            proof
        );
        assert!(calldata[4 + 12 * ABI_WORD_BYTES + proof.len()..]
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn cast_send_command_reads_calldata_file() {
        let command = cast_send_data_command(
            "0x1234567890123456789012345678901234567890",
            Path::new("/tmp/state root.calldata"),
            "ETH_RPC_URL",
            "PRIVATE_KEY",
        );

        assert_eq!(
            command,
            "cast send '0x1234567890123456789012345678901234567890' --data \"$(cat '/tmp/state root.calldata')\" --rpc-url \"$ETH_RPC_URL\" --private-key \"$PRIVATE_KEY\""
        );
    }

    #[test]
    fn submit_state_root_writes_calldata_artifact() {
        let guest_input_path = temp_path("submit-guest-input");
        let proof_path = temp_path("submit-proof");
        let calldata_path = temp_path("submit-calldata");
        let input = minimal_guest_input();
        let proof = b"proof";
        std::fs::write(&guest_input_path, rmp_serde::to_vec_named(&input).unwrap()).unwrap();
        std::fs::write(&proof_path, proof).unwrap();

        submit_state_root(SubmitStateRootArgs {
            guest_input: guest_input_path.clone(),
            proof: proof_path.clone(),
            settlement: "0x1234567890123456789012345678901234567890".to_string(),
            calldata: calldata_path.clone(),
            rpc_url_env: "ETH_RPC_URL".to_string(),
            private_key_env: "PRIVATE_KEY".to_string(),
        })
        .unwrap();

        let calldata = std::fs::read_to_string(&calldata_path).unwrap();
        let _ = std::fs::remove_file(&guest_input_path);
        let _ = std::fs::remove_file(&proof_path);
        let _ = std::fs::remove_file(&calldata_path);

        assert_eq!(
            calldata.trim(),
            format!(
                "0x{}",
                hex::encode(submit_state_root_calldata(&input.public_inputs, proof))
            )
        );
    }
}
