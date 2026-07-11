//! Ethereum L1 protocol boundary for Sybil bridge code.
//!
//! This crate owns Solidity ABI/event parsing and the hash domains shared with
//! `SybilVault`. Higher-level crates should convert these neutral L1 structs
//! into sequencer-specific account operations instead of parsing logs directly.

use sha3::{Digest, Keccak256};

pub type Bytes32 = [u8; 32];
pub type EthAddress = [u8; 20];
pub type DepositFrontier = [[u8; 32]; DEPOSIT_TREE_DEPTH];

pub const DEPOSIT_DOMAIN: &[u8] = b"sybil/l1-deposit/v1";
pub const WITHDRAWAL_NULLIFIER_DOMAIN: &[u8] = b"sybil/withdrawal-nullifier/v1";
pub const DEPOSIT_RECEIVED_SIGNATURE: &str =
    "DepositReceived(uint64,address,bytes32,address,uint256,bytes32)";
pub const WITHDRAWAL_QUEUED_SIGNATURE: &str =
    "WithdrawalQueued(bytes32,address,address,uint256,bytes32,uint64,uint64,uint64)";
pub const WITHDRAWAL_FINALIZED_SIGNATURE: &str =
    "WithdrawalFinalized(bytes32,address,uint256,uint64,uint64)";
pub const WITHDRAWAL_CANCELLED_SIGNATURE: &str =
    "WithdrawalCancelled(bytes32,address,uint256,uint64,uint64,string)";
/// Solidity signature of the auto-generated getter for
/// `mapping(uint64 count => bytes32 root) public depositRootByCount` on
/// `SybilVault`. Used by the indexer to reconcile a log's cumulative deposit
/// root against the canonical on-chain root before crediting (reorg safety).
pub const DEPOSIT_ROOT_BY_COUNT_SIGNATURE: &str = "depositRootByCount(uint64)";
pub const DEPOSIT_TREE_DEPTH: usize = 32;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum L1ProtocolError {
    #[error("unexpected DepositReceived topic count: expected 4, got {0}")]
    UnexpectedTopicCount(usize),
    #[error("unexpected DepositReceived topic0")]
    UnexpectedTopic0,
    #[error("unexpected DepositReceived data length: expected 96 bytes, got {0}")]
    UnexpectedDataLength(usize),
    #[error("unexpected {event} topic count: expected {expected}, got {actual}")]
    UnexpectedEventTopicCount {
        event: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("unexpected vault event topic0")]
    UnexpectedVaultEventTopic0,
    #[error(
        "unexpected {event} data length: expected at least {expected_min} bytes, got {actual}"
    )]
    UnexpectedEventDataLength {
        event: &'static str,
        expected_min: usize,
        actual: usize,
    },
    #[error("ABI word for {field} has non-zero high bytes")]
    NonZeroHighBytes { field: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct L1Log {
    pub address: EthAddress,
    pub topics: Vec<Bytes32>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DepositReceived {
    pub deposit_id: u64,
    pub sender: EthAddress,
    pub sybil_account_key: Bytes32,
    pub token_address: EthAddress,
    pub amount_token_units: u64,
    pub deposit_root: Bytes32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DepositLeaf {
    pub chain_id: u64,
    pub vault_address: EthAddress,
    pub deposit_id: u64,
    pub token_address: EthAddress,
    pub sender: EthAddress,
    pub sybil_account_key: Bytes32,
    pub amount_token_units: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalQueued {
    pub nullifier: Bytes32,
    pub recipient: EthAddress,
    pub token_address: EthAddress,
    pub amount_token_units: u64,
    pub state_root: Bytes32,
    pub height: u64,
    pub requested_at_unix: u64,
    pub executable_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalFinalized {
    pub nullifier: Bytes32,
    pub recipient: EthAddress,
    pub amount_token_units: u64,
    pub finalized_at_unix: u64,
    pub executable_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalCancelled {
    pub nullifier: Bytes32,
    pub recipient: EthAddress,
    pub amount_token_units: u64,
    pub cancelled_at_unix: u64,
    pub executable_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WithdrawalEvent {
    Queued(WithdrawalQueued),
    Finalized(WithdrawalFinalized),
    Cancelled(WithdrawalCancelled),
}

impl DepositReceived {
    pub fn into_deposit_leaf(self, chain_id: u64, vault_address: EthAddress) -> DepositLeaf {
        DepositLeaf {
            chain_id,
            vault_address,
            deposit_id: self.deposit_id,
            token_address: self.token_address,
            sender: self.sender,
            sybil_account_key: self.sybil_account_key,
            amount_token_units: self.amount_token_units,
        }
    }
}

pub fn deposit_received_topic0() -> Bytes32 {
    keccak256(DEPOSIT_RECEIVED_SIGNATURE.as_bytes())
}

pub fn withdrawal_queued_topic0() -> Bytes32 {
    keccak256(WITHDRAWAL_QUEUED_SIGNATURE.as_bytes())
}

pub fn withdrawal_finalized_topic0() -> Bytes32 {
    keccak256(WITHDRAWAL_FINALIZED_SIGNATURE.as_bytes())
}

pub fn withdrawal_cancelled_topic0() -> Bytes32 {
    keccak256(WITHDRAWAL_CANCELLED_SIGNATURE.as_bytes())
}

/// ABI-encoded `eth_call` calldata for `depositRootByCount(count)`.
///
/// The selector is `keccak256("depositRootByCount(uint64)")[..4]` followed by
/// the `uint64` count padded to a 32-byte word. The call returns the cumulative
/// `bytes32` deposit root recorded on-chain at that deposit count, which the
/// indexer compares against the root carried by a `DepositReceived` log to
/// detect reorgs/replacements before crediting.
pub fn deposit_root_by_count_calldata(count: u64) -> Vec<u8> {
    let selector = keccak256(DEPOSIT_ROOT_BY_COUNT_SIGNATURE.as_bytes());
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&selector[..4]);
    out.extend_from_slice(&abi_u64_word(count));
    out
}

pub fn parse_deposit_received_log(log: &L1Log) -> Result<DepositReceived, L1ProtocolError> {
    parse_deposit_received(&log.topics, &log.data)
}

pub fn parse_withdrawal_event_log(log: &L1Log) -> Result<WithdrawalEvent, L1ProtocolError> {
    let Some(topic0) = log.topics.first() else {
        return Err(L1ProtocolError::UnexpectedVaultEventTopic0);
    };
    if *topic0 == withdrawal_queued_topic0() {
        return parse_withdrawal_queued(&log.topics, &log.data).map(WithdrawalEvent::Queued);
    }
    if *topic0 == withdrawal_finalized_topic0() {
        return parse_withdrawal_finalized(&log.topics, &log.data).map(WithdrawalEvent::Finalized);
    }
    if *topic0 == withdrawal_cancelled_topic0() {
        return parse_withdrawal_cancelled(&log.topics, &log.data).map(WithdrawalEvent::Cancelled);
    }
    Err(L1ProtocolError::UnexpectedVaultEventTopic0)
}

pub fn parse_deposit_received(
    topics: &[Bytes32],
    data: &[u8],
) -> Result<DepositReceived, L1ProtocolError> {
    if topics.len() != 4 {
        return Err(L1ProtocolError::UnexpectedTopicCount(topics.len()));
    }
    if topics[0] != deposit_received_topic0() {
        return Err(L1ProtocolError::UnexpectedTopic0);
    }
    if data.len() != 96 {
        return Err(L1ProtocolError::UnexpectedDataLength(data.len()));
    }

    Ok(DepositReceived {
        deposit_id: decode_u64_word(&topics[1], "depositId")?,
        sender: decode_address_word(&topics[2], "sender")?,
        sybil_account_key: topics[3],
        token_address: decode_address_word(data_word(data, 0), "token")?,
        amount_token_units: decode_u64_word(data_word(data, 1), "amount")?,
        deposit_root: *data_word(data, 2),
    })
}

pub fn parse_withdrawal_queued(
    topics: &[Bytes32],
    data: &[u8],
) -> Result<WithdrawalQueued, L1ProtocolError> {
    ensure_event_shape(
        "WithdrawalQueued",
        topics,
        data,
        withdrawal_queued_topic0(),
        3,
        192,
    )?;
    Ok(WithdrawalQueued {
        nullifier: topics[1],
        recipient: decode_address_word(&topics[2], "recipient")?,
        token_address: decode_address_word(data_word(data, 0), "token")?,
        amount_token_units: decode_u64_word(data_word(data, 1), "amount")?,
        state_root: *data_word(data, 2),
        height: decode_u64_word(data_word(data, 3), "height")?,
        requested_at_unix: decode_u64_word(data_word(data, 4), "requestedAt")?,
        executable_at_unix: decode_u64_word(data_word(data, 5), "executableAt")?,
    })
}

pub fn parse_withdrawal_finalized(
    topics: &[Bytes32],
    data: &[u8],
) -> Result<WithdrawalFinalized, L1ProtocolError> {
    ensure_event_shape(
        "WithdrawalFinalized",
        topics,
        data,
        withdrawal_finalized_topic0(),
        3,
        96,
    )?;
    Ok(WithdrawalFinalized {
        nullifier: topics[1],
        recipient: decode_address_word(&topics[2], "recipient")?,
        amount_token_units: decode_u64_word(data_word(data, 0), "amount")?,
        finalized_at_unix: decode_u64_word(data_word(data, 1), "finalizedAt")?,
        executable_at_unix: decode_u64_word(data_word(data, 2), "executableAt")?,
    })
}

pub fn parse_withdrawal_cancelled(
    topics: &[Bytes32],
    data: &[u8],
) -> Result<WithdrawalCancelled, L1ProtocolError> {
    ensure_event_shape(
        "WithdrawalCancelled",
        topics,
        data,
        withdrawal_cancelled_topic0(),
        3,
        128,
    )?;
    Ok(WithdrawalCancelled {
        nullifier: topics[1],
        recipient: decode_address_word(&topics[2], "recipient")?,
        amount_token_units: decode_u64_word(data_word(data, 0), "amount")?,
        cancelled_at_unix: decode_u64_word(data_word(data, 1), "cancelledAt")?,
        executable_at_unix: decode_u64_word(data_word(data, 2), "executableAt")?,
    })
}

pub fn empty_deposit_root() -> Bytes32 {
    deposit_zero_hashes()[DEPOSIT_TREE_DEPTH]
}

pub fn empty_deposit_frontier() -> DepositFrontier {
    [[0u8; 32]; DEPOSIT_TREE_DEPTH]
}

pub fn deposit_leaf(deposit: &DepositLeaf) -> Bytes32 {
    keccak256(&abi_encode_domain_and_words(
        DEPOSIT_DOMAIN,
        &[
            AbiWord::Uint(deposit.chain_id),
            AbiWord::Address(deposit.vault_address),
            AbiWord::Uint(deposit.deposit_id),
            AbiWord::Address(deposit.token_address),
            AbiWord::Address(deposit.sender),
            AbiWord::Bytes32(deposit.sybil_account_key),
            AbiWord::Uint(deposit.amount_token_units),
        ],
    ))
}

pub fn deposit_tree_leaf(deposit: &DepositLeaf) -> Bytes32 {
    let leaf = deposit_leaf(deposit);
    let mut bytes = Vec::with_capacity(1 + 32);
    bytes.push(0x00);
    bytes.extend_from_slice(&leaf);
    keccak256(&bytes)
}

/// Cumulative deposit roots after appending each leaf in order.
///
/// This mirrors `SybilVault._appendDepositLeaf`: the first item is appended at
/// deposit index 0, the second at index 1, and so on. Callers that carry
/// explicit `deposit_id`s must verify those ids are exactly `1..=n` before
/// treating the returned final root as a checkpoint for count `n`.
pub fn deposit_prefix_roots(deposits: &[DepositLeaf]) -> Vec<Bytes32> {
    deposit_frontier_prefix_roots(&empty_deposit_frontier(), 0, deposits)
        .expect("empty-prefix deposit count is within tree capacity")
}

pub fn deposit_root_from_prefix(deposits: &[DepositLeaf]) -> Bytes32 {
    deposit_prefix_roots(deposits)
        .last()
        .copied()
        .unwrap_or_else(empty_deposit_root)
}

pub fn deposit_root_from_frontier(frontier: &DepositFrontier, count: u64) -> Option<Bytes32> {
    if count > deposit_tree_capacity() {
        return None;
    }

    let zero_hashes = deposit_zero_hashes();
    let mut root = zero_hashes[0];
    for level in 0..DEPOSIT_TREE_DEPTH {
        if (count >> level) & 1 == 1 {
            root = hash_node(frontier[level], root);
        } else {
            root = hash_node(root, zero_hashes[level]);
        }
    }
    Some(root)
}

pub fn append_deposit_frontier(
    frontier: &mut DepositFrontier,
    pre_count: u64,
    deposit: &DepositLeaf,
) -> Option<Bytes32> {
    if pre_count >= deposit_tree_capacity() {
        return None;
    }

    let zero_hashes = deposit_zero_hashes();
    let mut index = pre_count;
    let mut root = deposit_tree_leaf(deposit);
    for level in 0..DEPOSIT_TREE_DEPTH {
        if index & 1 == 0 {
            frontier[level] = root;
            root = hash_node(root, zero_hashes[level]);
        } else {
            root = hash_node(frontier[level], root);
        }
        index >>= 1;
    }
    Some(root)
}

pub fn deposit_frontier_prefix_roots(
    pre_frontier: &DepositFrontier,
    pre_count: u64,
    deposits: &[DepositLeaf],
) -> Option<Vec<Bytes32>> {
    let post_count = pre_count.checked_add(deposits.len() as u64)?;
    if post_count > deposit_tree_capacity() {
        return None;
    }

    let mut frontier = *pre_frontier;
    let mut roots = Vec::with_capacity(deposits.len());
    for (offset, deposit) in deposits.iter().enumerate() {
        let root = append_deposit_frontier(&mut frontier, pre_count + offset as u64, deposit)?;
        roots.push(root);
    }
    Some(roots)
}

pub fn deposit_frontier_after_prefix(
    pre_frontier: &DepositFrontier,
    pre_count: u64,
    deposits: &[DepositLeaf],
) -> Option<DepositFrontier> {
    let post_count = pre_count.checked_add(deposits.len() as u64)?;
    if post_count > deposit_tree_capacity() {
        return None;
    }

    let mut frontier = *pre_frontier;
    for (offset, deposit) in deposits.iter().enumerate() {
        append_deposit_frontier(&mut frontier, pre_count + offset as u64, deposit)?;
    }
    Some(frontier)
}

pub const fn deposit_tree_capacity() -> u64 {
    1u64 << DEPOSIT_TREE_DEPTH
}

pub fn hash_node(left: Bytes32, right: Bytes32) -> Bytes32 {
    let mut bytes = Vec::with_capacity(1 + 32 + 32);
    bytes.push(0x01);
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&right);
    keccak256(&bytes)
}

pub fn withdrawal_nullifier(
    chain_id: u64,
    vault_address: EthAddress,
    withdrawal_id: u64,
    account_id: u64,
    recipient: EthAddress,
    token_address: EthAddress,
    amount_token_units: u64,
) -> Bytes32 {
    keccak256(&abi_encode_domain_and_words(
        WITHDRAWAL_NULLIFIER_DOMAIN,
        &[
            AbiWord::Uint(chain_id),
            AbiWord::Address(vault_address),
            AbiWord::Uint(withdrawal_id),
            AbiWord::Uint(account_id),
            AbiWord::Address(recipient),
            AbiWord::Address(token_address),
            AbiWord::Uint(amount_token_units),
        ],
    ))
}

fn data_word(data: &[u8], idx: usize) -> &Bytes32 {
    data[idx * 32..(idx + 1) * 32]
        .try_into()
        .expect("data length checked before word access")
}

fn ensure_event_shape(
    event: &'static str,
    topics: &[Bytes32],
    data: &[u8],
    topic0: Bytes32,
    expected_topics: usize,
    expected_min_data_len: usize,
) -> Result<(), L1ProtocolError> {
    if topics.len() != expected_topics {
        return Err(L1ProtocolError::UnexpectedEventTopicCount {
            event,
            expected: expected_topics,
            actual: topics.len(),
        });
    }
    if topics[0] != topic0 {
        return Err(L1ProtocolError::UnexpectedVaultEventTopic0);
    }
    if data.len() < expected_min_data_len {
        return Err(L1ProtocolError::UnexpectedEventDataLength {
            event,
            expected_min: expected_min_data_len,
            actual: data.len(),
        });
    }
    Ok(())
}

fn decode_u64_word(word: &Bytes32, field: &'static str) -> Result<u64, L1ProtocolError> {
    if word[..24].iter().any(|byte| *byte != 0) {
        return Err(L1ProtocolError::NonZeroHighBytes { field });
    }
    Ok(u64::from_be_bytes(word[24..].try_into().expect("8 bytes")))
}

fn decode_address_word(word: &Bytes32, field: &'static str) -> Result<EthAddress, L1ProtocolError> {
    if word[..12].iter().any(|byte| *byte != 0) {
        return Err(L1ProtocolError::NonZeroHighBytes { field });
    }
    Ok(word[12..].try_into().expect("20 bytes"))
}

fn keccak256(bytes: &[u8]) -> Bytes32 {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn deposit_zero_hashes() -> [Bytes32; DEPOSIT_TREE_DEPTH + 1] {
    let mut zero_hashes = [[0u8; 32]; DEPOSIT_TREE_DEPTH + 1];
    for level in 0..DEPOSIT_TREE_DEPTH {
        zero_hashes[level + 1] = hash_node(zero_hashes[level], zero_hashes[level]);
    }
    zero_hashes
}

/// One static ABI word following a leading dynamic domain string.
///
/// Kept here as the shared Rust twin of Solidity's `abi.encode` statement
/// hashes; guest statements should consume this instead of minting encoders.
pub enum AbiWord {
    Uint(u64),
    Address(EthAddress),
    Bytes32(Bytes32),
}

pub fn abi_encode_domain_and_words(domain: &[u8], words: &[AbiWord]) -> Vec<u8> {
    let head_words = 1 + words.len();
    let mut out = Vec::with_capacity(head_words * 32 + 32 + padded_len(domain.len()));
    out.extend_from_slice(&abi_usize_word(head_words * 32));
    for word in words {
        match word {
            AbiWord::Uint(value) => out.extend_from_slice(&abi_u64_word(*value)),
            AbiWord::Address(address) => {
                let mut encoded = [0u8; 32];
                encoded[12..].copy_from_slice(address);
                out.extend_from_slice(&encoded);
            }
            AbiWord::Bytes32(bytes) => out.extend_from_slice(bytes),
        }
    }

    out.extend_from_slice(&abi_usize_word(domain.len()));
    out.extend_from_slice(domain);
    out.resize(out.len() + padded_len(domain.len()) - domain.len(), 0);
    out
}

/// Keccak-256 of [`abi_encode_domain_and_words`].
pub fn abi_keccak256_domain_and_words(domain: &[u8], words: &[AbiWord]) -> Bytes32 {
    keccak256(&abi_encode_domain_and_words(domain, words))
}

fn abi_u64_word(value: u64) -> Bytes32 {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

fn abi_usize_word(value: usize) -> Bytes32 {
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

    fn address(byte: u8) -> EthAddress {
        [byte; 20]
    }

    fn bytes32(byte: u8) -> Bytes32 {
        [byte; 32]
    }

    fn topic_u64(value: u64) -> Bytes32 {
        abi_u64_word(value)
    }

    fn topic_address(address: EthAddress) -> Bytes32 {
        let mut word = [0u8; 32];
        word[12..].copy_from_slice(&address);
        word
    }

    #[test]
    fn deposit_received_topic0_is_stable() {
        assert_eq!(
            hex::encode(deposit_received_topic0()),
            "fa74ded3e985bc09b760223cc7143260bad45fbf26ecad33032acb5b661f63fb"
        );
    }

    #[test]
    fn parses_deposit_received_log() {
        let topics = vec![
            deposit_received_topic0(),
            topic_u64(7),
            topic_address(address(0x33)),
            bytes32(0x44),
        ];
        let mut data = Vec::new();
        data.extend_from_slice(&topic_address(address(0x22)));
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&bytes32(0x99));

        let parsed = parse_deposit_received(&topics, &data).unwrap();

        assert_eq!(
            parsed,
            DepositReceived {
                deposit_id: 7,
                sender: address(0x33),
                sybil_account_key: bytes32(0x44),
                token_address: address(0x22),
                amount_token_units: 1_000_000,
                deposit_root: bytes32(0x99),
            }
        );
    }

    #[test]
    fn parses_withdrawal_queued_log() {
        let nullifier = bytes32(0xab);
        let recipient = address(0x33);
        let token = address(0x22);
        let state_root = bytes32(0x99);
        let mut data = Vec::new();
        data.extend_from_slice(&topic_address(token));
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&state_root);
        data.extend_from_slice(&abi_u64_word(42));
        data.extend_from_slice(&abi_u64_word(1_700_000_000));
        data.extend_from_slice(&abi_u64_word(1_700_086_400));
        let log = L1Log {
            address: address(0x11),
            topics: vec![
                withdrawal_queued_topic0(),
                nullifier,
                topic_address(recipient),
            ],
            data,
        };

        let parsed = parse_withdrawal_event_log(&log).unwrap();

        assert_eq!(
            parsed,
            WithdrawalEvent::Queued(WithdrawalQueued {
                nullifier,
                recipient,
                token_address: token,
                amount_token_units: 1_000_000,
                state_root,
                height: 42,
                requested_at_unix: 1_700_000_000,
                executable_at_unix: 1_700_086_400,
            })
        );
    }

    #[test]
    fn parses_withdrawal_finalized_log() {
        let nullifier = bytes32(0xab);
        let recipient = address(0x33);
        let mut data = Vec::new();
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&abi_u64_word(1_700_086_500));
        data.extend_from_slice(&abi_u64_word(1_700_086_400));
        let log = L1Log {
            address: address(0x11),
            topics: vec![
                withdrawal_finalized_topic0(),
                nullifier,
                topic_address(recipient),
            ],
            data,
        };

        let parsed = parse_withdrawal_event_log(&log).unwrap();

        assert_eq!(
            parsed,
            WithdrawalEvent::Finalized(WithdrawalFinalized {
                nullifier,
                recipient,
                amount_token_units: 1_000_000,
                finalized_at_unix: 1_700_086_500,
                executable_at_unix: 1_700_086_400,
            })
        );
    }

    #[test]
    fn parses_withdrawal_cancelled_log_without_decoding_reason() {
        let nullifier = bytes32(0xab);
        let recipient = address(0x33);
        let mut data = Vec::new();
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&abi_u64_word(1_700_000_100));
        data.extend_from_slice(&abi_u64_word(1_700_086_400));
        data.extend_from_slice(&abi_u64_word(128));
        data.extend_from_slice(&abi_u64_word(5));
        data.extend_from_slice(b"fraud");
        data.resize(192, 0);
        let log = L1Log {
            address: address(0x11),
            topics: vec![
                withdrawal_cancelled_topic0(),
                nullifier,
                topic_address(recipient),
            ],
            data,
        };

        let parsed = parse_withdrawal_event_log(&log).unwrap();

        assert_eq!(
            parsed,
            WithdrawalEvent::Cancelled(WithdrawalCancelled {
                nullifier,
                recipient,
                amount_token_units: 1_000_000,
                cancelled_at_unix: 1_700_000_100,
                executable_at_unix: 1_700_086_400,
            })
        );
    }

    #[test]
    fn parsed_deposit_can_be_lifted_to_deposit_leaf() {
        let event = DepositReceived {
            deposit_id: 1,
            sender: address(0x33),
            sybil_account_key: bytes32(0x44),
            token_address: address(0x22),
            amount_token_units: 1_000_000,
            deposit_root: bytes32(0x99),
        };

        let leaf = event.into_deposit_leaf(31_337, address(0x11));

        assert_eq!(leaf.chain_id, 31_337);
        assert_eq!(leaf.vault_address, address(0x11));
        assert_eq!(deposit_leaf(&leaf), deposit_leaf(&leaf));
        assert_ne!(deposit_leaf(&leaf), deposit_tree_leaf(&leaf));
    }

    #[test]
    fn rejects_wide_uints_that_do_not_fit_sequencer_types() {
        let topics = vec![
            deposit_received_topic0(),
            topic_u64(7),
            topic_address(address(0x33)),
            bytes32(0x44),
        ];
        let mut too_large_amount = [0u8; 32];
        too_large_amount[0] = 1;
        let mut data = Vec::new();
        data.extend_from_slice(&topic_address(address(0x22)));
        data.extend_from_slice(&too_large_amount);
        data.extend_from_slice(&bytes32(0x99));

        assert_eq!(
            parse_deposit_received(&topics, &data),
            Err(L1ProtocolError::NonZeroHighBytes { field: "amount" })
        );
    }

    #[test]
    fn deposit_root_by_count_calldata_is_selector_plus_word() {
        let calldata = deposit_root_by_count_calldata(7);
        assert_eq!(calldata.len(), 4 + 32);
        // Selector = keccak256("depositRootByCount(uint64)")[..4].
        assert_eq!(
            hex::encode(&calldata[..4]),
            hex::encode(&keccak256(DEPOSIT_ROOT_BY_COUNT_SIGNATURE.as_bytes())[..4])
        );
        assert_eq!(calldata[4..], abi_u64_word(7));
    }

    #[test]
    fn empty_deposit_root_is_deterministic() {
        assert_ne!(empty_deposit_root(), [0u8; 32]);
        assert_eq!(empty_deposit_root(), empty_deposit_root());
    }

    #[test]
    fn deposit_leaf_and_prefix_roots_golden_vector() {
        // Twin: contracts/test/SybilGoldenVectors.t.sol. Both suites consume
        // the generator-owned repo-root JSON rather than maintaining literals.
        let deposits = [
            DepositLeaf {
                chain_id: 31_337,
                vault_address: address(0x11),
                deposit_id: 1,
                token_address: address(0x22),
                sender: address(0x33),
                sybil_account_key: bytes32(0x44),
                amount_token_units: 1_000_000,
            },
            DepositLeaf {
                chain_id: 31_337,
                vault_address: address(0x11),
                deposit_id: 2,
                token_address: address(0x22),
                sender: address(0x55),
                sybil_account_key: bytes32(0x66),
                amount_token_units: 2_500_000,
            },
            DepositLeaf {
                chain_id: 31_337,
                vault_address: address(0x11),
                deposit_id: 3,
                token_address: address(0x22),
                sender: address(0x77),
                sybil_account_key: bytes32(0x88),
                amount_token_units: 42_000_001,
            },
        ];
        let high_id_max_amount = DepositLeaf {
            chain_id: 31_337,
            vault_address: address(0x11),
            deposit_id: 0xfedc_ba98_7654_3210,
            token_address: address(0x22),
            sender: address(0x99),
            sybil_account_key: bytes32(0xaa),
            amount_token_units: u64::MAX,
        };

        println!(
            "deposit_1_leaf=0x{}",
            hex::encode(deposit_leaf(&deposits[0]))
        );
        println!(
            "deposit_1_tree_leaf=0x{}",
            hex::encode(deposit_tree_leaf(&deposits[0]))
        );
        println!(
            "deposit_2_leaf=0x{}",
            hex::encode(deposit_leaf(&deposits[1]))
        );
        println!(
            "deposit_2_tree_leaf=0x{}",
            hex::encode(deposit_tree_leaf(&deposits[1]))
        );
        println!(
            "deposit_3_leaf=0x{}",
            hex::encode(deposit_leaf(&deposits[2]))
        );
        println!(
            "deposit_3_tree_leaf=0x{}",
            hex::encode(deposit_tree_leaf(&deposits[2]))
        );
        println!(
            "deposit_high_leaf=0x{}",
            hex::encode(deposit_leaf(&high_id_max_amount))
        );
        println!(
            "deposit_high_tree_leaf=0x{}",
            hex::encode(deposit_tree_leaf(&high_id_max_amount))
        );
        println!("empty_deposit_root=0x{}", hex::encode(empty_deposit_root()));
        println!(
            "deposit_prefix_roots={:?}",
            deposit_prefix_roots(&deposits)
                .into_iter()
                .map(hex::encode)
                .collect::<Vec<_>>()
        );
        let mut frontier = empty_deposit_frontier();
        let root_after_one =
            append_deposit_frontier(&mut frontier, 0, &deposits[0]).expect("deposit 1 fits");
        let frontier_after_one = frontier;
        let root_after_two =
            append_deposit_frontier(&mut frontier, 1, &deposits[1]).expect("deposit 2 fits");
        let frontier_after_two = frontier;
        let root_after_three =
            append_deposit_frontier(&mut frontier, 2, &deposits[2]).expect("deposit 3 fits");
        let frontier_after_three = frontier;
        println!(
            "deposit_frontier_after_1_level_0=0x{}",
            hex::encode(frontier_after_one[0])
        );
        println!(
            "deposit_frontier_after_2_level_1=0x{}",
            hex::encode(frontier_after_two[1])
        );
        println!(
            "deposit_frontier_after_3_level_0=0x{}",
            hex::encode(frontier_after_three[0])
        );
        println!(
            "deposit_frontier_after_3_level_1=0x{}",
            hex::encode(frontier_after_three[1])
        );

        assert_eq!(
            prefixed_hex(&deposit_leaf(&deposits[0])),
            golden_hex("/deposits/entries/0/leaf"),
            "deposit 1 leaf differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&deposit_tree_leaf(&deposits[0])),
            golden_hex("/deposits/entries/0/tree_leaf"),
            "deposit 1 tree leaf differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&empty_deposit_root()),
            golden_hex("/deposits/empty_root"),
            "empty deposit root differs from committed golden vector"
        );
        assert_eq!(
            deposit_prefix_roots(&deposits)
                .into_iter()
                .map(|root| prefixed_hex(&root))
                .collect::<Vec<_>>(),
            vec![
                golden_hex("/deposits/entries/0/prefix_root"),
                golden_hex("/deposits/entries/1/prefix_root"),
                golden_hex("/deposits/entries/2/prefix_root"),
            ],
            "deposit prefix roots differ from committed golden vectors"
        );
        assert_eq!(
            deposit_frontier_prefix_roots(&empty_deposit_frontier(), 0, &deposits)
                .expect("frontier fold fits"),
            deposit_prefix_roots(&deposits)
        );
        assert_eq!(root_after_one, deposit_prefix_roots(&deposits)[0]);
        assert_eq!(root_after_two, deposit_prefix_roots(&deposits)[1]);
        assert_eq!(root_after_three, deposit_prefix_roots(&deposits)[2]);
        assert_eq!(
            deposit_root_from_frontier(&frontier_after_one, 1),
            Some(deposit_prefix_roots(&deposits)[0])
        );
        assert_eq!(
            deposit_frontier_prefix_roots(&frontier_after_one, 1, &deposits[1..])
                .expect("split frontier fold fits"),
            deposit_prefix_roots(&deposits)[1..].to_vec()
        );
        assert_eq!(
            deposit_root_from_frontier(&frontier_after_three, 3),
            Some(deposit_prefix_roots(&deposits)[2])
        );
        assert_eq!(
            prefixed_hex(&frontier_after_one[0]),
            golden_hex("/deposits/entries/0/tree_leaf"),
            "deposit frontier after one differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&frontier_after_two[1]),
            golden_hex("/deposits/frontier_after_two_level_1"),
            "deposit frontier after two differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&frontier_after_three[0]),
            golden_hex("/deposits/entries/2/tree_leaf"),
            "deposit frontier after three level 0 differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&frontier_after_three[1]),
            golden_hex("/deposits/frontier_after_two_level_1"),
            "deposit frontier after three level 1 differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&deposit_root_from_prefix(&deposits)),
            golden_hex("/deposits/entries/2/prefix_root"),
            "final deposit root differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&deposit_leaf(&high_id_max_amount)),
            golden_hex("/deposits/high_id_max_amount/leaf"),
            "high-id deposit leaf differs from committed golden vector"
        );
        assert_eq!(
            prefixed_hex(&deposit_tree_leaf(&high_id_max_amount)),
            golden_hex("/deposits/high_id_max_amount/tree_leaf"),
            "high-id deposit tree leaf differs from committed golden vector"
        );
    }

    fn golden_hex(pointer: &str) -> String {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../golden/golden-vectors.json"))
                .expect("committed golden-vectors.json must be valid JSON");
        vectors
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("golden vector {pointer} must be a hex string"))
            .to_string()
    }

    fn prefixed_hex(bytes: &[u8]) -> String {
        format!("0x{}", hex::encode(bytes))
    }
}
