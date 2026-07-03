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
    let mut root = [0u8; 32];
    for _ in 0..DEPOSIT_TREE_DEPTH {
        root = hash_node(root, root);
    }
    root
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
    fn empty_deposit_root_is_deterministic() {
        assert_ne!(empty_deposit_root(), [0u8; 32]);
        assert_eq!(empty_deposit_root(), empty_deposit_root());
    }
}
