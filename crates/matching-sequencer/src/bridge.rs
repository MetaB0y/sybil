use std::collections::BTreeMap;

use sha3::{Digest, Keccak256};

use crate::account::AccountId;

pub type Bytes32 = [u8; 32];
pub type EthAddress = [u8; 20];

pub const NANOS_PER_TOKEN_UNIT: u64 = 1_000;
pub const DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS: u64 = 604_800;

const DEPOSIT_TREE_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct L1Deposit {
    pub deposit_id: u64,
    pub account_id: AccountId,
    pub chain_id: u64,
    pub vault_address: EthAddress,
    pub token_address: EthAddress,
    pub sender: EthAddress,
    pub sybil_account_key: Bytes32,
    pub amount_token_units: u64,
    pub deposit_root: Bytes32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgeWithdrawalRequest {
    pub account_id: AccountId,
    pub chain_id: u64,
    pub vault_address: EthAddress,
    pub recipient: EthAddress,
    pub token_address: EthAddress,
    pub amount_token_units: u64,
    pub expiry_height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalLeaf {
    pub withdrawal_id: u64,
    pub account_id: AccountId,
    pub recipient: EthAddress,
    pub token_address: EthAddress,
    pub amount_token_units: u64,
    pub amount_nanos: u64,
    pub expiry_height: u64,
    pub nullifier: Bytes32,
    pub created_at_height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgeState {
    pub deposit_cursor: u64,
    pub deposit_root: Bytes32,
    pub next_withdrawal_id: u64,
    pub withdrawals: BTreeMap<u64, WithdrawalLeaf>,
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            deposit_cursor: 0,
            deposit_root: empty_deposit_root(),
            next_withdrawal_id: 1,
            withdrawals: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgeBlockData {
    pub deposit_count: u64,
    pub deposit_root: Bytes32,
    pub consumed_deposits: Vec<L1Deposit>,
    pub withdrawal_leaves: Vec<WithdrawalLeaf>,
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("amount must be non-zero")]
    AmountZero,
    #[error("amount overflows nanos")]
    AmountOverflow,
    #[error("account bridge key mismatch")]
    AccountKeyMismatch,
    #[error("non-sequential deposit id: expected {expected}, got {actual}")]
    NonSequentialDeposit { expected: u64, actual: u64 },
    #[error("insufficient available balance: required {required}, available {available}")]
    InsufficientAvailableBalance { required: i64, available: i64 },
    #[error("withdrawal expiry {expiry_height} is before next committed height {next_height}")]
    WithdrawalExpired {
        expiry_height: u64,
        next_height: u64,
    },
}

pub fn account_key(account_id: AccountId) -> Bytes32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/bridge/account-key/v1");
    hasher.update(&account_id.0.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub fn amount_token_units_to_nanos(amount_token_units: u64) -> Result<u64, BridgeError> {
    if amount_token_units == 0 {
        return Err(BridgeError::AmountZero);
    }
    amount_token_units
        .checked_mul(NANOS_PER_TOKEN_UNIT)
        .ok_or(BridgeError::AmountOverflow)
}

pub fn amount_token_units_to_i64_nanos(amount_token_units: u64) -> Result<i64, BridgeError> {
    let amount = amount_token_units_to_nanos(amount_token_units)?;
    i64::try_from(amount).map_err(|_| BridgeError::AmountOverflow)
}

pub fn empty_deposit_root() -> Bytes32 {
    let mut root = [0u8; 32];
    for _ in 0..DEPOSIT_TREE_DEPTH {
        let mut bytes = Vec::with_capacity(1 + 32 + 32);
        bytes.push(0x01);
        bytes.extend_from_slice(&root);
        bytes.extend_from_slice(&root);
        root = keccak256(&bytes);
    }
    root
}

pub fn deposit_leaf(deposit: &L1Deposit) -> Bytes32 {
    keccak256(&abi_encode_domain_and_words(
        b"sybil/l1-deposit/v1",
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

pub fn deposit_tree_leaf(deposit: &L1Deposit) -> Bytes32 {
    let leaf = deposit_leaf(deposit);
    let mut bytes = Vec::with_capacity(1 + 32);
    bytes.push(0x00);
    bytes.extend_from_slice(&leaf);
    keccak256(&bytes)
}

pub fn withdrawal_nullifier(
    chain_id: u64,
    vault_address: EthAddress,
    withdrawal_id: u64,
    account_id: AccountId,
    recipient: EthAddress,
    token_address: EthAddress,
    amount_token_units: u64,
) -> Bytes32 {
    keccak256(&abi_encode_domain_and_words(
        b"sybil/withdrawal-nullifier/v1",
        &[
            AbiWord::Uint(chain_id),
            AbiWord::Address(vault_address),
            AbiWord::Uint(withdrawal_id),
            AbiWord::Uint(account_id.0),
            AbiWord::Address(recipient),
            AbiWord::Address(token_address),
            AbiWord::Uint(amount_token_units),
        ],
    ))
}

pub fn withdrawal_leaf_bytes(leaf: &WithdrawalLeaf) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + 8 * 5 + 20 + 20 + 32);
    bytes.extend_from_slice(b"sybil/state/withdrawal/v1");
    bytes.extend_from_slice(&leaf.withdrawal_id.to_le_bytes());
    bytes.extend_from_slice(&leaf.account_id.0.to_le_bytes());
    bytes.extend_from_slice(&leaf.recipient);
    bytes.extend_from_slice(&leaf.token_address);
    bytes.extend_from_slice(&leaf.amount_token_units.to_le_bytes());
    bytes.extend_from_slice(&leaf.amount_nanos.to_le_bytes());
    bytes.extend_from_slice(&leaf.expiry_height.to_le_bytes());
    bytes.extend_from_slice(&leaf.nullifier);
    bytes
}

pub fn withdrawal_leaf_digest(leaf: &WithdrawalLeaf) -> Bytes32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/state-leaf/withdrawal/v1");
    hasher.update(&withdrawal_leaf_bytes(leaf));
    *hasher.finalize().as_bytes()
}

pub fn bridge_state_snapshot(state: &BridgeState) -> sybil_verifier::BridgeStateSnapshot {
    let mut withdrawals: Vec<_> = state
        .withdrawals
        .values()
        .map(|withdrawal| sybil_verifier::WithdrawalSnapshot {
            withdrawal_id: withdrawal.withdrawal_id,
            account_id: withdrawal.account_id.0,
            recipient: withdrawal.recipient,
            token: withdrawal.token_address,
            amount_token_units: withdrawal.amount_token_units,
            amount_nanos: withdrawal.amount_nanos,
            expiry_height: withdrawal.expiry_height,
            nullifier: withdrawal.nullifier,
        })
        .collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    sybil_verifier::BridgeStateSnapshot {
        deposit_cursor: state.deposit_cursor,
        deposit_root: state.deposit_root,
        next_withdrawal_id: state.next_withdrawal_id,
        withdrawals,
    }
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

    #[test]
    fn account_key_is_stable() {
        assert_eq!(account_key(AccountId(7)), account_key(AccountId(7)));
        assert_ne!(account_key(AccountId(7)), account_key(AccountId(8)));
    }

    #[test]
    fn empty_deposit_root_is_not_zero() {
        assert_ne!(empty_deposit_root(), [0u8; 32]);
        assert_eq!(empty_deposit_root(), BridgeState::default().deposit_root);
    }

    #[test]
    fn deposit_leaf_hash_is_stable() {
        let deposit = L1Deposit {
            deposit_id: 1,
            account_id: AccountId(4),
            chain_id: 31_337,
            vault_address: address(1),
            token_address: address(2),
            sender: address(3),
            sybil_account_key: account_key(AccountId(4)),
            amount_token_units: 1_000_000,
            deposit_root: [9; 32],
        };
        assert_eq!(deposit_leaf(&deposit), deposit_leaf(&deposit));
        assert_ne!(deposit_leaf(&deposit), deposit_tree_leaf(&deposit));
    }

    #[test]
    fn withdrawal_nullifier_ignores_state_root_by_construction() {
        let nullifier = withdrawal_nullifier(
            31_337,
            address(1),
            9,
            AccountId(4),
            address(5),
            address(2),
            1_000_000,
        );
        assert_eq!(
            nullifier,
            withdrawal_nullifier(
                31_337,
                address(1),
                9,
                AccountId(4),
                address(5),
                address(2),
                1_000_000,
            )
        );
    }

    #[test]
    fn amount_conversion_uses_usdc_units_to_nanos() {
        assert_eq!(
            amount_token_units_to_nanos(1_000_000).unwrap(),
            1_000_000_000
        );
        assert!(matches!(
            amount_token_units_to_nanos(0),
            Err(BridgeError::AmountZero)
        ));
    }
}
