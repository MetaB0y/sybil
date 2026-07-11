use super::*;

pub(super) fn prune_historical_block_rows(db: &Database) -> Result<bool, StoreError> {
    let txn = db.begin_write()?;
    let Some(height) = ({
        let counters = txn.open_table(COUNTERS)?;

        counters.get(KEY_HEIGHT)?.map(|value| value.value())
    }) else {
        txn.commit()?;
        return Ok(false);
    };

    let mut pruned = false;
    {
        let mut headers = txn.open_table(BLOCK_HEADERS)?;
        headers.retain(|key, _| {
            let keep = key == height;
            pruned |= !keep;
            keep
        })?;
    }
    {
        let mut witnesses = txn.open_table(BLOCK_WITNESSES)?;
        witnesses.retain(|key, _| {
            let keep = key == height;
            pruned |= !keep;
            keep
        })?;
    }
    txn.commit()?;
    if pruned {
        info!(height, "pruned historical block rows from store");
    }
    Ok(pruned)
}

/// Retention settings for durable history tables.
///
/// A value of 0 disables pruning for that stream. `prune_max_rows` bounds the
/// memory and write work of one maintenance pass; when it is exhausted,
/// metadata remains at the oldest row still present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryRetentionPolicy {
    pub block_history_retention_blocks: u64,
    pub raw_price_retention_blocks: u64,
    pub price_candle_resolutions_secs: Vec<u32>,
    pub price_candle_retention_secs: Vec<u64>,
    pub prune_interval_blocks: u64,
    pub prune_max_rows: usize,
}

impl HistoryRetentionPolicy {
    pub fn should_prune_at(&self, height: u64) -> bool {
        let prunes_price_candles = self
            .price_candle_resolutions_secs
            .iter()
            .zip(&self.price_candle_retention_secs)
            .any(|(&resolution_secs, &retention_secs)| resolution_secs > 0 && retention_secs > 0);
        height > 0
            && self.prune_interval_blocks > 0
            && self.prune_max_rows > 0
            && (self.block_history_retention_blocks > 0
                || self.raw_price_retention_blocks > 0
                || prunes_price_candles)
            && height.is_multiple_of(self.prune_interval_blocks)
    }

    fn blocks_full_floor(&self, head_height: u64) -> Option<u64> {
        retention_floor(head_height, self.block_history_retention_blocks)
    }

    fn price_points_floor(&self, head_height: u64) -> Option<u64> {
        retention_floor(head_height, self.raw_price_retention_blocks)
    }

    fn price_candle_cutoffs(&self, head_timestamp_ms: u64) -> BTreeMap<u32, u64> {
        self.price_candle_resolutions_secs
            .iter()
            .zip(&self.price_candle_retention_secs)
            .filter_map(|(&resolution_secs, &retention_secs)| {
                if resolution_secs == 0 || retention_secs == 0 {
                    return None;
                }
                let retention_ms = retention_secs.saturating_mul(1000);
                Some((
                    resolution_secs,
                    head_timestamp_ms.saturating_sub(retention_ms),
                ))
            })
            .collect()
    }
}

fn retention_floor(head_height: u64, retention_blocks: u64) -> Option<u64> {
    if head_height == 0 || retention_blocks == 0 {
        return None;
    }
    Some(
        head_height
            .saturating_sub(retention_blocks.saturating_sub(1))
            .max(1),
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryRetentionMeta {
    pub blocks_full_min_height: Option<u64>,
    pub price_points_min_height: Option<u64>,
    pub price_candles_min_bucket_ms: BTreeMap<u32, u64>,
    pub last_history_prune_height: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryPruneReport {
    pub blocks_full_pruned: usize,
    pub da_artifacts_pruned: usize,
    pub price_points_pruned: usize,
    pub price_candles_pruned: usize,
    pub meta: HistoryRetentionMeta,
}

pub(super) fn read_history_retention_meta(
    db: &Database,
) -> Result<HistoryRetentionMeta, StoreError> {
    let txn = db.begin_read()?;
    let table = txn.open_table(HISTORY_META)?;
    let mut price_candles_min_bucket_ms = BTreeMap::new();
    for entry in table.iter()? {
        let (key, value) = entry?;
        if let Some(resolution_secs) = parse_price_candles_min_bucket_key(key.value()) {
            price_candles_min_bucket_ms.insert(resolution_secs, value.value());
        }
    }
    Ok(HistoryRetentionMeta {
        blocks_full_min_height: table
            .get(KEY_BLOCKS_FULL_MIN_HEIGHT)?
            .map(|value| value.value()),
        price_points_min_height: table
            .get(KEY_PRICE_POINTS_MIN_HEIGHT)?
            .map(|value| value.value()),
        price_candles_min_bucket_ms,
        last_history_prune_height: table
            .get(KEY_LAST_HISTORY_PRUNE_HEIGHT)?
            .map(|value| value.value()),
    })
}

pub(super) fn prune_history_redb(
    db: &Database,
    head_height: u64,
    policy: HistoryRetentionPolicy,
    block_floor: Option<u64>,
    price_floor: Option<u64>,
    price_candle_cutoffs: BTreeMap<u32, u64>,
) -> Result<HistoryPruneReport, StoreError> {
    let txn = db.begin_write()?;
    let mut remaining = policy.prune_max_rows;
    let mut blocks_full_pruned = 0usize;
    let mut da_artifacts_pruned = 0usize;
    let mut price_points_pruned = 0usize;
    let mut price_candles_pruned = 0usize;

    if let Some(floor) = block_floor {
        if remaining > 0 {
            let mut table = txn.open_table(BLOCKS_FULL)?;
            let mut iter = table.extract_from_if(0..floor, |_, _| true)?;
            while remaining > 0 {
                let Some(_) = iter.next().transpose()? else {
                    break;
                };
                blocks_full_pruned += 1;
                remaining -= 1;
            }
        }
        if remaining > 0 {
            let mut table = txn.open_table(DA_ARTIFACTS)?;
            let mut iter = table.extract_from_if(0..floor, |_, _| true)?;
            while remaining > 0 {
                let Some(_) = iter.next().transpose()? else {
                    break;
                };
                da_artifacts_pruned += 1;
                remaining -= 1;
            }
        }
    }

    if remaining > 0
        && let Some(floor) = price_floor
    {
        let lo = price_point_by_height_key(0, MarketId(0));
        let hi = price_point_by_height_key(floor, MarketId(0));
        let mut points = txn.open_table(PRICE_POINTS)?;
        let mut by_height = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
        let mut iter = by_height.extract_from_if(lo.as_slice()..hi.as_slice(), |_, _| true)?;
        while remaining > 0 {
            let Some((key, _)) = iter.next().transpose()? else {
                break;
            };
            if let Some((height, market_id)) = price_point_by_height_parts_from_key(key.value()) {
                if points
                    .remove(price_point_key(market_id, height).as_slice())?
                    .is_some()
                {
                    price_points_pruned += 1;
                }
            } else {
                warn!("invalid price point retention index key in store");
            }
            remaining -= 1;
        }
    }

    if remaining > 0 {
        for (&resolution_secs, &cutoff_ms) in &price_candle_cutoffs {
            if remaining == 0 {
                break;
            }
            if cutoff_ms == 0 {
                continue;
            }
            let lo = price_candle_by_resolution_key(resolution_secs, 0, MarketId(0));
            let hi = price_candle_by_resolution_key(resolution_secs, cutoff_ms, MarketId(0));
            let mut candles = txn.open_table(PRICE_CANDLES)?;
            let mut by_resolution = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
            let mut iter =
                by_resolution.extract_from_if(lo.as_slice()..hi.as_slice(), |_, _| true)?;
            while remaining > 0 {
                let Some((key, _)) = iter.next().transpose()? else {
                    break;
                };
                if let Some((indexed_resolution, bucket_start_ms, market_id)) =
                    price_candle_by_resolution_parts_from_key(key.value())
                {
                    if candles
                        .remove(
                            price_candle_key(market_id, indexed_resolution, bucket_start_ms)
                                .as_slice(),
                        )?
                        .is_some()
                    {
                        price_candles_pruned += 1;
                    }
                } else {
                    warn!("invalid price candle retention index key in store");
                }
                remaining -= 1;
            }
        }
    }

    let blocks_full_min_height = if block_floor.is_some() {
        let table = txn.open_table(BLOCKS_FULL)?;

        table
            .iter()?
            .next()
            .transpose()?
            .map(|(key, _)| key.value())
    } else {
        None
    };
    let price_points_min_height =
        if price_floor.is_some() {
            let table = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;

            table.iter()?.next().transpose()?.and_then(|(key, _)| {
                price_point_by_height_parts_from_key(key.value()).map(|(h, _)| h)
            })
        } else {
            None
        };
    let mut price_candles_min_bucket_ms = BTreeMap::new();
    if !price_candle_cutoffs.is_empty() {
        let table = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
        for &resolution_secs in price_candle_cutoffs.keys() {
            let (lo, hi) = price_candle_resolution_bounds(resolution_secs);
            if let Some((key, _)) = table
                .range(lo.as_slice()..=hi.as_slice())?
                .next()
                .transpose()?
                && let Some((_, bucket_start_ms, _)) =
                    price_candle_by_resolution_parts_from_key(key.value())
            {
                price_candles_min_bucket_ms.insert(resolution_secs, bucket_start_ms);
            }
        }
    }

    {
        let mut meta = txn.open_table(HISTORY_META)?;
        if block_floor.is_some() {
            match blocks_full_min_height {
                Some(height) => {
                    meta.insert(KEY_BLOCKS_FULL_MIN_HEIGHT, height)?;
                }
                None => {
                    meta.remove(KEY_BLOCKS_FULL_MIN_HEIGHT)?;
                }
            }
        }
        if price_floor.is_some() {
            match price_points_min_height {
                Some(height) => {
                    meta.insert(KEY_PRICE_POINTS_MIN_HEIGHT, height)?;
                }
                None => {
                    meta.remove(KEY_PRICE_POINTS_MIN_HEIGHT)?;
                }
            }
        }
        for &resolution_secs in price_candle_cutoffs.keys() {
            let key = price_candles_min_bucket_key(resolution_secs);
            match price_candles_min_bucket_ms.get(&resolution_secs) {
                Some(bucket_start_ms) => {
                    meta.insert(key.as_str(), *bucket_start_ms)?;
                }
                None => {
                    meta.remove(key.as_str())?;
                }
            }
        }
        meta.insert(KEY_LAST_HISTORY_PRUNE_HEIGHT, head_height)?;
    }

    txn.commit()?;
    Ok(HistoryPruneReport {
        blocks_full_pruned,
        da_artifacts_pruned,
        price_points_pruned,
        price_candles_pruned,
        meta: read_history_retention_meta(db)?,
    })
}

pub(super) fn backfill_price_history_indexes(db: &Database) -> Result<(), StoreError> {
    let (price_points_len, price_points_index_len, price_candles_len, price_candles_index_len) = {
        let txn = db.begin_read()?;
        let price_points_len = txn.open_table(PRICE_POINTS)?.len()?;
        let price_points_index_len = txn.open_table(PRICE_POINTS_BY_HEIGHT)?.len()?;
        let price_candles_len = txn.open_table(PRICE_CANDLES)?.len()?;
        let price_candles_index_len = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?.len()?;
        (
            price_points_len,
            price_points_index_len,
            price_candles_len,
            price_candles_index_len,
        )
    };

    if price_points_index_len >= price_points_len && price_candles_index_len >= price_candles_len {
        return Ok(());
    }

    let txn = db.begin_write()?;
    let mut price_points_backfilled = 0u64;
    let mut price_candles_backfilled = 0u64;

    if price_points_index_len < price_points_len {
        let mut rows = Vec::new();
        {
            let table = txn.open_table(PRICE_POINTS)?;
            for entry in table.iter()? {
                let (key, _) = entry?;
                let Some((market_id, height)) = price_point_parts_from_key(key.value()) else {
                    warn!("invalid price point key in store; skipping index backfill");
                    continue;
                };
                rows.push(price_point_by_height_key(height, market_id));
            }
        }
        {
            let mut index = txn.open_table(PRICE_POINTS_BY_HEIGHT)?;
            for key in rows {
                if index.insert(key.as_slice(), 0)?.is_none() {
                    price_points_backfilled += 1;
                }
            }
        }
    }

    if price_candles_index_len < price_candles_len {
        let mut rows = Vec::new();
        {
            let table = txn.open_table(PRICE_CANDLES)?;
            for entry in table.iter()? {
                let (key, _) = entry?;
                let Some((market_id, resolution_secs, bucket_start_ms)) =
                    price_candle_parts_from_key(key.value())
                else {
                    warn!("invalid price candle key in store; skipping index backfill");
                    continue;
                };
                rows.push(price_candle_by_resolution_key(
                    resolution_secs,
                    bucket_start_ms,
                    market_id,
                ));
            }
        }
        {
            let mut index = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?;
            for key in rows {
                if index.insert(key.as_slice(), 0)?.is_none() {
                    price_candles_backfilled += 1;
                }
            }
        }
    }

    txn.commit()?;
    if price_points_backfilled > 0 || price_candles_backfilled > 0 {
        info!(
            price_points_backfilled,
            price_candles_backfilled, "backfilled price history retention indexes"
        );
    }
    Ok(())
}

impl Store {
    pub fn history_retention_meta(&self) -> Result<HistoryRetentionMeta, StoreError> {
        read_history_retention_meta(&self.db)
    }

    /// Delete old durable history rows under a bounded row budget.
    ///
    /// This is deliberately separate from block commit. If the budget is too
    /// small to reach the target floor, rows remain and the metadata floor
    /// stays at the oldest row still present.
    pub async fn prune_history(
        &self,
        head_height: u64,
        head_timestamp_ms: u64,
        policy: HistoryRetentionPolicy,
    ) -> Result<HistoryPruneReport, StoreError> {
        let block_floor = policy.blocks_full_floor(head_height);
        let price_floor = policy.price_points_floor(head_height);
        let price_candle_cutoffs = policy.price_candle_cutoffs(head_timestamp_ms);
        if policy.prune_max_rows == 0
            || (block_floor.is_none() && price_floor.is_none() && price_candle_cutoffs.is_empty())
        {
            return Ok(HistoryPruneReport {
                meta: self.history_retention_meta()?,
                ..HistoryPruneReport::default()
            });
        }

        self.redb_write(move |db| {
            prune_history_redb(
                &db,
                head_height,
                policy,
                block_floor,
                price_floor,
                price_candle_cutoffs,
            )
        })
        .await
    }
}
