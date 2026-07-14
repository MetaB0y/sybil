use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use sybil_escape_claim::EscapeClaimGuestInput;
use sybil_zk::{
    EpochTransitionAccumulator, EpochTransitionHeader, StateTransitionGuestInput,
    epoch_transition_public_input_hash,
};

const MAX_BLOCK_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EPOCH_INPUT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "sybil-openvm-tools")]
#[command(about = "Sybil OpenVM artifact tools", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Wrap one prepared block as a one-block epoch OpenVM input stream.
    #[command(name = "encode-input")]
    Block(EncodeInputArgs),
    /// Convert ordered prepared blocks into a streamed epoch OpenVM input.
    #[command(name = "encode-epoch-input")]
    Epoch(EncodeEpochInputArgs),
    /// Convert a MessagePack Form-L claim into OpenVM CLI input JSON.
    #[command(name = "encode-escape-input")]
    Escape(EncodeEscapeInputArgs),
}

#[derive(Args)]
struct EncodeEscapeInputArgs {
    /// MessagePack-encoded EscapeClaimGuestInput from the smoke exporter.
    #[arg(long)]
    guest_input: PathBuf,
    /// Output path for OpenVM CLI input JSON.
    #[arg(long)]
    openvm_input: PathBuf,
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

#[derive(Args)]
struct EncodeEpochInputArgs {
    /// Ordered MessagePack block inputs. Repeat once per contiguous block.
    #[arg(long = "guest-input", required = true)]
    guest_inputs: Vec<PathBuf>,
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
    #[error("read MessagePack escape guest input from {path}: {source}")]
    DecodeEscapeGuestInput {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("encode OpenVM serde input: {0}")]
    EncodeOpenVm(#[source] openvm::serde::Error),
    #[error("encode OpenVM input JSON: {0}")]
    EncodeJson(#[source] serde_json::Error),
    #[error("invalid epoch input: {0}")]
    Epoch(#[from] sybil_zk::EpochTransitionError),
    #[error("block input {path} is {actual} bytes; maximum is {max}")]
    BlockInputTooLarge {
        path: PathBuf,
        actual: u64,
        max: u64,
    },
    #[error("encoded epoch input is {actual} bytes; maximum is {max}")]
    EpochInputTooLarge { actual: usize, max: usize },
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
        Command::Block(args) => encode_input(args),
        Command::Epoch(args) => encode_epoch_input(args),
        Command::Escape(args) => encode_escape_input(args),
    }
}

fn encode_escape_input(args: EncodeEscapeInputArgs) -> Result<(), OpenVmToolError> {
    let file = File::open(&args.guest_input).map_err(|source| OpenVmToolError::Open {
        path: args.guest_input.clone(),
        source,
    })?;
    let reader = BufReader::new(file);
    let guest_input: EscapeClaimGuestInput =
        rmp_serde::from_read(reader).map_err(|source| OpenVmToolError::DecodeEscapeGuestInput {
            path: args.guest_input.clone(),
            source,
        })?;
    let (word_count, bytes) = encode_openvm_value(&guest_input)?;
    write_openvm_json(&args.openvm_input, &[bytes])?;
    println!(
        "public_input_hash=0x{}",
        hex::encode(sybil_escape_claim::escape_claim_public_input_hash(
            &guest_input.public_inputs
        ))
    );
    println!("openvm_words={word_count}");
    println!("openvm_input={}", args.openvm_input.display());
    Ok(())
}

fn encode_input(args: EncodeInputArgs) -> Result<(), OpenVmToolError> {
    let guest_input = read_guest_input(&args.guest_input)?;
    let encoded = encode_epoch_stream(std::iter::once(Ok(guest_input)))?;
    write_openvm_json(&args.openvm_input, &encoded.items)?;
    println!(
        "public_input_hash=0x{}",
        hex::encode(encoded.public_input_hash)
    );
    println!("openvm_words={}", encoded.word_count);
    println!("openvm_input={}", args.openvm_input.display());
    Ok(())
}

fn encode_epoch_input(args: EncodeEpochInputArgs) -> Result<(), OpenVmToolError> {
    let inputs = args.guest_inputs.iter().map(|path| read_guest_input(path));
    let encoded = encode_epoch_stream(inputs)?;
    write_openvm_json(&args.openvm_input, &encoded.items)?;
    println!(
        "public_input_hash=0x{}",
        hex::encode(encoded.public_input_hash)
    );
    println!("openvm_words={}", encoded.word_count);
    println!("openvm_stream_items={}", encoded.items.len());
    println!("openvm_input={}", args.openvm_input.display());
    Ok(())
}

struct EncodedEpochInput {
    word_count: usize,
    items: Vec<Vec<u8>>,
    public_input_hash: [u8; 32],
}

fn encode_epoch_stream<I>(inputs: I) -> Result<EncodedEpochInput, OpenVmToolError>
where
    I: IntoIterator<Item = Result<StateTransitionGuestInput, OpenVmToolError>>,
{
    let mut accumulator = EpochTransitionAccumulator::new();
    let mut block_items = Vec::new();
    let mut total_bytes = 0usize;
    let mut word_count = 0usize;
    for input in inputs {
        let input = input?;
        accumulator.push(&input)?;
        let (words, bytes) = encode_openvm_input_bytes(&input)?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > MAX_EPOCH_INPUT_BYTES {
            return Err(OpenVmToolError::EpochInputTooLarge {
                actual: total_bytes,
                max: MAX_EPOCH_INPUT_BYTES,
            });
        }
        word_count = word_count.saturating_add(words);
        block_items.push(bytes);
    }

    let public_inputs = accumulator.finish()?;
    let public_input_hash = epoch_transition_public_input_hash(&public_inputs);
    let header = EpochTransitionHeader::new(public_inputs);
    let (header_words, header_bytes) = encode_openvm_value(&header)?;
    total_bytes = total_bytes.saturating_add(header_bytes.len());
    if total_bytes > MAX_EPOCH_INPUT_BYTES {
        return Err(OpenVmToolError::EpochInputTooLarge {
            actual: total_bytes,
            max: MAX_EPOCH_INPUT_BYTES,
        });
    }
    word_count = word_count.saturating_add(header_words);
    block_items.insert(0, header_bytes);
    Ok(EncodedEpochInput {
        word_count,
        items: block_items,
        public_input_hash,
    })
}

fn encode_openvm_input_bytes(
    guest_input: &StateTransitionGuestInput,
) -> Result<(usize, Vec<u8>), OpenVmToolError> {
    encode_openvm_value(guest_input)
}

fn encode_openvm_value<T: Serialize>(value: &T) -> Result<(usize, Vec<u8>), OpenVmToolError> {
    let words = openvm::serde::to_vec(value).map_err(OpenVmToolError::EncodeOpenVm)?;
    let mut bytes = Vec::with_capacity(words.len() * std::mem::size_of::<u32>());
    for word in &words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok((words.len(), bytes))
}

fn write_openvm_json(path: &Path, items: &[Vec<u8>]) -> Result<(), OpenVmToolError> {
    let input = serde_json::json!({
        "input": items
            .iter()
            .map(|bytes| format!("0x01{}", hex::encode(bytes)))
            .collect::<Vec<_>>()
    });
    let json = serde_json::to_vec_pretty(&input).map_err(OpenVmToolError::EncodeJson)?;
    std::fs::write(path, json).map_err(|source| OpenVmToolError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn read_guest_input(path: &Path) -> Result<StateTransitionGuestInput, OpenVmToolError> {
    let file = File::open(path).map_err(|source| OpenVmToolError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let input_len = file
        .metadata()
        .map_err(|source| OpenVmToolError::Open {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if input_len > MAX_BLOCK_INPUT_BYTES {
        return Err(OpenVmToolError::BlockInputTooLarge {
            path: path.to_path_buf(),
            actual: input_len,
            max: MAX_BLOCK_INPUT_BYTES,
        });
    }
    let reader = BufReader::new(file);
    rmp_serde::from_read(reader).map_err(|source| OpenVmToolError::DecodeGuestInput {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use matching_engine::MarketId;
    use sybil_escape_claim::{
        AccountReservationLeafWitness, EscapeClaimGuestInput, EscapeClaimPublicInputs,
    };
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
            genesis_hash: [0u8; 32],
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: Vec::new(),
            deposit_accumulator: sybil_verifier::DepositAccumulatorWitness::default(),
            fills: Vec::new(),
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: Vec::new(),
            post_system_state: Vec::new(),
            post_state: Vec::new(),
            account_keys: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        };
        StateTransitionGuestInput {
            public_inputs: sybil_zk::public_inputs_from_witness(&witness),
            witness,
            da_provider_refs: Vec::new(),
            state_root_proof: sybil_zk::QmdbStateRootProof::default(),
            pre_state_root_proof: sybil_zk::QmdbStateRootProof::default(),
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

    #[test]
    fn one_block_epoch_encodes_header_then_independent_block_item() {
        let input = minimal_guest_input();
        let header = EpochTransitionHeader::new(sybil_zk::EpochTransitionPublicInputs {
            start_height: 0,
            end_height: 1,
            start_state_root: [0; 32],
            end_state_root: [1; 32],
            block_count: 1,
            blocks_commitment: [2; 32],
            epoch_da_commitment: [3; 32],
            deposit_root: [4; 32],
            deposit_count: 0,
        });
        let (header_words, header_bytes) = encode_openvm_value(&header).unwrap();
        let (block_words, block_bytes) = encode_openvm_input_bytes(&input).unwrap();
        let word_count = header_words + block_words;
        let items = [header_bytes, block_bytes];
        assert_eq!(items.len(), 2);

        let decode_words = |bytes: &[u8]| {
            bytes
                .chunks_exact(std::mem::size_of::<u32>())
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        let decoded_header: EpochTransitionHeader =
            openvm::serde::from_slice(&decode_words(&items[0])).unwrap();
        let decoded: StateTransitionGuestInput =
            openvm::serde::from_slice(&decode_words(&items[1])).unwrap();

        assert_eq!(decoded_header, header);
        assert_eq!(decoded.witness.header.height, input.witness.header.height);
        assert_eq!(
            epoch_transition_public_input_hash(&decoded_header.public_inputs),
            epoch_transition_public_input_hash(&header.public_inputs)
        );
        assert_eq!(
            word_count,
            items.iter().map(|item| item.len() / 4).sum::<usize>()
        );
    }

    #[test]
    fn escape_key_and_signature_byte_fields_roundtrip_through_openvm_serde() {
        let operation = sybil_zk::QmdbStateOperationProof {
            location: 0,
            activity_chunk: [0; sybil_zk::QMDB_STATE_CHUNK_SIZE],
            range: sybil_zk::QmdbStateRangeProof {
                leaves: 0,
                digests: vec![],
                inactive_peaks: 0,
                partial_chunk_digest: None,
                ops_root: [0; 32],
            },
        };
        let proof = sybil_zk::QmdbStateKeyValueProof {
            operation: operation.clone(),
            next_key: vec![],
        };
        let key = sybil_verifier::KeyRecord {
            auth_scheme: 0,
            pubkey_sec1: [0x22; 33],
            capability_mask: u32::MAX,
        };
        let input = EscapeClaimGuestInput {
            public_inputs: EscapeClaimPublicInputs {
                state_root: [1; 32],
                height: 1,
                account_id: 7,
                recipient: [2; 20],
                amount: 9,
                nullifier: [3; 32],
            },
            genesis_hash: [4; 32],
            chain_id: 31_337,
            vault_address: [5; 20],
            account: sybil_verifier::AccountSnapshot {
                id: 7,
                balance: 10,
                total_deposited: 10,
                positions: vec![(MarketId(1), 0, 1)],
                events_digest: [6; 32],
                keys_digest: [7; 32],
                last_trading_nonce: 0,
            },
            account_proof: proof,
            account_reservation: AccountReservationLeafWitness::Exclusion {
                proof: sybil_zk::QmdbStateExclusionProof::Commit {
                    operation,
                    metadata: None,
                },
            },
            markets: vec![],
            active_keys: vec![key],
            authorization: sybil_verifier::KeyOpAuth::RawP256 {
                signer_pubkey: key.pubkey_sec1,
                signature: [0x33; 64],
            },
        };
        let words = openvm::serde::to_vec(&input).expect("encode escape input");
        let decoded: EscapeClaimGuestInput =
            openvm::serde::from_slice(&words).expect("decode escape input");
        assert_eq!(decoded.active_keys, input.active_keys);
        assert_eq!(decoded.authorization, input.authorization);
    }
}
