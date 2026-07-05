//! Ethereum L1 protocol boundary for Sybil bridge code.
//!
//! This crate owns Solidity ABI/event parsing and the hash domains shared with
//! `SybilVault`. Higher-level crates should convert these neutral L1 structs
//! into sequencer-specific account operations instead of parsing logs directly.

use sha3::{Digest, Keccak256};

pub type Bytes32 = [u8; 32];
pub type EthAddress = [u8; 20];

pub const DEPOSIT_DOMAIN: &[u8] = b"sybil/l1-deposit/v1";
pub const WITHDRAWAL_NULLIFIER_DOMAIN: &[u8] = b"sybil/withdrawal-nullifier/v1";
pub const DEPOSIT_RECEIVED_SIGNATURE: &str =
    "DepositReceived(uint64,address,bytes32,address,uint256,bytes32)";
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

pub fn empty_deposit_root() -> Bytes32 {
    deposit_zero_hashes()[DEPOSIT_TREE_DEPTH]
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
    let zero_hashes = deposit_zero_hashes();
    let mut filled_subtrees = [[0u8; 32]; DEPOSIT_TREE_DEPTH];
    let mut roots = Vec::with_capacity(deposits.len());

    for (deposit_index, deposit) in deposits.iter().enumerate() {
        let mut index = deposit_index as u64;
        let mut root = deposit_tree_leaf(deposit);
        for level in 0..DEPOSIT_TREE_DEPTH {
            if index & 1 == 0 {
                filled_subtrees[level] = root;
                root = hash_node(root, zero_hashes[level]);
            } else {
                root = hash_node(filled_subtrees[level], root);
            }
            index >>= 1;
        }
        roots.push(root);
    }

    roots
}

pub fn deposit_root_from_prefix(deposits: &[DepositLeaf]) -> Bytes32 {
    deposit_prefix_roots(deposits)
        .last()
        .copied()
        .unwrap_or_else(empty_deposit_root)
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

enum AbiWord {
    Uint(u64),
    Address(EthAddress),
    Bytes32(Bytes32),
}

fn abi_encode_domain_and_words(domain: &[u8], words: &[AbiWord]) -> Vec<u8> {
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
        // Twin: contracts/test/SybilGoldenVectors.t.sol. Keep these constants
        // byte-for-byte aligned with the Solidity suite.
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

        assert_eq!(
            hex::encode(deposit_leaf(&deposits[0])),
            "10348417835957783f646308469b0c1a7d42fcb7e8a67cc0774b969cd3bc4e78"
        );
        assert_eq!(
            hex::encode(deposit_tree_leaf(&deposits[0])),
            "cab93c3c5e862aa9e8fc0cff679d4d6febdf3305c81f65207871cea439975d5f"
        );
        assert_eq!(
            hex::encode(empty_deposit_root()),
            "7c1d0e8a93ea9c09cc13b91ead8f72de66a33cb695c30934dc2d75bffac1248e"
        );
        assert_eq!(
            deposit_prefix_roots(&deposits)
                .into_iter()
                .map(hex::encode)
                .collect::<Vec<_>>(),
            vec![
                "2e7fc1c1f7494f98b453f8be88ee3b99b47321b95425faf6853c3e59618de440",
                "bf00beb7a033f95b583dfb040f9f962db5f538c56e11cb9b3fa303b69d820b1f",
                "5d9b49419ded14b47faf0f943198c33647c016bd37f998b1d9196b103acfecda",
            ]
        );
        assert_eq!(
            hex::encode(deposit_root_from_prefix(&deposits)),
            "5d9b49419ded14b47faf0f943198c33647c016bd37f998b1d9196b103acfecda"
        );
        assert_eq!(
            hex::encode(deposit_leaf(&high_id_max_amount)),
            "0e0fe498f14aa8310467572c634bc13d6617573ca1fe7587c1fd642fbad168a1"
        );
        assert_eq!(
            hex::encode(deposit_tree_leaf(&high_id_max_amount)),
            "f7f3a6aeef19f4464f11bdfe4358124d745de1295dd03a116cccb1ab7ff2e90f"
        );
    }
}
