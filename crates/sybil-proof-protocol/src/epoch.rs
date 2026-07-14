use std::fmt;

use serde::{Deserialize, Serialize};
use sha3::{Digest as _, Keccak256};

pub const EPOCH_TRANSITION_DOMAIN: &[u8] = b"sybil/openvm/epoch-transition/v1";
pub const EPOCH_BLOCKS_DOMAIN: &[u8] = b"sybil/epoch/blocks/v1";
pub const EPOCH_BLOCKS_FOLD_DOMAIN: &[u8] = b"sybil/epoch/blocks/fold/v1";
pub const EPOCH_DA_DOMAIN: &[u8] = b"sybil/epoch/da/v1";
pub const EPOCH_DA_FOLD_DOMAIN: &[u8] = b"sybil/epoch/da/fold/v1";
/// Future guest protocol ceiling. Deployments should use a substantially
/// smaller operational epoch size.
pub const MAX_EPOCH_BLOCKS: u64 = 4_096;

/// Public statement produced by verifying one contiguous sequence of blocks.
///
/// This host-side type and fold deliberately live outside `sybil-zk` until the
/// authorization witness and streaming epoch guest are ready for their single
/// intentional commitment repin. The guest implementation must reuse these
/// exact domains, layout, and golden vector in that migration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochTransitionPublicInputs {
    pub start_height: u64,
    pub end_height: u64,
    pub start_state_root: [u8; 32],
    pub end_state_root: [u8; 32],
    pub block_count: u64,
    pub blocks_commitment: [u8; 32],
    pub epoch_da_commitment: [u8; 32],
    pub deposit_root: [u8; 32],
    pub deposit_count: u64,
}

pub fn epoch_transition_public_input_hash(inputs: &EpochTransitionPublicInputs) -> [u8; 32] {
    keccak256(&abi_encode_domain_and_words(
        EPOCH_TRANSITION_DOMAIN,
        &[
            AbiWord::Uint(inputs.start_height),
            AbiWord::Uint(inputs.end_height),
            AbiWord::Bytes32(inputs.start_state_root),
            AbiWord::Bytes32(inputs.end_state_root),
            AbiWord::Uint(inputs.block_count),
            AbiWord::Bytes32(inputs.blocks_commitment),
            AbiWord::Bytes32(inputs.epoch_da_commitment),
            AbiWord::Bytes32(inputs.deposit_root),
            AbiWord::Uint(inputs.deposit_count),
        ],
    ))
}

/// Incremental host verifier/fold used by epoch assembly and the mock backend.
/// It owns only the prior header, so callers may drop each large block input
/// immediately after `push` returns.
#[derive(Clone, Debug)]
pub struct EpochTransitionAccumulator {
    block_count: u64,
    start_height: Option<u64>,
    start_state_root: Option<[u8; 32]>,
    end_height: u64,
    end_state_root: [u8; 32],
    blocks_commitment: [u8; 32],
    epoch_da_commitment: [u8; 32],
    deposit_root: [u8; 32],
    deposit_count: u64,
    genesis_hash: Option<[u8; 32]>,
    previous_header: Option<sybil_verifier::WitnessBlockHeader>,
}

impl Default for EpochTransitionAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl EpochTransitionAccumulator {
    pub fn new() -> Self {
        Self {
            block_count: 0,
            start_height: None,
            start_state_root: None,
            end_height: 0,
            end_state_root: [0; 32],
            blocks_commitment: epoch_commitment_seed(EPOCH_BLOCKS_DOMAIN),
            epoch_da_commitment: epoch_commitment_seed(EPOCH_DA_DOMAIN),
            deposit_root: [0; 32],
            deposit_count: 0,
            genesis_hash: None,
            previous_header: None,
        }
    }

    pub fn block_count(&self) -> u64 {
        self.block_count
    }

    pub fn push(
        &mut self,
        input: &sybil_zk::StateTransitionGuestInput,
    ) -> Result<(), EpochTransitionError> {
        if self.block_count >= MAX_EPOCH_BLOCKS {
            return Err(EpochTransitionError::TooManyBlocks {
                max: MAX_EPOCH_BLOCKS,
            });
        }

        let index = self.block_count;
        let block_public_input_hash = sybil_zk::verify_state_transition_input(input)
            .map_err(|source| EpochTransitionError::BlockVerification { index, source })?;

        if let Some(expected_genesis_hash) = self.genesis_hash {
            if input.witness.genesis_hash != expected_genesis_hash {
                return Err(EpochTransitionError::GenesisHashMismatch { index });
            }

            let previous_header = self
                .previous_header
                .as_ref()
                .expect("non-empty epoch accumulator has a previous header");
            let expected_height = previous_header.height.checked_add(1).ok_or(
                EpochTransitionError::HeightOverflow {
                    previous: previous_header.height,
                },
            )?;
            if input.public_inputs.new_height != expected_height {
                return Err(EpochTransitionError::NonConsecutiveHeight {
                    index,
                    expected: expected_height,
                    actual: input.public_inputs.new_height,
                });
            }
            if input.public_inputs.previous_state_root != previous_header.state_root {
                return Err(EpochTransitionError::StateChainMismatch { index });
            }
            match &input.witness.previous_header {
                Some(actual) if headers_equal(previous_header, actual) => {}
                _ => return Err(EpochTransitionError::PreviousHeaderMismatch { index }),
            }
        } else {
            self.start_height = Some(input.public_inputs.previous_height);
            self.start_state_root = Some(input.public_inputs.previous_state_root);
            self.genesis_hash = Some(input.witness.genesis_hash);
        }

        self.blocks_commitment = epoch_commitment_fold(
            EPOCH_BLOCKS_FOLD_DOMAIN,
            self.blocks_commitment,
            block_public_input_hash,
        );
        self.epoch_da_commitment = epoch_commitment_fold(
            EPOCH_DA_FOLD_DOMAIN,
            self.epoch_da_commitment,
            input.public_inputs.da_commitment,
        );
        self.block_count += 1;
        self.end_height = input.public_inputs.new_height;
        self.end_state_root = input.public_inputs.new_state_root;
        self.deposit_root = input.public_inputs.deposit_root;
        self.deposit_count = input.public_inputs.deposit_count;
        self.previous_header = Some(input.witness.header.clone());
        Ok(())
    }

    pub fn finish(self) -> Result<EpochTransitionPublicInputs, EpochTransitionError> {
        let start_height = self.start_height.ok_or(EpochTransitionError::EmptyEpoch)?;
        let start_state_root = self
            .start_state_root
            .ok_or(EpochTransitionError::EmptyEpoch)?;
        Ok(EpochTransitionPublicInputs {
            start_height,
            end_height: self.end_height,
            start_state_root,
            end_state_root: self.end_state_root,
            block_count: self.block_count,
            blocks_commitment: self.blocks_commitment,
            epoch_da_commitment: self.epoch_da_commitment,
            deposit_root: self.deposit_root,
            deposit_count: self.deposit_count,
        })
    }
}

pub fn verify_epoch_transition_inputs(
    claimed: &EpochTransitionPublicInputs,
    blocks: &[sybil_zk::StateTransitionGuestInput],
) -> Result<[u8; 32], EpochTransitionError> {
    let mut accumulator = EpochTransitionAccumulator::new();
    for block in blocks {
        accumulator.push(block)?;
    }
    let computed = accumulator.finish()?;
    if &computed != claimed {
        return Err(EpochTransitionError::PublicInputsMismatch);
    }
    Ok(epoch_transition_public_input_hash(claimed))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpochTransitionError {
    EmptyEpoch,
    TooManyBlocks {
        max: u64,
    },
    BlockVerification {
        index: u64,
        source: sybil_zk::ZkTransitionError,
    },
    GenesisHashMismatch {
        index: u64,
    },
    HeightOverflow {
        previous: u64,
    },
    NonConsecutiveHeight {
        index: u64,
        expected: u64,
        actual: u64,
    },
    StateChainMismatch {
        index: u64,
    },
    PreviousHeaderMismatch {
        index: u64,
    },
    PublicInputsMismatch,
}

impl fmt::Display for EpochTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEpoch => write!(f, "epoch must contain at least one block"),
            Self::TooManyBlocks { max } => {
                write!(f, "epoch exceeds the protocol maximum of {max} blocks")
            }
            Self::BlockVerification { index, source } => {
                write!(f, "epoch block {index} failed verification: {source}")
            }
            Self::GenesisHashMismatch { index } => {
                write!(f, "epoch block {index} has a different genesis hash")
            }
            Self::HeightOverflow { previous } => {
                write!(f, "epoch height overflows after block {previous}")
            }
            Self::NonConsecutiveHeight {
                index,
                expected,
                actual,
            } => write!(
                f,
                "epoch block {index} is not consecutive: expected height {expected}, got {actual}"
            ),
            Self::StateChainMismatch { index } => {
                write!(
                    f,
                    "epoch block {index} does not continue the prior state root"
                )
            }
            Self::PreviousHeaderMismatch { index } => {
                write!(
                    f,
                    "epoch block {index} does not embed the exact prior header"
                )
            }
            Self::PublicInputsMismatch => {
                write!(
                    f,
                    "claimed epoch public inputs do not match verified blocks"
                )
            }
        }
    }
}

impl std::error::Error for EpochTransitionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BlockVerification { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn epoch_commitment_seed(domain: &[u8]) -> [u8; 32] {
    *blake3::hash(domain).as_bytes()
}

fn epoch_commitment_fold(domain: &[u8], previous: [u8; 32], value: [u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(&previous);
    hasher.update(&value);
    *hasher.finalize().as_bytes()
}

fn headers_equal(
    left: &sybil_verifier::WitnessBlockHeader,
    right: &sybil_verifier::WitnessBlockHeader,
) -> bool {
    left.height == right.height
        && left.parent_hash == right.parent_hash
        && left.state_root == right.state_root
        && left.events_root == right.events_root
        && left.order_count == right.order_count
        && left.fill_count == right.fill_count
        && left.timestamp_ms == right.timestamp_ms
}

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

enum AbiWord {
    Uint(u64),
    Bytes32([u8; 32]),
}

fn abi_encode_domain_and_words(domain: &[u8], words: &[AbiWord]) -> Vec<u8> {
    let head_words = 1 + words.len();
    let mut out = Vec::with_capacity(head_words * 32 + 32 + padded_len(domain.len()));
    out.extend_from_slice(&abi_usize_word(head_words * 32));
    for word in words {
        match word {
            AbiWord::Uint(value) => out.extend_from_slice(&abi_u64_word(*value)),
            AbiWord::Bytes32(bytes) => out.extend_from_slice(bytes),
        }
    }

    out.extend_from_slice(&abi_usize_word(domain.len()));
    out.extend_from_slice(domain);
    out.resize(out.len() + padded_len(domain.len()) - domain.len(), 0);
    out
}

fn abi_u64_word(value: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

fn abi_usize_word(value: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&(value as u64).to_be_bytes());
    out
}

fn padded_len(len: usize) -> usize {
    len.div_ceil(32) * 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_public_input_hash_solidity_golden_vector() {
        let inputs = EpochTransitionPublicInputs {
            start_height: 10,
            end_height: 12,
            start_state_root: [1; 32],
            end_state_root: [2; 32],
            block_count: 2,
            blocks_commitment: [3; 32],
            epoch_da_commitment: [4; 32],
            deposit_root: [5; 32],
            deposit_count: 7,
        };

        // Independently generated with `cast abi-encode` + `cast keccak`.
        assert_eq!(
            epoch_transition_public_input_hash(&inputs),
            [
                0x7e, 0xf3, 0xa4, 0xd5, 0x37, 0x3c, 0x7d, 0xa2, 0xb5, 0xa6, 0x95, 0x20, 0x06, 0xaf,
                0xf3, 0xb0, 0xd7, 0xb8, 0x4b, 0x3e, 0xed, 0xfa, 0xe7, 0xfa, 0x30, 0x95, 0xda, 0x5b,
                0x7f, 0x3a, 0x53, 0x2d,
            ]
        );
    }

    #[test]
    fn empty_epoch_is_rejected() {
        assert_eq!(
            EpochTransitionAccumulator::new().finish(),
            Err(EpochTransitionError::EmptyEpoch)
        );
    }
}
