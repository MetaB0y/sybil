use std::path::{Path, PathBuf};

use clap::Args;
use serde::Serialize;

use crate::artifacts::{
    hex32, print_job_id, read_guest_input, read_job, write_hex_hash, write_json_pretty,
    write_msgpack_named,
};
use crate::{
    build_state_transition_guest_input, ProverCliError, StateTransitionProofJob,
    StateTransitionProofJobId,
};

const FILE_DA_PROVIDER_REF_ENCODING: &str = "sybil-da-file-ref-v1";
const FILE_DA_PROVIDER_REF_DOMAIN: &[u8] = b"sybil/da/provider-ref/file/v1";

#[derive(Args)]
pub struct PrepareFileDaArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    pub job: PathBuf,
    /// Output path for MessagePack-encoded StateTransitionGuestInput.
    #[arg(long)]
    pub guest_input: PathBuf,
    /// Directory where canonical witness payload bytes will be written.
    #[arg(long)]
    pub payload_dir: PathBuf,
    /// Output path for the JSON DA manifest.
    #[arg(long)]
    pub manifest: PathBuf,
    /// Optional output path for the hex public input hash.
    #[arg(long)]
    pub public_input_hash: Option<PathBuf>,
}

#[derive(Args)]
pub struct PublishDaArgs {
    /// MessagePack-encoded StateTransitionGuestInput produced by `prepare-file-da`.
    #[arg(long)]
    pub guest_input: PathBuf,
    /// Output path for canonical witness payload bytes.
    #[arg(long)]
    pub payload: PathBuf,
    /// Output path for the JSON DA manifest.
    #[arg(long)]
    pub manifest: PathBuf,
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

pub struct PreparedFileDaArtifacts {
    pub job_id: StateTransitionProofJobId,
    pub public_input_hash: [u8; 32],
    pub da_commitment: [u8; 32],
    pub provider_ref_uri: String,
    pub payload_path: PathBuf,
    pub guest_input_path: PathBuf,
    pub manifest_path: PathBuf,
    pub public_input_hash_path: Option<PathBuf>,
}

pub fn prepare_file_da(args: PrepareFileDaArgs) -> Result<(), ProverCliError> {
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

pub fn prepare_file_da_job(
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

pub fn publish_da(args: PublishDaArgs) -> Result<(), ProverCliError> {
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

pub fn write_da_artifacts(
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
        payload_encoding: "sybil-canonical-witness-v3",
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
