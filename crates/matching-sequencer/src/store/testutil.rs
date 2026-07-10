use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn temp_db_path(prefix: &str) -> PathBuf {
    let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sybil-{prefix}-{}-{unique}.redb",
        std::process::id()
    ))
}

impl Store {
    pub(crate) fn seed_fill_history_for_test(
        &self,
        records: &[(AccountId, AccountFillRecord)],
    ) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(FILL_HISTORY)?;
            for (account_id, record) in records {
                let key = fill_history_key(*account_id, record);
                let value = rmp_serde::to_vec(record)?;
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

pub(super) fn sample_header(height: u64) -> BlockHeader {
    BlockHeader {
        height,
        parent_hash: [height as u8; 32],
        state_root: [height as u8; 32],
        events_root: [height as u8; 32],
        order_count: 0,
        fill_count: 0,
        timestamp_ms: height * 1000,
    }
}

pub(super) fn sample_witness(header: &BlockHeader) -> BlockWitness {
    BlockWitness {
        header: header.to_witness_header(),
        previous_header: None,
        orders: Vec::new(),
        rejections: Vec::new(),
        system_events: Vec::new(),
        deposit_accumulator: sybil_verifier::DepositAccumulatorWitness::default(),
        fills: Vec::new(),
        clearing_prices: HashMap::new(),
        total_welfare: 0,
        minting_cost: 0,
        mm_constraints: Vec::new(),
        market_groups: Vec::new(),
        pre_state: Vec::new(),
        post_system_state: Vec::new(),
        post_state: Vec::new(),
        account_keys: vec![],
        state_sidecar: sybil_verifier::StateSidecarSnapshot::default(),
        pre_state_sidecar: sybil_verifier::StateSidecarSnapshot::default(),
        resolved_markets: Vec::new(),
    }
}

pub(super) fn eth_address(seed: u8) -> [u8; 20] {
    [seed; 20]
}

pub(super) fn next_l1_deposit_for(
    seq: &crate::sequencer::BlockSequencer,
    account_id: AccountId,
    amount_token_units: u64,
) -> crate::bridge::L1Deposit {
    let mut deposit = crate::bridge::L1Deposit {
        deposit_id: seq.bridge_state().deposit_cursor.saturating_add(1),
        account_id,
        chain_id: 1,
        vault_address: eth_address(0x10),
        token_address: eth_address(0x20),
        sender: eth_address(0x30),
        sybil_account_key: crate::bridge::account_key(account_id),
        amount_token_units,
        deposit_root: [0; 32],
    };
    let mut frontier = seq.bridge_state().deposit_frontier;
    deposit.deposit_root = crate::bridge::append_deposit_frontier(
        &mut frontier,
        seq.bridge_state().deposit_cursor,
        &deposit,
    )
    .expect("test deposit fits in frontier");
    deposit
}

pub(super) fn sample_sealed_block(header: &BlockHeader) -> SealedBlock {
    SealedBlock {
        canonical: crate::block::Block {
            header: header.clone(),
            order_ids: Vec::new(),
            system_events: Vec::new(),
            bridge: crate::bridge::BridgeBlockData::default(),
            fills: Vec::new(),
            clearing_prices: HashMap::new(),
            rejections: Vec::new(),
        },
        analytics: crate::block::BlockAnalytics::default(),
        derived_view_sidecar: crate::block::DerivedViewSidecar::default(),
    }
}

pub(super) fn coherent_header_and_witness(
    height: u64,
    accounts: &AccountStore,
    markets: &MarketSet,
    lifecycle: &MarketLifecycle,
    bridge_state: &BridgeState,
) -> (BlockHeader, BlockWitness) {
    let canonical_accounts = crate::canonical_state::CanonicalState::from_accounts(accounts);
    let state_sidecar =
        state_sidecar_snapshot_from_resting_orders(bridge_state, &[], markets, &[], lifecycle);
    let state_root = sybil_verifier::block::compute_state_root_with_sidecar(
        canonical_accounts.as_snapshots(),
        &state_sidecar,
    );
    let header = BlockHeader {
        height,
        parent_hash: [(height - 1) as u8; 32],
        state_root,
        events_root: sybil_verifier::event_commitment::empty_events_root(),
        order_count: 0,
        fill_count: 0,
        timestamp_ms: height * 1000,
    };
    let mut witness = sample_witness(&header);
    witness.post_state = canonical_accounts.into_snapshots();
    witness.state_sidecar = state_sidecar;
    (header, witness)
}

/// Owns the empty defaults for `SequencerSnapshot` references so test code
/// doesn't have to repeat the ceremony on every call site.
pub(super) struct TestEnv {
    empty_pk: HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>,
    empty_prices: HashMap<MarketId, Vec<Nanos>>,
    empty_volumes: HashMap<MarketId, u64>,
    pub(super) bridge_state: BridgeState,
    genesis_hash: [u8; 32],
}

impl TestEnv {
    pub(super) fn new() -> Self {
        Self {
            empty_pk: HashMap::new(),
            empty_prices: HashMap::new(),
            empty_volumes: HashMap::new(),
            bridge_state: BridgeState::default(),
            genesis_hash: [0x42; 32],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn snapshot<'a>(
        &'a self,
        accounts: &'a AccountStore,
        markets: &'a MarketSet,
        lifecycle: &'a MarketLifecycle,
        header: &'a BlockHeader,
        next_order_id: u64,
        market_volumes: Option<&'a HashMap<MarketId, u64>>,
        resting_orders: Vec<RestingOrder>,
    ) -> SequencerSnapshot<'a> {
        SequencerSnapshot {
            accounts,
            markets,
            market_groups: &[],
            lifecycle,
            header,
            genesis_hash: self.genesis_hash,
            next_order_id,
            pubkey_registry: &self.empty_pk,
            analytics: AnalyticsSnapshot {
                last_clearing_prices: &self.empty_prices,
                market_volumes: market_volumes.unwrap_or(&self.empty_volumes),
                account_fills: Vec::new(),
                trader_tracker: Default::default(),
                price_tracker_volume: Default::default(),
                price_tracker_clearing_history: Default::default(),
                liquidity_tracker: Default::default(),
                order_stats_tracker: Default::default(),
                welfare_tracker: Default::default(),
                first_deposit_ms: HashMap::new(),
                fill_total_counts: HashMap::new(),
                cost_basis_tracker: Default::default(),
                history_event_next_seq: 0,
                fill_history_delta: Vec::new(),
                equity_points_delta: Vec::new(),
                price_points_delta: Vec::new(),
                history_events_delta: Vec::new(),
            },
            price_candle_resolutions_secs: &[],
            bridge_state: &self.bridge_state,
            resting_orders,
        }
    }

    pub(super) fn snapshot_with_fills<'a>(
        &'a self,
        accounts: &'a AccountStore,
        markets: &'a MarketSet,
        lifecycle: &'a MarketLifecycle,
        header: &'a BlockHeader,
        account_fills: Vec<(AccountId, AccountFillRecord)>,
    ) -> SequencerSnapshot<'a> {
        SequencerSnapshot {
            accounts,
            markets,
            market_groups: &[],
            lifecycle,
            header,
            genesis_hash: self.genesis_hash,
            next_order_id: 1,
            pubkey_registry: &self.empty_pk,
            analytics: AnalyticsSnapshot {
                last_clearing_prices: &self.empty_prices,
                market_volumes: &self.empty_volumes,
                account_fills,
                trader_tracker: Default::default(),
                price_tracker_volume: Default::default(),
                price_tracker_clearing_history: Default::default(),
                liquidity_tracker: Default::default(),
                order_stats_tracker: Default::default(),
                welfare_tracker: Default::default(),
                first_deposit_ms: HashMap::new(),
                fill_total_counts: HashMap::new(),
                cost_basis_tracker: Default::default(),
                history_event_next_seq: 0,
                fill_history_delta: Vec::new(),
                equity_points_delta: Vec::new(),
                price_points_delta: Vec::new(),
                history_events_delta: Vec::new(),
            },
            price_candle_resolutions_secs: &[],
            bridge_state: &self.bridge_state,
            resting_orders: Vec::new(),
        }
    }

    pub(super) fn snapshot_with_price_points<'a>(
        &'a self,
        accounts: &'a AccountStore,
        markets: &'a MarketSet,
        lifecycle: &'a MarketLifecycle,
        header: &'a BlockHeader,
        price_points_delta: Vec<(MarketId, crate::market_info::PricePoint)>,
    ) -> SequencerSnapshot<'a> {
        SequencerSnapshot {
            accounts,
            markets,
            market_groups: &[],
            lifecycle,
            header,
            genesis_hash: self.genesis_hash,
            next_order_id: 1,
            pubkey_registry: &self.empty_pk,
            analytics: AnalyticsSnapshot {
                last_clearing_prices: &self.empty_prices,
                market_volumes: &self.empty_volumes,
                account_fills: Vec::new(),
                trader_tracker: Default::default(),
                price_tracker_volume: Default::default(),
                price_tracker_clearing_history: Default::default(),
                liquidity_tracker: Default::default(),
                order_stats_tracker: Default::default(),
                welfare_tracker: Default::default(),
                first_deposit_ms: HashMap::new(),
                fill_total_counts: HashMap::new(),
                cost_basis_tracker: Default::default(),
                history_event_next_seq: 0,
                fill_history_delta: Vec::new(),
                equity_points_delta: Vec::new(),
                price_points_delta,
                history_events_delta: Vec::new(),
            },
            price_candle_resolutions_secs: &[60, 300, 3_600],
            bridge_state: &self.bridge_state,
            resting_orders: Vec::new(),
        }
    }

    pub(super) fn snapshot_with_history_events<'a>(
        &'a self,
        accounts: &'a AccountStore,
        markets: &'a MarketSet,
        lifecycle: &'a MarketLifecycle,
        header: &'a BlockHeader,
        history_event_next_seq: u64,
        history_events_delta: Vec<crate::aggregates::StoredHistoryEvent>,
    ) -> SequencerSnapshot<'a> {
        SequencerSnapshot {
            accounts,
            markets,
            market_groups: &[],
            lifecycle,
            header,
            genesis_hash: self.genesis_hash,
            next_order_id: 1,
            pubkey_registry: &self.empty_pk,
            analytics: AnalyticsSnapshot {
                last_clearing_prices: &self.empty_prices,
                market_volumes: &self.empty_volumes,
                account_fills: Vec::new(),
                trader_tracker: Default::default(),
                price_tracker_volume: Default::default(),
                price_tracker_clearing_history: Default::default(),
                liquidity_tracker: Default::default(),
                order_stats_tracker: Default::default(),
                welfare_tracker: Default::default(),
                first_deposit_ms: HashMap::new(),
                fill_total_counts: HashMap::new(),
                cost_basis_tracker: Default::default(),
                history_event_next_seq,
                fill_history_delta: Vec::new(),
                equity_points_delta: Vec::new(),
                price_points_delta: Vec::new(),
                history_events_delta,
            },
            price_candle_resolutions_secs: &[],
            bridge_state: &self.bridge_state,
            resting_orders: Vec::new(),
        }
    }
}
