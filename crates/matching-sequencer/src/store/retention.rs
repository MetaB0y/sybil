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
    pub fill_history_retention_secs: u64,
    pub equity_retention_secs: u64,
    pub account_event_retention_secs: u64,
    pub max_durable_fill_rows: usize,
    pub max_durable_equity_rows: usize,
    pub max_durable_account_event_rows: usize,
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
                || self.fill_history_retention_secs > 0
                || self.equity_retention_secs > 0
                || self.account_event_retention_secs > 0
                || self.max_durable_fill_rows > 0
                || self.max_durable_equity_rows > 0
                || self.max_durable_account_event_rows > 0
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

    fn account_history_cutoffs(
        &self,
        head_timestamp_ms: u64,
    ) -> (Option<u64>, Option<u64>, Option<u64>) {
        let cutoff = |seconds: u64| {
            (seconds > 0).then(|| head_timestamp_ms.saturating_sub(seconds.saturating_mul(1000)))
        };
        (
            cutoff(self.fill_history_retention_secs),
            cutoff(self.equity_retention_secs),
            cutoff(self.account_event_retention_secs),
        )
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
    pub fill_history_min_timestamp_ms: Option<u64>,
    pub fill_history_pruned_through_height: Option<u64>,
    pub equity_points_min_timestamp_ms: Option<u64>,
    pub history_events_min_timestamp_ms: Option<u64>,
    pub price_candles_min_bucket_ms: BTreeMap<u32, u64>,
    pub last_history_prune_height: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryPruneReport {
    pub blocks_full_pruned: usize,
    pub da_artifacts_pruned: usize,
    pub price_points_pruned: usize,
    pub price_candles_pruned: usize,
    pub fill_history_pruned: usize,
    pub equity_points_pruned: usize,
    pub history_events_pruned: usize,
    pub meta: HistoryRetentionMeta,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountHistoryRetention {
    pub fill_pruned_through_height: Option<u64>,
    pub fill_pruned_through_timestamp_ms: Option<u64>,
    pub equity_pruned_through_timestamp_ms: Option<u64>,
    pub events_pruned_through_timestamp_ms: Option<u64>,
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
        fill_history_min_timestamp_ms: table
            .get(KEY_FILL_HISTORY_MIN_TIMESTAMP_MS)?
            .map(|value| value.value()),
        fill_history_pruned_through_height: table
            .get(KEY_FILL_HISTORY_PRUNED_THROUGH_HEIGHT)?
            .map(|value| value.value()),
        equity_points_min_timestamp_ms: table
            .get(KEY_EQUITY_POINTS_MIN_TIMESTAMP_MS)?
            .map(|value| value.value()),
        history_events_min_timestamp_ms: table
            .get(KEY_HISTORY_EVENTS_MIN_TIMESTAMP_MS)?
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
    account_history_cutoffs: (Option<u64>, Option<u64>, Option<u64>),
) -> Result<HistoryPruneReport, StoreError> {
    let txn = db.begin_write()?;
    let mut remaining = policy.prune_max_rows;
    let mut blocks_full_pruned = 0usize;
    let mut da_artifacts_pruned = 0usize;
    let mut price_points_pruned = 0usize;
    let mut price_candles_pruned = 0usize;
    let mut fill_history_pruned = 0usize;
    let mut fill_history_pruned_through_height = None;
    let mut fill_pruned_heights = HashMap::new();
    let mut fill_pruned_timestamps = HashMap::new();
    let mut equity_pruned_timestamps = HashMap::new();
    let mut event_pruned_timestamps = HashMap::new();
    let mut equity_points_pruned = 0usize;
    let mut history_events_pruned = 0usize;

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
            let mut manifests = txn.open_table(DA_MANIFESTS)?;
            let mut iter = table.extract_from_if(0..floor, |_, _| true)?;
            while remaining > 0 {
                let Some((height, _)) = iter.next().transpose()? else {
                    break;
                };
                manifests.remove(height.value())?;
                da_artifacts_pruned += 1;
                remaining -= 1;
            }
        }
    }

    let (fill_cutoff, equity_cutoff, event_cutoff) = account_history_cutoffs;

    if remaining > 0 && (fill_cutoff.is_some() || policy.max_durable_fill_rows > 0) {
        let index = txn.open_table(FILL_HISTORY_BY_TIME)?;
        let excess = if policy.max_durable_fill_rows > 0 {
            index
                .len()?
                .saturating_sub(policy.max_durable_fill_rows as u64)
        } else {
            0
        };
        let mut keys = Vec::new();
        for (ordinal, entry) in index.iter()?.enumerate() {
            if keys.len() >= remaining {
                break;
            }
            let (key, _) = entry?;
            let Some((timestamp_ms, ..)) = fill_history_by_time_parts(key.value()) else {
                warn!("invalid fill history retention index key in store");
                continue;
            };
            if (ordinal as u64) < excess || fill_cutoff.is_some_and(|cutoff| timestamp_ms < cutoff)
            {
                keys.push(key.value().to_vec());
            } else {
                break;
            }
        }
        drop(index);
        let mut primary = txn.open_table(FILL_HISTORY)?;
        let mut index = txn.open_table(FILL_HISTORY_BY_TIME)?;
        for key in keys {
            if let Some((timestamp_ms, account_id, height, order_id)) =
                fill_history_by_time_parts(&key)
            {
                primary
                    .remove(fill_history_primary_key(account_id, height, order_id).as_slice())?;
                fill_history_pruned_through_height = Some(
                    fill_history_pruned_through_height.map_or(height, |old: u64| old.max(height)),
                );
                fill_pruned_heights
                    .entry(account_id.0)
                    .and_modify(|old: &mut u64| *old = (*old).max(height))
                    .or_insert(height);
                fill_pruned_timestamps
                    .entry(account_id.0)
                    .and_modify(|old: &mut u64| *old = (*old).max(timestamp_ms))
                    .or_insert(timestamp_ms);
            }
            index.remove(key.as_slice())?;
            fill_history_pruned += 1;
            remaining -= 1;
        }
    }

    if remaining > 0 && (equity_cutoff.is_some() || policy.max_durable_equity_rows > 0) {
        let index = txn.open_table(EQUITY_POINTS_BY_TIME)?;
        let excess = if policy.max_durable_equity_rows > 0 {
            index
                .len()?
                .saturating_sub(policy.max_durable_equity_rows as u64)
        } else {
            0
        };
        let mut keys = Vec::new();
        for (ordinal, entry) in index.iter()?.enumerate() {
            if keys.len() >= remaining {
                break;
            }
            let (key, _) = entry?;
            let Some((timestamp_ms, ..)) = equity_by_time_parts(key.value()) else {
                continue;
            };
            if (ordinal as u64) < excess
                || equity_cutoff.is_some_and(|cutoff| timestamp_ms < cutoff)
            {
                keys.push(key.value().to_vec());
            } else {
                break;
            }
        }
        drop(index);
        let mut primary = txn.open_table(EQUITY_POINTS)?;
        let mut index = txn.open_table(EQUITY_POINTS_BY_TIME)?;
        for key in keys {
            if let Some((timestamp_ms, account_id, height)) = equity_by_time_parts(&key) {
                primary.remove(equity_key(account_id, height).as_slice())?;
                equity_pruned_timestamps
                    .entry(account_id.0)
                    .and_modify(|old: &mut u64| *old = (*old).max(timestamp_ms))
                    .or_insert(timestamp_ms);
            }
            index.remove(key.as_slice())?;
            equity_points_pruned += 1;
            remaining -= 1;
        }
    }

    if remaining > 0 && (event_cutoff.is_some() || policy.max_durable_account_event_rows > 0) {
        let index = txn.open_table(HISTORY_EVENTS_BY_TIME)?;
        let excess = if policy.max_durable_account_event_rows > 0 {
            index
                .len()?
                .saturating_sub(policy.max_durable_account_event_rows as u64)
        } else {
            0
        };
        let mut keys = Vec::new();
        for (ordinal, entry) in index.iter()?.enumerate() {
            if keys.len() >= remaining {
                break;
            }
            let (key, _) = entry?;
            let Some((timestamp_ms, ..)) = history_event_by_time_parts(key.value()) else {
                continue;
            };
            if (ordinal as u64) < excess || event_cutoff.is_some_and(|cutoff| timestamp_ms < cutoff)
            {
                keys.push(key.value().to_vec());
            } else {
                break;
            }
        }
        drop(index);
        let mut primary = txn.open_table(HISTORY_EVENTS)?;
        let mut index = txn.open_table(HISTORY_EVENTS_BY_TIME)?;
        for key in keys {
            if let Some((timestamp_ms, account_id, height, seq)) = history_event_by_time_parts(&key)
            {
                primary.remove(history_event_key(account_id, height, seq).as_slice())?;
                event_pruned_timestamps
                    .entry(account_id.0)
                    .and_modify(|old: &mut u64| *old = (*old).max(timestamp_ms))
                    .or_insert(timestamp_ms);
            }
            index.remove(key.as_slice())?;
            history_events_pruned += 1;
            remaining -= 1;
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

    let first_timestamp = |table: TableDefinition<&[u8], u64>| -> Result<Option<u64>, StoreError> {
        let table = txn.open_table(table)?;
        Ok(table.iter()?.next().transpose()?.and_then(|(key, _)| {
            key.value()
                .get(..8)?
                .try_into()
                .ok()
                .map(u64::from_be_bytes)
        }))
    };
    let fill_enabled = fill_cutoff.is_some() || policy.max_durable_fill_rows > 0;
    let equity_enabled = equity_cutoff.is_some() || policy.max_durable_equity_rows > 0;
    let event_enabled = event_cutoff.is_some() || policy.max_durable_account_event_rows > 0;
    let fill_history_min_timestamp_ms = if fill_enabled {
        first_timestamp(FILL_HISTORY_BY_TIME)?.or(fill_cutoff)
    } else {
        None
    };
    let equity_points_min_timestamp_ms = if equity_enabled {
        first_timestamp(EQUITY_POINTS_BY_TIME)?.or(equity_cutoff)
    } else {
        None
    };
    let history_events_min_timestamp_ms = if event_enabled {
        first_timestamp(HISTORY_EVENTS_BY_TIME)?.or(event_cutoff)
    } else {
        None
    };

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
        for (enabled, key, value) in [
            (
                fill_enabled,
                KEY_FILL_HISTORY_MIN_TIMESTAMP_MS,
                fill_history_min_timestamp_ms,
            ),
            (
                equity_enabled,
                KEY_EQUITY_POINTS_MIN_TIMESTAMP_MS,
                equity_points_min_timestamp_ms,
            ),
            (
                event_enabled,
                KEY_HISTORY_EVENTS_MIN_TIMESTAMP_MS,
                history_events_min_timestamp_ms,
            ),
        ] {
            if enabled {
                if let Some(value) = value {
                    meta.insert(key, value)?;
                } else {
                    meta.remove(key)?;
                }
            }
        }
        if let Some(height) = fill_history_pruned_through_height {
            let previous = meta
                .get(KEY_FILL_HISTORY_PRUNED_THROUGH_HEIGHT)?
                .map(|value| value.value())
                .unwrap_or(0);
            meta.insert(KEY_FILL_HISTORY_PRUNED_THROUGH_HEIGHT, previous.max(height))?;
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

    for (table, updates) in [
        (FILL_HISTORY_PRUNED_TIMESTAMP, &fill_pruned_timestamps),
        (EQUITY_HISTORY_PRUNED_TIMESTAMP, &equity_pruned_timestamps),
        (ACCOUNT_EVENTS_PRUNED_TIMESTAMP, &event_pruned_timestamps),
    ] {
        let mut highwaters = txn.open_table(table)?;
        for (&account_id, &timestamp_ms) in updates {
            let previous = highwaters
                .get(account_id)?
                .map(|value| value.value())
                .unwrap_or(0);
            highwaters.insert(account_id, previous.max(timestamp_ms))?;
        }
    }
    {
        let mut highwaters = txn.open_table(FILL_HISTORY_PRUNED_HEIGHT)?;
        for (&account_id, &height) in &fill_pruned_heights {
            let previous = highwaters
                .get(account_id)?
                .map(|value| value.value())
                .unwrap_or(0);
            highwaters.insert(account_id, previous.max(height))?;
        }
    }

    txn.commit()?;
    Ok(HistoryPruneReport {
        blocks_full_pruned,
        da_artifacts_pruned,
        price_points_pruned,
        price_candles_pruned,
        fill_history_pruned,
        equity_points_pruned,
        history_events_pruned,
        meta: read_history_retention_meta(db)?,
    })
}

pub(super) fn backfill_history_indexes(db: &Database) -> Result<(), StoreError> {
    let (
        price_points_len,
        price_points_index_len,
        price_candles_len,
        price_candles_index_len,
        account_indexes_current,
    ) = {
        let txn = db.begin_read()?;
        let price_points_len = txn.open_table(PRICE_POINTS)?.len()?;
        let price_points_index_len = txn.open_table(PRICE_POINTS_BY_HEIGHT)?.len()?;
        let price_candles_len = txn.open_table(PRICE_CANDLES)?.len()?;
        let price_candles_index_len = txn.open_table(PRICE_CANDLES_BY_RESOLUTION)?.len()?;
        let account_indexes_current = txn
            .open_table(HISTORY_META)?
            .get(KEY_ACCOUNT_HISTORY_INDEX_VERSION)?
            .is_some_and(|value| value.value() == 1);
        (
            price_points_len,
            price_points_index_len,
            price_candles_len,
            price_candles_index_len,
            account_indexes_current,
        )
    };

    if account_indexes_current
        && price_points_index_len >= price_points_len
        && price_candles_index_len >= price_candles_len
    {
        return Ok(());
    }

    let txn = db.begin_write()?;
    let mut fills_backfilled = 0u64;
    let mut equity_backfilled = 0u64;
    let mut events_backfilled = 0u64;
    let mut price_points_backfilled = 0u64;
    let mut price_candles_backfilled = 0u64;

    if !account_indexes_current {
        {
            let mut index = txn.open_table(FILL_HISTORY_BY_TIME)?;
            index.retain(|_, _| false)?;
            let table = txn.open_table(FILL_HISTORY)?;
            for entry in table.iter()? {
                let (key, value) = entry?;
                let Some((account_id, block_height, order_id)) =
                    fill_history_parts_from_key(key.value())
                else {
                    warn!("invalid fill history key in store; skipping index backfill");
                    continue;
                };
                let record: AccountFillRecord = rmp_serde::from_slice(value.value())?;
                let time_key = fill_history_by_time_key(
                    record.timestamp_ms,
                    account_id,
                    block_height,
                    order_id,
                );
                if index.insert(time_key.as_slice(), 0)?.is_none() {
                    fills_backfilled += 1;
                }
            }
        }
        {
            let mut index = txn.open_table(EQUITY_POINTS_BY_TIME)?;
            index.retain(|_, _| false)?;
            let table = txn.open_table(EQUITY_POINTS)?;
            for entry in table.iter()? {
                let (key, value) = entry?;
                let Some((account_id, height)) = equity_parts_from_key(key.value()) else {
                    warn!("invalid equity key in store; skipping index backfill");
                    continue;
                };
                let point: crate::aggregates::EquityPoint = rmp_serde::from_slice(value.value())?;
                let time_key = equity_by_time_key(point.timestamp_ms, account_id, height);
                if index.insert(time_key.as_slice(), 0)?.is_none() {
                    equity_backfilled += 1;
                }
            }
        }
        {
            let mut index = txn.open_table(HISTORY_EVENTS_BY_TIME)?;
            index.retain(|_, _| false)?;
            let table = txn.open_table(HISTORY_EVENTS)?;
            for entry in table.iter()? {
                let (key, value) = entry?;
                let Some((account_id, block_height, seq)) =
                    history_event_parts_from_key(key.value())
                else {
                    warn!("invalid account event key in store; skipping index backfill");
                    continue;
                };
                let event: crate::aggregates::StoredHistoryEvent =
                    rmp_serde::from_slice(value.value())?;
                let time_key =
                    history_event_by_time_key(event.timestamp_ms, account_id, block_height, seq);
                if index.insert(time_key.as_slice(), 0)?.is_none() {
                    events_backfilled += 1;
                }
            }
        }
        txn.open_table(HISTORY_META)?
            .insert(KEY_ACCOUNT_HISTORY_INDEX_VERSION, 1)?;
    }

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
    if fills_backfilled > 0
        || equity_backfilled > 0
        || events_backfilled > 0
        || price_points_backfilled > 0
        || price_candles_backfilled > 0
    {
        info!(
            fills_backfilled,
            equity_backfilled,
            events_backfilled,
            price_points_backfilled,
            price_candles_backfilled,
            "backfilled durable history retention indexes"
        );
    }
    Ok(())
}

impl Store {
    pub fn history_retention_meta(&self) -> Result<HistoryRetentionMeta, StoreError> {
        read_history_retention_meta(&self.db)
    }

    pub fn account_history_retention(
        &self,
        account_id: AccountId,
    ) -> Result<AccountHistoryRetention, StoreError> {
        let txn = self.db.begin_read()?;
        Ok(AccountHistoryRetention {
            fill_pruned_through_height: txn
                .open_table(FILL_HISTORY_PRUNED_HEIGHT)?
                .get(account_id.0)?
                .map(|value| value.value()),
            fill_pruned_through_timestamp_ms: txn
                .open_table(FILL_HISTORY_PRUNED_TIMESTAMP)?
                .get(account_id.0)?
                .map(|value| value.value()),
            equity_pruned_through_timestamp_ms: txn
                .open_table(EQUITY_HISTORY_PRUNED_TIMESTAMP)?
                .get(account_id.0)?
                .map(|value| value.value()),
            events_pruned_through_timestamp_ms: txn
                .open_table(ACCOUNT_EVENTS_PRUNED_TIMESTAMP)?
                .get(account_id.0)?
                .map(|value| value.value()),
        })
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
        let account_history_cutoffs = policy.account_history_cutoffs(head_timestamp_ms);
        if policy.prune_max_rows == 0
            || (block_floor.is_none()
                && price_floor.is_none()
                && price_candle_cutoffs.is_empty()
                && account_history_cutoffs.0.is_none()
                && account_history_cutoffs.1.is_none()
                && account_history_cutoffs.2.is_none()
                && policy.max_durable_fill_rows == 0
                && policy.max_durable_equity_rows == 0
                && policy.max_durable_account_event_rows == 0)
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
                account_history_cutoffs,
            )
        })
        .await
    }
}
