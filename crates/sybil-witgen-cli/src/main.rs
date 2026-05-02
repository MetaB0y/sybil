use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use matching_engine::MarketSet;
use matching_sequencer::store::Store;
use matching_sequencer::{AccountStore, AdminOracle, BlockSequencer, SequencerConfig};
use sybil_witgen::{collect_state_transition_proof_job, StateTransitionProofJobId};

#[derive(Parser)]
#[command(name = "sybil-witgen")]
#[command(about = "Sybil proof-job export tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Export the latest committed block as a state-transition proof job.
    ExportLatest(ExportLatestArgs),
    /// Create a one-block local smoke fixture and export its proof job.
    SmokeJob(SmokeJobArgs),
}

#[derive(Args)]
struct ExportLatestArgs {
    /// Path to the sequencer redb store, usually data/sybil.redb.
    #[arg(long)]
    store: PathBuf,
    /// Output path for the MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
}

#[derive(Args)]
struct SmokeJobArgs {
    /// Path to the sequencer redb store to create.
    #[arg(long)]
    store: PathBuf,
    /// Output path for the MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
    /// Timestamp to use for the single smoke block.
    #[arg(long, default_value_t = 1_000)]
    timestamp_ms: u64,
}

#[derive(Debug, thiserror::Error)]
enum WitgenCliError {
    #[error("sequencer store does not exist: {path}")]
    StoreNotFound { path: PathBuf },
    #[error("refusing to overwrite existing smoke store: {path}")]
    SmokeStoreExists { path: PathBuf },
    #[error("open sequencer store {path}: {source}")]
    OpenStore {
        path: PathBuf,
        #[source]
        source: matching_sequencer::store::StoreError,
    },
    #[error("sequencer store has no persisted latest block witness")]
    MissingLatestWitness,
    #[error("collect proof job: {0}")]
    CollectProofJob(#[from] sybil_witgen::SequencerStoreWitgenError),
    #[error("encode MessagePack proof job for {path}: {source}")]
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
    #[error("read latest block witness: {0}")]
    ReadWitness(#[source] matching_sequencer::store::StoreError),
    #[error("persist smoke block: {0}")]
    PersistSmokeBlock(#[source] matching_sequencer::store::StoreError),
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), WitgenCliError> {
    match cli.command {
        Command::ExportLatest(args) => export_latest(args).await,
        Command::SmokeJob(args) => smoke_job(args).await,
    }
}

async fn export_latest(args: ExportLatestArgs) -> Result<(), WitgenCliError> {
    if !args.store.exists() {
        return Err(WitgenCliError::StoreNotFound {
            path: args.store.clone(),
        });
    }

    let store = Store::open(&args.store).map_err(|source| WitgenCliError::OpenStore {
        path: args.store.clone(),
        source,
    })?;
    let witness = store
        .latest_block_witness()
        .map_err(WitgenCliError::ReadWitness)?
        .ok_or(WitgenCliError::MissingLatestWitness)?;
    let job = collect_state_transition_proof_job(&store, witness).await?;
    let job_id = job.id();

    write_msgpack_named(&args.job, &job)?;

    print_job_id(&job_id);
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("job={}", args.job.display());
    Ok(())
}

async fn smoke_job(args: SmokeJobArgs) -> Result<(), WitgenCliError> {
    if args.store.exists() {
        return Err(WitgenCliError::SmokeStoreExists {
            path: args.store.clone(),
        });
    }

    let store = Store::open(&args.store).map_err(|source| WitgenCliError::OpenStore {
        path: args.store.clone(),
        source,
    })?;

    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let mut sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    let production = sequencer.produce_block(vec![], args.timestamp_ms);
    store
        .save_block_with_witness(sequencer.snapshot(), &production.witness)
        .await
        .map_err(WitgenCliError::PersistSmokeBlock)?;

    let job = collect_state_transition_proof_job(&store, production.witness).await?;
    write_msgpack_named(&args.job, &job)?;

    print_job_id(&job.id());
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("store={}", args.store.display());
    println!("job={}", args.job.display());
    Ok(())
}

fn write_msgpack_named<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), WitgenCliError> {
    let bytes = rmp_serde::to_vec_named(value).map_err(|source| WitgenCliError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, bytes).map_err(|source| WitgenCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn print_job_id(job_id: &StateTransitionProofJobId) {
    println!("block_height={}", job_id.block_height);
    println!("block_hash=0x{}", hex::encode(job_id.block_hash));
    println!("state_root=0x{}", hex::encode(job_id.state_root));
}
