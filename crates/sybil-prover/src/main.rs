use clap::{Parser, Subcommand};
use sybil_prover::ProverCliError;
use sybil_prover::abi;
use sybil_prover::artifacts;
use sybil_prover::da;
use sybil_prover::serve;

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
    Inspect(artifacts::JobPathArgs),
    /// Validate a proof job and write the OpenVM guest input artifact.
    Prepare(artifacts::PrepareArgs),
    /// Validate a proof job, bind a file DA provider ref, and write proof artifacts.
    PrepareFileDa(da::PrepareFileDaArgs),
    /// Write the file-backed DA payload and manifest for a prepared guest input.
    PublishDa(da::PublishDaArgs),
    /// Run a local filesystem prover worker over exported proof jobs.
    Worker(artifacts::WorkerArgs),
    /// Serve prepared prover artifacts over a small read API.
    Serve(serve::ServeArgs),
    /// Encode a state-root submission for SybilSettlement.
    SubmitStateRoot(abi::SubmitStateRootArgs),
    /// Export proof jobs from the sequencer store.
    #[cfg(feature = "sequencer-store")]
    Witgen(sybil_prover::witgen_cli::WitgenArgs),
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), ProverCliError> {
    match cli.command {
        Command::Inspect(args) => artifacts::inspect(args),
        Command::Prepare(args) => artifacts::prepare(args),
        Command::PrepareFileDa(args) => da::prepare_file_da(args),
        Command::PublishDa(args) => da::publish_da(args),
        Command::Worker(args) => artifacts::run_worker(args),
        Command::Serve(args) => serve::serve(args).await,
        Command::SubmitStateRoot(args) => abi::submit_state_root(args),
        #[cfg(feature = "sequencer-store")]
        Command::Witgen(args) => sybil_prover::witgen_cli::run(args)
            .await
            .map_err(ProverCliError::from),
    }
}
