use super::*;

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Persistent store for sequencer state. Wraps a redb database.
pub struct Store {
    pub(crate) db: Arc<Database>,
    pub(crate) account_state_store: Box<dyn AccountStateStore>,
    #[cfg(test)]
    pub(crate) fault_injection: Arc<Mutex<StoreFaultInjection>>,
}

/// Borrowed analytics view needed to persist one block.
pub struct AnalyticsSnapshot<'a> {
    pub last_clearing_prices: &'a HashMap<MarketId, Vec<Nanos>>,
    pub market_volumes: &'a HashMap<MarketId, u64>,
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
    pub fill_history_delta: Vec<(AccountId, AccountFillRecord)>,
    pub price_points_delta: Vec<(MarketId, crate::market_info::PricePoint)>,
    pub equity_points_delta: Vec<(AccountId, crate::aggregates::EquityPoint)>,
    pub history_events_delta: Vec<crate::aggregates::StoredHistoryEvent>,
}

/// Borrowed view of sequencer state needed to persist one block.
/// Constructed by `BlockSequencer::snapshot()` and consumed by `Store::save_block`.
pub struct SequencerSnapshot<'a> {
    pub accounts: &'a AccountStore,
    pub markets: &'a MarketSet,
    pub market_groups: &'a [MarketGroup],
    pub lifecycle: &'a MarketLifecycle,
    pub header: &'a BlockHeader,
    pub genesis_hash: [u8; 32],
    pub next_order_id: u64,
    pub pubkey_registry: &'a HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>,
    pub analytics: AnalyticsSnapshot<'a>,
    pub price_candle_resolutions_secs: &'a [u32],
    /// Owned because the snapshot clones the live book — cheap for bounded sizes.
    pub resting_orders: Vec<RestingOrder>,
    pub bridge_state: &'a BridgeState,
}

struct RedbBlockCommit {
    height: u64,
    genesis_hash: [u8; 32],
    market_rows: Vec<(u32, Vec<u8>)>,
    market_meta_rows: Vec<(u32, Vec<u8>)>,
    market_status_rows: Vec<(u32, Vec<u8>)>,
    market_group_rows: Vec<(u32, Vec<u8>)>,
    header_bytes: Vec<u8>,
    history_block_bytes: Option<Vec<u8>>,
    witness_bytes: Option<Vec<u8>>,
    pubkey_rows: Vec<(Vec<u8>, crate::crypto::RegisteredPubkey)>,
    clearing_price_rows: Vec<(u32, Vec<u8>)>,
    market_volume_rows: Vec<(u32, u64)>,
    resting_orders_bytes: Vec<u8>,
    fill_history_rows: Vec<([u8; 24], Vec<u8>)>,
    equity_point_rows: Vec<([u8; 16], Vec<u8>)>,
    history_event_rows: Vec<([u8; 24], Vec<u8>)>,
    history_event_next_seq: u64,
    price_point_rows: Vec<RedbPricePointRow>,
    price_candle_resolutions_secs: Vec<u32>,
    data_feed_rows: Vec<(u64, Vec<u8>)>,
    resolution_template_rows: Vec<(String, Vec<u8>)>,
    bridge_state_bytes: Vec<u8>,
    trader_tracker_bytes: Vec<u8>,
    price_tracker_volume_bytes: Vec<u8>,
    price_tracker_clearing_history_bytes: Vec<u8>,
    liquidity_tracker_bytes: Vec<u8>,
    order_stats_tracker_bytes: Vec<u8>,
    welfare_tracker_bytes: Vec<u8>,
    first_deposit_ms_bytes: Vec<u8>,
    fill_total_counts_bytes: Vec<u8>,
    cost_basis_tracker_bytes: Vec<u8>,
    counters: PersistedCoreCounters,
}

struct RedbPricePointRow {
    market_id: MarketId,
    point: PricePoint,
    key: [u8; 12],
    bytes: Vec<u8>,
}

fn build_redb_block_commit(
    snapshot: &SequencerSnapshot<'_>,
    witness: Option<&BlockWitness>,
    history_block: Option<&SealedBlock>,
    next_slot: AccountSnapshotSlot,
) -> Result<RedbBlockCommit, StoreError> {
    let mut market_rows = Vec::new();
    for (id, market) in snapshot.markets.iter_with_ids() {
        market_rows.push((id.0, rmp_serde::to_vec(market)?));
    }

    let mut market_status_rows = Vec::new();
    for (&market_id, status) in snapshot.lifecycle.market_statuses() {
        market_status_rows.push((market_id.0, rmp_serde::to_vec(status)?));
    }

    let mut market_meta_rows = Vec::new();
    for (id, _) in snapshot.markets.iter_with_ids() {
        if let Some(meta) = snapshot.lifecycle.market_metadata(*id) {
            market_meta_rows.push((id.0, rmp_serde::to_vec(meta)?));
        }
    }

    let mut market_group_rows = Vec::new();
    for (i, group) in snapshot.market_groups.iter().enumerate() {
        market_group_rows.push((i as u32, rmp_serde::to_vec(group)?));
    }

    let witness_bytes = witness.map(rmp_serde::to_vec).transpose()?;
    let history_block_bytes = history_block.map(rmp_serde::to_vec).transpose()?;

    let pubkey_rows = snapshot
        .pubkey_registry
        .iter()
        .map(|(pubkey, registered)| (pubkey.compressed_bytes().to_vec(), registered.clone()))
        .collect();

    let mut clearing_price_rows = Vec::new();
    for (&market_id, prices) in snapshot.analytics.last_clearing_prices {
        clearing_price_rows.push((market_id.0, rmp_serde::to_vec(prices)?));
    }

    let market_volume_rows = snapshot
        .analytics
        .market_volumes
        .iter()
        .map(|(&market_id, &volume)| (market_id.0, volume))
        .collect();

    let mut fill_history_rows = Vec::new();
    for (account_id, record) in &snapshot.analytics.account_fills {
        fill_history_rows.push((
            fill_history_key(*account_id, record),
            rmp_serde::to_vec(record)?,
        ));
    }
    for (account_id, record) in &snapshot.analytics.fill_history_delta {
        fill_history_rows.push((
            fill_history_key(*account_id, record),
            rmp_serde::to_vec(record)?,
        ));
    }

    let mut equity_point_rows = Vec::new();
    for (aid, point) in &snapshot.analytics.equity_points_delta {
        equity_point_rows.push((equity_key(*aid, point.height), rmp_serde::to_vec(point)?));
    }

    let mut history_event_rows = Vec::new();
    for event in &snapshot.analytics.history_events_delta {
        history_event_rows.push((
            history_event_key(AccountId(event.account_id), event.block_height, event.seq),
            rmp_serde::to_vec(event)?,
        ));
    }

    let mut price_point_rows = Vec::new();
    for (market_id, point) in &snapshot.analytics.price_points_delta {
        price_point_rows.push(RedbPricePointRow {
            market_id: *market_id,
            point: point.clone(),
            key: price_point_key(*market_id, point.height),
            bytes: rmp_serde::to_vec(point)?,
        });
    }

    let mut data_feed_rows = Vec::new();
    for feed in snapshot.lifecycle.feeds().iter() {
        data_feed_rows.push((feed.id.0, rmp_serde::to_vec(feed)?));
    }

    let mut resolution_template_rows = Vec::new();
    for (template_id, template) in snapshot.lifecycle.templates().iter() {
        resolution_template_rows.push((template_id.0.clone(), rmp_serde::to_vec(template)?));
    }

    let mut first_deposit_entries: Vec<(AccountId, u64)> = snapshot
        .analytics
        .first_deposit_ms
        .iter()
        .map(|(&aid, &ts)| (aid, ts))
        .collect();
    first_deposit_entries.sort_by_key(|(aid, _)| aid.0);

    let mut fill_total_entries: Vec<(AccountId, u64)> = snapshot
        .analytics
        .fill_total_counts
        .iter()
        .map(|(&aid, &n)| (aid, n))
        .collect();
    fill_total_entries.sort_by_key(|(aid, _)| aid.0);

    Ok(RedbBlockCommit {
        height: snapshot.header.height,
        genesis_hash: snapshot.genesis_hash,
        market_rows,
        market_meta_rows,
        market_status_rows,
        market_group_rows,
        header_bytes: rmp_serde::to_vec(snapshot.header)?,
        history_block_bytes,
        witness_bytes,
        pubkey_rows,
        clearing_price_rows,
        market_volume_rows,
        resting_orders_bytes: rmp_serde::to_vec(&snapshot.resting_orders)?,
        fill_history_rows,
        equity_point_rows,
        history_event_rows,
        history_event_next_seq: snapshot.analytics.history_event_next_seq,
        price_point_rows,
        price_candle_resolutions_secs: snapshot.price_candle_resolutions_secs.to_vec(),
        data_feed_rows,
        resolution_template_rows,
        bridge_state_bytes: rmp_serde::to_vec(snapshot.bridge_state)?,
        trader_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.trader_tracker)?,
        price_tracker_volume_bytes: rmp_serde::to_vec(&snapshot.analytics.price_tracker_volume)?,
        price_tracker_clearing_history_bytes: rmp_serde::to_vec(
            &snapshot.analytics.price_tracker_clearing_history,
        )?,
        liquidity_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.liquidity_tracker)?,
        order_stats_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.order_stats_tracker)?,
        welfare_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.welfare_tracker)?,
        first_deposit_ms_bytes: rmp_serde::to_vec(&first_deposit_entries)?,
        fill_total_counts_bytes: rmp_serde::to_vec(&fill_total_entries)?,
        cost_basis_tracker_bytes: rmp_serde::to_vec(&snapshot.analytics.cost_basis_tracker)?,
        counters: PersistedCoreCounters {
            height: snapshot.header.height,
            next_account_id: snapshot.accounts.next_id(),
            next_market_id: snapshot.markets.next_id() as u64,
            next_order_id: snapshot.next_order_id,
            account_state_fence: AccountStateFence {
                height: snapshot.header.height,
                slot: next_slot,
            },
        },
    })
}

#[cfg(test)]
fn write_redb_block_commit(
    db: &Database,
    commit: RedbBlockCommit,
    fault_injection: Arc<Mutex<StoreFaultInjection>>,
) -> Result<(), StoreError> {
    write_redb_block_commit_inner(db, commit, || {
        pop_save_block_fault(&fault_injection, StoreFaultPoint::BeforeRedbFenceCommit)
    })
}

#[cfg(not(test))]
fn write_redb_block_commit(db: &Database, commit: RedbBlockCommit) -> Result<(), StoreError> {
    write_redb_block_commit_inner(db, commit, || Ok(()))
}

fn write_redb_block_commit_inner<F>(
    db: &Database,
    commit: RedbBlockCommit,
    before_commit: F,
) -> Result<(), StoreError>
where
    F: FnOnce() -> Result<(), StoreError>,
{
    let txn = db.begin_write()?;

    {
        let mut table = txn.open_table(MARKETS)?;
        for (id, bytes) in &commit.market_rows {
            table.insert(*id, bytes.as_slice())?;
        }
    }

    {
        let mut meta_table = txn.open_table(MARKET_META)?;
        let mut status_table = txn.open_table(MARKET_STATUSES)?;
        for (market_id, bytes) in &commit.market_status_rows {
            status_table.insert(*market_id, bytes.as_slice())?;
        }
        for (market_id, bytes) in &commit.market_meta_rows {
            meta_table.insert(*market_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(MARKET_GROUPS)?;
        table.retain(|_, _| false)?;
        for (index, bytes) in &commit.market_group_rows {
            table.insert(*index, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(BLOCK_HEADERS)?;
        table.retain(|height, _| height == commit.height)?;
        table.insert(commit.height, commit.header_bytes.as_slice())?;
    }

    {
        let mut table = txn.open_table(CHAIN_META)?;
        table.insert(KEY_GENESIS_HASH, commit.genesis_hash.as_slice())?;
    }

    if let Some(bytes) = &commit.history_block_bytes {
        let mut table = txn.open_table(BLOCKS_FULL)?;
        table.insert(commit.height, bytes.as_slice())?;
    }

    {
        let mut table = txn.open_table(BLOCK_WITNESSES)?;
        table.retain(|height, _| height == commit.height)?;
        if let Some(bytes) = &commit.witness_bytes {
            table.insert(commit.height, bytes.as_slice())?;
        } else {
            table.remove(commit.height)?;
        }
    }

    {
        // SYB-60: the registry is rewritten in full each block, so clear the
        // three parallel tables first — this makes signing-key REVOCATION
        // durable (a removed key leaves no lingering row) rather than only
        // supporting additions.
        let mut table = txn.open_table(PUBKEY_REGISTRY)?;
        let mut scheme_table = txn.open_table(PUBKEY_AUTH_SCHEMES)?;
        let mut meta_table = txn.open_table(PUBKEY_META)?;
        table.retain(|_, _| false)?;
        scheme_table.retain(|_, _| false)?;
        meta_table.retain(|_, _| false)?;
        for (pubkey, registered) in &commit.pubkey_rows {
            table.insert(pubkey.as_slice(), registered.account_id.0)?;
            scheme_table.insert(
                pubkey.as_slice(),
                account_auth_scheme_to_store(registered.auth_scheme),
            )?;
            let meta = PubkeyMetaRow {
                label: registered.label.clone(),
                scope: key_scope_to_store(registered.scope),
                created_at_ms: registered.created_at_ms,
            };
            meta_table.insert(pubkey.as_slice(), rmp_serde::to_vec(&meta)?.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(CLEARING_PRICES)?;
        for (market_id, bytes) in &commit.clearing_price_rows {
            table.insert(*market_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(MARKET_VOLUMES)?;
        for (market_id, volume) in &commit.market_volume_rows {
            table.insert(*market_id, *volume)?;
        }
    }

    {
        let mut table = txn.open_table(RESTING_ORDERS)?;
        table.insert(
            KEY_RESTING_ORDERS_SNAPSHOT,
            commit.resting_orders_bytes.as_slice(),
        )?;
    }

    {
        let mut table = txn.open_table(FILL_HISTORY)?;
        for (key, bytes) in &commit.fill_history_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(EQUITY_POINTS)?;
        for (key, bytes) in &commit.equity_point_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(HISTORY_EVENTS)?;
        for (key, bytes) in &commit.history_event_rows {
            table.insert(key.as_slice(), bytes.as_slice())?;
        }
    }
    {
        let mut counters = txn.open_table(COUNTERS)?;
        counters.insert(KEY_HISTORY_EVENT_NEXT_SEQ, commit.history_event_next_seq)?;
    }

    {
        let mut table = txn.open_table(PRICE_POINTS)?;
        let mut by_height = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        for row in &commit.price_point_rows {
            table.insert(row.key.as_slice(), row.bytes.as_slice())?;
            by_height.insert(
                price_point_by_height_key(row.point.height, row.market_id).as_slice(),
                0,
            )?;
        }
    }

    if !commit.price_point_rows.is_empty() && !commit.price_candle_resolutions_secs.is_empty() {
        let mut candles = txn.open_table(PRICE_CANDLES)?;
        let mut candles_by_resolution = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        for row in &commit.price_point_rows {
            for &resolution_secs in &commit.price_candle_resolutions_secs {
                if resolution_secs == 0 {
                    continue;
                }
                let mut candle = PriceCandle::from_point(resolution_secs, &row.point);
                let key = price_candle_key(row.market_id, resolution_secs, candle.bucket_start_ms);
                let index_key = price_candle_by_resolution_key(
                    resolution_secs,
                    candle.bucket_start_ms,
                    row.market_id,
                );
                if let Some(existing) = {
                    candles
                        .get(key.as_slice())?
                        .map(|value| rmp_serde::from_slice(value.value()))
                        .transpose()?
                } {
                    candle = existing;
                    candle.merge_point(&row.point);
                }
                let bytes = rmp_serde::to_vec(&candle)?;
                candles.insert(key.as_slice(), bytes.as_slice())?;
                candles_by_resolution.insert(index_key.as_slice(), 0)?;
            }
        }
    }

    {
        let mut table = txn.open_table(DATA_FEEDS)?;
        for (feed_id, bytes) in &commit.data_feed_rows {
            table.insert(*feed_id, bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(RESOLUTION_TEMPLATES)?;
        table.retain(|_, _| false)?;
        for (template_id, bytes) in &commit.resolution_template_rows {
            table.insert(template_id.as_str(), bytes.as_slice())?;
        }
    }

    {
        let mut table = txn.open_table(BRIDGE_STATE)?;
        table.insert(KEY_BRIDGE_STATE, commit.bridge_state_bytes.as_slice())?;
    }

    {
        let mut table = txn.open_table(PENDING_BUNDLES)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(ADMIT_LOG)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(CONTROL_PLANE_LOG)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(PENDING_L1_DEPOSITS)?;
        table.retain(|_, _| false)?;
    }
    {
        let mut table = txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
        table.retain(|_, _| false)?;
    }

    {
        let mut table = txn.open_table(TRADER_TRACKER)?;
        table.insert(
            KEY_TRADER_TRACKER_SNAPSHOT,
            commit.trader_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(PRICE_TRACKER_VOLUME)?;
        table.insert(
            KEY_PRICE_TRACKER_VOLUME_SNAPSHOT,
            commit.price_tracker_volume_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
        table.insert(
            KEY_PRICE_TRACKER_CLEARING_HISTORY_SNAPSHOT,
            commit.price_tracker_clearing_history_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(LIQUIDITY_TRACKER)?;
        table.insert(
            KEY_LIQUIDITY_TRACKER_SNAPSHOT,
            commit.liquidity_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(ORDER_STATS_TRACKER)?;
        table.insert(
            KEY_ORDER_STATS_TRACKER_SNAPSHOT,
            commit.order_stats_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(WELFARE_TRACKER)?;
        table.insert(
            KEY_WELFARE_TRACKER_SNAPSHOT,
            commit.welfare_tracker_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(FIRST_DEPOSIT_MS)?;
        table.insert(
            KEY_FIRST_DEPOSIT_MS_SNAPSHOT,
            commit.first_deposit_ms_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(FILL_TOTAL_COUNTS)?;
        table.insert(
            KEY_FILL_TOTAL_COUNTS_SNAPSHOT,
            commit.fill_total_counts_bytes.as_slice(),
        )?;
    }
    {
        let mut table = txn.open_table(COST_BASIS_TRACKER)?;
        table.insert(
            KEY_COST_BASIS_TRACKER_SNAPSHOT,
            commit.cost_basis_tracker_bytes.as_slice(),
        )?;
    }

    {
        let mut table = txn.open_table(COUNTERS)?;
        write_core_counters(&mut table, commit.counters)?;
    }

    before_commit()?;
    txn.commit()?;
    Ok(())
}

impl Store {
    /// Open (or create) a store at the given path.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut db = Database::create(path)?;
        let qmdb_path = path.with_extension("qmdb");
        std::fs::create_dir_all(&qmdb_path)?;
        let account_state_store =
            Box::new(FencedAccountStorage::open(&qmdb_path)?) as Box<dyn AccountStateStore>;

        // Ensure all tables exist (redb creates on first write, but this
        // makes the schema explicit).
        let txn = db.begin_write()?;
        txn.open_table(MARKETS)?;
        txn.open_table(MARKET_META)?;
        txn.open_table(MARKET_STATUSES)?;
        txn.open_table(MARKET_GROUPS)?;
        txn.open_table(BLOCK_HEADERS)?;
        txn.open_table(BLOCKS_FULL)?;
        txn.open_table(BLOCK_WITNESSES)?;
        txn.open_table(DA_ARTIFACTS)?;
        txn.open_table(PUBKEY_REGISTRY)?;
        txn.open_table(PUBKEY_AUTH_SCHEMES)?;
        txn.open_table(COUNTERS)?;
        txn.open_table(HISTORY_META)?;
        txn.open_table(CHAIN_META)?;
        txn.open_table(CLEARING_PRICES)?;
        txn.open_table(MARKET_VOLUMES)?;
        txn.open_table(RESTING_ORDERS)?;
        txn.open_table(PENDING_BUNDLES)?;
        txn.open_table(ADMIT_LOG)?;
        txn.open_table(CONTROL_PLANE_LOG)?;
        txn.open_table(FILL_HISTORY)?;
        txn.open_table(EQUITY_POINTS)?;
        txn.open_table(HISTORY_EVENTS)?;
        txn.open_table(DATA_FEEDS)?;
        txn.open_table(RESOLUTION_TEMPLATES)?;
        txn.open_table(BRIDGE_STATE)?;
        txn.open_table(PENDING_L1_DEPOSITS)?;
        txn.open_table(PENDING_BRIDGE_WITHDRAWALS)?;
        txn.open_table(TRADER_TRACKER)?;
        txn.open_table(PRICE_TRACKER_VOLUME)?;
        txn.open_table(PRICE_TRACKER_CLEARING_HISTORY)?;
        txn.open_table(PRICE_POINTS)?;
        txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        txn.open_table(PRICE_CANDLES)?;
        txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        txn.open_table(LIQUIDITY_TRACKER)?;
        txn.open_table(ORDER_STATS_TRACKER)?;
        txn.open_table(WELFARE_TRACKER)?;
        txn.open_table(FIRST_DEPOSIT_MS)?;
        txn.open_table(FILL_TOTAL_COUNTS)?;
        txn.open_table(COST_BASIS_TRACKER)?;
        txn.open_table(AUTO_RESOLUTION_RECORDS)?;
        txn.commit()?;

        initialize_or_validate_layout(&db)?;
        backfill_price_history_indexes(&db)?;
        if prune_historical_block_rows(&db)? {
            match db.compact() {
                Ok(true) => info!(?path, "compacted store after pruning historical block rows"),
                Ok(false) => debug!(?path, "store compaction found no reclaimable pages"),
                Err(error) => warn!(?path, %error, "store compaction failed after pruning"),
            }
        }

        let db = Arc::new(db);

        info!(?path, "store opened");
        Ok(Self {
            db,
            account_state_store,
            #[cfg(test)]
            fault_injection: Arc::new(Mutex::new(StoreFaultInjection::default())),
        })
    }
    pub(super) async fn redb_write<R, F>(&self, write: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(Arc<Database>) -> Result<R, StoreError> + Send + 'static,
    {
        let db = Arc::clone(&self.db);
        // Redb begin_write/commit is synchronous and can fsync. The actor
        // awaits this task before making the corresponding state visible or
        // committing a prepared block, so the durable-before-visible and qMDB
        // fence ordering stays identical while the Tokio worker is not blocked.
        tokio::task::spawn_blocking(move || write(db))
            .await
            .map_err(|error| StoreError::BlockingTask(error.to_string()))?
    }
    /// Save the sequencer state after a block. Single ACID transaction.
    pub async fn save_block(&self, snapshot: SequencerSnapshot<'_>) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, None, None).await
    }

    /// Save the sequencer state and its witness after a block.
    ///
    /// The witness is committed in the same redb transaction as the block
    /// metadata, so an asynchronous witgen process can later export a proof job
    /// for the latest committed block.
    pub async fn save_block_with_witness(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: &BlockWitness,
    ) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, Some(witness), None).await
    }

    /// Save sequencer state, witness, and the API replay block payload after
    /// a block. Actor commits use this path so historical reads have the same
    /// durability boundary as recovery state.
    pub async fn save_block_with_witness_and_history(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: &BlockWitness,
        block: &SealedBlock,
    ) -> Result<(), StoreError> {
        self.save_block_inner(snapshot, Some(witness), Some(block))
            .await
    }

    async fn save_block_inner(
        &self,
        snapshot: SequencerSnapshot<'_>,
        witness: Option<&BlockWitness>,
        history_block: Option<&SealedBlock>,
    ) -> Result<(), StoreError> {
        if let Some(witness) = witness {
            validate_witness_header(snapshot.header, witness)?;
        }

        let current_fence = read_account_state_fence(&self.db)?;
        let next_slot = current_fence
            .map(|fence| fence.slot.inactive())
            .unwrap_or(AccountSnapshotSlot::A);

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::BeforeQmdbPersist)?;

        // Persist the inactive qmdb slot first. It becomes committed only when the
        // redb transaction below flips the fence to point at it.
        let state_sidecar = state_sidecar_snapshot_from_resting_orders(
            snapshot.bridge_state,
            &snapshot.resting_orders,
            snapshot.markets,
            snapshot.market_groups,
            snapshot.lifecycle,
        );

        self.account_state_store
            .persist(CommittedAccountState {
                accounts: snapshot.accounts,
                state_sidecar: &state_sidecar,
                height: snapshot.header.height,
                next_account_id: snapshot.accounts.next_id(),
                slot: next_slot,
            })
            .await?;

        if witness.is_some() {
            let state_root = self.account_state_store.qmdb_state_root(next_slot).await?;
            if state_root.root != snapshot.header.state_root {
                metrics::counter!("sybil_store_qmdb_root_mismatch_total", "phase" => "commit")
                    .increment(1);
                return Err(StoreError::CorruptLayout(format!(
                    "typed qMDB root mismatch at height {} before commit: slot {:?} root={:?} header_root={:?}",
                    snapshot.header.height, state_root.slot, state_root.root, snapshot.header.state_root
                )));
            }
            metrics::counter!("sybil_store_commit_root_verified_total").increment(1);
        }

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::AfterQmdbPersistBeforeRedbFence)?;

        let commit = build_redb_block_commit(&snapshot, witness, history_block, next_slot)?;
        #[cfg(test)]
        let fault_injection = self.save_block_faults();
        self.redb_write(move |db| {
            #[cfg(test)]
            {
                write_redb_block_commit(&db, commit, fault_injection)
            }
            #[cfg(not(test))]
            {
                write_redb_block_commit(&db, commit)
            }
        })
        .await?;

        #[cfg(test)]
        self.fail_save_block_at(StoreFaultPoint::AfterRedbFenceCommit)?;

        debug!(height = snapshot.header.height, "block persisted");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("redb: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb database: {0}")]
    Database(#[from] redb::DatabaseError),
    #[error("redb transaction: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("redb table: {0}")]
    Table(#[from] redb::TableError),
    #[error("redb storage: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("redb commit: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("msgpack encode: {0}")]
    MsgpackEncode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode: {0}")]
    MsgpackDecode(#[from] rmp_serde::decode::Error),
    #[error("blocking store task failed: {0}")]
    BlockingTask(String),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("qmdb: {0}")]
    Qmdb(String),
    #[error("block witness header does not match persisted block header")]
    WitnessHeaderMismatch,
    #[error("unsupported store layout: {0}")]
    UnsupportedLayout(String),
    #[error("corrupt store layout: {0}")]
    CorruptLayout(String),
    #[error("witness import refused: {0}")]
    WitnessImport(String),
    #[cfg(test)]
    #[error("injected store fault: {0}")]
    InjectedFault(String),
}
