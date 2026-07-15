use std::collections::VecDeque;
use std::ops::Bound::{Excluded, Included};
use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadTransaction, ReadableDatabase, ReadableTable, TableDefinition};
use sybil_history_types::{
    AccountEquityFact, AccountEventFact, AccountEventQuery, AccountFillFact, ApplyBatchOutcome,
    ApplyBatchResponse, CommittedHistoryBatchV1, EquityBaselines, EquityBaselinesQuery,
    EquityQuery, FillQuery, HistoryPage, MarketPriceFact, PriceCandle, PriceCandlePage,
    PriceCandleQuery, PriceHistoryPage, PriceHistoryQuery, ProjectionStatus,
};

const RAW_BATCHES: TableDefinition<u64, &[u8]> = TableDefinition::new("raw_batches_v1");
const FILLS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("account_fills_v1");
const EQUITY: TableDefinition<&[u8], &[u8]> = TableDefinition::new("account_equity_v1");
const EVENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("account_events_v1");
const PRICES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("market_prices_v1");
const PRICES_BY_TIME: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("market_prices_by_time_v1");
const CANDLES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("price_candles_v1");
const META_U64: TableDefinition<&str, u64> = TableDefinition::new("history_meta_u64_v1");
const META_BYTES: TableDefinition<&str, &[u8]> = TableDefinition::new("history_meta_bytes_v1");

const META_GENESIS_HASH: &str = "genesis_hash";
const META_FIRST_HEIGHT: &str = "first_height";
const META_FIRST_TIMESTAMP_MS: &str = "first_timestamp_ms";
const META_INDEXED_THROUGH_HEIGHT: &str = "indexed_through_height";
const META_INDEXED_THROUGH_TIMESTAMP_MS: &str = "indexed_through_timestamp_ms";
const META_LAST_BLOCK_HASH: &str = "last_block_hash";
const META_CANDLE_RESOLUTIONS: &str = "candle_resolutions_secs_v1";
const MAX_QUERY_POINTS: usize = 5_000;
const MAX_PAGE_SIZE: usize = 1_000;
const MAX_BASELINE_ACCOUNTS: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
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
    Encode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("invalid committed history batch: {0}")]
    InvalidBatch(String),
    #[error("history genesis mismatch")]
    GenesisMismatch,
    #[error("history gap: expected height {expected}, received {received}")]
    Gap { expected: u64, received: u64 },
    #[error("history parent hash mismatch at height {height}")]
    ParentHashMismatch { height: u64 },
    #[error("conflicting history batch at height {height}")]
    ConflictingBatch { height: u64 },
    #[error("conflicting projected {kind} row")]
    ConflictingProjection { kind: &'static str },
    #[error("invalid history query: {0}")]
    InvalidQuery(String),
    #[error("invalid history configuration: {0}")]
    Configuration(String),
    #[error("blocking history task failed: {0}")]
    BlockingTask(String),
}

#[derive(Clone)]
pub struct HistoryStore {
    db: Arc<Database>,
    candle_resolutions_secs: Arc<Vec<u32>>,
}

impl HistoryStore {
    pub fn open(
        path: impl AsRef<Path>,
        candle_resolutions_secs: Vec<u32>,
    ) -> Result<Self, HistoryError> {
        let mut resolutions: Vec<u32> = candle_resolutions_secs
            .into_iter()
            .filter(|resolution| *resolution > 0)
            .collect();
        resolutions.sort_unstable();
        resolutions.dedup();
        let db = Database::create(path)?;
        let txn = db.begin_write()?;
        txn.open_table(RAW_BATCHES)?;
        txn.open_table(FILLS)?;
        txn.open_table(EQUITY)?;
        txn.open_table(EVENTS)?;
        txn.open_table(PRICES)?;
        txn.open_table(PRICES_BY_TIME)?;
        txn.open_table(CANDLES)?;
        txn.open_table(META_U64)?;
        {
            let mut meta = txn.open_table(META_BYTES)?;
            if let Some(stored) = meta.get(META_CANDLE_RESOLUTIONS)? {
                let stored: Vec<u32> = rmp_serde::from_slice(stored.value())?;
                if stored != resolutions {
                    return Err(HistoryError::Configuration(format!(
                        "candle resolutions are persisted as {stored:?}, configured {resolutions:?}"
                    )));
                }
            } else {
                let encoded = rmp_serde::to_vec(&resolutions)?;
                meta.insert(META_CANDLE_RESOLUTIONS, encoded.as_slice())?;
            }
        }
        // Additive metadata introduced after the initial projector release is
        // recoverable from the immutable raw checkpoint batch. Backfill it on
        // open so an upgraded, otherwise caught-up projector does not report a
        // false completeness gap until the next block arrives.
        let timestamp_backfill_height = {
            let meta = txn.open_table(META_U64)?;
            let indexed_through_height = meta
                .get(META_INDEXED_THROUGH_HEIGHT)?
                .map(|value| value.value());
            let has_timestamp = meta.get(META_INDEXED_THROUGH_TIMESTAMP_MS)?.is_some();
            indexed_through_height.filter(|_| !has_timestamp)
        };
        if let Some(height) = timestamp_backfill_height {
            let committed_at_ms = {
                let batches = txn.open_table(RAW_BATCHES)?;
                let raw = batches.get(height)?.ok_or_else(|| {
                    HistoryError::InvalidBatch(format!(
                        "checkpoint height {height} has no immutable raw batch"
                    ))
                })?;
                let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(raw.value())?;
                if batch.height != height {
                    return Err(HistoryError::InvalidBatch(format!(
                        "checkpoint key {height} contains batch height {}",
                        batch.height
                    )));
                }
                batch.committed_at_ms
            };
            txn.open_table(META_U64)?
                .insert(META_INDEXED_THROUGH_TIMESTAMP_MS, committed_at_ms)?;
        }
        txn.commit()?;
        Ok(Self {
            db: Arc::new(db),
            candle_resolutions_secs: Arc::new(resolutions),
        })
    }

    pub fn status(&self) -> Result<ProjectionStatus, HistoryError> {
        let txn = self.db.begin_read()?;
        projection_status(&txn)
    }

    pub fn raw_batch(&self, height: u64) -> Result<Option<CommittedHistoryBatchV1>, HistoryError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RAW_BATCHES)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(HistoryError::from)
    }

    pub fn apply_batch(
        &self,
        batch: CommittedHistoryBatchV1,
    ) -> Result<ApplyBatchResponse, HistoryError> {
        batch
            .validate()
            .map_err(|error| HistoryError::InvalidBatch(error.to_string()))?;
        let txn = self.db.begin_write()?;

        let existing_genesis = {
            let meta = txn.open_table(META_BYTES)?;
            meta.get(META_GENESIS_HASH)?
                .map(|value| value.value().to_vec())
        };
        if existing_genesis
            .as_deref()
            .is_some_and(|genesis| genesis != batch.genesis_hash)
        {
            return Err(HistoryError::GenesisMismatch);
        }

        let indexed_through = {
            let meta = txn.open_table(META_U64)?;
            meta.get(META_INDEXED_THROUGH_HEIGHT)?
                .map(|value| value.value())
        };
        if indexed_through.is_some_and(|height| batch.height <= height) {
            let batches = txn.open_table(RAW_BATCHES)?;
            let Some(stored) = batches.get(batch.height)? else {
                return Err(HistoryError::ConflictingBatch {
                    height: batch.height,
                });
            };
            let stored: CommittedHistoryBatchV1 = rmp_serde::from_slice(stored.value())?;
            if stored.payload_hash != batch.payload_hash {
                return Err(HistoryError::ConflictingBatch {
                    height: batch.height,
                });
            }
            return Ok(ApplyBatchResponse {
                outcome: ApplyBatchOutcome::AlreadyApplied,
                indexed_through_height: indexed_through.unwrap_or(batch.height),
            });
        }

        if let Some(indexed_through) = indexed_through {
            let expected = indexed_through.saturating_add(1);
            if batch.height != expected {
                return Err(HistoryError::Gap {
                    expected,
                    received: batch.height,
                });
            }
            let meta = txn.open_table(META_BYTES)?;
            let last_hash =
                meta.get(META_LAST_BLOCK_HASH)?
                    .ok_or(HistoryError::ParentHashMismatch {
                        height: batch.height,
                    })?;
            if last_hash.value() != batch.parent_hash {
                return Err(HistoryError::ParentHashMismatch {
                    height: batch.height,
                });
            }
        }

        {
            let mut fills = txn.open_table(FILLS)?;
            for fact in &batch.fills {
                insert_projection(&mut fills, fill_key(fact).as_slice(), fact, "fill")?;
            }
        }
        {
            let mut equity = txn.open_table(EQUITY)?;
            for fact in &batch.equity {
                insert_projection(
                    &mut equity,
                    equity_key(fact.account_id, fact.timestamp_ms, fact.height).as_slice(),
                    fact,
                    "equity",
                )?;
            }
        }
        {
            let mut events = txn.open_table(EVENTS)?;
            for fact in &batch.events {
                insert_projection(
                    &mut events,
                    event_key(fact.account_id, fact.block_height, fact.seq).as_slice(),
                    fact,
                    "event",
                )?;
            }
        }
        {
            let mut prices = txn.open_table(PRICES)?;
            let mut prices_by_time = txn.open_table(PRICES_BY_TIME)?;
            let mut candles = txn.open_table(CANDLES)?;
            for fact in &batch.prices {
                insert_projection(
                    &mut prices,
                    price_key(fact.market_id, fact.height).as_slice(),
                    fact,
                    "price",
                )?;
                insert_projection(
                    &mut prices_by_time,
                    price_time_key(fact.market_id, fact.timestamp_ms, fact.height).as_slice(),
                    fact,
                    "price-time index",
                )?;
                for &resolution_secs in self.candle_resolutions_secs.iter() {
                    merge_candle(&mut candles, resolution_secs, *fact)?;
                }
            }
        }

        {
            let mut batches = txn.open_table(RAW_BATCHES)?;
            let bytes = rmp_serde::to_vec(&batch)?;
            batches.insert(batch.height, bytes.as_slice())?;
        }
        {
            let mut meta = txn.open_table(META_BYTES)?;
            if existing_genesis.is_none() {
                meta.insert(META_GENESIS_HASH, batch.genesis_hash.as_slice())?;
            }
            meta.insert(META_LAST_BLOCK_HASH, batch.block_hash.as_slice())?;
        }
        {
            let mut meta = txn.open_table(META_U64)?;
            if indexed_through.is_none() {
                meta.insert(META_FIRST_HEIGHT, batch.height)?;
                meta.insert(META_FIRST_TIMESTAMP_MS, batch.committed_at_ms)?;
            }
            meta.insert(META_INDEXED_THROUGH_HEIGHT, batch.height)?;
            meta.insert(META_INDEXED_THROUGH_TIMESTAMP_MS, batch.committed_at_ms)?;
        }
        txn.commit()?;
        Ok(ApplyBatchResponse {
            outcome: ApplyBatchOutcome::Applied,
            indexed_through_height: batch.height,
        })
    }

    pub fn fills(&self, query: FillQuery) -> Result<HistoryPage<AccountFillFact>, HistoryError> {
        let limit = query.limit.min(MAX_PAGE_SIZE);
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        if limit == 0 {
            return Ok(empty_page(status));
        }
        let table = txn.open_table(FILLS)?;
        let (lo, hi) = account_bounds(query.account_id);
        let mut items = Vec::new();
        if let Some(after) = query.after {
            let cursor = event_key(query.account_id, after.block_height, after.order_id);
            for entry in
                table.range::<&[u8]>((Excluded(cursor.as_slice()), Included(hi.as_slice())))?
            {
                let (_, value) = entry?;
                let fact: AccountFillFact = rmp_serde::from_slice(value.value())?;
                if !fill_matches_market(&fact, query.market_id) {
                    continue;
                }
                items.push(fact);
                if items.len() >= limit {
                    break;
                }
            }
        } else {
            let mut skipped = 0usize;
            for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
                let (_, value) = entry?;
                let fact: AccountFillFact = rmp_serde::from_slice(value.value())?;
                if !fill_matches_market(&fact, query.market_id) {
                    continue;
                }
                if skipped < query.offset {
                    skipped += 1;
                    continue;
                }
                items.push(fact);
                if items.len() >= limit {
                    break;
                }
            }
        }
        let source_points = items.len();
        Ok(HistoryPage {
            items,
            status,
            source_points,
            downsampled: false,
        })
    }

    pub fn events(
        &self,
        query: AccountEventQuery,
    ) -> Result<HistoryPage<AccountEventFact>, HistoryError> {
        let limit = query.limit.min(MAX_PAGE_SIZE);
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        if limit == 0 {
            return Ok(empty_page(status));
        }
        let table = txn.open_table(EVENTS)?;
        let (lo, hi) = account_bounds(query.account_id);
        let before_key = query
            .before
            .map(|(height, sequence)| event_key(query.account_id, height, sequence));
        let mut items = Vec::new();
        let upper = before_key
            .as_ref()
            .map_or(Included(hi.as_slice()), |key| Excluded(key.as_slice()));
        for entry in table
            .range::<&[u8]>((Included(lo.as_slice()), upper))?
            .rev()
        {
            let (_, value) = entry?;
            let fact: AccountEventFact = rmp_serde::from_slice(value.value())?;
            if query
                .category
                .as_deref()
                .is_some_and(|category| fact.kind.category() != category)
            {
                continue;
            }
            items.push(fact);
            if items.len() >= limit {
                break;
            }
        }
        let source_points = items.len();
        Ok(HistoryPage {
            items,
            status,
            source_points,
            downsampled: false,
        })
    }

    pub fn equity(
        &self,
        query: EquityQuery,
    ) -> Result<HistoryPage<AccountEquityFact>, HistoryError> {
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        let table = txn.open_table(EQUITY)?;
        let (lo, hi) = equity_account_bounds(query.account_id);
        let mut items = Vec::new();
        let mut source_points = 0usize;
        if query.since_ms > 0 {
            let anchor_hi =
                equity_key(query.account_id, query.since_ms.saturating_sub(1), u64::MAX);
            if let Some(entry) = table
                .range::<&[u8]>(lo.as_slice()..=anchor_hi.as_slice())?
                .next_back()
            {
                let (_, value) = entry?;
                items.push(rmp_serde::from_slice(value.value())?);
                source_points += 1;
            }
        }
        let series_lo = equity_key(query.account_id, query.since_ms, 0);
        for entry in table.range::<&[u8]>(series_lo.as_slice()..=hi.as_slice())? {
            let (_, value) = entry?;
            let fact: AccountEquityFact = rmp_serde::from_slice(value.value())?;
            items.push(fact);
            source_points += 1;
            if items.len() > MAX_QUERY_POINTS * 2 {
                let latest = items.pop().expect("equity page is non-empty");
                items = items.into_iter().step_by(2).collect();
                items.push(latest);
            }
        }
        if items.len() > MAX_QUERY_POINTS {
            let last = items.len() - 1;
            items = (0..MAX_QUERY_POINTS)
                .map(|index| items[index * last / (MAX_QUERY_POINTS - 1)])
                .collect();
        }
        Ok(HistoryPage {
            downsampled: source_points > items.len(),
            items,
            status,
            source_points,
        })
    }

    pub fn equity_baselines(
        &self,
        query: EquityBaselinesQuery,
    ) -> Result<EquityBaselines, HistoryError> {
        if query.account_ids.len() > MAX_BASELINE_ACCOUNTS {
            return Err(HistoryError::InvalidQuery(format!(
                "at most {MAX_BASELINE_ACCOUNTS} baseline accounts are allowed"
            )));
        }
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        let table = txn.open_table(EQUITY)?;
        let mut account_ids = query.account_ids;
        account_ids.sort_unstable();
        account_ids.dedup();
        let mut baselines = Vec::with_capacity(account_ids.len());
        for account_id in account_ids {
            let lo = equity_key(account_id, 0, 0);
            let hi = equity_key(account_id, query.at_or_before_ms, u64::MAX);
            let Some(entry) = table
                .range::<&[u8]>(lo.as_slice()..=hi.as_slice())?
                .next_back()
            else {
                continue;
            };
            let (_, value) = entry?;
            baselines.push(rmp_serde::from_slice(value.value())?);
        }
        Ok(EquityBaselines { baselines, status })
    }

    pub fn prices(&self, query: PriceHistoryQuery) -> Result<PriceHistoryPage, HistoryError> {
        let limit = query.limit.min(MAX_PAGE_SIZE);
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        if limit == 0 {
            return Ok(PriceHistoryPage {
                points: vec![],
                next_before_height: None,
                status,
            });
        }
        let mut points = VecDeque::new();
        if query.from_ms.is_some() || query.to_ms.is_some() {
            let table = txn.open_table(PRICES_BY_TIME)?;
            let lo = price_time_key(query.market_id, query.from_ms.unwrap_or(0), 0);
            let hi = price_time_key(query.market_id, query.to_ms.unwrap_or(u64::MAX), u64::MAX);
            for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
                let (_, value) = entry?;
                let point: MarketPriceFact = rmp_serde::from_slice(value.value())?;
                if query
                    .before_height
                    .is_some_and(|before| point.height >= before)
                {
                    continue;
                }
                retain_latest(&mut points, point, limit);
            }
        } else {
            let table = txn.open_table(PRICES)?;
            let lo = price_key(query.market_id, 0);
            let hi = price_key(query.market_id, u64::MAX);
            let before_key = query
                .before_height
                .map(|height| price_key(query.market_id, height));
            let upper = before_key
                .as_ref()
                .map_or(Included(hi.as_slice()), |key| Excluded(key.as_slice()));
            for entry in table
                .range::<&[u8]>((Included(lo.as_slice()), upper))?
                .rev()
                .take(limit.saturating_add(1))
            {
                let (_, value) = entry?;
                points.push_front(rmp_serde::from_slice(value.value())?);
            }
        }
        let mut points: Vec<_> = points.into_iter().collect();
        let next_before_height = if points.len() > limit {
            points.remove(0);
            points.first().map(|point| point.height)
        } else {
            None
        };
        Ok(PriceHistoryPage {
            points,
            next_before_height,
            status,
        })
    }

    pub fn candles(&self, query: PriceCandleQuery) -> Result<PriceCandlePage, HistoryError> {
        let limit = query.limit.min(MAX_PAGE_SIZE);
        let txn = self.db.begin_read()?;
        let status = projection_status(&txn)?;
        if !self
            .candle_resolutions_secs
            .contains(&query.resolution_secs)
        {
            return Err(HistoryError::InvalidQuery(format!(
                "unsupported candle resolution {}; configured resolutions are {:?}",
                query.resolution_secs, self.candle_resolutions_secs
            )));
        }
        if limit == 0 {
            return Ok(PriceCandlePage {
                resolution_secs: query.resolution_secs,
                candles: vec![],
                next_before_ms: None,
                status,
            });
        }
        let table = txn.open_table(CANDLES)?;
        let lo = candle_key(
            query.market_id,
            query.resolution_secs,
            query.from_ms.unwrap_or(0),
        );
        let hi = candle_key(
            query.market_id,
            query.resolution_secs,
            query.to_ms.unwrap_or(u64::MAX),
        );
        let before_key = query
            .before_ms
            .map(|before| candle_key(query.market_id, query.resolution_secs, before));
        let upper = match before_key.as_ref() {
            Some(_)
                if query
                    .to_ms
                    .is_some_and(|to_ms| to_ms < query.before_ms.unwrap_or(u64::MAX)) =>
            {
                Included(hi.as_slice())
            }
            Some(before) => Excluded(before.as_slice()),
            None => Included(hi.as_slice()),
        };
        let mut candles = VecDeque::new();
        for entry in table.range::<&[u8]>((Included(lo.as_slice()), upper))? {
            let (_, value) = entry?;
            let candle: PriceCandle = rmp_serde::from_slice(value.value())?;
            retain_latest(&mut candles, candle, limit);
        }
        let mut candles: Vec<_> = candles.into_iter().collect();
        let next_before_ms = if candles.len() > limit {
            candles.remove(0);
            candles.first().map(|candle| candle.bucket_start_ms)
        } else {
            None
        };
        Ok(PriceCandlePage {
            resolution_secs: query.resolution_secs,
            candles,
            next_before_ms,
            status,
        })
    }
}

fn projection_status(txn: &ReadTransaction) -> Result<ProjectionStatus, HistoryError> {
    let bytes = txn.open_table(META_BYTES)?;
    let genesis_hash = bytes
        .get(META_GENESIS_HASH)?
        .map(|value| value.value().try_into())
        .transpose()
        .map_err(|_| HistoryError::InvalidBatch("stored genesis hash is not 32 bytes".into()))?;
    let meta = txn.open_table(META_U64)?;
    Ok(ProjectionStatus {
        genesis_hash,
        first_height: meta.get(META_FIRST_HEIGHT)?.map(|value| value.value()),
        first_timestamp_ms: meta
            .get(META_FIRST_TIMESTAMP_MS)?
            .map(|value| value.value()),
        indexed_through_height: meta
            .get(META_INDEXED_THROUGH_HEIGHT)?
            .map(|value| value.value()),
        indexed_through_timestamp_ms: meta
            .get(META_INDEXED_THROUGH_TIMESTAMP_MS)?
            .map(|value| value.value()),
    })
}

fn insert_projection<T: serde::Serialize>(
    table: &mut redb::Table<'_, &[u8], &[u8]>,
    key: &[u8],
    value: &T,
    kind: &'static str,
) -> Result<(), HistoryError> {
    let bytes = rmp_serde::to_vec(value)?;
    if let Some(existing) = table.get(key)? {
        if existing.value() != bytes.as_slice() {
            return Err(HistoryError::ConflictingProjection { kind });
        }
        return Ok(());
    }
    table.insert(key, bytes.as_slice())?;
    Ok(())
}

fn merge_candle(
    table: &mut redb::Table<'_, &[u8], &[u8]>,
    resolution_secs: u32,
    point: MarketPriceFact,
) -> Result<(), HistoryError> {
    let mut candle = PriceCandle::from_point(resolution_secs, point);
    let key = candle_key(point.market_id, resolution_secs, candle.bucket_start_ms);
    if let Some(existing) = table.get(key.as_slice())? {
        candle = rmp_serde::from_slice(existing.value())?;
        candle.merge_point(point);
    }
    let bytes = rmp_serde::to_vec(&candle)?;
    table.insert(key.as_slice(), bytes.as_slice())?;
    Ok(())
}

fn empty_page<T>(status: ProjectionStatus) -> HistoryPage<T> {
    HistoryPage {
        items: vec![],
        status,
        source_points: 0,
        downsampled: false,
    }
}

fn retain_latest<T>(items: &mut VecDeque<T>, item: T, limit: usize) {
    if items.len() == limit.saturating_add(1) {
        items.pop_front();
    }
    items.push_back(item);
}

fn fill_matches_market(fact: &AccountFillFact, market_id: Option<u32>) -> bool {
    market_id.is_none_or(|market_id| {
        fact.position_deltas
            .iter()
            .any(|delta| delta.market_id == market_id)
    })
}

fn fill_key(fact: &AccountFillFact) -> [u8; 24] {
    event_key(fact.account_id, fact.block_height, fact.order_id)
}

fn event_key(account_id: u64, height: u64, sequence: u64) -> [u8; 24] {
    let mut key = [0; 24];
    key[..8].copy_from_slice(&account_id.to_be_bytes());
    key[8..16].copy_from_slice(&height.to_be_bytes());
    key[16..].copy_from_slice(&sequence.to_be_bytes());
    key
}

fn equity_key(account_id: u64, timestamp_ms: u64, height: u64) -> [u8; 24] {
    let mut key = [0; 24];
    key[..8].copy_from_slice(&account_id.to_be_bytes());
    key[8..16].copy_from_slice(&timestamp_ms.to_be_bytes());
    key[16..].copy_from_slice(&height.to_be_bytes());
    key
}

fn account_bounds(account_id: u64) -> ([u8; 24], [u8; 24]) {
    (
        event_key(account_id, 0, 0),
        event_key(account_id, u64::MAX, u64::MAX),
    )
}

fn equity_account_bounds(account_id: u64) -> ([u8; 24], [u8; 24]) {
    (
        equity_key(account_id, 0, 0),
        equity_key(account_id, u64::MAX, u64::MAX),
    )
}

fn price_key(market_id: u32, height: u64) -> [u8; 12] {
    let mut key = [0; 12];
    key[..4].copy_from_slice(&market_id.to_be_bytes());
    key[4..].copy_from_slice(&height.to_be_bytes());
    key
}

fn price_time_key(market_id: u32, timestamp_ms: u64, height: u64) -> [u8; 20] {
    let mut key = [0; 20];
    key[..4].copy_from_slice(&market_id.to_be_bytes());
    key[4..12].copy_from_slice(&timestamp_ms.to_be_bytes());
    key[12..].copy_from_slice(&height.to_be_bytes());
    key
}

fn candle_key(market_id: u32, resolution_secs: u32, bucket_start_ms: u64) -> [u8; 16] {
    let mut key = [0; 16];
    key[..4].copy_from_slice(&market_id.to_be_bytes());
    key[4..8].copy_from_slice(&resolution_secs.to_be_bytes());
    key[8..].copy_from_slice(&bucket_start_ms.to_be_bytes());
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use sybil_history_types::{
        AccountEventKind, COMMITTED_HISTORY_SCHEMA_V1, FillCursor, PositionDeltaFact,
    };

    fn store() -> (tempfile::TempDir, HistoryStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = HistoryStore::open(dir.path().join("history.redb"), vec![60])
            .expect("open history store");
        (dir, store)
    }

    fn batch(
        height: u64,
        parent_hash: [u8; 32],
        block_hash: [u8; 32],
        timestamp_ms: u64,
    ) -> CommittedHistoryBatchV1 {
        CommittedHistoryBatchV1::new(
            [9; 32],
            height,
            parent_hash,
            block_hash,
            [height as u8; 32],
            timestamp_ms,
            vec![AccountFillFact {
                account_id: 7,
                order_id: height * 10,
                fill_qty: height,
                fill_price_nanos: 400_000_000 + height,
                block_height: height,
                timestamp_ms,
                position_deltas: vec![PositionDeltaFact {
                    market_id: 3,
                    outcome: 0,
                    delta: height as i64,
                }],
            }],
            vec![AccountEquityFact {
                account_id: 7,
                height,
                timestamp_ms,
                portfolio_value_nanos: 1_000 + height as i64,
                deposited_nanos: 900,
            }],
            vec![AccountEventFact {
                account_id: 7,
                seq: height,
                block_height: height,
                timestamp_ms,
                kind: AccountEventKind::Filled,
                market_id: Some(3),
                order_id: Some(height * 10),
                side: Some("buy".into()),
                outcome: Some("yes".into()),
                qty: Some(height),
                price_nanos: Some(400_000_000 + height),
                amount_nanos: None,
                realized_pnl_nanos: None,
                payout_outcome: None,
                reason: None,
                required_nanos: None,
                available_nanos: None,
            }],
            vec![MarketPriceFact {
                market_id: 3,
                height,
                timestamp_ms,
                yes_price_nanos: 400_000_000 + height,
                no_price_nanos: 600_000_000 - height,
                volume_nanos: height * 100,
            }],
        )
        .expect("valid history batch")
    }

    #[test]
    fn applies_atomically_and_projects_query_shapes() {
        let (_dir, store) = store();
        let first = batch(10, [1; 32], [10; 32], 100_000);
        let second = batch(11, [10; 32], [11; 32], 110_000);
        assert_eq!(
            store
                .apply_batch(first.clone())
                .expect("first apply")
                .outcome,
            ApplyBatchOutcome::Applied
        );
        assert_eq!(
            store
                .apply_batch(first.clone())
                .expect("duplicate apply")
                .outcome,
            ApplyBatchOutcome::AlreadyApplied
        );
        store.apply_batch(second).expect("second apply");

        let status = store.status().expect("status");
        assert_eq!(status.first_height, Some(10));
        assert_eq!(status.first_timestamp_ms, Some(100_000));
        assert_eq!(status.indexed_through_height, Some(11));
        assert_eq!(status.indexed_through_timestamp_ms, Some(110_000));
        assert_eq!(store.raw_batch(10).expect("raw batch"), Some(first));

        let fills = store
            .fills(FillQuery {
                account_id: 7,
                market_id: Some(3),
                after: None,
                limit: 10,
                offset: 0,
            })
            .expect("fills");
        assert_eq!(
            fills
                .items
                .iter()
                .map(|fact| fact.block_height)
                .collect::<Vec<_>>(),
            vec![11, 10]
        );
        let fills_after = store
            .fills(FillQuery {
                account_id: 7,
                market_id: None,
                after: Some(FillCursor {
                    block_height: 10,
                    order_id: 100,
                }),
                limit: 10,
                offset: 0,
            })
            .expect("fills after cursor");
        assert_eq!(fills_after.items[0].block_height, 11);

        let equity = store
            .equity(EquityQuery {
                account_id: 7,
                since_ms: 105_000,
            })
            .expect("equity");
        assert_eq!(
            equity
                .items
                .iter()
                .map(|fact| fact.height)
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
        let baselines = store
            .equity_baselines(EquityBaselinesQuery {
                account_ids: vec![8, 7, 7],
                at_or_before_ms: 105_000,
            })
            .expect("baselines");
        assert_eq!(baselines.baselines.len(), 1);
        assert_eq!(baselines.baselines[0].height, 10);

        let events = store
            .events(AccountEventQuery {
                account_id: 7,
                limit: 10,
                before: None,
                category: Some("trades".into()),
            })
            .expect("events");
        assert_eq!(events.items[0].block_height, 11);

        let prices = store
            .prices(PriceHistoryQuery {
                market_id: 3,
                from_ms: None,
                to_ms: None,
                before_height: None,
                limit: 10,
            })
            .expect("prices");
        assert_eq!(prices.points.len(), 2);
        let candles = store
            .candles(PriceCandleQuery {
                market_id: 3,
                resolution_secs: 60,
                from_ms: None,
                to_ms: None,
                before_ms: None,
                limit: 10,
            })
            .expect("candles");
        assert_eq!(candles.candles.len(), 1);
        assert_eq!(candles.candles[0].point_count, 2);
        assert_eq!(candles.candles[0].first_height, 10);
        assert_eq!(candles.candles[0].last_height, 11);
    }

    #[test]
    fn rejects_gaps_parent_mismatches_and_conflicting_replays() {
        let (_dir, store) = store();
        let first = batch(10, [1; 32], [10; 32], 100_000);
        store.apply_batch(first.clone()).expect("first apply");

        assert!(matches!(
            store.apply_batch(batch(12, [11; 32], [12; 32], 120_000)),
            Err(HistoryError::Gap {
                expected: 11,
                received: 12
            })
        ));
        assert!(matches!(
            store.apply_batch(batch(11, [99; 32], [11; 32], 110_000)),
            Err(HistoryError::ParentHashMismatch { height: 11 })
        ));

        let mut conflict = first;
        conflict.state_root = [77; 32];
        conflict.payload_hash = conflict.compute_payload_hash().expect("hash conflict");
        assert!(matches!(
            store.apply_batch(conflict),
            Err(HistoryError::ConflictingBatch { height: 10 })
        ));

        let mut wrong_genesis = batch(11, [10; 32], [11; 32], 110_000);
        wrong_genesis.genesis_hash = [8; 32];
        wrong_genesis.payload_hash = wrong_genesis.compute_payload_hash().expect("hash genesis");
        assert!(matches!(
            store.apply_batch(wrong_genesis),
            Err(HistoryError::GenesisMismatch)
        ));
        assert_eq!(
            store.raw_batch(11).expect("raw gap remains absent"),
            None,
            "failed batches must not partially project"
        );
    }

    #[test]
    fn query_schema_constant_is_v1() {
        assert_eq!(COMMITTED_HISTORY_SCHEMA_V1, 1);
    }

    #[test]
    fn candle_resolution_set_is_persisted_and_cannot_silently_drift() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.redb");
        let store = HistoryStore::open(&path, vec![300, 60, 60]).expect("initial open");
        store
            .apply_batch(batch(10, [1; 32], [10; 32], 100_000))
            .expect("first apply");
        drop(store);

        HistoryStore::open(&path, vec![60, 300]).expect("same normalized configuration");
        let error = match HistoryStore::open(&path, vec![60, 300, 3_600]) {
            Ok(_) => panic!("adding a partial candle projection must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(error, HistoryError::Configuration(_)));
    }

    #[test]
    fn reopen_backfills_checkpoint_timestamp_from_the_raw_batch() {
        let (dir, store) = store();
        let path = dir.path().join("history.redb");
        store
            .apply_batch(batch(10, [1; 32], [10; 32], 100_000))
            .expect("first apply");
        store
            .apply_batch(batch(11, [10; 32], [11; 32], 110_000))
            .expect("second apply");
        {
            let txn = store.db.begin_write().expect("legacy metadata write");
            txn.open_table(META_U64)
                .expect("metadata table")
                .remove(META_INDEXED_THROUGH_TIMESTAMP_MS)
                .expect("remove new metadata");
            txn.commit().expect("legacy metadata commit");
        }
        drop(store);

        let reopened = HistoryStore::open(path, vec![60]).expect("upgraded store reopens");
        assert_eq!(
            reopened
                .status()
                .expect("reopened status")
                .indexed_through_timestamp_ms,
            Some(110_000)
        );
    }

    #[test]
    fn unsupported_candle_resolution_is_an_explicit_query_error() {
        let (_dir, store) = store();
        let error = store
            .candles(PriceCandleQuery {
                market_id: 3,
                resolution_secs: 300,
                from_ms: None,
                to_ms: None,
                before_ms: None,
                limit: 10,
            })
            .expect_err("unsupported resolution must not look like empty history");
        assert!(matches!(error, HistoryError::InvalidQuery(_)));
    }
}
