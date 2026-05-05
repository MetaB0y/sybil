use std::fs::File;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha3::{Digest as _, Keccak256};
use sybil_witgen::{
    build_state_transition_guest_input, StateTransitionProofJob, StateTransitionProofJobId,
};

const SUBMIT_STATE_ROOT_SIGNATURE: &str =
    "submitStateRoot((uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64),bytes)";
const STATE_TRANSITION_PUBLIC_INPUT_WORDS: usize = 10;
const ABI_WORD_BYTES: usize = 32;
const SHELL_SAFE_CALLDATA_BYTES: usize = 128 * 1024;
const OPENVM_EVM_ADAPTER_PROOF_WORDS: usize = 4;
const FILE_DA_PROVIDER_REF_ENCODING: &str = "sybil-da-file-ref-v1";
const FILE_DA_PROVIDER_REF_DOMAIN: &[u8] = b"sybil/da/provider-ref/file/v1";

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
    /// Validate a proof job, bind a file DA provider ref, and write proof artifacts.
    PrepareFileDa(PrepareFileDaArgs),
    /// Write the file-backed DA payload and manifest for a prepared guest input.
    PublishDa(PublishDaArgs),
    /// Run a local filesystem prover worker over exported proof jobs.
    Worker(WorkerArgs),
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
struct PrepareFileDaArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
    /// Output path for MessagePack-encoded StateTransitionGuestInput.
    #[arg(long)]
    guest_input: PathBuf,
    /// Directory where canonical witness payload bytes will be written.
    #[arg(long)]
    payload_dir: PathBuf,
    /// Output path for the JSON DA manifest.
    #[arg(long)]
    manifest: PathBuf,
    /// Optional output path for the hex public input hash.
    #[arg(long)]
    public_input_hash: Option<PathBuf>,
}

#[derive(Args)]
struct PublishDaArgs {
    /// MessagePack-encoded StateTransitionGuestInput produced by `prepare`.
    #[arg(long)]
    guest_input: PathBuf,
    /// Output path for canonical witness payload bytes.
    #[arg(long)]
    payload: PathBuf,
    /// Output path for the JSON DA manifest.
    #[arg(long)]
    manifest: PathBuf,
}

#[derive(Args)]
struct WorkerArgs {
    /// Directory containing MessagePack-encoded StateTransitionProofJob files.
    #[arg(long)]
    jobs_dir: PathBuf,
    /// Directory where per-block prover artifacts and status JSON are written.
    #[arg(long)]
    artifacts_dir: PathBuf,
    /// Poll interval for service mode.
    #[arg(long, default_value_t = 1_000)]
    poll_ms: u64,
    /// Run one scan and exit.
    #[arg(long, default_value_t = false)]
    once: bool,
    /// Optional cap on jobs processed per scan.
    #[arg(long)]
    max_jobs: Option<usize>,
}

#[derive(Args)]
struct SubmitStateRootArgs {
    /// MessagePack-encoded StateTransitionGuestInput produced by `prepare`.
    #[arg(long)]
    guest_input: PathBuf,
    /// OpenVM proof bytes to submit.
    #[arg(long)]
    proof: PathBuf,
    /// Proof file format. `openvm-evm-json` converts OpenVM's EVM proof JSON
    /// into the ABI payload expected by OpenVmVerifierAdapter.
    #[arg(long, value_enum, default_value_t = ProofFormat::Raw)]
    proof_format: ProofFormat,
    /// Deployed SybilSettlement address.
    #[arg(long)]
    settlement: String,
    /// Output path for hex calldata accepted by `cast send --data`.
    #[arg(long, default_value = "/tmp/sybil-submit-state-root.calldata")]
    calldata: PathBuf,
    /// Optional output path for an eth_sendTransaction JSON-RPC request.
    #[arg(long)]
    rpc_request: Option<PathBuf>,
    /// Sender address to include in the optional eth_sendTransaction request.
    #[arg(long)]
    from: Option<String>,
    /// Optional gas limit to include in the eth_sendTransaction request.
    #[arg(long)]
    gas: Option<String>,
    /// Environment variable containing the RPC URL for the printed cast command.
    #[arg(long, default_value = "ETH_RPC_URL")]
    rpc_url_env: String,
    /// Environment variable containing the private key for the printed cast command.
    #[arg(long, default_value = "PRIVATE_KEY")]
    private_key_env: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ProofFormat {
    Raw,
    #[value(name = "openvm-evm-json")]
    OpenVmEvmJson,
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
    #[error("create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("list directory {path}: {source}")]
    ListDir {
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
    #[error("read OpenVM EVM proof JSON from {path}: {source}")]
    DecodeOpenVmEvmProof {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("decode hex field {field}: {source}")]
    DecodeHex {
        field: &'static str,
        #[source]
        source: hex::FromHexError,
    },
    #[error("field {field} must be 32 bytes, got {actual}")]
    InvalidBytes32Field { field: &'static str, actual: usize },
    #[error("proof file is empty: {path}")]
    EmptyProof { path: PathBuf },
    #[error("--from is required when --rpc-request is set")]
    MissingRpcRequestFrom,
    #[error("encode JSON artifact for {path}: {source}")]
    EncodeJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    ProofJob(#[from] sybil_witgen::ProofJobError),
    #[error("verify prepared guest input: {0}")]
    ZkTransition(#[from] sybil_zk::ZkTransitionError),
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

#[derive(Serialize)]
struct DaManifestJson {
    version: u8,
    payload_kind: &'static str,
    payload_encoding: &'static str,
    provider_refs_encoding: &'static str,
    block_height: u64,
    block_hash: String,
    state_root: String,
    witness_root: String,
    payload_root: String,
    payload_len: u64,
    provider_refs_hash: String,
    provider_refs: Vec<DaProviderRefJson>,
    da_commitment: String,
    public_input_hash: String,
    local_payload_path: String,
    local_payload_path_proof_bound: bool,
}

#[derive(Clone, Serialize)]
struct DaProviderRefJson {
    kind: &'static str,
    encoding: &'static str,
    bytes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_len: Option<u64>,
}

struct FileDaProviderRef {
    uri: String,
    payload_path: PathBuf,
    canonical_bytes: Vec<u8>,
    manifest_ref: DaProviderRefJson,
}

struct PreparedFileDaArtifacts {
    job_id: StateTransitionProofJobId,
    public_input_hash: [u8; 32],
    da_commitment: [u8; 32],
    provider_ref_uri: String,
    payload_path: PathBuf,
    guest_input_path: PathBuf,
    manifest_path: PathBuf,
    public_input_hash_path: Option<PathBuf>,
}

#[derive(Serialize)]
struct WorkerStatusJson {
    version: u8,
    status: &'static str,
    job_path: String,
    artifact_dir: String,
    block_height: u64,
    block_hash: String,
    state_root: String,
    public_input_hash: String,
    da_commitment: String,
    da_provider_ref: String,
    da_payload: String,
    guest_input: String,
    da_manifest: String,
    public_input_hash_path: Option<String>,
    proof_status: &'static str,
    updated_at_ms: u128,
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
        Command::PrepareFileDa(args) => prepare_file_da(args),
        Command::PublishDa(args) => publish_da(args),
        Command::Worker(args) => run_worker(args),
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

fn prepare_file_da(args: PrepareFileDaArgs) -> Result<(), ProverCliError> {
    let job = read_job(&args.job)?;
    let artifacts = prepare_file_da_job(
        job,
        &args.guest_input,
        &args.payload_dir,
        &args.manifest,
        args.public_input_hash.as_deref(),
    )?;

    print_job_id(&artifacts.job_id);
    println!(
        "public_input_hash=0x{}",
        hex::encode(artifacts.public_input_hash)
    );
    println!("da_commitment=0x{}", hex::encode(artifacts.da_commitment));
    println!("da_provider_ref={}", artifacts.provider_ref_uri);
    println!("da_payload={}", artifacts.payload_path.display());
    println!("da_manifest={}", args.manifest.display());
    println!("guest_input={}", args.guest_input.display());
    Ok(())
}

fn prepare_file_da_job(
    job: StateTransitionProofJob,
    guest_input_path: &Path,
    payload_dir: &Path,
    manifest_path: &Path,
    public_input_hash_path: Option<&Path>,
) -> Result<PreparedFileDaArtifacts, ProverCliError> {
    let job_id = job.id();
    let mut guest_input = build_state_transition_guest_input(job)?;
    let payload = sybil_zk::da_witness_payload_bytes(&guest_input.witness);
    let payload_root = sybil_zk::da_witness_payload_root(&payload);
    let provider_ref = file_da_provider_ref(payload_dir, payload_root, payload.len() as u64)?;

    guest_input.da_provider_refs = vec![provider_ref.canonical_bytes.clone()];
    guest_input.public_inputs = sybil_zk::public_inputs_from_witness_and_provider_refs(
        &guest_input.witness,
        &guest_input.da_provider_refs,
    );
    let public_input_hash = sybil_zk::verify_state_transition_input(&guest_input)?;
    let da_commitment = guest_input.public_inputs.da_commitment;

    write_msgpack_named(guest_input_path, &guest_input)?;
    write_da_artifacts_with_payload(
        &guest_input,
        public_input_hash,
        &payload,
        &provider_ref.payload_path,
        manifest_path,
        vec![provider_ref.manifest_ref.clone()],
        false,
    )?;
    if let Some(path) = public_input_hash_path {
        write_hex_hash(path, public_input_hash)?;
    }

    Ok(PreparedFileDaArtifacts {
        job_id,
        public_input_hash,
        da_commitment,
        provider_ref_uri: provider_ref.uri,
        payload_path: provider_ref.payload_path,
        guest_input_path: guest_input_path.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        public_input_hash_path: public_input_hash_path.map(Path::to_path_buf),
    })
}

fn publish_da(args: PublishDaArgs) -> Result<(), ProverCliError> {
    let guest_input = read_guest_input(&args.guest_input)?;
    let public_input_hash = sybil_zk::verify_state_transition_input(&guest_input)?;
    write_da_artifacts(
        &guest_input,
        public_input_hash,
        &args.payload,
        &args.manifest,
    )?;

    println!("block_height={}", guest_input.public_inputs.new_height);
    println!(
        "block_hash=0x{}",
        hex::encode(guest_input.public_inputs.block_hash)
    );
    println!(
        "state_root=0x{}",
        hex::encode(guest_input.public_inputs.new_state_root)
    );
    println!(
        "da_commitment=0x{}",
        hex::encode(guest_input.public_inputs.da_commitment)
    );
    println!("public_input_hash=0x{}", hex::encode(public_input_hash));
    println!("da_payload={}", args.payload.display());
    println!("da_manifest={}", args.manifest.display());
    Ok(())
}

fn run_worker(args: WorkerArgs) -> Result<(), ProverCliError> {
    std::fs::create_dir_all(&args.jobs_dir).map_err(|source| ProverCliError::CreateDir {
        path: args.jobs_dir.clone(),
        source,
    })?;
    std::fs::create_dir_all(&args.artifacts_dir).map_err(|source| ProverCliError::CreateDir {
        path: args.artifacts_dir.clone(),
        source,
    })?;

    loop {
        let processed = process_worker_scan(&args)?;
        println!("worker_processed={processed}");
        if args.once {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(args.poll_ms));
    }
}

fn process_worker_scan(args: &WorkerArgs) -> Result<usize, ProverCliError> {
    let jobs = discover_proof_jobs(&args.jobs_dir)?;
    let mut processed = 0usize;
    for job_path in jobs {
        if args.max_jobs.is_some_and(|max_jobs| processed >= max_jobs) {
            break;
        }
        if process_worker_job(&job_path, &args.artifacts_dir)? {
            processed += 1;
        }
    }
    Ok(processed)
}

fn discover_proof_jobs(jobs_dir: &Path) -> Result<Vec<PathBuf>, ProverCliError> {
    let entries = std::fs::read_dir(jobs_dir).map_err(|source| ProverCliError::ListDir {
        path: jobs_dir.to_path_buf(),
        source,
    })?;
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProverCliError::ListDir {
            path: jobs_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "msgpack")
        {
            jobs.push(path);
        }
    }
    jobs.sort();
    Ok(jobs)
}

fn process_worker_job(job_path: &Path, artifacts_dir: &Path) -> Result<bool, ProverCliError> {
    let job = read_job(job_path)?;
    let job_id = job.id();
    let artifact_dir = worker_artifact_dir(artifacts_dir, &job_id);
    let status_path = artifact_dir.join("status.json");
    if status_path.exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(&artifact_dir).map_err(|source| ProverCliError::CreateDir {
        path: artifact_dir.clone(),
        source,
    })?;
    let guest_input = artifact_dir.join("guest-input.msgpack");
    let payload_dir = artifact_dir.join("da");
    let manifest = artifact_dir.join("da-manifest.json");
    let public_input_hash = artifact_dir.join("public-input-hash.hex");
    let artifacts = prepare_file_da_job(
        job,
        &guest_input,
        &payload_dir,
        &manifest,
        Some(&public_input_hash),
    )?;
    let status = worker_status_json(job_path, &artifact_dir, &artifacts);
    write_json_pretty(&status_path, &status)?;

    println!("worker_job={}", job_path.display());
    println!("worker_status=prepared");
    println!("artifact_dir={}", artifact_dir.display());
    println!(
        "public_input_hash=0x{}",
        hex::encode(artifacts.public_input_hash)
    );
    Ok(true)
}

fn worker_artifact_dir(artifacts_dir: &Path, job_id: &StateTransitionProofJobId) -> PathBuf {
    artifacts_dir.join(format!(
        "block-{:020}-{}",
        job_id.block_height,
        hex::encode(job_id.block_hash)
    ))
}

fn worker_status_json(
    job_path: &Path,
    artifact_dir: &Path,
    artifacts: &PreparedFileDaArtifacts,
) -> WorkerStatusJson {
    WorkerStatusJson {
        version: 1,
        status: "prepared",
        job_path: job_path.display().to_string(),
        artifact_dir: artifact_dir.display().to_string(),
        block_height: artifacts.job_id.block_height,
        block_hash: hex32(artifacts.job_id.block_hash),
        state_root: hex32(artifacts.job_id.state_root),
        public_input_hash: hex32(artifacts.public_input_hash),
        da_commitment: hex32(artifacts.da_commitment),
        da_provider_ref: artifacts.provider_ref_uri.clone(),
        da_payload: artifacts.payload_path.display().to_string(),
        guest_input: artifacts.guest_input_path.display().to_string(),
        da_manifest: artifacts.manifest_path.display().to_string(),
        public_input_hash_path: artifacts
            .public_input_hash_path
            .as_ref()
            .map(|path| path.display().to_string()),
        proof_status: "not_started",
        updated_at_ms: unix_time_ms(),
    }
}

fn submit_state_root(args: SubmitStateRootArgs) -> Result<(), ProverCliError> {
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

fn write_da_artifacts(
    guest_input: &sybil_zk::StateTransitionGuestInput,
    public_input_hash: [u8; 32],
    payload_path: &Path,
    manifest_path: &Path,
) -> Result<(), ProverCliError> {
    let payload = sybil_zk::da_witness_payload_bytes(&guest_input.witness);
    let provider_refs = raw_provider_refs_json(&guest_input.da_provider_refs);
    write_da_artifacts_with_payload(
        guest_input,
        public_input_hash,
        &payload,
        payload_path,
        manifest_path,
        provider_refs,
        false,
    )
}

fn write_da_artifacts_with_payload(
    guest_input: &sybil_zk::StateTransitionGuestInput,
    public_input_hash: [u8; 32],
    payload: &[u8],
    payload_path: &Path,
    manifest_path: &Path,
    provider_refs: Vec<DaProviderRefJson>,
    local_payload_path_proof_bound: bool,
) -> Result<(), ProverCliError> {
    let components = sybil_zk::da_commitment_components_from_payload_and_provider_refs(
        &guest_input.witness,
        payload,
        &guest_input.da_provider_refs,
    );
    let manifest = da_manifest_json(
        guest_input,
        &components,
        public_input_hash,
        payload_path,
        provider_refs,
        local_payload_path_proof_bound,
    );

    if let Some(parent) = payload_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ProverCliError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(payload_path, payload).map_err(|source| ProverCliError::Write {
        path: payload_path.to_path_buf(),
        source,
    })?;
    write_json_pretty(manifest_path, &manifest)
}

fn da_manifest_json(
    guest_input: &sybil_zk::StateTransitionGuestInput,
    components: &sybil_zk::DaCommitmentComponents,
    public_input_hash: [u8; 32],
    payload_path: &Path,
    provider_refs: Vec<DaProviderRefJson>,
    local_payload_path_proof_bound: bool,
) -> DaManifestJson {
    DaManifestJson {
        version: 1,
        payload_kind: "block_witness",
        payload_encoding: "sybil-canonical-witness-v1",
        provider_refs_encoding: if provider_refs.is_empty() {
            "empty-v1"
        } else {
            "bytes-v1"
        },
        block_height: components.block_height,
        block_hash: hex32(guest_input.public_inputs.block_hash),
        state_root: hex32(components.state_root),
        witness_root: hex32(components.witness_root),
        payload_root: hex32(components.payload_root),
        payload_len: components.payload_len,
        provider_refs_hash: hex32(components.provider_refs_hash),
        provider_refs,
        da_commitment: hex32(components.da_commitment),
        public_input_hash: hex32(public_input_hash),
        local_payload_path: payload_path.display().to_string(),
        local_payload_path_proof_bound,
    }
}

fn file_da_provider_ref(
    payload_dir: &Path,
    payload_root: [u8; 32],
    payload_len: u64,
) -> Result<FileDaProviderRef, ProverCliError> {
    std::fs::create_dir_all(payload_dir).map_err(|source| ProverCliError::CreateDir {
        path: payload_dir.to_path_buf(),
        source,
    })?;
    let filename = format!("{}.witness.bin", hex::encode(payload_root));
    let payload_path = payload_dir.join(filename);
    let uri = format!(
        "sybil-file://witness/{}.witness.bin",
        hex::encode(payload_root)
    );
    let canonical_bytes = file_da_provider_ref_bytes(&uri, payload_root, payload_len);
    let manifest_ref = DaProviderRefJson {
        kind: "file",
        encoding: FILE_DA_PROVIDER_REF_ENCODING,
        bytes: format!("0x{}", hex::encode(&canonical_bytes)),
        uri: Some(uri.clone()),
        payload_root: Some(hex32(payload_root)),
        payload_len: Some(payload_len),
    };

    Ok(FileDaProviderRef {
        uri,
        payload_path,
        canonical_bytes,
        manifest_ref,
    })
}

fn file_da_provider_ref_bytes(uri: &str, payload_root: [u8; 32], payload_len: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        FILE_DA_PROVIDER_REF_DOMAIN.len() + 8 + uri.len() + payload_root.len() + 8,
    );
    bytes.extend_from_slice(FILE_DA_PROVIDER_REF_DOMAIN);
    bytes.extend_from_slice(&(uri.len() as u64).to_le_bytes());
    bytes.extend_from_slice(uri.as_bytes());
    bytes.extend_from_slice(&payload_root);
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes
}

fn raw_provider_refs_json(provider_refs: &[Vec<u8>]) -> Vec<DaProviderRefJson> {
    provider_refs
        .iter()
        .map(|provider_ref| DaProviderRefJson {
            kind: "raw",
            encoding: "raw-bytes",
            bytes: format!("0x{}", hex::encode(provider_ref)),
            uri: None,
            payload_root: None,
            payload_len: None,
        })
        .collect()
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), ProverCliError> {
    let json = serde_json::to_vec_pretty(value).map_err(|source| ProverCliError::EncodeJson {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, json).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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
        let public_inputs = sybil_zk::public_inputs_from_witness(&witness);
        sybil_zk::StateTransitionGuestInput {
            public_inputs,
            witness,
            da_provider_refs: vec![],
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
    fn openvm_evm_adapter_proof_uses_solidity_abi_layout() {
        let public_values = vec![0x11; 64];
        let proof_data = vec![0x22; 65];
        let app_exe_commit = [0x33; 32];
        let app_vm_commit = [0x44; 32];
        let encoded =
            openvm_evm_adapter_proof(&public_values, &proof_data, &app_exe_commit, &app_vm_commit);

        let proof_data_offset = (OPENVM_EVM_ADAPTER_PROOF_WORDS * ABI_WORD_BYTES)
            + ABI_WORD_BYTES
            + padded_abi_len(public_values.len());

        assert_eq!(
            &encoded[24..32],
            &((OPENVM_EVM_ADAPTER_PROOF_WORDS * ABI_WORD_BYTES) as u64).to_be_bytes()
        );
        assert_eq!(
            &encoded[ABI_WORD_BYTES + 24..2 * ABI_WORD_BYTES],
            &(proof_data_offset as u64).to_be_bytes()
        );
        assert_eq!(
            &encoded[2 * ABI_WORD_BYTES..3 * ABI_WORD_BYTES],
            &app_exe_commit
        );
        assert_eq!(
            &encoded[3 * ABI_WORD_BYTES..4 * ABI_WORD_BYTES],
            &app_vm_commit
        );
        assert_eq!(
            &encoded[4 * ABI_WORD_BYTES + 24..5 * ABI_WORD_BYTES],
            &(public_values.len() as u64).to_be_bytes()
        );
        assert_eq!(
            &encoded[5 * ABI_WORD_BYTES..5 * ABI_WORD_BYTES + public_values.len()],
            public_values.as_slice()
        );
        assert_eq!(
            &encoded[proof_data_offset + 24..proof_data_offset + ABI_WORD_BYTES],
            &(proof_data.len() as u64).to_be_bytes()
        );
        assert_eq!(
            &encoded[proof_data_offset + ABI_WORD_BYTES
                ..proof_data_offset + ABI_WORD_BYTES + proof_data.len()],
            proof_data.as_slice()
        );
    }

    #[test]
    fn reads_openvm_evm_json_as_adapter_proof() {
        let path = temp_path("openvm-evm-proof");
        let json = serde_json::json!({
            "version": "v2.0.0",
            "app_exe_commit": format!("0x{}", "33".repeat(32)),
            "app_vm_commit": "44".repeat(32),
            "user_public_values": "11".repeat(64),
            "proof_data": {
                "accumulator": "22".repeat(12 * 32),
                "proof": "55".repeat(43 * 32),
            }
        });
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        let proof = read_proof(&path, ProofFormat::OpenVmEvmJson).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(&proof[2 * ABI_WORD_BYTES..3 * ABI_WORD_BYTES], &[0x33; 32]);
        assert_eq!(&proof[3 * ABI_WORD_BYTES..4 * ABI_WORD_BYTES], &[0x44; 32]);
        let proof_data_offset = u64::from_be_bytes(
            proof[ABI_WORD_BYTES + 24..2 * ABI_WORD_BYTES]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(
            &proof[proof_data_offset + 24..proof_data_offset + ABI_WORD_BYTES],
            &((55 * 32) as u64).to_be_bytes()
        );
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
    fn curl_rpc_request_command_reads_request_file() {
        let command = curl_rpc_request_command(Path::new("/tmp/state root.json"), "ETH_RPC_URL");

        assert_eq!(
            command,
            "curl -sS -H 'content-type: application/json' --data-binary @'/tmp/state root.json' \"$ETH_RPC_URL\""
        );
    }

    #[test]
    fn writes_eth_send_transaction_request_artifact() {
        let path = temp_path("rpc-request");
        let calldata = [0xf2, 0x33, 0x91, 0xb1];

        write_eth_send_transaction_request(
            &path,
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "0x1234567890123456789012345678901234567890",
            Some("0x1c9c380"),
            &calldata,
        )
        .unwrap();

        let json = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let request: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["method"], "eth_sendTransaction");
        assert_eq!(
            request["params"][0]["from"],
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
        assert_eq!(
            request["params"][0]["to"],
            "0x1234567890123456789012345678901234567890"
        );
        assert_eq!(request["params"][0]["gas"], "0x1c9c380");
        assert_eq!(request["params"][0]["data"], "0xf23391b1");
    }

    #[test]
    fn writes_da_payload_and_manifest_artifacts() {
        let payload_path = temp_path("da-payload");
        let manifest_path = temp_path("da-manifest");
        let input = minimal_guest_input();
        let public_input_hash = sybil_zk::state_transition_public_input_hash(&input.public_inputs);

        write_da_artifacts(&input, public_input_hash, &payload_path, &manifest_path).unwrap();

        let payload = std::fs::read(&payload_path).unwrap();
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let _ = std::fs::remove_file(&payload_path);
        let _ = std::fs::remove_file(&manifest_path);
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let components = sybil_zk::da_commitment_components_from_payload(&input.witness, &payload);

        assert_eq!(payload, sybil_zk::da_witness_payload_bytes(&input.witness));
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["payload_kind"], "block_witness");
        assert_eq!(
            manifest["da_commitment"],
            hex32(input.public_inputs.da_commitment)
        );
        assert_eq!(manifest["payload_root"], hex32(components.payload_root));
        assert_eq!(manifest["payload_len"], payload.len() as u64);
        assert_eq!(manifest["provider_refs"].as_array().unwrap().len(), 0);
        assert_eq!(manifest["local_payload_path_proof_bound"], false);
    }

    #[test]
    fn writes_proof_bound_file_da_manifest() {
        let payload_dir = temp_path("file-da-dir");
        let manifest_path = temp_path("file-da-manifest");
        let mut input = minimal_guest_input();
        let payload = sybil_zk::da_witness_payload_bytes(&input.witness);
        let payload_root = sybil_zk::da_witness_payload_root(&payload);
        let provider_ref =
            file_da_provider_ref(&payload_dir, payload_root, payload.len() as u64).unwrap();
        input.da_provider_refs = vec![provider_ref.canonical_bytes.clone()];
        input.public_inputs = sybil_zk::public_inputs_from_witness_and_provider_refs(
            &input.witness,
            &input.da_provider_refs,
        );
        let public_input_hash = sybil_zk::state_transition_public_input_hash(&input.public_inputs);

        write_da_artifacts_with_payload(
            &input,
            public_input_hash,
            &payload,
            &provider_ref.payload_path,
            &manifest_path,
            vec![provider_ref.manifest_ref],
            false,
        )
        .unwrap();

        let payload_written = std::fs::read(&provider_ref.payload_path).unwrap();
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let _ = std::fs::remove_file(&provider_ref.payload_path);
        let _ = std::fs::remove_file(&manifest_path);
        let _ = std::fs::remove_dir(&payload_dir);
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();

        assert_eq!(payload_written, payload);
        assert_eq!(
            manifest["provider_refs_hash"],
            hex32(sybil_zk::da_provider_refs_hash(&input.da_provider_refs))
        );
        assert_eq!(
            manifest["da_commitment"],
            hex32(input.public_inputs.da_commitment)
        );
        assert_eq!(manifest["provider_refs"][0]["kind"], "file");
        assert_eq!(
            manifest["provider_refs"][0]["encoding"],
            FILE_DA_PROVIDER_REF_ENCODING
        );
        assert_eq!(manifest["local_payload_path_proof_bound"], false);
    }

    #[test]
    fn discovers_msgpack_jobs_in_stable_order() {
        let jobs_dir = temp_path("worker-jobs");
        std::fs::create_dir_all(&jobs_dir).unwrap();
        let second = jobs_dir.join("b.msgpack");
        let first = jobs_dir.join("a.msgpack");
        let ignored = jobs_dir.join("note.txt");
        std::fs::write(&second, b"second").unwrap();
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&ignored, b"ignored").unwrap();

        let jobs = discover_proof_jobs(&jobs_dir).unwrap();
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
        let _ = std::fs::remove_file(&ignored);
        let _ = std::fs::remove_dir(&jobs_dir);

        assert_eq!(jobs, vec![first, second]);
    }

    #[test]
    fn worker_artifact_dir_is_height_and_block_hash_stable() {
        let artifacts_dir = Path::new("/tmp/prover-artifacts");
        let job_id = StateTransitionProofJobId {
            block_height: 42,
            block_hash: [0xab; 32],
            state_root: [0xcd; 32],
        };

        assert_eq!(
            worker_artifact_dir(artifacts_dir, &job_id),
            artifacts_dir.join(format!("block-{:020}-{}", 42, "ab".repeat(32)))
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
            rpc_request: None,
            from: None,
            gas: None,
            proof_format: ProofFormat::Raw,
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
