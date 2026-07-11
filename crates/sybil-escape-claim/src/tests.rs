use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};

use base64::Engine as _;
use commonware_codec::RangeCfg;
use commonware_cryptography::{sha256::Digest as QmdbDigest, Sha256 as QmdbSha256};
use commonware_parallel::Sequential;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{deterministic, Runner as _};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::full::Config as MmrConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::qmdb::current::ordered::variable::{
    Db as OrderedVariableDb, KeyValueProof,
};
use commonware_storage::qmdb::current::ordered::ExclusionProof as NativeExclusionProof;
use commonware_storage::qmdb::current::proof::{OperationProof, RangeProof};
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::translator::OneCap;
use matching_engine::{MarketId, Nanos, NANOS_PER_DOLLAR};
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use proptest::prelude::*;
use sha2::{Digest as _, Sha256};
use sybil_verifier::{
    commitments::state_schema, AccountReservationSnapshot, AccountSnapshot, KeyOpAuth, KeyRecord,
    MarketSnapshot, MarketStatusSnapshot, EXPECTED_RP_ID_HASH, EXPECTED_WEBAUTHN_RP_ID,
};
use sybil_zk::{
    QmdbStateExclusionProof, QmdbStateKeyValueProof, QmdbStateOperationProof, QmdbStateRangeProof,
    QMDB_STATE_CHUNK_SIZE,
};

use super::*;

const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;
const ACCOUNT_ID: u64 = 7;
const CHAIN_ID: u64 = 31_337;
const VAULT: [u8; 20] = [0x44; 20];
const RECIPIENT: [u8; 20] = [0x55; 20];
const GENESIS_HASH: [u8; 32] = [0x66; 32];

type TestStateDb = OrderedVariableDb<
    MmrFamily,
    deterministic::Context,
    Vec<u8>,
    Vec<u8>,
    QmdbSha256,
    OneCap,
    QMDB_STATE_CHUNK_SIZE,
    Sequential,
>;
type NativeKeyValueProof = KeyValueProof<MmrFamily, Vec<u8>, QmdbDigest, QMDB_STATE_CHUNK_SIZE>;
type NativeOperationProof = OperationProof<MmrFamily, QmdbDigest, QMDB_STATE_CHUNK_SIZE>;

struct PortableProofs {
    root: [u8; 32],
    inclusions: Vec<QmdbStateKeyValueProof>,
    exclusion: QmdbStateExclusionProof,
}

fn key_record(signing: &SigningKey, auth_scheme: u8) -> KeyRecord {
    let mut pubkey_sec1 = [0u8; 33];
    pubkey_sec1.copy_from_slice(signing.verifying_key().to_sec1_point(true).as_bytes());
    KeyRecord {
        auth_scheme,
        pubkey_sec1,
        capability_mask: KeyRecord::FULL_CAPABILITY_MASK,
    }
}

fn market(prices: Vec<Nanos>) -> MarketSnapshot {
    MarketSnapshot {
        market_id: MarketId(3),
        name: "escape fixture".to_string(),
        num_outcomes: 2,
        status: MarketStatusSnapshot::Active,
        metadata_digest: [0x12; 32],
        resolution_template: "binary".to_string(),
        last_clearing_prices: prices,
    }
}

fn fixture(
    qty: i64,
    prices: Vec<Nanos>,
    balance: i64,
    reservation: Option<AccountReservationSnapshot>,
) -> (EscapeClaimGuestInput, SigningKey) {
    let signing = SigningKey::from_slice(&[0x31; 32]).expect("valid fixture key");
    let record = key_record(&signing, 0);
    let account = AccountSnapshot {
        id: ACCOUNT_ID,
        balance,
        total_deposited: balance,
        positions: vec![(MarketId(3), 0, qty)],
        events_digest: [0x13; 32],
        keys_digest: sybil_verifier::account_keys_digest(ACCOUNT_ID, [record]),
    };
    let market = market(prices);
    let mut leaves = vec![
        (
            state_schema::account_leaf_key(ACCOUNT_ID),
            state_schema::account_leaf_value(&account),
        ),
        (
            state_schema::market_leaf_key(market.market_id),
            state_schema::market_leaf_value(&market),
        ),
    ];
    if let Some(reservation) = &reservation {
        leaves.push((
            state_schema::account_reservation_leaf_key(ACCOUNT_ID),
            state_schema::account_reservation_leaf_value(reservation),
        ));
    }
    leaves.sort_by(|left, right| left.0.cmp(&right.0));
    let inclusion_keys = vec![
        state_schema::account_leaf_key(ACCOUNT_ID),
        state_schema::market_leaf_key(market.market_id),
    ];
    let proofs = portable_proofs(
        leaves,
        inclusion_keys,
        state_schema::account_reservation_leaf_key(ACCOUNT_ID),
        reservation.is_none(),
    );

    let reservation_witness = match reservation {
        Some(reservation) => {
            let reservation_key = state_schema::account_reservation_leaf_key(ACCOUNT_ID);
            let inclusion = inclusion_proof_for_key(
                &proofs.root,
                &reservation_key,
                &state_schema::account_reservation_leaf_value(&reservation),
                &proofs.exclusion,
            );
            // Inclusion fixtures are rebuilt below because an exclusion proof
            // cannot exist for a present key.
            let inclusion = inclusion.unwrap_or_else(|| {
                portable_single_inclusion(
                    proofs.root,
                    reservation_key,
                    state_schema::account_reservation_leaf_value(&reservation),
                )
            });
            AccountReservationLeafWitness::Inclusion {
                reservation,
                proof: inclusion,
            }
        }
        None => AccountReservationLeafWitness::Exclusion {
            proof: proofs.exclusion,
        },
    };

    let mut input = EscapeClaimGuestInput {
        public_inputs: EscapeClaimPublicInputs {
            state_root: proofs.root,
            height: 42,
            account_id: ACCOUNT_ID,
            recipient: RECIPIENT,
            amount: 0,
            nullifier: escape_nullifier(CHAIN_ID, VAULT, ACCOUNT_ID, proofs.root),
        },
        genesis_hash: GENESIS_HASH,
        chain_id: CHAIN_ID,
        vault_address: VAULT,
        account,
        account_proof: proofs.inclusions[0].clone(),
        account_reservation: reservation_witness,
        markets: vec![MarketLeafWitness {
            market,
            proof: proofs.inclusions[1].clone(),
        }],
        active_keys: vec![record],
        authorization: KeyOpAuth::RawP256 {
            signer_pubkey: record.pubkey_sec1,
            signature: [0u8; 64],
        },
    };
    let reserved_balance = match &input.account_reservation {
        AccountReservationLeafWitness::Inclusion { reservation, .. } => {
            reservation.reserved_balance
        }
        AccountReservationLeafWitness::Exclusion { .. } => 0,
    };
    input.public_inputs.amount = compute_withdrawable_token_units(
        &input.account,
        reserved_balance,
        &input.markets,
        &input.public_inputs.state_root,
    )
    .unwrap_or(0);
    sign_raw(&mut input, &signing);
    (input, signing)
}

// Present-reservation fixtures need a third inclusion proof. Keep the proof
// builder generic and reconstruct the fixture rather than trusting leaf bytes.
fn fixture_with_reservation(reserved_balance: i64) -> EscapeClaimGuestInput {
    let reservation = AccountReservationSnapshot {
        account_id: ACCOUNT_ID,
        reserved_balance,
        reserved_positions: vec![(MarketId(3), 0, 999_999)],
    };
    let signing = SigningKey::from_slice(&[0x31; 32]).expect("valid fixture key");
    let record = key_record(&signing, 0);
    let account = AccountSnapshot {
        id: ACCOUNT_ID,
        balance: 2_000_000_000,
        total_deposited: 2_000_000_000,
        positions: vec![(MarketId(3), 0, 1_000)],
        events_digest: [0x13; 32],
        keys_digest: sybil_verifier::account_keys_digest(ACCOUNT_ID, [record]),
    };
    let market = market(vec![Nanos(500_000_000), Nanos(500_000_000)]);
    let mut leaves = vec![
        (
            state_schema::account_leaf_key(ACCOUNT_ID),
            state_schema::account_leaf_value(&account),
        ),
        (
            state_schema::market_leaf_key(market.market_id),
            state_schema::market_leaf_value(&market),
        ),
        (
            state_schema::account_reservation_leaf_key(ACCOUNT_ID),
            state_schema::account_reservation_leaf_value(&reservation),
        ),
    ];
    leaves.sort_by(|a, b| a.0.cmp(&b.0));
    let keys = vec![
        state_schema::account_leaf_key(ACCOUNT_ID),
        state_schema::market_leaf_key(market.market_id),
        state_schema::account_reservation_leaf_key(ACCOUNT_ID),
    ];
    let proofs = portable_inclusions(leaves, keys);
    let mut input = EscapeClaimGuestInput {
        public_inputs: EscapeClaimPublicInputs {
            state_root: proofs.0,
            height: 42,
            account_id: ACCOUNT_ID,
            recipient: RECIPIENT,
            amount: 0,
            nullifier: escape_nullifier(CHAIN_ID, VAULT, ACCOUNT_ID, proofs.0),
        },
        genesis_hash: GENESIS_HASH,
        chain_id: CHAIN_ID,
        vault_address: VAULT,
        account,
        account_proof: proofs.1[0].clone(),
        account_reservation: AccountReservationLeafWitness::Inclusion {
            reservation,
            proof: proofs.1[2].clone(),
        },
        markets: vec![MarketLeafWitness {
            market,
            proof: proofs.1[1].clone(),
        }],
        active_keys: vec![record],
        authorization: KeyOpAuth::RawP256 {
            signer_pubkey: record.pubkey_sec1,
            signature: [0; 64],
        },
    };
    input.public_inputs.amount = compute_withdrawable_token_units(
        &input.account,
        reserved_balance,
        &input.markets,
        &input.public_inputs.state_root,
    )
    .expect("fixture valuation");
    sign_raw(&mut input, &signing);
    input
}

fn canonical(input: &EscapeClaimGuestInput) -> Vec<u8> {
    sybil_verifier::canonical_escape_claim_bytes(
        input.genesis_hash,
        input.chain_id,
        input.vault_address,
        input.public_inputs.state_root,
        input.public_inputs.height,
        input.public_inputs.account_id,
        input.public_inputs.recipient,
        input.public_inputs.amount,
    )
}

fn sign_raw(input: &mut EscapeClaimGuestInput, signing: &SigningKey) {
    let signature: Signature = signing.sign(&canonical(input));
    input.authorization = KeyOpAuth::RawP256 {
        signer_pubkey: key_record(signing, 0).pubkey_sec1,
        signature: signature.to_bytes().into(),
    };
}

fn sign_webauthn(input: &mut EscapeClaimGuestInput, signing: &SigningKey) {
    let digest = Sha256::digest(canonical(input));
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    let client_data_json = format!(
        "{{\"type\":\"webauthn.get\",\"challenge\":\"{challenge}\",\"origin\":\"https://{EXPECTED_WEBAUTHN_RP_ID}\",\"crossOrigin\":false}}"
    )
    .into_bytes();
    let mut authenticator_data = EXPECTED_RP_ID_HASH.to_vec();
    authenticator_data.push(0x05);
    authenticator_data.extend_from_slice(&1u32.to_be_bytes());
    let client_hash = Sha256::digest(&client_data_json);
    let mut signed_message = authenticator_data.clone();
    signed_message.extend_from_slice(&client_hash);
    let signature: Signature = signing.sign(&signed_message);
    input.authorization = KeyOpAuth::WebAuthn {
        signer_pubkey: key_record(signing, 1).pubkey_sec1,
        authenticator_data,
        client_data_json,
        signature: signature.to_bytes().into(),
    };
}

fn portable_proofs(
    leaves: Vec<(Vec<u8>, Vec<u8>)>,
    inclusion_keys: Vec<Vec<u8>>,
    exclusion_key: Vec<u8>,
    expect_exclusion: bool,
) -> PortableProofs {
    deterministic::Runner::default().start(|context| async move {
        let mut db = open_test_state_db(context).await;
        let mut batch = db.new_batch();
        for (key, value) in leaves {
            batch = batch.write(key, Some(value));
        }
        let merkleized = batch.merkleize(&db, None).await.expect("merkleize fixture");
        db.apply_batch(merkleized).await.expect("apply fixture");
        let root = db.root().0;
        let mut inclusions = Vec::new();
        for key in inclusion_keys {
            let hasher = commonware_storage::qmdb::hasher::<QmdbSha256>();
            let proof = db
                .key_value_proof(&hasher, key)
                .await
                .expect("inclusion proof");
            inclusions.push(key_value_parts(&proof));
        }
        let exclusion = if expect_exclusion {
            let hasher = commonware_storage::qmdb::hasher::<QmdbSha256>();
            let proof = db
                .exclusion_proof(&hasher, &exclusion_key)
                .await
                .expect("exclusion proof");
            exclusion_parts(&proof)
        } else {
            // Unused sentinel; present-reservation fixtures use portable_inclusions.
            QmdbStateExclusionProof::Commit {
                operation: inclusions[0].operation.clone(),
                metadata: None,
            }
        };
        PortableProofs {
            root,
            inclusions,
            exclusion,
        }
    })
}

fn portable_inclusions(
    leaves: Vec<(Vec<u8>, Vec<u8>)>,
    inclusion_keys: Vec<Vec<u8>>,
) -> ([u8; 32], Vec<QmdbStateKeyValueProof>) {
    deterministic::Runner::default().start(|context| async move {
        let mut db = open_test_state_db(context).await;
        let mut batch = db.new_batch();
        for (key, value) in leaves {
            batch = batch.write(key, Some(value));
        }
        let merkleized = batch.merkleize(&db, None).await.expect("merkleize fixture");
        db.apply_batch(merkleized).await.expect("apply fixture");
        let root = db.root().0;
        let mut proofs = Vec::new();
        for key in inclusion_keys {
            let hasher = commonware_storage::qmdb::hasher::<QmdbSha256>();
            proofs.push(key_value_parts(
                &db.key_value_proof(&hasher, key)
                    .await
                    .expect("inclusion proof"),
            ));
        }
        (root, proofs)
    })
}

// These helpers are unreachable in valid fixtures; they keep the generic
// constructor simple while present-reservation coverage uses its dedicated path.
fn inclusion_proof_for_key(
    _root: &[u8; 32],
    _key: &[u8],
    _value: &[u8],
    _proof: &QmdbStateExclusionProof,
) -> Option<QmdbStateKeyValueProof> {
    None
}

fn portable_single_inclusion(
    _root: [u8; 32],
    _key: Vec<u8>,
    _value: Vec<u8>,
) -> QmdbStateKeyValueProof {
    panic!("present reservations use fixture_with_reservation")
}

async fn open_test_state_db(context: deterministic::Context) -> TestStateDb {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).expect("page size"),
        NonZeroUsize::new(PAGE_CACHE_PAGES).expect("page cache"),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: "escape-state-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).expect("items per blob"),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).expect("write buffer"),
            metadata_partition: "escape-state-mmr-metadata".to_string(),
            strategy: Sequential,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: "escape-state-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).expect("write buffer"),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).expect("items per section"),
            page_cache,
        },
        grafted_metadata_partition: "escape-state-grafted-mmr-metadata".to_string(),
        translator: OneCap,
    };
    TestStateDb::init(context, config)
        .await
        .expect("init fixture db")
}

fn key_value_parts(proof: &NativeKeyValueProof) -> QmdbStateKeyValueProof {
    QmdbStateKeyValueProof {
        operation: operation_parts(&proof.proof),
        next_key: proof.next_key.clone(),
    }
}

fn exclusion_parts(
    proof: &NativeExclusionProof<
        MmrFamily,
        Vec<u8>,
        commonware_storage::qmdb::any::value::VariableEncoding<Vec<u8>>,
        QmdbDigest,
        QMDB_STATE_CHUNK_SIZE,
    >,
) -> QmdbStateExclusionProof {
    match proof {
        NativeExclusionProof::KeyValue(operation, update) => QmdbStateExclusionProof::KeyValue {
            operation: operation_parts(operation),
            span_key: update.key.clone(),
            span_value: update.value.clone(),
            span_next_key: update.next_key.clone(),
        },
        NativeExclusionProof::Commit(operation, metadata) => QmdbStateExclusionProof::Commit {
            operation: operation_parts(operation),
            metadata: metadata.clone(),
        },
    }
}

fn operation_parts(proof: &NativeOperationProof) -> QmdbStateOperationProof {
    QmdbStateOperationProof {
        location: u64::from(proof.loc),
        activity_chunk: proof.chunk,
        range: range_parts(&proof.range_proof),
    }
}

fn range_parts(proof: &RangeProof<MmrFamily, QmdbDigest>) -> QmdbStateRangeProof {
    QmdbStateRangeProof {
        leaves: u64::from(proof.proof.leaves),
        inactive_peaks: proof.proof.inactive_peaks as u64,
        digests: proof.proof.digests.iter().copied().map(|d| d.0).collect(),
        partial_chunk_digest: proof.partial_chunk_digest.map(|d| d.0),
        ops_root: proof.ops_root.0,
    }
}

#[test]
fn golden_public_input_hash_matches_solidity_twin() {
    let inputs = EscapeClaimPublicInputs {
        state_root: [0x71; 32],
        height: 42,
        account_id: 1001,
        recipient: [0x73; 20],
        amount: 9_876_543_210,
        nullifier: [0x72; 32],
    };
    assert_eq!(
        escape_claim_public_input_hash(&inputs),
        [
            0x35, 0xe7, 0x54, 0x90, 0x9d, 0x75, 0xfe, 0x06, 0x58, 0xf0, 0xbc, 0x45, 0x14, 0x71,
            0xd8, 0x5f, 0x04, 0x81, 0x10, 0xd7, 0x95, 0x0b, 0x13, 0x91, 0x10, 0xe7, 0xcd, 0x2e,
            0x90, 0xff, 0xb5, 0xd5,
        ]
    );
}

#[test]
fn raw_p256_claim_with_proven_reservation_absence_verifies() {
    let (input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        2_000_000_000,
        None,
    );
    assert_eq!(input.public_inputs.amount, 2_500_000);
    assert_eq!(
        verify_escape_claim(&input),
        Ok(escape_claim_public_input_hash(&input.public_inputs))
    );
}

#[test]
fn reservation_inclusion_subtracts_cash_but_not_reserved_positions() {
    let input = fixture_with_reservation(250_000_000);
    assert_eq!(input.public_inputs.amount, 2_250_000);
    assert!(verify_escape_claim(&input).is_ok());
}

#[test]
fn short_position_uses_consensus_signed_notional() {
    let (input, _) = fixture(
        -1_000,
        vec![Nanos(600_000_000), Nanos(400_000_000)],
        1_000_000_000,
        None,
    );
    assert_eq!(input.public_inputs.amount, 400_000);
    assert!(verify_escape_claim(&input).is_ok());
}

#[test]
fn never_cleared_market_values_position_at_zero() {
    let (input, _) = fixture(9_000, vec![], 1_000_000_000, None);
    assert_eq!(input.public_inputs.amount, 1_000_000);
    assert!(verify_escape_claim(&input).is_ok());
}

#[test]
fn price_vector_outcome_mismatch_fails_closed() {
    let (input, _) = fixture(1_000, vec![Nanos(500_000_000)], 1_000_000_000, None);
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::MarketPrices)
    );
}

#[test]
fn out_of_range_outcome_fails_closed() {
    let (mut input, signing) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    input.account.positions[0].1 = 2;
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::AccountProof)
    );
    // The account proof binds the malformed outcome before valuation can use it.
    sign_raw(&mut input, &signing);
}

#[test]
fn missing_market_proof_fails_closed() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    input.markets.clear();
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::MarketProofSet)
    );
}

#[test]
fn absence_without_a_valid_exclusion_proof_fails_closed() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    let AccountReservationLeafWitness::Exclusion { proof } = &mut input.account_reservation else {
        panic!("fixture uses exclusion")
    };
    match proof {
        QmdbStateExclusionProof::KeyValue { span_next_key, .. } => span_next_key.push(0xff),
        QmdbStateExclusionProof::Commit { operation, .. } => operation.location ^= 1,
    }
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::ReservationProof)
    );
}

#[test]
fn signed_notional_overflow_fails_closed() {
    let (input, _) = fixture(i64::MIN, vec![Nanos(NANOS_PER_DOLLAR), Nanos(0)], 0, None);
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::SignedNotionalOverflow)
    );
}

#[test]
fn i128_accumulation_overflow_fails_closed() {
    assert_eq!(
        checked_x_nanos(i128::MAX, [1], 0),
        Err(EscapeClaimError::AccumulationOverflow)
    );
    assert_eq!(
        checked_x_nanos(i128::MIN, [], 1),
        Err(EscapeClaimError::AccumulationOverflow)
    );
}

#[test]
fn wrong_signer_fails_closed() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    let attacker = SigningKey::from_slice(&[0x32; 32]).expect("valid attacker key");
    sign_raw(&mut input, &attacker);
    assert!(matches!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::Authorization(_))
    ));
}

#[test]
fn cross_account_key_set_fails_digest_weld() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    let other = SigningKey::from_slice(&[0x33; 32]).expect("valid other key");
    input.active_keys = vec![key_record(&other, 0)];
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::KeysDigestMismatch)
    );
}

#[test]
fn wrong_genesis_hash_domain_fails_signature() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    input.genesis_hash[0] ^= 1;
    assert!(matches!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::Authorization(_))
    ));
}

#[test]
fn webauthn_claim_arm_verifies_with_scheme_matching_welded_key() {
    let (mut input, signing) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    input.active_keys[0].auth_scheme = 1;
    input.account.keys_digest =
        sybil_verifier::account_keys_digest(ACCOUNT_ID, input.active_keys.iter().copied());
    // Rebuild the account opening because keys_digest is committed in the leaf.
    let account_value = state_schema::account_leaf_value(&input.account);
    let market = input.markets[0].market.clone();
    let mut leaves = vec![
        (state_schema::account_leaf_key(ACCOUNT_ID), account_value),
        (
            state_schema::market_leaf_key(market.market_id),
            state_schema::market_leaf_value(&market),
        ),
    ];
    leaves.sort_by(|a, b| a.0.cmp(&b.0));
    let proofs = portable_proofs(
        leaves,
        vec![
            state_schema::account_leaf_key(ACCOUNT_ID),
            state_schema::market_leaf_key(market.market_id),
        ],
        state_schema::account_reservation_leaf_key(ACCOUNT_ID),
        true,
    );
    input.public_inputs.state_root = proofs.root;
    input.public_inputs.nullifier = escape_nullifier(CHAIN_ID, VAULT, ACCOUNT_ID, proofs.root);
    input.account_proof = proofs.inclusions[0].clone();
    input.markets[0].proof = proofs.inclusions[1].clone();
    input.account_reservation = AccountReservationLeafWitness::Exclusion {
        proof: proofs.exclusion,
    };
    sign_webauthn(&mut input, &signing);
    assert!(verify_escape_claim(&input).is_ok());
}

#[test]
fn zero_key_and_mint_accounts_fail_closed() {
    let (mut input, _) = fixture(
        1_000,
        vec![Nanos(500_000_000), Nanos(500_000_000)],
        1_000_000_000,
        None,
    );
    input.active_keys.clear();
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::EmptyKeySet)
    );
    input.public_inputs.account_id = MINT_ACCOUNT_ID;
    assert_eq!(
        verify_escape_claim(&input),
        Err(EscapeClaimError::MintAccount)
    );
}

proptest! {
    #[test]
    fn cash_only_floor_matches_checked_formula(balance in any::<i64>(), reserved in any::<i64>()) {
        let checked = checked_x_nanos(i128::from(balance), [], i128::from(reserved)).expect("i64 difference fits i128");
        let expected = i128::from(balance) - i128::from(reserved);
        prop_assert_eq!(checked, expected);
        prop_assert_eq!(checked.max(0), expected.max(0));
    }
}
