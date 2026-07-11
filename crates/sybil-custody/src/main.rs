use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use sybil_custody::api::{collect_snapshot, read_json, write_json, SnapshotRequest};
use sybil_custody::claim::{assemble_claim, parse_address, ClaimRequest};
use sybil_custody::format::{CustodySnapshot, CUSTODY_SNAPSHOT_VERSION};
use sybil_custody::reconstruct::{reconstruct, ReconstructRequest};
use sybil_custody::rpc::send_raw_calldata_with_cast;

#[derive(Parser)]
#[command(name = "sybil-custody")]
#[command(
    about = "Anyone-can-prove custody and emergency escape tooling",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save current own-leaf openings and the matching DA manifest.
    Snapshot(SnapshotArgs),
    /// Authenticate and decode a full DA witness at a target height.
    Reconstruct(ReconstructArgs),
    /// Assemble, prove, encode, and optionally submit a Form-L escape claim.
    EscapeClaim(EscapeClaimArgs),
}

#[derive(Args)]
struct ApiArgs {
    #[arg(long, default_value = "http://127.0.0.1:3000")]
    api_url: String,
    /// Bearer accepted by the state-proof and DA-payload custody surfaces.
    #[arg(long, env = "SYBIL_API_TOKEN")]
    api_token: Option<String>,
}

#[derive(Args)]
struct SnapshotArgs {
    #[command(flatten)]
    api: ApiArgs,
    #[arg(long)]
    account_id: u64,
    #[arg(long, default_value = "custody-proof.json")]
    proof_out: PathBuf,
    #[arg(long, default_value = "custody-manifest.json")]
    manifest_out: PathBuf,
    /// Ethereum RPC used to authenticate the manifest against RootRecord.
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    settlement: Option<String>,
}

#[derive(Args)]
struct ReconstructArgs {
    #[command(flatten)]
    api: ApiArgs,
    #[arg(long)]
    height: u64,
    #[arg(long)]
    account_id: u64,
    /// Saved custody-manifest wrapper. Otherwise fetched from the API.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Saved canonical witness payload. Otherwise fetched from the API.
    #[arg(long)]
    payload: Option<PathBuf>,
    /// Matching compact own-leaf openings. Otherwise collected from live API.
    #[arg(long)]
    snapshot: Option<PathBuf>,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    settlement: Option<String>,
}

#[derive(Args)]
struct EscapeClaimArgs {
    #[command(flatten)]
    api: ApiArgs,
    /// Existing own-leaf custody snapshot. If absent, collect a fresh one.
    #[arg(long)]
    snapshot: Option<PathBuf>,
    #[arg(long, required_unless_present = "snapshot")]
    account_id: Option<u64>,
    #[arg(long)]
    rpc_url: String,
    #[arg(long)]
    settlement: String,
    #[arg(long)]
    vault: String,
    #[arg(long)]
    recipient: String,
    /// Raw P256 scalar used only to authorize the guest statement.
    #[arg(long, env = "SYBIL_P256_PRIVATE_KEY", hide_env_values = true)]
    p256_private_key: String,
    #[arg(long, default_value = "target/custody")]
    work_dir: PathBuf,
    /// Submit through `cast send` after printing calldata.
    #[arg(long)]
    submit: bool,
    #[arg(long, env = "PRIVATE_KEY", hide_env_values = true, requires = "submit")]
    eth_private_key: Option<String>,
    /// Unsafe test-only path: ABI-wrap a deterministic placeholder proof.
    #[arg(long, hide = true)]
    fixture_proof: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Snapshot(args) => snapshot(args).await,
        Command::Reconstruct(args) => reconstruct_command(args).await,
        Command::EscapeClaim(args) => escape_claim(args).await,
    }
}

async fn snapshot(args: SnapshotArgs) -> Result<()> {
    let settlement = args
        .settlement
        .as_deref()
        .map(|value| parse_address("settlement", value))
        .transpose()?;
    let (snapshot, manifest) = collect_snapshot(SnapshotRequest {
        api_url: &args.api.api_url,
        api_token: args.api.api_token.as_deref(),
        account_id: args.account_id,
        rpc_url: args.rpc_url.as_deref(),
        settlement,
    })
    .await?;
    write_json(&args.proof_out, &snapshot)?;
    write_json(&args.manifest_out, &manifest)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": CUSTODY_SNAPSHOT_VERSION,
            "account_id": snapshot.account_id,
            "height": snapshot.block_height,
            "state_root": snapshot.state_root,
            "proof_file": args.proof_out,
            "manifest_file": args.manifest_out,
            "l1_authenticated": manifest.root_record.is_some(),
        }))?
    );
    Ok(())
}

async fn reconstruct_command(args: ReconstructArgs) -> Result<()> {
    let settlement = args
        .settlement
        .as_deref()
        .map(|value| parse_address("settlement", value))
        .transpose()?;
    let summary = reconstruct(ReconstructRequest {
        height: args.height,
        account_id: args.account_id,
        api_url: Some(&args.api.api_url),
        api_token: args.api.api_token.as_deref(),
        manifest_path: args.manifest.as_deref(),
        payload_path: args.payload.as_deref(),
        snapshot_path: args.snapshot.as_deref(),
        rpc_url: args.rpc_url.as_deref(),
        settlement,
    })
    .await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn escape_claim(args: EscapeClaimArgs) -> Result<()> {
    let settlement = parse_address("settlement", &args.settlement)?;
    let vault = parse_address("vault", &args.vault)?;
    let recipient = parse_address("recipient", &args.recipient)?;
    let snapshot: CustodySnapshot = match &args.snapshot {
        Some(path) => read_json(path)?,
        None => {
            let account_id = args
                .account_id
                .context("--account-id is required without --snapshot")?;
            collect_snapshot(SnapshotRequest {
                api_url: &args.api.api_url,
                api_token: args.api.api_token.as_deref(),
                account_id,
                rpc_url: None,
                settlement: None,
            })
            .await?
            .0
        }
    };
    let artifacts = assemble_claim(ClaimRequest {
        snapshot: &snapshot,
        rpc_url: &args.rpc_url,
        settlement,
        vault,
        recipient,
        p256_private_key: &args.p256_private_key,
        work_dir: &args.work_dir,
        fixture_proof: args.fixture_proof,
    })
    .await?;
    let calldata_hex = format!("0x{}", hex::encode(&artifacts.calldata));
    let public_hash =
        sybil_escape_claim::escape_claim_public_input_hash(&artifacts.input.public_inputs);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "height": artifacts.input.public_inputs.height,
            "state_root": format!("0x{}", hex::encode(artifacts.input.public_inputs.state_root)),
            "account_id": artifacts.input.public_inputs.account_id,
            "recipient": format!("0x{}", hex::encode(artifacts.input.public_inputs.recipient)),
            "amount": artifacts.input.public_inputs.amount,
            "nullifier": format!("0x{}", hex::encode(artifacts.input.public_inputs.nullifier)),
            "public_input_hash": format!("0x{}", hex::encode(public_hash)),
            "adapter_proof_bytes": artifacts.adapter_proof.len(),
            "guest_input": artifacts.guest_input_path,
            "openvm_proof": artifacts.proof_path,
            "calldata": calldata_hex,
            "fixture_proof": args.fixture_proof,
        }))?
    );
    if args.submit {
        if args.fixture_proof {
            eprintln!("warning: submitting a fixture proof; this succeeds only with the unsafe dev adapter");
        }
        let private_key = args
            .eth_private_key
            .as_deref()
            .context("--eth-private-key or PRIVATE_KEY is required with --submit")?;
        let receipt =
            send_raw_calldata_with_cast(&args.rpc_url, private_key, vault, &artifacts.calldata)
                .await?;
        println!("submission={receipt}");
    } else if args.eth_private_key.is_some() {
        bail!("--eth-private-key has no effect without --submit");
    }
    Ok(())
}
