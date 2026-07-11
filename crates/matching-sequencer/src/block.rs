use std::collections::HashMap;

use matching_engine::{Fill, MarketGroup, MarketId, MarketSet, Nanos};
use matching_solver::PipelineResult;
use sybil_verifier::{BlockWitness, WitnessBlockHeader};

use crate::account::AccountStore;
use crate::bridge::{bridge_state_snapshot, BridgeBlockData, BridgeState};
use crate::canonical_state::CanonicalState;
use crate::error::{Rejection, RejectionReason};
use crate::market_info::MarketMetadata;
use crate::market_lifecycle::MarketLifecycle;
use crate::order_book::{
    reservation_snapshots_from_resting_orders, resting_order_snapshots, OrderBook, RestingOrder,
};
use crate::system_event::SystemEvent;

/// Named result of [`BlockSequencer::produce_block`].
pub struct BlockProduction {
    pub block: Block,
    pub analytics: BlockAnalytics,
    pub derived_view_sidecar: DerivedViewSidecar,
    pub pipeline: PipelineResult,
    pub witness: BlockWitness,
    pub flow_metrics: BlockFlowMetrics,
}

impl BlockProduction {
    pub fn sealed_block(&self) -> SealedBlock {
        SealedBlock {
            canonical: self.block.clone(),
            analytics: self.analytics.clone(),
            derived_view_sidecar: self.derived_view_sidecar.clone(),
        }
    }
}

/// Per-block flow composition for metrics and observability.
pub struct BlockFlowMetrics {
    pub fresh_submissions: usize,
    pub fresh_orders_received: usize,
    pub carried_resting_orders: usize,
    pub fresh_orders_accepted: usize,
    pub rejected_orders: usize,
    pub pending_orders_after: usize,
}

/// Header of a sequencer block.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    /// blake3(previous header bytes), zeros for genesis.
    pub parent_hash: [u8; 32],
    /// Current typed qMDB state root over canonical account leaves plus bridge,
    /// market, and order-book sidecar leaves.
    pub state_root: [u8; 32],
    /// Keyless qMDB root over canonical block event leaves.
    pub events_root: [u8; 32],
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
}

impl BlockHeader {
    pub fn to_witness_header(&self) -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: self.height,
            parent_hash: self.parent_hash,
            state_root: self.state_root,
            events_root: self.events_root,
            order_count: self.order_count,
            fill_count: self.fill_count,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// A canonical block plus derived, API-facing analytics.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SealedBlock {
    pub canonical: Block,
    pub analytics: BlockAnalytics,
    /// Unproven derived-view lifecycle stream. This is not part of
    /// `canonical_witness_bytes`, `witness_root`, `da_commitment`, or the
    /// guest input; it rides with the block record for analytics consumers.
    #[serde(default)]
    pub derived_view_sidecar: DerivedViewSidecar,
}

/// A sequencer block produced each tick.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub order_ids: Vec<u64>,
    pub system_events: Vec<SystemEvent>,
    pub bridge: BridgeBlockData,
    pub fills: Vec<Fill>,
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub rejections: Vec<Rejection>,
}

/// Per-block sidecar data derived during block production.
///
/// This is not canonical protocol data: it is not part of the block hash,
/// state root, events root, or witness. API/SSE consumers receive it next to
/// the block so product views do not have to reconstruct common summaries.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BlockAnalytics {
    pub total_welfare: i64,
    pub total_volume: u64,
    pub orders_filled: usize,
    /// Unique placers (non-MM accounts) admitted into this block. Platform
    /// scalar — derived from `WitnessOrder.account_id` after filtering MM.
    pub unique_placers: u32,
    /// Per-market unique placers in this block. A multi-market order
    /// credits each active market; the platform `unique_placers` counts the
    /// account once (so sum-of-by-market over-counts for spreads).
    pub placers_by_market: HashMap<MarketId, u32>,
    /// Per-market volume contribution from this block's fills, in nanos. A
    /// multi-market fill credits every active market with its full notional;
    /// the platform `total_volume` counts each fill once (so sum-of-by-market
    /// over-counts for bundles, just like `placers_by_market`).
    pub volume_by_market: HashMap<MarketId, u64>,
    /// Per-market order placement/matching/unmatching counts for this block.
    /// `placed` counts every active market of every successful non-MM admit;
    /// `matched` and `unmatched` count exits from the resting book (expire,
    /// revalidate, post-solve settle). Multi-market orders over-count
    /// per-market vs. the platform scalars in `BlockFlowMetrics`. Cancels
    /// are NOT counted here (D1 carries them via OrderCancelled).
    pub orders_by_market: HashMap<MarketId, crate::aggregates::OrderStats>,
    /// Per-market welfare contribution in nanos from this block's fills
    /// (B7). Multi-market fills credit every active market with their full
    /// welfare; the platform `total_welfare` counts each fill once, so
    /// sum-of-per-market over-counts for spreads/bundles. Signed because
    /// solver rounding can yield small negatives.
    pub welfare_by_market: HashMap<MarketId, i64>,
}

/// Provenance marker for fields derived by the sequencer read model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedViewProvenance {
    #[default]
    DerivedUnproven,
}

/// Unproven lifecycle stream for read-model reconstruction.
///
/// This sidecar deliberately stays outside the verifier and canonical witness
/// bytes. It carries data the analytics views need but the proof system does
/// not currently bind: removed resting-order identity, `has_been_matched`,
/// view-level exit reasons, and direct-admit timing.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DerivedViewSidecar {
    #[serde(default)]
    pub provenance: DerivedViewProvenance,
    #[serde(default)]
    pub removed_orders: Vec<RemovedOrderView>,
    #[serde(default)]
    pub admits: Vec<AdmitTimingView>,
    #[serde(default)]
    pub rejection_history: Vec<RejectedOrderView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovedOrderPhase {
    BlockStartExpire,
    BlockStartRevalidate,
    PostSolve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovedOrderExitReason {
    Expired,
    RevalidateInsufficientBalance,
    RevalidateInsufficientPosition,
    RevalidateMarketInactive,
    RevalidateAccountGone,
    RevalidateAccountInsolvent,
    RevalidateRejected,
    Filled,
    Settled,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemovedOrderView {
    pub order: matching_engine::Order,
    pub order_id: u64,
    pub account_id: u64,
    pub phase: RemovedOrderPhase,
    pub exit_reason: RemovedOrderExitReason,
    pub has_been_matched: bool,
    pub reserved_balance_released: i64,
    pub reserved_positions_released: Vec<(MarketId, u8, i64)>,
    pub active_markets: Vec<MarketId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<RejectionReason>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AdmitTimingView {
    pub order_id: u64,
    pub account_id: u64,
    pub admit_height: u64,
    pub admit_timestamp_ms: u64,
    pub is_new: bool,
    pub is_mm: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RejectedOrderView {
    pub order: matching_engine::Order,
    pub order_id: u64,
    pub account_id: u64,
    pub reason: RejectionReason,
}

impl RemovedOrderView {
    pub(crate) fn from_resting_order(
        resting: &RestingOrder,
        phase: RemovedOrderPhase,
        exit_reason: RemovedOrderExitReason,
        rejection_reason: Option<RejectionReason>,
    ) -> Self {
        let mut reserved_positions_released: Vec<_> = resting
            .reserved_positions
            .iter()
            .map(|&((market, outcome), qty)| (market, outcome, qty))
            .collect();
        reserved_positions_released.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
        let active_markets: Vec<_> = resting.order.active_markets().collect();
        Self {
            order: resting.order.clone(),
            order_id: resting.order.id,
            account_id: resting.account_id.0,
            phase,
            exit_reason,
            has_been_matched: resting.has_been_matched,
            reserved_balance_released: resting.reserved_balance,
            reserved_positions_released,
            active_markets,
            rejection_reason,
        }
    }
}

/// Compute a deterministic account-only state root with the verifier's
/// zero-valued bridge sidecar.
///
/// Production blocks should call [`compute_complete_state_root`] so the sidecar
/// committed by the witness is included.
pub fn compute_state_root(accounts: &AccountStore) -> [u8; 32] {
    CanonicalState::from_accounts(accounts).state_root()
}

pub fn compute_complete_state_root(
    accounts: &AccountStore,
    bridge: &BridgeState,
    order_book: &OrderBook,
    markets: &MarketSet,
    market_groups: &[MarketGroup],
    lifecycle: &MarketLifecycle,
    last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> [u8; 32] {
    let accounts = CanonicalState::from_accounts(accounts);
    sybil_verifier::block::compute_state_root_with_sidecar(
        accounts.as_snapshots(),
        &state_sidecar_snapshot(
            bridge,
            order_book,
            markets,
            market_groups,
            lifecycle,
            last_clearing_prices,
        ),
    )
}

pub fn state_sidecar_snapshot(
    bridge: &BridgeState,
    order_book: &OrderBook,
    markets: &MarketSet,
    market_groups: &[MarketGroup],
    lifecycle: &MarketLifecycle,
    last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> sybil_verifier::StateSidecarSnapshot {
    state_sidecar_snapshot_from_resting_orders(
        bridge,
        &order_book.snapshot(),
        markets,
        market_groups,
        lifecycle,
        last_clearing_prices,
    )
}

pub(crate) fn state_sidecar_snapshot_from_resting_orders(
    bridge: &BridgeState,
    resting_orders: &[RestingOrder],
    markets: &MarketSet,
    market_groups: &[MarketGroup],
    lifecycle: &MarketLifecycle,
    last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> sybil_verifier::StateSidecarSnapshot {
    sybil_verifier::StateSidecarSnapshot {
        bridge: bridge_state_snapshot(bridge),
        markets: market_snapshots(markets, lifecycle, last_clearing_prices),
        market_groups: market_group_snapshots(market_groups),
        resting_orders: resting_order_snapshots(resting_orders),
        account_reservations: reservation_snapshots_from_resting_orders(resting_orders),
    }
}

fn market_snapshots(
    markets: &MarketSet,
    lifecycle: &MarketLifecycle,
    last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Vec<sybil_verifier::MarketSnapshot> {
    let mut snapshots: Vec<_> = markets
        .iter()
        .map(|market| {
            let metadata = lifecycle.market_metadata(market.id);
            sybil_verifier::MarketSnapshot {
                market_id: market.id,
                name: market.name.clone(),
                num_outcomes: market.num_outcomes(),
                status: market_status_snapshot(lifecycle.market_status(market.id)),
                metadata_digest: market_metadata_digest(metadata),
                resolution_template: lifecycle.template_for_market(market.id).to_string(),
                last_clearing_prices: last_clearing_prices
                    .get(&market.id)
                    .cloned()
                    .unwrap_or_default(),
            }
        })
        .collect();
    snapshots.sort_by_key(|market| market.market_id.0);
    snapshots
}

fn market_group_snapshots(groups: &[MarketGroup]) -> Vec<sybil_verifier::MarketGroupSnapshot> {
    groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mut markets = group.markets.clone();
            markets.sort_by_key(|market| market.0);
            sybil_verifier::MarketGroupSnapshot {
                group_id: index as u64,
                name: group.name.clone(),
                markets,
            }
        })
        .collect()
}

fn market_metadata_digest(metadata: Option<&MarketMetadata>) -> [u8; 32] {
    let mut payload = Vec::new();
    match metadata {
        None => payload.push(0),
        Some(metadata) => {
            if let Some(digest) = metadata.committed_metadata_digest {
                return digest;
            }
            payload.push(1);
            append_string(&mut payload, &metadata.description);
            append_string(&mut payload, &metadata.category);

            let mut tags = metadata.tags.clone();
            tags.sort();
            payload.extend_from_slice(&(tags.len() as u64).to_le_bytes());
            for tag in tags {
                append_string(&mut payload, &tag);
            }

            append_string(&mut payload, &metadata.resolution_criteria);
            payload.extend_from_slice(&metadata.expiry_timestamp_ms.to_le_bytes());
            payload.extend_from_slice(&metadata.created_at_ms.to_le_bytes());
            append_string(&mut payload, metadata.effective_template());
        }
    }
    sybil_verifier::block::market_metadata_digest(&payload)
}

fn append_string(value: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    value.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    value.extend_from_slice(bytes);
}

fn market_status_snapshot(
    status: sybil_oracle::MarketStatus,
) -> sybil_verifier::MarketStatusSnapshot {
    match status {
        sybil_oracle::MarketStatus::Active => sybil_verifier::MarketStatusSnapshot::Active,
        sybil_oracle::MarketStatus::Proposed {
            proposal,
            challenge_deadline_ms,
        } => sybil_verifier::MarketStatusSnapshot::Proposed {
            proposal: resolution_proposal_snapshot(proposal),
            challenge_deadline_ms,
        },
        sybil_oracle::MarketStatus::Challenged {
            proposal,
            challenge,
        } => sybil_verifier::MarketStatusSnapshot::Challenged {
            proposal: resolution_proposal_snapshot(proposal),
            challenge: challenge_snapshot(challenge),
        },
        sybil_oracle::MarketStatus::Resolved { record } => {
            sybil_verifier::MarketStatusSnapshot::Resolved {
                record: resolution_record_snapshot(record),
            }
        }
        sybil_oracle::MarketStatus::Voided => sybil_verifier::MarketStatusSnapshot::Voided,
    }
}

fn resolution_proposal_snapshot(
    proposal: sybil_oracle::ResolutionProposal,
) -> sybil_verifier::ResolutionProposalSnapshot {
    sybil_verifier::ResolutionProposalSnapshot {
        id: proposal.id.0,
        market_id: proposal.market_id,
        payout_nanos: proposal.payout_nanos,
        source: oracle_source_snapshot(proposal.source),
        proposed_at_ms: proposal.proposed_at_ms,
        reason: proposal.reason,
    }
}

fn challenge_snapshot(challenge: sybil_oracle::Challenge) -> sybil_verifier::ChallengeSnapshot {
    sybil_verifier::ChallengeSnapshot {
        id: challenge.id.0,
        challenger: challenge.challenger,
        proposal_id: challenge.proposal_id.0,
        bond_amount: challenge.bond_amount,
        proposed_payout_nanos: challenge.proposed_payout_nanos,
        reason: challenge.reason,
        challenged_at_ms: challenge.challenged_at_ms,
    }
}

fn resolution_record_snapshot(
    record: sybil_oracle::ResolutionRecord,
) -> sybil_verifier::ResolutionRecordSnapshot {
    sybil_verifier::ResolutionRecordSnapshot {
        market_id: record.market_id,
        payout_nanos: record.payout_nanos,
        resolved_by: oracle_source_snapshot(record.resolved_by),
        resolved_at_ms: record.resolved_at_ms,
        proposal: record.proposal.map(resolution_proposal_snapshot),
        challenge: record.challenge.map(challenge_snapshot),
    }
}

fn oracle_source_snapshot(
    source: sybil_oracle::OracleSource,
) -> sybil_verifier::OracleSourceSnapshot {
    match source {
        sybil_oracle::OracleSource::Admin => sybil_verifier::OracleSourceSnapshot::Admin,
        sybil_oracle::OracleSource::DataFeed(feed_id) => {
            sybil_verifier::OracleSourceSnapshot::DataFeed(feed_id.0)
        }
        sybil_oracle::OracleSource::AutomatedL0 => {
            sybil_verifier::OracleSourceSnapshot::AutomatedL0
        }
    }
}

/// Compute blake3 hash of a block header for chaining.
pub fn hash_header(header: &BlockHeader) -> [u8; 32] {
    sybil_verifier::commitments::hash_header(&header.to_witness_header())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::canonical_state::CanonicalState;
    use crate::market_info::MarketMetadata;
    use crate::market_lifecycle::MarketLifecycle;
    use matching_engine::{MarketId, MarketSet};
    use proptest::prelude::*;
    use std::sync::Arc;
    use sybil_oracle::{AdminOracle, MarketStatus, OracleSource, ResolutionRecord};
    use sybil_verifier::AccountSnapshot;

    #[test]
    fn test_state_root_deterministic() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);
        let a1 = accounts.create_account(200);

        let m0 = MarketId::new(0);
        accounts.get_mut(a0).unwrap().positions.insert((m0, 0), 10);
        accounts.get_mut(a1).unwrap().positions.insert((m0, 1), 5);

        let root1 = compute_state_root(&accounts);
        let root2 = compute_state_root(&accounts);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_mutation() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);

        accounts.get_mut(a0).unwrap().balance = 200;
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_events_digest_only() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);
        accounts.get_mut(a0).unwrap().events_digest = [7u8; 32];
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_keys_digest_only() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);
        accounts.get_mut(a0).unwrap().keys_digest = [7u8; 32];
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_total_deposited_only() {
        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);

        let root1 = compute_state_root(&accounts);
        accounts.get_mut(a0).unwrap().total_deposited = 150;
        let root2 = compute_state_root(&accounts);

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_state_root_empty_accounts() {
        let accounts = AccountStore::new();
        let root = compute_state_root(&accounts);
        // Empty hash should be deterministic
        assert_eq!(root, compute_state_root(&accounts));
    }

    #[test]
    fn test_state_root_position_order_independence() {
        // Adding positions in different order should produce same root
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);

        let mut accounts1 = AccountStore::new();
        let a = accounts1.create_account(100);
        let acc = accounts1.get_mut(a).unwrap();
        acc.positions.insert((m0, 0), 10);
        acc.positions.insert((m1, 0), 20);

        let mut accounts2 = AccountStore::new();
        let b = accounts2.create_account(100);
        let acc = accounts2.get_mut(b).unwrap();
        acc.positions.insert((m1, 0), 20);
        acc.positions.insert((m0, 0), 10);

        assert_eq!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2)
        );
    }

    #[test]
    fn test_state_root_ignores_zero_quantity_positions() {
        let m0 = MarketId::new(0);

        let mut accounts = AccountStore::new();
        let a0 = accounts.create_account(100);
        accounts.get_mut(a0).unwrap().positions.insert((m0, 0), 0);

        let mut snapshot: Vec<_> = accounts
            .iter()
            .map(|(&id, account)| {
                let mut positions: Vec<_> = account
                    .positions
                    .iter()
                    .filter(|(_, &qty)| qty != 0)
                    .map(|(&(market, outcome), &qty)| (market, outcome, qty))
                    .collect();
                positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
                AccountSnapshot {
                    id: id.0,
                    balance: account.balance,
                    total_deposited: account.total_deposited,
                    positions,
                    events_digest: account.events_digest,
                    keys_digest: account.keys_digest,
                }
            })
            .collect();
        snapshot.sort_by_key(|account| account.id);

        assert_eq!(
            compute_state_root(&accounts),
            sybil_verifier::block::compute_state_root(&snapshot)
        );
    }

    #[test]
    fn test_hash_header_deterministic() {
        let header = BlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            events_root: [2u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        assert_eq!(hash_header(&header), hash_header(&header));
    }

    #[test]
    fn test_hash_header_changes_on_field_change() {
        let h1 = BlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            events_root: [2u8; 32],
            order_count: 5,
            fill_count: 3,
            timestamp_ms: 1000,
        };
        let h2 = BlockHeader {
            height: 2,
            ..h1.clone()
        };
        assert_ne!(hash_header(&h1), hash_header(&h2));
    }

    #[test]
    fn test_state_root_commits_market_registry() {
        let accounts = AccountStore::new();
        let bridge = BridgeState::default();
        let order_book = OrderBook::new(3);
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("Rain tomorrow");
        let groups = Vec::new();
        let prices = HashMap::new();
        let mut lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));

        let active_root = compute_complete_state_root(
            &accounts,
            &bridge,
            &order_book,
            &markets,
            &groups,
            &lifecycle,
            &prices,
        );

        lifecycle.set_market_metadata(
            market_id,
            MarketMetadata {
                description: "Rain in London by noon".to_string(),
                ..MarketMetadata::default()
            },
        );
        let metadata_root = compute_complete_state_root(
            &accounts,
            &bridge,
            &order_book,
            &markets,
            &groups,
            &lifecycle,
            &prices,
        );
        assert_ne!(active_root, metadata_root);

        lifecycle.set_market_status(
            market_id,
            MarketStatus::Resolved {
                record: ResolutionRecord {
                    market_id,
                    payout_nanos: matching_engine::Nanos(1_000_000_000),
                    resolved_by: OracleSource::Admin,
                    resolved_at_ms: 1_000,
                    proposal: None,
                    challenge: None,
                },
            },
        );
        let resolved_root = compute_complete_state_root(
            &accounts,
            &bridge,
            &order_book,
            &markets,
            &groups,
            &lifecycle,
            &prices,
        );
        assert_ne!(metadata_root, resolved_root);
    }

    proptest! {
        #[test]
        fn prop_sequencer_and_verifier_state_roots_agree(
            balances in prop::collection::vec(-1_000i64..=1_000, 0..6),
            digests in prop::collection::vec(prop::array::uniform32(any::<u8>()), 0..6),
        ) {
            let len = balances.len().min(digests.len());
            let mut accounts = AccountStore::new();

            for index in 0..len {
                let account_id = accounts.create_account(balances[index]);
                let account = accounts.get_mut(account_id).unwrap();
                account.total_deposited = balances[index].saturating_add(index as i64);
                account.events_digest = digests[index];

                if index % 2 == 0 {
                    account.positions.insert((MarketId::new(index as u32), 0), index as i64 + 1);
                } else {
                    account.positions.insert((MarketId::new(index as u32), 1), -((index as i64) + 1));
                }
            }

            let snapshots = CanonicalState::from_accounts(&accounts).into_snapshots();
            prop_assert_eq!(
                compute_state_root(&accounts),
                sybil_verifier::block::compute_state_root(&snapshots),
            );
        }
    }
}
