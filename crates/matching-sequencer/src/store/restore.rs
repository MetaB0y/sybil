use super::*;

/// Store-restored analytics projections. These are grouped separately from
/// core sequencer state, but still loaded from the existing redb tables.
pub struct AnalyticsRestoredState {
    pub last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: HashMap<MarketId, u64>,
    pub account_fills: Vec<(AccountId, AccountFillRecord)>,
    pub trader_tracker: TraderTrackerSnapshot,
    pub price_tracker_volume: PriceTrackerVolumeSnapshot,
    pub price_tracker_clearing_history: PriceTrackerClearingHistorySnapshot,
    pub liquidity_tracker: LiquidityTrackerSnapshot,
    pub order_stats_tracker: OrderStatsTrackerSnapshot,
    pub welfare_tracker: WelfareTrackerSnapshot,
    pub first_deposit_ms: HashMap<AccountId, u64>,
    pub fill_total_counts: HashMap<AccountId, u64>,
    pub cost_basis_tracker: CostBasisTrackerSnapshot,
    pub history_event_next_seq: u64,
}

/// State restored from the store on startup.
pub struct RestoredState {
    pub accounts: AccountStore,
    pub markets: MarketSet,
    pub market_groups: Vec<MarketGroup>,
    pub market_statuses: HashMap<MarketId, MarketStatus>,
    pub market_metadata: HashMap<MarketId, MarketMetadata>,
    pub height: u64,
    pub last_header: Option<BlockHeader>,
    pub genesis_hash: [u8; 32],
    pub next_order_id: u64,
    pub pubkey_registry: HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>,
    pub resting_orders: Vec<RestingOrder>,
    /// All registered data feeds.
    pub data_feeds: Vec<DataFeed>,
    /// All installed resolution templates.
    pub resolution_templates: Vec<ResolutionTemplate>,
    /// Bundle / MM / multi-market submissions that were admitted after the
    /// last committed block. The actor replays these into its in-memory
    /// pending queue so nothing acknowledged with a 200 OK is dropped by a
    /// crash.
    pub pending_bundles: Vec<crate::sequencer::OrderSubmission>,
    /// Non-MM single-market admissions that went into the resting book
    /// after the last committed block. On restart these are re-inserted
    /// on top of `resting_orders` before the sequencer starts processing.
    pub admit_log: Vec<RestingOrder>,
    /// Acknowledged control-plane mutations accepted after the last committed
    /// block. Replayed after snapshot restore so those writes are not lost.
    pub control_plane_log: Vec<ControlPlaneCommand>,
    /// Derived analytics projections restored from redb.
    pub analytics: AnalyticsRestoredState,
    /// L1 bridge sidecar state restored from the last committed block.
    pub bridge_state: BridgeState,
    /// L1 deposits durably accepted after the last committed block.
    pub pending_l1_deposits: Vec<L1Deposit>,
    /// Bridge withdrawals durably accepted after the last committed block.
    pub pending_bridge_withdrawals: Vec<BridgeWithdrawalRequest>,
    /// Confirmed bridge L1 inputs durably accepted after the last block.
    pub pending_bridge_l1_inputs: Vec<BridgeL1Input>,
}

impl Store {
    /// Load state from the store. Returns None if the store is empty (fresh start).
    pub async fn load_state(&self) -> Result<Option<RestoredState>, StoreError> {
        self.load_state_with_fill_history_cap(
            crate::fill_recorder::DEFAULT_MAX_FILL_HISTORY_PER_ACCOUNT,
        )
        .await
    }

    /// Load state while bounding each account's restored hot fill window.
    pub async fn load_state_with_fill_history_cap(
        &self,
        max_fill_history_per_account: usize,
    ) -> Result<Option<RestoredState>, StoreError> {
        let txn = self.db.begin_read()?;
        let Some(recovery_metadata) = read_recovery_metadata(&txn)? else {
            return Ok(None);
        };

        let accounts_map = self
            .account_state_store
            .recover(recovery_metadata.account_state)
            .await?;
        let num_accounts = accounts_map.len();
        let mut accounts = AccountStore::restore(accounts_map, recovery_metadata.next_account_id);
        let mut account_ids: Vec<_> = accounts.iter().map(|(account_id, _)| *account_id).collect();
        account_ids.sort_by_key(|account_id| account_id.0);

        // Markets
        let markets = {
            let table = txn.open_table(MARKETS)?;
            let mut market_map = HashMap::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let market: matching_engine::Market = rmp_serde::from_slice(value.value())?;
                market_map.insert(market.id, market);
            }
            MarketSet::restore(market_map, recovery_metadata.next_market_id)
        };

        // Market groups
        let market_groups: Vec<MarketGroup> = {
            let table = txn.open_table(MARKET_GROUPS)?;
            let mut groups = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let group: MarketGroup = rmp_serde::from_slice(value.value())?;
                groups.push((key.value(), group));
            }
            groups.sort_by_key(|(index, _)| *index);
            groups.into_iter().map(|(_, group)| group).collect()
        };

        // Market statuses
        let market_statuses = {
            let table = txn.open_table(MARKET_STATUSES)?;
            let mut statuses = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let status: MarketStatus = rmp_serde::from_slice(value.value())?;
                statuses.insert(MarketId(key.value()), status);
            }
            statuses
        };

        // Market metadata
        let market_metadata = {
            let table = txn.open_table(MARKET_META)?;
            let mut meta = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let metadata: MarketMetadata = rmp_serde::from_slice(value.value())?;
                meta.insert(MarketId(key.value()), metadata);
            }
            meta
        };

        // Last block header
        let last_header = {
            let table = txn.open_table(BLOCK_HEADERS)?;
            match table.get(recovery_metadata.height)? {
                Some(value) => {
                    let header: BlockHeader = rmp_serde::from_slice(value.value())?;
                    Some(header)
                }
                None => None,
            }
        };
        let latest_witness_exists = {
            let table = txn.open_table(BLOCK_WITNESSES)?;
            table.get(recovery_metadata.height)?.is_some()
        };

        let genesis_hash = {
            let table = txn.open_table(CHAIN_META)?;
            match table.get(KEY_GENESIS_HASH)? {
                Some(value) => parse_hash32(value.value(), KEY_GENESIS_HASH)?,
                None => match last_header.as_ref() {
                    Some(header) if header.height == 1 => crate::block::hash_header(header),
                    _ => {
                        return Err(StoreError::CorruptLayout(
                            "missing chain_meta genesis_hash".to_string(),
                        ));
                    }
                },
            }
        };

        // Pubkey registry
        let pubkey_registry = {
            let table = txn.open_table(PUBKEY_REGISTRY)?;
            let scheme_table = txn.open_table(PUBKEY_AUTH_SCHEMES)?;
            let meta_table = txn.open_table(PUBKEY_META)?;
            let mut registry = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let bytes = key.value();
                if let Some(pubkey) = crate::crypto::PublicKey::from_compressed_bytes(bytes) {
                    let auth_scheme = scheme_table
                        .get(bytes)?
                        .map(|stored| account_auth_scheme_from_store(stored.value()))
                        .unwrap_or_default();
                    // SYB-60 metadata; absent for keys registered before the
                    // feature landed, which default to a labelless primary key.
                    let meta: Option<PubkeyMetaRow> = meta_table
                        .get(bytes)?
                        .and_then(|stored| rmp_serde::from_slice(stored.value()).ok());
                    let (label, scope, created_at_ms) = match meta {
                        Some(m) => (m.label, key_scope_from_store(m.scope), m.created_at_ms),
                        None => (None, crate::crypto::KeyScope::Primary, 0),
                    };
                    registry.insert(
                        pubkey,
                        crate::crypto::RegisteredPubkey {
                            account_id: AccountId(value.value()),
                            auth_scheme,
                            label,
                            scope,
                            created_at_ms,
                        },
                    );
                } else {
                    warn!("invalid pubkey in store, skipping");
                }
            }
            registry
        };
        crate::digest::refresh_all_account_keys_digests(&mut accounts, &pubkey_registry);

        // Clearing prices
        let last_clearing_prices = {
            let table = txn.open_table(CLEARING_PRICES)?;
            let mut prices = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let price_vec: Vec<Nanos> = rmp_serde::from_slice(value.value())?;
                prices.insert(MarketId(key.value()), price_vec);
            }
            prices
        };

        let market_volumes = {
            let table = txn.open_table(MARKET_VOLUMES)?;
            let mut volumes = HashMap::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                volumes.insert(MarketId(key.value()), value.value());
            }
            volumes
        };

        let resting_orders: Vec<RestingOrder> = {
            let table = txn.open_table(RESTING_ORDERS)?;
            match table.get(KEY_RESTING_ORDERS_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => Vec::new(),
            }
        };

        let data_feeds: Vec<DataFeed> = {
            let table = txn.open_table(DATA_FEEDS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let resolution_templates: Vec<ResolutionTemplate> = {
            let table = txn.open_table(RESOLUTION_TEMPLATES)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let pending_bundles: Vec<crate::sequencer::OrderSubmission> = {
            let table = txn.open_table(PENDING_BUNDLES)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let admit_log: Vec<RestingOrder> = {
            let table = txn.open_table(ADMIT_LOG)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        validate_restored_reservations(&resting_orders)
            .map_err(|error| StoreError::CorruptLayout(error.to_string()))?;
        validate_restored_reservations(&admit_log)
            .map_err(|error| StoreError::CorruptLayout(format!("admit log {error}")))?;

        let control_plane_log: Vec<ControlPlaneCommand> = {
            let table = txn.open_table(CONTROL_PLANE_LOG)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let account_fills =
            self.recover_account_fills(&account_ids, max_fill_history_per_account)?;

        let bridge_state: BridgeState = {
            let table = txn.open_table(BRIDGE_STATE)?;
            match table.get(KEY_BRIDGE_STATE)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => BridgeState::default(),
            }
        };

        let pending_l1_deposits: Vec<L1Deposit> = {
            let table = txn.open_table(PENDING_L1_DEPOSITS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let pending_bridge_withdrawals: Vec<BridgeWithdrawalRequest> = {
            let table = txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        let pending_bridge_l1_inputs: Vec<BridgeL1Input> = {
            let table = txn.open_table(PENDING_BRIDGE_L1_INPUTS)?;
            let mut out = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                out.push(rmp_serde::from_slice(value.value())?);
            }
            out
        };

        if latest_witness_exists {
            let Some(header) = last_header.as_ref() else {
                return Err(StoreError::CorruptLayout(format!(
                    "missing block header for witnessed height {}",
                    recovery_metadata.height
                )));
            };
            self.ensure_state_qmdb_root(
                recovery_metadata.account_state,
                &accounts,
                &markets,
                &market_groups,
                &market_statuses,
                &market_metadata,
                &resting_orders,
                &bridge_state,
                &last_clearing_prices,
                header,
            )
            .await?;
        }

        // Trader tracker snapshot. Missing row -> cold-start default; the
        // tracker repopulates as admissions arrive after restart.
        let trader_tracker: TraderTrackerSnapshot = {
            let table = txn.open_table(TRADER_TRACKER)?;
            match table.get(KEY_TRADER_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => TraderTrackerSnapshot::default(),
            }
        };

        // Price-tracker volume extensions. Same missing-row → default
        // semantics as the trader tracker; cold restarts start with empty
        // hourly buckets and a zero platform total.
        let price_tracker_volume: PriceTrackerVolumeSnapshot = {
            let table = txn.open_table(PRICE_TRACKER_VOLUME)?;
            match table.get(KEY_PRICE_TRACKER_VOLUME_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => PriceTrackerVolumeSnapshot::default(),
            }
        };

        // Price-tracker clearing-history slice. Missing-row → default.
        let price_tracker_clearing_history: PriceTrackerClearingHistorySnapshot = {
            let table = txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
            match table.get(KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => PriceTrackerClearingHistorySnapshot::default(),
            }
        };

        // Liquidity tracker snapshot. Missing-row → default.
        let liquidity_tracker: LiquidityTrackerSnapshot = {
            let table = txn.open_table(LIQUIDITY_TRACKER)?;
            match table.get(KEY_LIQUIDITY_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => LiquidityTrackerSnapshot::default(),
            }
        };

        // OrderStatsTracker snapshot. Missing-row → default.
        let order_stats_tracker: OrderStatsTrackerSnapshot = {
            let table = txn.open_table(ORDER_STATS_TRACKER)?;
            match table.get(KEY_ORDER_STATS_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => OrderStatsTrackerSnapshot::default(),
            }
        };

        // WelfareTracker snapshot. Missing-row → default.
        let welfare_tracker: WelfareTrackerSnapshot = {
            let table = txn.open_table(WELFARE_TRACKER)?;
            match table.get(KEY_WELFARE_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => WelfareTrackerSnapshot::default(),
            }
        };

        // First-deposit timestamps (B8). Missing-row → empty.
        let first_deposit_ms: HashMap<AccountId, u64> = {
            let table = txn.open_table(FIRST_DEPOSIT_MS)?;
            match table.get(KEY_FIRST_DEPOSIT_MS_SNAPSHOT)? {
                Some(value) => {
                    let entries: Vec<(AccountId, u64)> = rmp_serde::from_slice(value.value())?;
                    entries.into_iter().collect()
                }
                None => HashMap::new(),
            }
        };

        // All-time fill counters per account (B8). Missing-row → empty.
        let fill_total_counts: HashMap<AccountId, u64> = {
            let table = txn.open_table(FILL_TOTAL_COUNTS)?;
            match table.get(KEY_FILL_TOTAL_COUNTS_SNAPSHOT)? {
                Some(value) => {
                    let entries: Vec<(AccountId, u64)> = rmp_serde::from_slice(value.value())?;
                    entries.into_iter().collect()
                }
                None => HashMap::new(),
            }
        };

        // CostBasisTracker snapshot (C1). Missing-row → default (cold start).
        let cost_basis_tracker: CostBasisTrackerSnapshot = {
            let table = txn.open_table(COST_BASIS_TRACKER)?;
            match table.get(KEY_COST_BASIS_TRACKER_SNAPSHOT)? {
                Some(value) => rmp_serde::from_slice(value.value())?,
                None => CostBasisTrackerSnapshot::default(),
            }
        };

        let history_event_next_seq = {
            let counters = txn.open_table(COUNTERS)?;
            counters
                .get(KEY_HISTORY_EVENT_NEXT_SEQ)?
                .map(|value| value.value())
        };
        let history_event_next_seq = match history_event_next_seq {
            Some(seq) => seq,
            None => {
                let table = txn.open_table(HISTORY_EVENTS)?;
                let mut max_seq = None;
                for entry in table.iter()? {
                    let (key, _) = entry?;
                    let Some(seq) = seq_from_history_event_key(key.value()) else {
                        warn!("invalid history event key in store, skipping for next_seq recovery");
                        continue;
                    };
                    max_seq = Some(max_seq.map_or(seq, |current: u64| current.max(seq)));
                }
                max_seq.map_or(0, |seq| seq.saturating_add(1))
            }
        };

        info!(
            height = recovery_metadata.height,
            accounts = num_accounts,
            markets = markets.len(),
            groups = market_groups.len(),
            clearing_prices = last_clearing_prices.len(),
            resting_orders = resting_orders.len(),
            pending_bundles = pending_bundles.len(),
            admit_log = admit_log.len(),
            control_plane_log = control_plane_log.len(),
            account_fills = account_fills.len(),
            data_feeds = data_feeds.len(),
            resolution_templates = resolution_templates.len(),
            bridge_deposit_cursor = bridge_state.deposit_cursor,
            pending_l1_deposits = pending_l1_deposits.len(),
            pending_bridge_withdrawals = pending_bridge_withdrawals.len(),
            pending_bridge_l1_inputs = pending_bridge_l1_inputs.len(),
            "state restored from store"
        );

        Ok(Some(RestoredState {
            accounts,
            markets,
            market_groups,
            market_statuses,
            market_metadata,
            height: recovery_metadata.height,
            last_header,
            genesis_hash,
            next_order_id: recovery_metadata.next_order_id,
            pubkey_registry,
            resting_orders,
            data_feeds,
            resolution_templates,
            pending_bundles,
            admit_log,
            control_plane_log,
            analytics: AnalyticsRestoredState {
                last_clearing_prices,
                market_volumes,
                account_fills,
                trader_tracker,
                price_tracker_volume,
                price_tracker_clearing_history,
                liquidity_tracker,
                order_stats_tracker,
                welfare_tracker,
                first_deposit_ms,
                fill_total_counts,
                cost_basis_tracker,
                history_event_next_seq,
            },
            bridge_state,
            pending_l1_deposits,
            pending_bridge_withdrawals,
            pending_bridge_l1_inputs,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn ensure_state_qmdb_root(
        &self,
        account_state: RecoveryAccountState,
        accounts: &AccountStore,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
        market_statuses: &HashMap<MarketId, MarketStatus>,
        market_metadata: &HashMap<MarketId, MarketMetadata>,
        resting_orders: &[RestingOrder],
        bridge_state: &BridgeState,
        last_clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        header: &BlockHeader,
    ) -> Result<(), StoreError> {
        let state_root = self
            .account_state_store
            .qmdb_state_root(account_state.slot)
            .await?;
        if state_root.root == header.state_root {
            return Ok(());
        }
        metrics::counter!("sybil_store_qmdb_root_mismatch_total", "phase" => "restore")
            .increment(1);

        warn!(
            height = account_state.height,
            slot = ?account_state.slot,
            root = ?state_root.root,
            header_root = ?header.state_root,
            "typed qMDB root mismatch during restore; rebuilding fenced state slot from redb snapshot"
        );

        let mut lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));
        for (&market_id, status) in market_statuses {
            lifecycle.set_market_status(market_id, status.clone());
        }
        for (&market_id, metadata) in market_metadata {
            lifecycle.set_market_metadata(market_id, metadata.clone());
        }
        let state_sidecar = state_sidecar_snapshot_from_resting_orders(
            bridge_state,
            resting_orders,
            markets,
            market_groups,
            &lifecycle,
            last_clearing_prices,
        );

        self.account_state_store
            .persist(CommittedAccountState {
                accounts,
                state_sidecar: &state_sidecar,
                height: account_state.height,
                next_account_id: account_state.next_account_id,
                slot: account_state.slot,
            })
            .await?;

        let repaired_root = self
            .account_state_store
            .qmdb_state_root(account_state.slot)
            .await?;
        if repaired_root.root == header.state_root {
            metrics::counter!("sybil_store_qmdb_repair_total", "result" => "success").increment(1);
            warn!(
                height = account_state.height,
                slot = ?account_state.slot,
                "repaired typed qMDB state slot from redb snapshot"
            );
            return Ok(());
        }

        warn!(
            height = account_state.height,
            slot = ?repaired_root.slot,
            root = ?repaired_root.root,
            header_root = ?header.state_root,
            "typed qMDB root still differs from committed header after repair"
        );
        metrics::counter!("sybil_store_qmdb_repair_total", "result" => "failed").increment(1);
        Err(StoreError::CorruptLayout(format!(
            "typed qMDB root mismatch at height {}: fence slot {:?} root={:?} header_root={:?}",
            account_state.height, repaired_root.slot, repaired_root.root, header.state_root
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AccountStateFence {
    pub(super) height: u64,
    pub(super) slot: AccountSnapshotSlot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PersistedCoreCounters {
    pub(super) height: u64,
    pub(super) next_account_id: u64,
    pub(super) next_market_id: u64,
    pub(super) next_order_id: u64,
    pub(super) account_state_fence: AccountStateFence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RecoveryMetadata {
    pub(super) height: u64,
    pub(super) next_account_id: u64,
    pub(super) next_market_id: u32,
    pub(super) next_order_id: u64,
    pub(super) account_state: RecoveryAccountState,
}

pub(super) fn validate_witness_header(
    header: &BlockHeader,
    witness: &BlockWitness,
) -> Result<(), StoreError> {
    let witness_header = &witness.header;
    if witness_header.height != header.height
        || witness_header.parent_hash != header.parent_hash
        || witness_header.state_root != header.state_root
        || witness_header.events_root != header.events_root
        || witness_header.order_count != header.order_count
        || witness_header.fill_count != header.fill_count
        || witness_header.timestamp_ms != header.timestamp_ms
    {
        return Err(StoreError::WitnessHeaderMismatch);
    }
    Ok(())
}

pub(super) fn initialize_or_validate_layout(db: &Database) -> Result<(), StoreError> {
    let txn = db.begin_read()?;
    let counters = txn.open_table(COUNTERS)?;
    match counters.get(KEY_STORE_LAYOUT_VERSION)? {
        Some(value) => {
            let version = value.value();
            if version != STORE_LAYOUT_VERSION {
                return Err(StoreError::UnsupportedLayout(format!(
                    "expected layout version {}, found {}",
                    STORE_LAYOUT_VERSION, version
                )));
            }
        }
        None => {
            let has_existing_state = counters.get(KEY_HEIGHT)?.is_some()
                || counters.get(KEY_ACCOUNT_STATE_HEIGHT)?.is_some();
            drop(counters);
            drop(txn);

            if has_existing_state {
                return Err(StoreError::UnsupportedLayout(
                    "legacy store layout detected; this account-state layout requires a fresh store"
                        .to_string(),
                ));
            }

            let txn = db.begin_write()?;
            let mut counters = txn.open_table(COUNTERS)?;
            counters.insert(KEY_STORE_LAYOUT_VERSION, STORE_LAYOUT_VERSION)?;
            drop(counters);
            txn.commit()?;
        }
    }
    Ok(())
}

pub(super) fn read_account_state_fence(
    db: &Database,
) -> Result<Option<AccountStateFence>, StoreError> {
    let txn = db.begin_read()?;
    let counters = txn.open_table(COUNTERS)?;
    let Some(height) = counters
        .get(KEY_ACCOUNT_STATE_HEIGHT)?
        .map(|value| value.value())
    else {
        return Ok(None);
    };
    let slot = required_counter(&counters, KEY_ACCOUNT_STATE_SLOT)?;
    Ok(Some(AccountStateFence {
        height,
        slot: AccountSnapshotSlot::decode(slot)?,
    }))
}

pub(super) fn read_recovery_metadata(
    txn: &redb::ReadTransaction,
) -> Result<Option<RecoveryMetadata>, StoreError> {
    let counters = txn.open_table(COUNTERS)?;
    let Some(height) = counters.get(KEY_HEIGHT)?.map(|value| value.value()) else {
        return Ok(None);
    };

    let next_account_id = counters
        .get(KEY_NEXT_ACCOUNT_ID)?
        .map(|value| value.value())
        .unwrap_or(0);
    let account_state_height = required_counter(&counters, KEY_ACCOUNT_STATE_HEIGHT)?;
    let account_state_slot =
        AccountSnapshotSlot::decode(required_counter(&counters, KEY_ACCOUNT_STATE_SLOT)?)?;

    if account_state_height != height {
        return Err(StoreError::CorruptLayout(format!(
            "metadata height mismatch: height={} account_state_height={}",
            height, account_state_height
        )));
    }

    Ok(Some(RecoveryMetadata {
        height,
        next_account_id,
        next_market_id: counters
            .get(KEY_NEXT_MARKET_ID)?
            .map(|value| value.value())
            .unwrap_or(0) as u32,
        next_order_id: counters
            .get(KEY_NEXT_ORDER_ID)?
            .map(|value| value.value())
            .unwrap_or(1),
        account_state: RecoveryAccountState {
            height: account_state_height,
            next_account_id,
            slot: account_state_slot,
        },
    }))
}

pub(super) fn write_core_counters(
    counters: &mut redb::Table<&str, u64>,
    persisted: PersistedCoreCounters,
) -> Result<(), StoreError> {
    counters.insert(KEY_HEIGHT, persisted.height)?;
    counters.insert(KEY_NEXT_ACCOUNT_ID, persisted.next_account_id)?;
    counters.insert(KEY_NEXT_MARKET_ID, persisted.next_market_id)?;
    counters.insert(KEY_NEXT_ORDER_ID, persisted.next_order_id)?;
    counters.insert(
        KEY_ACCOUNT_STATE_HEIGHT,
        persisted.account_state_fence.height,
    )?;
    counters.insert(
        KEY_ACCOUNT_STATE_SLOT,
        persisted.account_state_fence.slot.encode(),
    )?;
    Ok(())
}

fn required_counter(
    counters: &redb::ReadOnlyTable<&str, u64>,
    key: &'static str,
) -> Result<u64, StoreError> {
    counters
        .get(key)?
        .map(|value| value.value())
        .ok_or_else(|| StoreError::CorruptLayout(format!("missing required counter `{key}`")))
}
