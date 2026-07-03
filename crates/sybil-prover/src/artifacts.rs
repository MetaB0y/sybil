use std::fs::File;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Args;
use serde::{Deserialize, Serialize};

use crate::{
    build_state_transition_guest_input, ProverCliError, StateTransitionProofJob,
    StateTransitionProofJobId,
};

#[derive(Args)]
pub struct JobPathArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    pub job: PathBuf,
}

#[derive(Args)]
pub struct PrepareArgs {
    /// MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    pub job: PathBuf,
    /// Output path for MessagePack-encoded StateTransitionGuestInput.
    #[arg(long)]
    pub guest_input: PathBuf,
    /// Optional output path for the hex public input hash.
    #[arg(long)]
    pub public_input_hash: Option<PathBuf>,
}

#[derive(Args)]
pub struct WorkerArgs {
    /// Directory containing MessagePack-encoded StateTransitionProofJob files.
    #[arg(long)]
    pub jobs_dir: PathBuf,
    /// Directory where per-block prover artifacts and status JSON are written.
    #[arg(long)]
    pub artifacts_dir: PathBuf,
    /// Poll interval for service mode.
    #[arg(long, default_value_t = 1_000)]
    pub poll_ms: u64,
    /// Run one scan and exit.
    #[arg(long, default_value_t = false)]
    pub once: bool,
    /// Optional cap on jobs processed per scan.
    #[arg(long)]
    pub max_jobs: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkerStatusJson {
    pub version: u8,
    /// Which producer wrote this artifact (`worker`, `mock-live`, ...). Older
    /// status files predate the field, so decode falls back to `unknown`.
    #[serde(default = "unknown_status_producer")]
    pub producer: String,
    pub status: String,
    pub job_path: String,
    pub artifact_dir: String,
    pub block_height: u64,
    pub block_hash: String,
    pub state_root: String,
    pub public_input_hash: String,
    pub da_commitment: String,
    pub da_provider_ref: String,
    pub da_payload: String,
    pub guest_input: String,
    pub da_manifest: String,
    pub public_input_hash_path: Option<String>,
    pub proof_status: String,
    pub updated_at_ms: u128,
}

pub fn inspect(args: JobPathArgs) -> Result<(), ProverCliError> {
    let job = read_job(&args.job)?;
    print_job_summary(&job);
    Ok(())
}

pub fn prepare(args: PrepareArgs) -> Result<(), ProverCliError> {
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

pub fn run_worker(args: WorkerArgs) -> Result<(), ProverCliError> {
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

pub fn discover_proof_jobs(jobs_dir: &Path) -> Result<Vec<PathBuf>, ProverCliError> {
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
    let artifacts = crate::da::prepare_file_da_job(
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

pub fn worker_artifact_dir(artifacts_dir: &Path, job_id: &StateTransitionProofJobId) -> PathBuf {
    artifacts_dir.join(format!(
        "block-{:020}-{}",
        job_id.block_height,
        hex::encode(job_id.block_hash)
    ))
}

fn worker_status_json(
    job_path: &Path,
    artifact_dir: &Path,
    artifacts: &crate::da::PreparedFileDaArtifacts,
) -> WorkerStatusJson {
    WorkerStatusJson {
        version: 1,
        producer: "worker".to_string(),
        status: "prepared".to_string(),
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
        proof_status: "not_started".to_string(),
        updated_at_ms: unix_time_ms(),
    }
}

pub fn read_job(path: &Path) -> Result<StateTransitionProofJob, ProverCliError> {
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

pub fn read_guest_input(
    path: &Path,
) -> Result<sybil_zk::StateTransitionGuestInput, ProverCliError> {
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

pub fn write_msgpack_named<T: Serialize>(path: &Path, value: &T) -> Result<(), ProverCliError> {
    let bytes = rmp_serde::to_vec_named(value).map_err(|source| ProverCliError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, bytes).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_hex_bytes(path: &Path, bytes: &[u8]) -> Result<(), ProverCliError> {
    std::fs::write(path, format!("0x{}\n", hex::encode(bytes))).map_err(|source| {
        ProverCliError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

pub fn write_hex_hash(path: &Path, hash: [u8; 32]) -> Result<(), ProverCliError> {
    let mut file = File::create(path).map_err(|source| ProverCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    writeln!(file, "0x{}", hex::encode(hash)).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), ProverCliError> {
    let json = serde_json::to_vec_pretty(value).map_err(|source| ProverCliError::EncodeJson {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, json).map_err(|source| ProverCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// serde default for [`WorkerStatusJson::producer`] on artifacts written before
/// the field existed.
pub fn unknown_status_producer() -> String {
    "unknown".to_string()
}

pub fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn print_job_id(job_id: &StateTransitionProofJobId) {
    println!("block_height={}", job_id.block_height);
    println!("block_hash=0x{}", hex::encode(job_id.block_hash));
    println!("state_root=0x{}", hex::encode(job_id.state_root));
}

fn print_job_summary(job: &StateTransitionProofJob) {
    print_job_id(&job.id());
    println!("format_version={}", job.format_version);
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("orders={}", job.witness.orders.len());
    println!("rejections={}", job.witness.rejections.len());
    println!("fills={}", job.witness.fills.len());
}
