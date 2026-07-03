use clap::Parser;

#[tokio::main]
async fn main() {
    if let Err(error) =
        sybil_prover::mock_live::run(sybil_prover::mock_live::MockLiveArgs::parse()).await
    {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
