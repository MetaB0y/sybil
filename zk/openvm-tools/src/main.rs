use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use sybil_zk::StateTransitionGuestInput;

#[derive(Parser)]
#[command(name = "sybil-openvm-tools")]
#[command(about = "Sybil OpenVM artifact tools", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a prepared guest-input artifact into OpenVM CLI input JSON.
    EncodeInput(EncodeInputArgs),
}

#[derive(Args)]
struct EncodeInputArgs {
    /// MessagePack-encoded StateTransitionGuestInput from sybil-prover prepare.
    #[arg(long)]
    guest_input: PathBuf,
    /// Output path for OpenVM CLI input JSON.
    #[arg(long)]
    openvm_input: PathBuf,
}

#[derive(Debug, thiserror::Error)]
enum OpenVmToolError {
    #[error("open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read MessagePack guest input from {path}: {source}")]
    DecodeGuestInput {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("encode OpenVM serde input: {0}")]
    EncodeOpenVm(#[source] openvm::serde::Error),
    #[error("encode OpenVM input JSON: {0}")]
    EncodeJson(#[source] serde_json::Error),
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), OpenVmToolError> {
    match cli.command {
        Command::EncodeInput(args) => encode_input(args),
    }
}

fn encode_input(args: EncodeInputArgs) -> Result<(), OpenVmToolError> {
    let guest_input = read_guest_input(&args.guest_input)?;
    let (word_count, bytes) = encode_openvm_input_bytes(&guest_input)?;

    let input = serde_json::json!({
        "input": [format!("0x01{}", hex::encode(&bytes))]
    });
    let json = serde_json::to_vec_pretty(&input).map_err(OpenVmToolError::EncodeJson)?;
    std::fs::write(&args.openvm_input, json).map_err(|source| OpenVmToolError::Write {
        path: args.openvm_input.clone(),
        source,
    })?;

    let public_input_hash =
        sybil_zk::state_transition_public_input_hash(&guest_input.public_inputs);
    println!("public_input_hash=0x{}", hex::encode(public_input_hash));
    println!("openvm_words={word_count}");
    println!("openvm_input={}", args.openvm_input.display());
    Ok(())
}

fn encode_openvm_input_bytes(
    guest_input: &StateTransitionGuestInput,
) -> Result<(usize, Vec<u8>), OpenVmToolError> {
    let words = openvm::serde::to_vec(guest_input).map_err(OpenVmToolError::EncodeOpenVm)?;
    let mut bytes = Vec::with_capacity(words.len() * std::mem::size_of::<u32>());
    for word in &words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok((words.len(), bytes))
}

fn read_guest_input(path: &Path) -> Result<StateTransitionGuestInput, OpenVmToolError> {
    let file = File::open(path).map_err(|source| OpenVmToolError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    rmp_serde::from_read(reader).map_err(|source| OpenVmToolError::DecodeGuestInput {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use sybil_verifier::{BlockWitness, StateSidecarSnapshot, WitnessBlockHeader};

    use super::*;

    fn minimal_guest_input() -> StateTransitionGuestInput {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root: [0u8; 32],
                events_root: [0u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1_000,
            },
            previous_header: None,
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: Vec::new(),
            fills: Vec::new(),
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: Vec::new(),
            post_system_state: Vec::new(),
            post_state: Vec::new(),
            state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        };
        StateTransitionGuestInput {
            public_inputs: sybil_zk::public_inputs_from_witness(&witness),
            witness,
            state_root_proof: sybil_zk::QmdbStateRootProof::default(),
        }
    }

    #[test]
    fn openvm_input_bytes_roundtrip_through_openvm_serde() {
        let input = minimal_guest_input();
        let (word_count, bytes) = encode_openvm_input_bytes(&input).unwrap();
        let words: Vec<u32> = bytes
            .chunks_exact(std::mem::size_of::<u32>())
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();

        assert_eq!(words.len(), word_count);

        let decoded: StateTransitionGuestInput =
            openvm::serde::from_slice(words.as_slice()).expect("OpenVM input bytes decode");
        assert_eq!(decoded.public_inputs, input.public_inputs);
        assert_eq!(decoded.witness.header.height, input.witness.header.height);
    }
}
