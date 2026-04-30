//! Layer 3: Block header integrity verification.
//!
//! Checks state root, parent hash chaining, height, and counts.

use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::Sha256 as QmdbSha256;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{deterministic, Runner as _};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::journaled::Config as MmrConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::qmdb::current::ordered::variable::Db as OrderedVariableDb;
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::translator::OneCap;
use sha2::{Digest as _, Sha256};

use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot,
    OracleSourceSnapshot, ResolutionProposalSnapshot, ResolutionRecordSnapshot,
    RestingOrderSnapshot, StateSidecarSnapshot, WithdrawalSnapshot, WitnessBlockHeader,
};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

const QMDB_CHUNK_SIZE: usize = 32;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;

type StateRootDb = OrderedVariableDb<
    MmrFamily,
    deterministic::Context,
    Vec<u8>,
    Vec<u8>,
    QmdbSha256,
    OneCap,
    QMDB_CHUNK_SIZE,
>;

/// Verify block header integrity.
pub fn verify_block(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // 1. State root: recompute from post-state and non-account sidecar
    let computed_root =
        compute_state_root_with_sidecar(&witness.post_state, &witness.state_sidecar);
    if computed_root != witness.header.state_root {
        violations.push(Violation {
            kind: ViolationKind::StateRootMismatch,
            details: format!(
                "Computed state root {:?} != header state root {:?}",
                hex(&computed_root),
                hex(&witness.header.state_root),
            ),
        });
    }

    // 2. Parent hash
    match &witness.previous_header {
        Some(prev) => {
            let computed_parent = hash_header(prev);
            if computed_parent != witness.header.parent_hash {
                violations.push(Violation {
                    kind: ViolationKind::ParentHashMismatch,
                    details: format!(
                        "Computed parent hash {:?} != header parent hash {:?}",
                        hex(&computed_parent),
                        hex(&witness.header.parent_hash),
                    ),
                });
            }

            // 3. Height consecutive
            if witness.header.height != prev.height + 1 {
                violations.push(Violation {
                    kind: ViolationKind::HeightNotConsecutive,
                    details: format!(
                        "Height {} != previous {} + 1",
                        witness.header.height, prev.height
                    ),
                });
            }
        }
        None => {
            // Genesis block: parent hash must be zeros, height must be 1
            if witness.header.parent_hash != [0u8; 32] {
                violations.push(Violation {
                    kind: ViolationKind::GenesisParentHashNonZero,
                    details: format!(
                        "Genesis block has non-zero parent hash: {:?}",
                        hex(&witness.header.parent_hash),
                    ),
                });
            }
            if witness.header.height != 1 {
                violations.push(Violation {
                    kind: ViolationKind::HeightNotConsecutive,
                    details: format!("Genesis block height {} != 1", witness.header.height),
                });
            }
        }
    }

    // 4. Counts match
    let expected_order_count = witness.orders.len() + witness.rejections.len();
    if witness.header.order_count != expected_order_count as u32 {
        violations.push(Violation {
            kind: ViolationKind::OrderCountMismatch,
            details: format!(
                "header.order_count {} != orders ({}) + rejections ({})",
                witness.header.order_count,
                witness.orders.len(),
                witness.rejections.len(),
            ),
        });
    }

    if witness.header.fill_count != witness.fills.len() as u32 {
        violations.push(Violation {
            kind: ViolationKind::FillCountMismatch,
            details: format!(
                "header.fill_count {} != fills.len() {}",
                witness.header.fill_count,
                witness.fills.len(),
            ),
        });
    }

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats,
    }
}

/// Compute the deterministic state root from account snapshots and an empty
/// sidecar.
///
/// Use [`compute_state_root_with_sidecar`] when verifying real blocks.
pub fn compute_state_root(accounts: &[AccountSnapshot]) -> [u8; 32] {
    compute_state_root_with_sidecar(accounts, &StateSidecarSnapshot::default())
}

/// Compute the typed state root with bridge leaves.
///
/// This convenience wrapper commits account leaves plus bridge leaves, with an
/// otherwise empty sidecar. Use [`compute_state_root_with_sidecar`] for real
/// blocks so order and reservation leaves are included.
pub fn compute_state_root_with_bridge(
    accounts: &[AccountSnapshot],
    bridge: &BridgeStateSnapshot,
) -> [u8; 32] {
    let sidecar = StateSidecarSnapshot {
        bridge: bridge.clone(),
        ..StateSidecarSnapshot::default()
    };
    compute_state_root_with_sidecar(accounts, &sidecar)
}

pub fn compute_state_root_with_sidecar(
    accounts: &[AccountSnapshot],
    sidecar: &StateSidecarSnapshot,
) -> [u8; 32] {
    let leaves = state_root_leaves(accounts, sidecar);
    state_root_from_leaves(&leaves)
}

/// Return the sorted typed key/value leaves committed by `state_root`.
pub fn state_root_leaves(
    accounts: &[AccountSnapshot],
    sidecar: &StateSidecarSnapshot,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut leaves = Vec::new();

    let mut sorted_accounts: Vec<&AccountSnapshot> = accounts.iter().collect();
    sorted_accounts.sort_by_key(|account| account.id);
    for account in sorted_accounts {
        leaves.push((account_leaf_key(account.id), account_leaf_value(account)));
    }

    leaves.push((
        b"sys/deposit_cursor".to_vec(),
        sys_u64_leaf_value(b"deposit_cursor", sidecar.bridge.deposit_cursor),
    ));
    leaves.push((
        b"sys/deposit_root".to_vec(),
        sys_bytes32_leaf_value(b"deposit_root", &sidecar.bridge.deposit_root),
    ));
    leaves.push((
        b"sys/next_withdrawal_id".to_vec(),
        sys_u64_leaf_value(b"next_withdrawal_id", sidecar.bridge.next_withdrawal_id),
    ));

    let mut markets: Vec<&MarketSnapshot> = sidecar.markets.iter().collect();
    markets.sort_by_key(|market| market.market_id.0);
    for market in markets {
        leaves.push((market_leaf_key(market.market_id), market_leaf_value(market)));
    }

    let mut market_groups: Vec<&MarketGroupSnapshot> = sidecar.market_groups.iter().collect();
    market_groups.sort_by_key(|group| group.group_id);
    for group in market_groups {
        leaves.push((
            market_group_leaf_key(group.group_id),
            market_group_leaf_value(group),
        ));
    }

    let mut withdrawals: Vec<&WithdrawalSnapshot> = sidecar.bridge.withdrawals.iter().collect();
    withdrawals.sort_by_key(|withdrawal| withdrawal.withdrawal_id);
    for withdrawal in withdrawals {
        leaves.push((
            withdrawal_leaf_key(withdrawal.withdrawal_id),
            withdrawal_leaf_value(withdrawal),
        ));
    }

    let mut resting_orders: Vec<&RestingOrderSnapshot> = sidecar.resting_orders.iter().collect();
    resting_orders.sort_by_key(|resting| resting.order.id);
    for resting in resting_orders {
        leaves.push((
            resting_order_leaf_key(resting.order.id),
            resting_order_leaf_value(resting),
        ));
    }

    let mut reservations: Vec<&AccountReservationSnapshot> =
        sidecar.account_reservations.iter().collect();
    reservations.sort_by_key(|reservation| reservation.account_id);
    for reservation in reservations {
        leaves.push((
            account_reservation_leaf_key(reservation.account_id),
            account_reservation_leaf_value(reservation),
        ));
    }

    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));
    leaves
}

pub fn state_root_from_leaves(leaves: &[(Vec<u8>, Vec<u8>)]) -> [u8; 32] {
    let mut leaves = leaves.to_vec();
    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));

    // The verifier API is synchronous, while qMDB is async. Keep the fresh
    // in-memory runtime off any caller-owned Tokio worker.
    thread::Builder::new()
        .name("sybil-state-root-qmdb".to_string())
        .spawn(move || {
            deterministic::Runner::default().start(|context| async move {
                let mut db = open_state_root_db(context)
                    .await
                    .expect("state root qmdb should initialize");
                if !leaves.is_empty() {
                    let mut batch = db.new_batch();
                    for (key, value) in leaves {
                        assert!(
                            key.len() <= MAX_KEY_BYTES,
                            "state root key exceeds qmdb key limit"
                        );
                        assert!(
                            value.len() <= MAX_VALUE_BYTES,
                            "state root value exceeds qmdb value limit"
                        );
                        batch = batch.write(key, Some(value));
                    }
                    let merkleized = batch
                        .merkleize(&db, None)
                        .await
                        .expect("state root qmdb batch should merkleize");
                    db.apply_batch(merkleized)
                        .await
                        .expect("state root qmdb batch should apply");
                }
                db.root().0
            })
        })
        .expect("state root qmdb thread should spawn")
        .join()
        .expect("state root qmdb thread should not panic")
}

async fn open_state_root_db(context: deterministic::Context) -> Result<StateRootDb, String> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: "state-root-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: "state-root-mmr-metadata".to_string(),
            thread_pool: None,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: "state-root-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
        grafted_metadata_partition: "state-root-grafted-mmr-metadata".to_string(),
        translator: OneCap,
    };

    StateRootDb::init(context, config)
        .await
        .map_err(|error| format!("failed to initialize state root qmdb: {error}"))
}

pub fn account_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(13);
    key.extend_from_slice(b"acct/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

pub fn market_leaf_key(market_id: matching_engine::MarketId) -> Vec<u8> {
    let mut key = Vec::with_capacity(11);
    key.extend_from_slice(b"market/");
    key.extend_from_slice(&market_id.0.to_be_bytes());
    key
}

pub fn market_group_leaf_key(group_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(21);
    key.extend_from_slice(b"market_group/");
    key.extend_from_slice(&group_id.to_be_bytes());
    key
}

pub fn withdrawal_leaf_key(withdrawal_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(19);
    key.extend_from_slice(b"withdrawal/");
    key.extend_from_slice(&withdrawal_id.to_be_bytes());
    key
}

pub fn resting_order_leaf_key(order_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(14);
    key.extend_from_slice(b"order/");
    key.extend_from_slice(&order_id.to_be_bytes());
    key
}

pub fn account_reservation_leaf_key(account_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(18);
    key.extend_from_slice(b"acct_resv/");
    key.extend_from_slice(&account_id.to_be_bytes());
    key
}

fn account_leaf_value(account: &AccountSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/acct/v1");
    value.extend_from_slice(&account.id.to_le_bytes());
    value.extend_from_slice(&account.balance.to_le_bytes());
    value.extend_from_slice(&account.total_deposited.to_le_bytes());

    let mut positions = account.positions.clone();
    positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    positions.retain(|(_, _, qty)| *qty != 0);
    value.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    for (market, outcome, qty) in positions {
        value.extend_from_slice(&market.0.to_le_bytes());
        value.push(outcome);
        value.extend_from_slice(&qty.to_le_bytes());
    }

    value.extend_from_slice(&account.events_digest);
    value
}

fn sys_u64_leaf_value(name: &[u8], raw: u64) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 8);
    value.extend_from_slice(b"sybil/state/sys/v1");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(&raw.to_le_bytes());
    value
}

fn sys_bytes32_leaf_value(name: &[u8], raw: &[u8; 32]) -> Vec<u8> {
    let mut value = Vec::with_capacity(19 + 1 + name.len() + 32);
    value.extend_from_slice(b"sybil/state/sys/v1");
    value.push(name.len() as u8);
    value.extend_from_slice(name);
    value.extend_from_slice(raw);
    value
}

/// Canonical digest for sequencer-layer market metadata.
///
/// The market leaf stores this digest instead of large text fields. A caller
/// proving metadata can reveal the raw metadata bytes and recompute this
/// digest against the committed market leaf.
pub fn market_metadata_digest(payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sybil/state/market-meta/v1");
    hasher.update((payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().into()
}

fn market_leaf_value(market: &MarketSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/market/v1");
    value.extend_from_slice(&market.market_id.0.to_le_bytes());
    append_string(&mut value, &market.name);
    value.push(market.num_outcomes);
    append_market_status(&mut value, &market.status);
    value.extend_from_slice(&market.metadata_digest);
    append_string(&mut value, &market.resolution_template);
    value
}

fn market_group_leaf_value(group: &MarketGroupSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/market-group/v1");
    value.extend_from_slice(&group.group_id.to_le_bytes());
    append_string(&mut value, &group.name);

    let mut markets = group.markets.clone();
    markets.sort_by_key(|market| market.0);
    value.extend_from_slice(&(markets.len() as u64).to_le_bytes());
    for market in markets {
        value.extend_from_slice(&market.0.to_le_bytes());
    }
    value
}

fn withdrawal_leaf_value(withdrawal: &WithdrawalSnapshot) -> Vec<u8> {
    let mut value = Vec::with_capacity(25 + 8 + 8 + 20 + 20 + 8 + 8 + 8 + 32);
    value.extend_from_slice(b"sybil/state/withdrawal/v1");
    value.extend_from_slice(&withdrawal.withdrawal_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.account_id.to_le_bytes());
    value.extend_from_slice(&withdrawal.recipient);
    value.extend_from_slice(&withdrawal.token);
    value.extend_from_slice(&withdrawal.amount_token_units.to_le_bytes());
    value.extend_from_slice(&withdrawal.amount_nanos.to_le_bytes());
    value.extend_from_slice(&withdrawal.expiry_height.to_le_bytes());
    value.extend_from_slice(&withdrawal.nullifier);
    value
}

fn resting_order_leaf_value(resting: &RestingOrderSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/order/v1");
    value.extend_from_slice(&resting.account_id.to_le_bytes());
    value.extend_from_slice(&resting.created_at.to_le_bytes());
    value.extend_from_slice(&resting.expires_at_block.to_le_bytes());
    value.extend_from_slice(&resting.reserved_balance.to_le_bytes());
    append_position_reservations(&mut value, &resting.reserved_positions);
    append_order(&mut value, &resting.order);
    value
}

fn account_reservation_leaf_value(reservation: &AccountReservationSnapshot) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/state/acct-resv/v1");
    value.extend_from_slice(&reservation.account_id.to_le_bytes());
    value.extend_from_slice(&reservation.reserved_balance.to_le_bytes());
    append_position_reservations(&mut value, &reservation.reserved_positions);
    value
}

fn append_position_reservations(
    value: &mut Vec<u8>,
    positions: &[(matching_engine::MarketId, u8, i64)],
) {
    let mut positions = positions.to_vec();
    positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    positions.retain(|(_, _, qty)| *qty != 0);
    value.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    for (market, outcome, qty) in positions {
        value.extend_from_slice(&market.0.to_le_bytes());
        value.push(outcome);
        value.extend_from_slice(&qty.to_le_bytes());
    }
}

fn append_string(value: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    value.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    value.extend_from_slice(bytes);
}

fn append_option_string(value: &mut Vec<u8>, text: &Option<String>) {
    match text {
        None => value.push(0),
        Some(text) => {
            value.push(1);
            append_string(value, text);
        }
    }
}

fn append_market_status(value: &mut Vec<u8>, status: &MarketStatusSnapshot) {
    match status {
        MarketStatusSnapshot::Active => value.push(0),
        MarketStatusSnapshot::Proposed {
            proposal,
            challenge_deadline_ms,
        } => {
            value.push(1);
            append_resolution_proposal(value, proposal);
            value.extend_from_slice(&challenge_deadline_ms.to_le_bytes());
        }
        MarketStatusSnapshot::Challenged {
            proposal,
            challenge,
        } => {
            value.push(2);
            append_resolution_proposal(value, proposal);
            append_challenge(value, challenge);
        }
        MarketStatusSnapshot::Resolved { record } => {
            value.push(3);
            append_resolution_record(value, record);
        }
        MarketStatusSnapshot::Voided => value.push(4),
    }
}

fn append_resolution_proposal(value: &mut Vec<u8>, proposal: &ResolutionProposalSnapshot) {
    value.extend_from_slice(&proposal.id.to_le_bytes());
    value.extend_from_slice(&proposal.market_id.0.to_le_bytes());
    value.extend_from_slice(&proposal.payout_nanos.to_le_bytes());
    append_oracle_source(value, &proposal.source);
    value.extend_from_slice(&proposal.proposed_at_ms.to_le_bytes());
    append_option_string(value, &proposal.reason);
}

fn append_challenge(value: &mut Vec<u8>, challenge: &ChallengeSnapshot) {
    value.extend_from_slice(&challenge.id.to_le_bytes());
    value.extend_from_slice(&challenge.challenger.to_le_bytes());
    value.extend_from_slice(&challenge.proposal_id.to_le_bytes());
    value.extend_from_slice(&challenge.bond_amount.to_le_bytes());
    value.extend_from_slice(&challenge.proposed_payout_nanos.to_le_bytes());
    append_string(value, &challenge.reason);
    value.extend_from_slice(&challenge.challenged_at_ms.to_le_bytes());
}

fn append_resolution_record(value: &mut Vec<u8>, record: &ResolutionRecordSnapshot) {
    value.extend_from_slice(&record.market_id.0.to_le_bytes());
    value.extend_from_slice(&record.payout_nanos.to_le_bytes());
    append_oracle_source(value, &record.resolved_by);
    value.extend_from_slice(&record.resolved_at_ms.to_le_bytes());
    append_optional_resolution_proposal(value, &record.proposal);
    append_optional_challenge(value, &record.challenge);
}

fn append_optional_resolution_proposal(
    value: &mut Vec<u8>,
    proposal: &Option<ResolutionProposalSnapshot>,
) {
    match proposal {
        None => value.push(0),
        Some(proposal) => {
            value.push(1);
            append_resolution_proposal(value, proposal);
        }
    }
}

fn append_optional_challenge(value: &mut Vec<u8>, challenge: &Option<ChallengeSnapshot>) {
    match challenge {
        None => value.push(0),
        Some(challenge) => {
            value.push(1);
            append_challenge(value, challenge);
        }
    }
}

fn append_oracle_source(value: &mut Vec<u8>, source: &OracleSourceSnapshot) {
    match source {
        OracleSourceSnapshot::Admin => value.push(0),
        OracleSourceSnapshot::DataFeed(feed_id) => {
            value.push(1);
            value.extend_from_slice(&feed_id.to_le_bytes());
        }
        OracleSourceSnapshot::AutomatedL0 => value.push(2),
    }
}

fn append_order(value: &mut Vec<u8>, order: &matching_engine::Order) {
    value.extend_from_slice(&order.id.to_le_bytes());
    value.push(order.num_markets);
    for market in order.markets.iter().take(order.num_markets as usize) {
        value.extend_from_slice(&market.0.to_le_bytes());
    }
    value.push(order.num_states);
    for payoff in order.payoffs.iter().take(order.num_states as usize) {
        value.extend_from_slice(&payoff.to_le_bytes());
    }
    value.extend_from_slice(&order.limit_price.to_le_bytes());
    value.extend_from_slice(&order.max_fill.to_le_bytes());
    match &order.condition {
        None => value.push(0),
        Some(condition) => {
            value.push(1);
            value.extend_from_slice(&condition.market.0.to_le_bytes());
            value.extend_from_slice(&condition.threshold.to_le_bytes());
            value.push(match condition.direction {
                matching_engine::ConditionDir::Above => 0,
                matching_engine::ConditionDir::Below => 1,
            });
        }
    }
    match order.expires_at_block {
        None => value.push(0),
        Some(expires_at_block) => {
            value.push(1);
            value.extend_from_slice(&expires_at_block.to_le_bytes());
        }
    }
}

/// Compute blake3 hash of a block header for chaining.
///
/// Must match `matching-sequencer`'s `hash_header`.
pub fn hash_header(header: &WitnessBlockHeader) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.height.to_le_bytes());
    hasher.update(&header.parent_hash);
    hasher.update(&header.state_root);
    hasher.update(&header.order_count.to_le_bytes());
    hasher.update(&header.fill_count.to_le_bytes());
    hasher.update(&header.timestamp_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Format a hash as hex (first 8 bytes).
fn hex(bytes: &[u8; 32]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        + "..."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WitnessBlockHeader;
    use matching_engine::MarketId;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn genesis_header(state_root: [u8; 32]) -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 1000,
        }
    }

    #[test]
    fn test_state_root_deterministic() {
        let accounts = vec![
            AccountSnapshot {
                id: 0,
                balance: 100,
                total_deposited: 100,
                positions: vec![(MarketId::new(0), 0, 10)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: 1,
                balance: 200,
                total_deposited: 200,
                positions: vec![(MarketId::new(0), 1, 5)],
                events_digest: [0u8; 32],
            },
        ];

        let root1 = compute_state_root(&accounts);
        let root2 = compute_state_root(&accounts);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_mutation() {
        let accounts1 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 200,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        assert_ne!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_changes_on_total_deposited_only() {
        let accounts1 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let accounts2 = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 150,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        assert_ne!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_changes_on_bridge_cursor() {
        let accounts = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let mut bridge = BridgeStateSnapshot::default();
        let before = compute_state_root_with_bridge(&accounts, &bridge);

        bridge.deposit_cursor = 1;
        let after = compute_state_root_with_bridge(&accounts, &bridge);

        assert_ne!(before, after);
    }

    #[test]
    fn test_state_root_changes_on_withdrawal_leaf() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let bridge = BridgeStateSnapshot {
            deposit_cursor: 1,
            deposit_root: [1u8; 32],
            next_withdrawal_id: 3,
            withdrawals: vec![WithdrawalSnapshot {
                withdrawal_id: 2,
                account_id: 7,
                recipient: [2u8; 20],
                token: [3u8; 20],
                amount_token_units: 1_000,
                amount_nanos: 2_000,
                expiry_height: 99,
                nullifier: [4u8; 32],
            }],
        };
        let mut changed = bridge.clone();
        changed.withdrawals[0].amount_nanos += 1;

        assert_ne!(
            compute_state_root_with_bridge(&accounts, &bridge),
            compute_state_root_with_bridge(&accounts, &changed)
        );
    }

    #[test]
    fn test_state_root_bridge_withdrawals_are_order_independent() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let first = WithdrawalSnapshot {
            withdrawal_id: 2,
            account_id: 7,
            recipient: [2u8; 20],
            token: [3u8; 20],
            amount_token_units: 1_000,
            amount_nanos: 2_000,
            expiry_height: 99,
            nullifier: [4u8; 32],
        };
        let second = WithdrawalSnapshot {
            withdrawal_id: 1,
            account_id: 8,
            recipient: [5u8; 20],
            token: [6u8; 20],
            amount_token_units: 3_000,
            amount_nanos: 4_000,
            expiry_height: 100,
            nullifier: [7u8; 32],
        };
        let bridge_a = BridgeStateSnapshot {
            deposit_cursor: 1,
            deposit_root: [1u8; 32],
            next_withdrawal_id: 3,
            withdrawals: vec![first.clone(), second.clone()],
        };
        let bridge_b = BridgeStateSnapshot {
            withdrawals: vec![second, first],
            ..bridge_a.clone()
        };

        assert_eq!(
            compute_state_root_with_bridge(&accounts, &bridge_a),
            compute_state_root_with_bridge(&accounts, &bridge_b)
        );
    }

    #[test]
    fn test_state_root_changes_on_resting_order_leaf() {
        let accounts = vec![AccountSnapshot {
            id: 7,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let mut order = matching_engine::Order::new(42);
        order.markets[0] = MarketId::new(1);
        order.num_markets = 1;
        order.num_states = 2;
        order.payoffs[0] = 1;
        order.limit_price = 500_000_000;
        order.max_fill = 3;

        let sidecar = StateSidecarSnapshot {
            resting_orders: vec![RestingOrderSnapshot {
                order,
                account_id: 7,
                created_at: 3,
                expires_at_block: 10,
                reserved_balance: 1_500_000_000,
                reserved_positions: vec![],
            }],
            account_reservations: vec![AccountReservationSnapshot {
                account_id: 7,
                reserved_balance: 1_500_000_000,
                reserved_positions: vec![],
            }],
            ..StateSidecarSnapshot::default()
        };

        assert_ne!(
            compute_state_root(&accounts),
            compute_state_root_with_sidecar(&accounts, &sidecar)
        );
    }

    #[test]
    fn test_state_root_order_book_leaves_are_order_independent() {
        let accounts = vec![];
        let mut first_order = matching_engine::Order::new(2);
        first_order.limit_price = 500_000_000;
        first_order.max_fill = 2;
        let mut second_order = matching_engine::Order::new(1);
        second_order.limit_price = 600_000_000;
        second_order.max_fill = 1;
        let first = RestingOrderSnapshot {
            order: first_order,
            account_id: 7,
            created_at: 3,
            expires_at_block: 10,
            reserved_balance: 1_000_000_000,
            reserved_positions: vec![],
        };
        let second = RestingOrderSnapshot {
            order: second_order,
            account_id: 8,
            created_at: 4,
            expires_at_block: 11,
            reserved_balance: 600_000_000,
            reserved_positions: vec![],
        };
        let reservation_a = AccountReservationSnapshot {
            account_id: 8,
            reserved_balance: 600_000_000,
            reserved_positions: vec![],
        };
        let reservation_b = AccountReservationSnapshot {
            account_id: 7,
            reserved_balance: 1_000_000_000,
            reserved_positions: vec![],
        };
        let sidecar_a = StateSidecarSnapshot {
            resting_orders: vec![first.clone(), second.clone()],
            account_reservations: vec![reservation_a.clone(), reservation_b.clone()],
            ..StateSidecarSnapshot::default()
        };
        let sidecar_b = StateSidecarSnapshot {
            resting_orders: vec![second, first],
            account_reservations: vec![reservation_b, reservation_a],
            ..StateSidecarSnapshot::default()
        };

        assert_eq!(
            compute_state_root_with_sidecar(&accounts, &sidecar_a),
            compute_state_root_with_sidecar(&accounts, &sidecar_b)
        );
    }

    #[test]
    fn test_state_root_changes_on_market_leaf() {
        let accounts = vec![];
        let market = MarketSnapshot {
            market_id: MarketId::new(1),
            name: "Will it rain?".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [1u8; 32],
            resolution_template: "admin_immediate".to_string(),
        };
        let mut resolved = market.clone();
        resolved.status = MarketStatusSnapshot::Resolved {
            record: ResolutionRecordSnapshot {
                market_id: MarketId::new(1),
                payout_nanos: 1_000_000_000,
                resolved_by: OracleSourceSnapshot::Admin,
                resolved_at_ms: 42,
                proposal: None,
                challenge: None,
            },
        };

        let before = StateSidecarSnapshot {
            markets: vec![market],
            ..StateSidecarSnapshot::default()
        };
        let after = StateSidecarSnapshot {
            markets: vec![resolved],
            ..StateSidecarSnapshot::default()
        };

        assert_ne!(
            compute_state_root_with_sidecar(&accounts, &before),
            compute_state_root_with_sidecar(&accounts, &after)
        );
    }

    #[test]
    fn test_state_root_market_leaves_are_order_independent() {
        let accounts = vec![];
        let first_market = MarketSnapshot {
            market_id: MarketId::new(2),
            name: "B".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [2u8; 32],
            resolution_template: "admin_immediate".to_string(),
        };
        let second_market = MarketSnapshot {
            market_id: MarketId::new(1),
            name: "A".to_string(),
            num_outcomes: 2,
            status: MarketStatusSnapshot::Active,
            metadata_digest: [1u8; 32],
            resolution_template: "admin_immediate".to_string(),
        };
        let first_group = MarketGroupSnapshot {
            group_id: 1,
            name: "Group B".to_string(),
            markets: vec![MarketId::new(2), MarketId::new(1)],
        };
        let second_group = MarketGroupSnapshot {
            group_id: 0,
            name: "Group A".to_string(),
            markets: vec![MarketId::new(3), MarketId::new(1)],
        };
        let sidecar_a = StateSidecarSnapshot {
            markets: vec![first_market.clone(), second_market.clone()],
            market_groups: vec![first_group.clone(), second_group.clone()],
            ..StateSidecarSnapshot::default()
        };
        let sidecar_b = StateSidecarSnapshot {
            markets: vec![second_market, first_market],
            market_groups: vec![second_group, first_group],
            ..StateSidecarSnapshot::default()
        };

        assert_eq!(
            compute_state_root_with_sidecar(&accounts, &sidecar_a),
            compute_state_root_with_sidecar(&accounts, &sidecar_b)
        );
    }

    #[test]
    fn test_valid_genesis_block() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let state_root = compute_state_root(&post_state);

        let witness = BlockWitness {
            header: genesis_header(state_root),
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_state_root_mismatch() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = BlockWitness {
            header: genesis_header([0xff; 32]), // wrong root
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::StateRootMismatch));
    }

    #[test]
    fn test_parent_hash_chain() {
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: 100,
            total_deposited: 100,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let state_root = compute_state_root(&post_state);

        let prev_header = genesis_header(state_root);
        let parent_hash = hash_header(&prev_header);

        let header = WitnessBlockHeader {
            height: 2,
            parent_hash,
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_height_not_consecutive() {
        let post_state = vec![];
        let state_root = compute_state_root(&post_state);

        let prev_header = genesis_header(state_root);
        let parent_hash = hash_header(&prev_header);

        let header = WitnessBlockHeader {
            height: 5, // Should be 2
            parent_hash,
            state_root,
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 2000,
        };

        let witness = BlockWitness {
            header,
            previous_header: Some(prev_header),
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_block(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::HeightNotConsecutive));
    }

    #[test]
    fn test_hash_header_deterministic() {
        let header = WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        assert_eq!(hash_header(&header), hash_header(&header));
    }

    fn position_set_strategy() -> impl Strategy<Value = Vec<(u8, u8, i16)>> {
        prop::collection::btree_map(
            (0u8..6, 0u8..2),
            (-20i16..=20i16).prop_filter("qty must be non-zero", |qty| *qty != 0),
            0..12,
        )
        .prop_map(|map| {
            map.into_iter()
                .map(|((market, outcome), qty)| (market, outcome, qty))
                .collect::<Vec<_>>()
        })
    }

    proptest! {
        #[test]
        fn prop_state_root_invariant_to_position_order(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            events_digest in prop::array::uniform32(any::<u8>()),
            positions in position_set_strategy(),
        ) {
            let mut reversed_positions = positions.clone();
            reversed_positions.reverse();

            let account_a = AccountSnapshot {
                id: 7,
                balance,
                total_deposited,
                positions: positions
                    .iter()
                    .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                    .collect(),
                events_digest,
            };
            let account_b = AccountSnapshot {
                id: 7,
                balance,
                total_deposited,
                positions: reversed_positions
                    .iter()
                    .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                    .collect(),
                events_digest,
            };

            prop_assert_eq!(
                compute_state_root(&[account_a]),
                compute_state_root(&[account_b]),
            );
        }

        #[test]
        fn prop_state_root_changes_when_balance_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            events_digest in prop::array::uniform32(any::<u8>()),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance: balance.saturating_add(1),
                total_deposited,
                positions,
                events_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }

        #[test]
        fn prop_state_root_changes_when_total_deposited_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            events_digest in prop::array::uniform32(any::<u8>()),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited: total_deposited.saturating_add(1),
                positions,
                events_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }

        #[test]
        fn prop_state_root_changes_when_events_digest_changes(
            balance in -1_000i64..=1_000,
            total_deposited in 0i64..=2_000,
            positions in position_set_strategy(),
            seed in any::<u8>(),
        ) {
            let positions: Vec<_> = positions
                .iter()
                .map(|(market, outcome, qty)| (MarketId::new(*market as u32), *outcome, *qty as i64))
                .collect();

            let before_digest = [seed; 32];
            let mut after_digest = before_digest;
            after_digest[0] = after_digest[0].wrapping_add(1);

            let before = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions: positions.clone(),
                events_digest: before_digest,
            };
            let after = AccountSnapshot {
                id: 0,
                balance,
                total_deposited,
                positions,
                events_digest: after_digest,
            };

            prop_assert_ne!(compute_state_root(&[before]), compute_state_root(&[after]));
        }
    }
}
