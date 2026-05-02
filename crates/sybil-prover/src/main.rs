use std::fs::File;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use sybil_witgen::{
    build_state_transition_guest_input, StateTransitionProofJob, StateTransitionProofJobId,
};

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
    #[error(transparent)]
    ProofJob(#[from] sybil_witgen::ProofJobError),
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
    let public_input_hash =
        sybil_zk::state_transition_public_input_hash(&guest_input.public_inputs);

    write_msgpack_named(&args.guest_input, &guest_input)?;
    if let Some(path) = args.public_input_hash {
        write_hex_hash(&path, public_input_hash)?;
    }

    print_job_id(&job_id);
    println!("public_input_hash=0x{}", hex::encode(public_input_hash));
    println!("guest_input={}", args.guest_input.display());
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

fn print_job_id(job_id: &StateTransitionProofJobId) {
    println!("block_height={}", job_id.block_height);
    println!("block_hash=0x{}", hex::encode(job_id.block_hash));
    println!("state_root=0x{}", hex::encode(job_id.state_root));
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
}
