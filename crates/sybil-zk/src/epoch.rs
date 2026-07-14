use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    AbiWord, StateTransitionGuestInput, ZkTransitionError, abi_encode_domain_and_words, keccak256,
    verify_state_transition_input,
};

pub const EPOCH_TRANSITION_INPUT_VERSION: u8 = 1;
pub const EPOCH_TRANSITION_DOMAIN: &[u8] = b"sybil/openvm/epoch-transition/v1";
pub const EPOCH_BLOCKS_DOMAIN: &[u8] = b"sybil/epoch/blocks/v1";
pub const EPOCH_BLOCKS_FOLD_DOMAIN: &[u8] = b"sybil/epoch/blocks/fold/v1";
pub const EPOCH_DA_DOMAIN: &[u8] = b"sybil/epoch/da/v1";
pub const EPOCH_DA_FOLD_DOMAIN: &[u8] = b"sybil/epoch/da/fold/v1";

/// Protocol ceiling. Deployments should use a substantially smaller measured
/// operational epoch size.
pub const MAX_EPOCH_BLOCKS: u64 = 4_096;

/// Public statement produced by verifying one contiguous sequence of blocks.
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

/// First OpenVM stream item. Its bounded block count tells the guest exactly
/// how many independently encoded block items to read without allocating a
/// giant epoch vector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochTransitionHeader {
    pub format_version: u8,
    pub public_inputs: EpochTransitionPublicInputs,
}

impl EpochTransitionHeader {
    pub fn new(public_inputs: EpochTransitionPublicInputs) -> Self {
        Self {
            format_version: EPOCH_TRANSITION_INPUT_VERSION,
            public_inputs,
        }
    }

    pub fn validate(&self) -> Result<(), EpochTransitionError> {
        if self.format_version != EPOCH_TRANSITION_INPUT_VERSION {
            return Err(EpochTransitionError::UnsupportedInputVersion {
                expected: EPOCH_TRANSITION_INPUT_VERSION,
                actual: self.format_version,
            });
        }
        if self.public_inputs.block_count == 0 {
            return Err(EpochTransitionError::EmptyEpoch);
        }
        if self.public_inputs.block_count > MAX_EPOCH_BLOCKS {
            return Err(EpochTransitionError::TooManyBlocks {
                max: MAX_EPOCH_BLOCKS,
            });
        }
        let expected_end = self
            .public_inputs
            .start_height
            .checked_add(self.public_inputs.block_count)
            .ok_or(EpochTransitionError::HeightOverflow {
                previous: self.public_inputs.start_height,
            })?;
        if self.public_inputs.end_height != expected_end {
            return Err(EpochTransitionError::ClaimedHeightRangeMismatch {
                start: self.public_inputs.start_height,
                count: self.public_inputs.block_count,
                end: self.public_inputs.end_height,
            });
        }
        Ok(())
    }
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

/// Incremental native/guest verifier. It retains only the prior header and
/// commitment folds, so callers may drop each large block input after `push`.
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

    pub fn push(&mut self, input: &StateTransitionGuestInput) -> Result<(), EpochTransitionError> {
        if self.block_count >= MAX_EPOCH_BLOCKS {
            return Err(EpochTransitionError::TooManyBlocks {
                max: MAX_EPOCH_BLOCKS,
            });
        }

        let index = self.block_count;
        let block_public_input_hash = verify_state_transition_input(input)
            .map_err(|source| EpochTransitionError::BlockVerification { index, source })?;

        if let Some(expected_genesis_hash) = self.genesis_hash {
            if input.witness.genesis_hash != expected_genesis_hash {
                return Err(EpochTransitionError::GenesisHashMismatch { index });
            }

            let previous_header = self
                .previous_header
                .as_ref()
                .expect("non-empty epoch accumulator has a prior header");
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

    pub fn finish_and_verify(
        self,
        header: &EpochTransitionHeader,
    ) -> Result<[u8; 32], EpochTransitionError> {
        header.validate()?;
        let computed = self.finish()?;
        if computed != header.public_inputs {
            return Err(EpochTransitionError::PublicInputsMismatch);
        }
        Ok(epoch_transition_public_input_hash(&header.public_inputs))
    }
}

pub fn verify_epoch_transition_inputs(
    claimed: &EpochTransitionPublicInputs,
    blocks: &[StateTransitionGuestInput],
) -> Result<[u8; 32], EpochTransitionError> {
    let header = EpochTransitionHeader::new(claimed.clone());
    let mut accumulator = EpochTransitionAccumulator::new();
    for block in blocks {
        accumulator.push(block)?;
    }
    accumulator.finish_and_verify(&header)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpochTransitionError {
    UnsupportedInputVersion {
        expected: u8,
        actual: u8,
    },
    EmptyEpoch,
    TooManyBlocks {
        max: u64,
    },
    ClaimedHeightRangeMismatch {
        start: u64,
        count: u64,
        end: u64,
    },
    BlockVerification {
        index: u64,
        source: ZkTransitionError,
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
            Self::UnsupportedInputVersion { expected, actual } => write!(
                f,
                "unsupported epoch input version: expected {expected}, got {actual}"
            ),
            Self::EmptyEpoch => write!(f, "epoch must contain at least one block"),
            Self::TooManyBlocks { max } => {
                write!(f, "epoch exceeds the protocol maximum of {max} blocks")
            }
            Self::ClaimedHeightRangeMismatch { start, count, end } => write!(
                f,
                "claimed epoch height range is inconsistent: start {start} + count {count} != end {end}"
            ),
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
            Self::StateChainMismatch { index } => write!(
                f,
                "epoch block {index} does not continue the prior state root"
            ),
            Self::PreviousHeaderMismatch { index } => write!(
                f,
                "epoch block {index} does not embed the exact prior header"
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn public_inputs(block_count: u64) -> EpochTransitionPublicInputs {
        EpochTransitionPublicInputs {
            start_height: 10,
            end_height: 10 + block_count,
            start_state_root: [1; 32],
            end_state_root: [2; 32],
            block_count,
            blocks_commitment: [3; 32],
            epoch_da_commitment: [4; 32],
            deposit_root: [5; 32],
            deposit_count: 7,
        }
    }

    #[test]
    fn epoch_public_input_hash_solidity_golden_vector() {
        assert_eq!(
            epoch_transition_public_input_hash(&public_inputs(2)),
            [
                0x7e, 0xf3, 0xa4, 0xd5, 0x37, 0x3c, 0x7d, 0xa2, 0xb5, 0xa6, 0x95, 0x20, 0x06, 0xaf,
                0xf3, 0xb0, 0xd7, 0xb8, 0x4b, 0x3e, 0xed, 0xfa, 0xe7, 0xfa, 0x30, 0x95, 0xda, 0x5b,
                0x7f, 0x3a, 0x53, 0x2d,
            ]
        );
    }

    #[test]
    fn header_rejects_zero_oversize_version_and_inconsistent_range() {
        assert_eq!(
            EpochTransitionHeader::new(public_inputs(0)).validate(),
            Err(EpochTransitionError::EmptyEpoch)
        );

        let oversized = EpochTransitionHeader::new(public_inputs(MAX_EPOCH_BLOCKS + 1));
        assert_eq!(
            oversized.validate(),
            Err(EpochTransitionError::TooManyBlocks {
                max: MAX_EPOCH_BLOCKS
            })
        );

        let mut wrong_version = EpochTransitionHeader::new(public_inputs(1));
        wrong_version.format_version += 1;
        assert!(matches!(
            wrong_version.validate(),
            Err(EpochTransitionError::UnsupportedInputVersion { .. })
        ));

        let mut bad_range = EpochTransitionHeader::new(public_inputs(2));
        bad_range.public_inputs.end_height += 1;
        assert!(matches!(
            bad_range.validate(),
            Err(EpochTransitionError::ClaimedHeightRangeMismatch { .. })
        ));
    }

    #[test]
    fn empty_accumulator_is_rejected() {
        assert_eq!(
            EpochTransitionAccumulator::new().finish(),
            Err(EpochTransitionError::EmptyEpoch)
        );
    }
}
