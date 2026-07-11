//! Form-L escape claims: selective state openings, account-key authorization,
//! conservative mark-to-last valuation, and the L1 public statement hash.

use std::collections::{BTreeMap, BTreeSet};

use matching_engine::{MarketId, NANOS_PER_DOLLAR, checked_signed_notional_nanos};
use serde::{Deserialize, Serialize};
use sybil_l1_protocol::{AbiWord, abi_keccak256_domain_and_words};
use sybil_verifier::{
    AccountReservationSnapshot, AccountSnapshot, KeyOpAuth, KeyRecord, MarketSnapshot,
    commitments::state_schema,
};
use sybil_zk::{
    NANOS_PER_TOKEN_UNIT, QmdbStateExclusionProof, QmdbStateKeyValueProof,
    verify_qmdb_exclusion_proof, verify_qmdb_key_value_proof,
};

pub const ESCAPE_CLAIM_PUBLIC_INPUT_DOMAIN: &[u8] = b"sybil/openvm/escape-claim/v1";
pub const ESCAPE_NULLIFIER_DOMAIN: &[u8] = b"sybil/escape-nullifier/v1";
pub const MINT_ACCOUNT_ID: u64 = u64::MAX;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapeClaimPublicInputs {
    pub state_root: [u8; 32],
    pub height: u64,
    pub account_id: u64,
    pub recipient: [u8; 20],
    /// L1 token units, not nanodollars.
    pub amount: u64,
    pub nullifier: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketLeafWitness {
    pub market: MarketSnapshot,
    pub proof: QmdbStateKeyValueProof,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountReservationLeafWitness {
    Inclusion {
        reservation: AccountReservationSnapshot,
        proof: QmdbStateKeyValueProof,
    },
    Exclusion {
        proof: QmdbStateExclusionProof,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscapeClaimGuestInput {
    pub public_inputs: EscapeClaimPublicInputs,
    pub genesis_hash: [u8; 32],
    pub chain_id: u64,
    pub vault_address: [u8; 20],
    pub account: AccountSnapshot,
    pub account_proof: QmdbStateKeyValueProof,
    pub account_reservation: AccountReservationLeafWitness,
    pub markets: Vec<MarketLeafWitness>,
    pub active_keys: Vec<KeyRecord>,
    pub authorization: KeyOpAuth,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EscapeClaimError {
    #[error("MINT is not escape-claimable")]
    MintAccount,
    #[error("account leaf does not match the claimed account")]
    AccountMismatch,
    #[error("account leaf inclusion proof failed")]
    AccountProof,
    #[error("account reservation witness does not match the claimed account")]
    ReservationMismatch,
    #[error("account reservation proof failed")]
    ReservationProof,
    #[error("account key set is empty")]
    EmptyKeySet,
    #[error("account key set exceeds the protocol cap")]
    TooManyKeys,
    #[error("account key set contains an unsupported scheme or duplicate public key")]
    InvalidKeySet,
    #[error("account keys_digest does not match the witnessed active key set")]
    KeysDigestMismatch,
    #[error("escape authorization failed: {0}")]
    Authorization(String),
    #[error("market proof set is missing, duplicate, or contains an unreferenced market")]
    MarketProofSet,
    #[error("market leaf inclusion proof failed")]
    MarketProof,
    #[error("market price vector is malformed")]
    MarketPrices,
    #[error("account position outcome is outside the market outcome count")]
    PositionOutcome,
    #[error("signed notional overflow")]
    SignedNotionalOverflow,
    #[error("valuation accumulation overflow")]
    AccumulationOverflow,
    #[error("non-negative valuation does not fit u64 nanos")]
    NanosOutOfRange,
    #[error("claimed token amount does not equal the proven valuation")]
    AmountMismatch,
    #[error("escape nullifier does not match the deployment/account/root domain")]
    NullifierMismatch,
}

/// Verify a complete Form-L claim and return the exact 32-byte OpenVM reveal.
pub fn verify_escape_claim(input: &EscapeClaimGuestInput) -> Result<[u8; 32], EscapeClaimError> {
    let public = &input.public_inputs;
    if public.account_id == MINT_ACCOUNT_ID {
        return Err(EscapeClaimError::MintAccount);
    }
    if input.account.id != public.account_id {
        return Err(EscapeClaimError::AccountMismatch);
    }

    let account_key = state_schema::account_leaf_key(public.account_id);
    let account_value = state_schema::account_leaf_value(&input.account);
    if !verify_qmdb_key_value_proof(
        &public.state_root,
        &account_key,
        &account_value,
        &input.account_proof,
    ) {
        return Err(EscapeClaimError::AccountProof);
    }

    let reserved_balance = verify_reservation(input)?;
    verify_key_binding_and_authorization(input)?;
    let amount = compute_withdrawable_token_units(
        &input.account,
        reserved_balance,
        &input.markets,
        &public.state_root,
    )?;
    if public.amount != amount {
        return Err(EscapeClaimError::AmountMismatch);
    }

    let expected_nullifier = escape_nullifier(
        input.chain_id,
        input.vault_address,
        public.account_id,
        public.state_root,
    );
    if public.nullifier != expected_nullifier {
        return Err(EscapeClaimError::NullifierMismatch);
    }

    Ok(escape_claim_public_input_hash(public))
}

fn verify_reservation(input: &EscapeClaimGuestInput) -> Result<i64, EscapeClaimError> {
    let account_id = input.public_inputs.account_id;
    let key = state_schema::account_reservation_leaf_key(account_id);
    match &input.account_reservation {
        AccountReservationLeafWitness::Inclusion { reservation, proof } => {
            if reservation.account_id != account_id {
                return Err(EscapeClaimError::ReservationMismatch);
            }
            let value = state_schema::account_reservation_leaf_value(reservation);
            if !verify_qmdb_key_value_proof(&input.public_inputs.state_root, &key, &value, proof) {
                return Err(EscapeClaimError::ReservationProof);
            }
            Ok(reservation.reserved_balance)
        }
        AccountReservationLeafWitness::Exclusion { proof } => {
            if !verify_qmdb_exclusion_proof(&input.public_inputs.state_root, &key, proof) {
                return Err(EscapeClaimError::ReservationProof);
            }
            Ok(0)
        }
    }
}

fn verify_key_binding_and_authorization(
    input: &EscapeClaimGuestInput,
) -> Result<(), EscapeClaimError> {
    if input.active_keys.is_empty() {
        return Err(EscapeClaimError::EmptyKeySet);
    }
    if input.active_keys.len() > sybil_verifier::MAX_KEYS_PER_ACCOUNT {
        return Err(EscapeClaimError::TooManyKeys);
    }
    let mut pubkeys = BTreeSet::new();
    for key in &input.active_keys {
        if key.auth_scheme > 1
            || !matches!(key.pubkey_sec1[0], 0x02 | 0x03)
            || !pubkeys.insert(key.pubkey_sec1)
        {
            return Err(EscapeClaimError::InvalidKeySet);
        }
    }
    let digest = sybil_verifier::account_keys_digest(
        input.public_inputs.account_id,
        input.active_keys.iter().copied(),
    );
    if input.account.keys_digest != digest {
        return Err(EscapeClaimError::KeysDigestMismatch);
    }

    let canonical = sybil_verifier::canonical_escape_claim_bytes(
        input.genesis_hash,
        input.chain_id,
        input.vault_address,
        input.public_inputs.state_root,
        input.public_inputs.height,
        input.public_inputs.account_id,
        input.public_inputs.recipient,
        input.public_inputs.amount,
    );
    sybil_verifier::verify_keyop_auth(&input.authorization, input.active_keys.iter(), &canonical)
        .map_err(EscapeClaimError::Authorization)
}

pub fn compute_withdrawable_token_units(
    account: &AccountSnapshot,
    reserved_balance: i64,
    markets: &[MarketLeafWitness],
    state_root: &[u8; 32],
) -> Result<u64, EscapeClaimError> {
    let required: BTreeSet<MarketId> = account
        .positions
        .iter()
        .filter_map(|(market, _, qty)| (*qty != 0).then_some(*market))
        .collect();
    let mut opened = BTreeMap::new();
    for witnessed in markets {
        if !required.contains(&witnessed.market.market_id)
            || opened
                .insert(witnessed.market.market_id, &witnessed.market)
                .is_some()
        {
            return Err(EscapeClaimError::MarketProofSet);
        }
        validate_market_prices(&witnessed.market)?;
        let key = state_schema::market_leaf_key(witnessed.market.market_id);
        let value = state_schema::market_leaf_value(&witnessed.market);
        if !verify_qmdb_key_value_proof(state_root, &key, &value, &witnessed.proof) {
            return Err(EscapeClaimError::MarketProof);
        }
    }
    if opened.len() != required.len() {
        return Err(EscapeClaimError::MarketProofSet);
    }

    let mut position_values = Vec::with_capacity(account.positions.len());
    for (market_id, outcome, qty) in &account.positions {
        if *qty == 0 {
            continue;
        }
        let market = opened
            .get(market_id)
            .copied()
            .ok_or(EscapeClaimError::MarketProofSet)?;
        if usize::from(*outcome) >= usize::from(market.num_outcomes) {
            return Err(EscapeClaimError::PositionOutcome);
        }
        let value = if market.last_clearing_prices.is_empty() {
            0
        } else {
            let price = *market
                .last_clearing_prices
                .get(usize::from(*outcome))
                .ok_or(EscapeClaimError::MarketPrices)?;
            checked_signed_notional_nanos(price, *qty)
                .ok_or(EscapeClaimError::SignedNotionalOverflow)?
        };
        position_values.push(i128::from(value));
    }

    let nanos = checked_x_nanos(
        i128::from(account.balance),
        position_values,
        i128::from(reserved_balance),
    )?;
    let clamped = nanos.max(0);
    let clamped = u64::try_from(clamped).map_err(|_| EscapeClaimError::NanosOutOfRange)?;
    Ok(clamped / NANOS_PER_TOKEN_UNIT)
}

fn validate_market_prices(market: &MarketSnapshot) -> Result<(), EscapeClaimError> {
    let count = market.last_clearing_prices.len();
    if (count != 0 && count != usize::from(market.num_outcomes))
        || market
            .last_clearing_prices
            .iter()
            .any(|price| price.0 > NANOS_PER_DOLLAR)
    {
        return Err(EscapeClaimError::MarketPrices);
    }
    Ok(())
}

/// Checked i128 accumulation used by valuation. Exposed to pin the overflow
/// edge independently of how many i64-valued positions fit in memory.
pub fn checked_x_nanos<I>(
    balance: i128,
    position_values: I,
    reserved_balance: i128,
) -> Result<i128, EscapeClaimError>
where
    I: IntoIterator<Item = i128>,
{
    let mut value = balance;
    for position in position_values {
        value = value
            .checked_add(position)
            .ok_or(EscapeClaimError::AccumulationOverflow)?;
    }
    value
        .checked_sub(reserved_balance)
        .ok_or(EscapeClaimError::AccumulationOverflow)
}

pub fn escape_nullifier(
    chain_id: u64,
    vault_address: [u8; 20],
    account_id: u64,
    state_root: [u8; 32],
) -> [u8; 32] {
    abi_keccak256_domain_and_words(
        ESCAPE_NULLIFIER_DOMAIN,
        &[
            AbiWord::Uint(chain_id),
            AbiWord::Address(vault_address),
            AbiWord::Uint(account_id),
            AbiWord::Bytes32(state_root),
        ],
    )
}

pub fn escape_claim_public_input_hash(inputs: &EscapeClaimPublicInputs) -> [u8; 32] {
    abi_keccak256_domain_and_words(
        ESCAPE_CLAIM_PUBLIC_INPUT_DOMAIN,
        &[
            AbiWord::Bytes32(inputs.state_root),
            AbiWord::Uint(inputs.height),
            AbiWord::Uint(inputs.account_id),
            AbiWord::Address(inputs.recipient),
            AbiWord::Uint(inputs.amount),
            AbiWord::Bytes32(inputs.nullifier),
        ],
    )
}

#[cfg(test)]
mod tests;
