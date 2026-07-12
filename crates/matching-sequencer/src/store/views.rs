use super::*;

impl Store {
    pub const MAX_EQUITY_QUERY_POINTS: usize = 5_000;
    /// Recover at most `cap` of the newest durable fills for each account.
    ///
    /// Fill-history keys cluster records by account and sort them oldest-first,
    /// so a reverse range scan can stop as soon as the hot restore window is
    /// full. Records are returned newest-first within each account's group.
    pub fn recover_account_fills(
        &self,
        account_ids: &[AccountId],
        cap: usize,
    ) -> Result<Vec<(AccountId, AccountFillRecord)>, StoreError> {
        if cap == 0 || account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(FILL_HISTORY)?;
        let mut out = Vec::new();
        for &account_id in account_ids {
            let (lo, hi) = fill_history_account_bounds(account_id);
            for entry in table
                .range::<&[u8]>(lo.as_slice()..=hi.as_slice())?
                .rev()
                .take(cap)
            {
                let (_key, value) = entry?;
                out.push((account_id, rmp_serde::from_slice(value.value())?));
            }
        }
        Ok(out)
    }

    pub async fn state_qmdb_root(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<QmdbStateRoot, StoreError> {
        self.account_state_store.qmdb_state_root(slot).await
    }

    pub async fn state_qmdb_leaves(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
        self.account_state_store.qmdb_state_leaves(slot).await
    }

    pub async fn state_qmdb_leaf_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafProof>, StoreError> {
        self.account_state_store
            .qmdb_state_leaf_proof(slot, leaf_key)
            .await
    }

    pub async fn state_qmdb_leaf_exclusion_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
        self.account_state_store
            .qmdb_state_leaf_exclusion_proof(slot, leaf_key)
            .await
    }

    pub async fn current_state_qmdb_root(&self) -> Result<Option<QmdbStateRoot>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_root(fence.slot).await.map(Some)
    }

    pub async fn current_state_qmdb_leaf_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafProof>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_leaf_proof(fence.slot, leaf_key).await
    }

    pub async fn current_state_qmdb_leaf_exclusion_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
        let Some(fence) = read_account_state_fence(&self.db)? else {
            return Ok(None);
        };
        self.state_qmdb_leaf_exclusion_proof(fence.slot, leaf_key)
            .await
    }

    /// Load a persisted block witness by height.
    pub fn block_witness(&self, height: u64) -> Result<Option<BlockWitness>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCK_WITNESSES)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Load a historical API replay block by exact height.
    pub async fn load_block(&self, height: u64) -> Result<Option<SealedBlock>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCKS_FULL)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }
    /// Load a newest-first page of historical API replay blocks. When
    /// `before_height` is present, only blocks with height strictly below that
    /// cursor are returned.
    pub async fn load_block_page(
        &self,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<SealedBlock>, StoreError> {
        if limit == 0 || before_height == Some(0) {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCKS_FULL)?;
        let mut blocks = Vec::new();
        match before_height {
            Some(before) => {
                for entry in table.range(0..before)?.rev() {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                    if blocks.len() >= limit {
                        break;
                    }
                }
            }
            None => {
                for entry in table.iter()?.rev() {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                    if blocks.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(blocks)
    }
    /// Load raw mark-price points for one market. The scan is bounded in
    /// memory: if more than `limit` points match, the newest `limit` points
    /// are returned in chronological order with a `before_height` cursor for
    /// the next older page.
    pub async fn load_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<crate::market_info::PriceHistoryPage, StoreError> {
        let txn = self.db.begin_read()?;
        let retention_min_height = {
            let meta = txn.open_table(HISTORY_META)?;
            meta.get(KEY_PRICE_POINTS_MIN_HEIGHT)?
                .map(|value| value.value())
        };
        if limit == 0 {
            return Ok(crate::market_info::PriceHistoryPage {
                points: Vec::new(),
                next_before_height: None,
                retention_min_height,
            });
        }
        let table = txn.open_table(PRICE_POINTS)?;
        let (lo, hi) = price_point_market_bounds(market_id);
        let mut points = VecDeque::new();
        for entry in table.range(lo.as_slice()..=hi.as_slice())? {
            let (_, value) = entry?;
            let point: crate::market_info::PricePoint = rmp_serde::from_slice(value.value())?;
            if from_ms.is_some_and(|from| point.timestamp_ms < from)
                || to_ms.is_some_and(|to| point.timestamp_ms > to)
                || before_height.is_some_and(|before| point.height >= before)
            {
                continue;
            }
            if points.len() == limit.saturating_add(1) {
                points.pop_front();
            }
            points.push_back(point);
        }
        let mut points: Vec<_> = points.into_iter().collect();
        let next_before_height = if points.len() > limit {
            points.remove(0);
            points.first().map(|point| point.height)
        } else {
            None
        };
        Ok(crate::market_info::PriceHistoryPage {
            points,
            next_before_height,
            retention_min_height,
        })
    }

    /// Load downsampled price candles for one market/resolution. The newest
    /// matching candles are returned in chronological order, with
    /// `before_ms` cursoring to the next older page.
    pub async fn load_price_candles(
        &self,
        market_id: MarketId,
        resolution_secs: u32,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<PriceCandlePage, StoreError> {
        if resolution_secs == 0 {
            return Ok(PriceCandlePage {
                resolution_secs,
                candles: Vec::new(),
                next_before_ms: None,
                retention_min_bucket_ms: None,
            });
        }
        let txn = self.db.begin_read()?;
        let retention_min_bucket_ms = {
            let meta = txn.open_table(HISTORY_META)?;
            let key = price_candles_min_bucket_key(resolution_secs);
            meta.get(key.as_str())?.map(|value| value.value())
        };
        if limit == 0 {
            return Ok(PriceCandlePage {
                resolution_secs,
                candles: Vec::new(),
                next_before_ms: None,
                retention_min_bucket_ms,
            });
        }
        let table = txn.open_table(PRICE_CANDLES)?;
        let (lo, hi) = price_candle_market_resolution_bounds(market_id, resolution_secs);
        let mut candles = VecDeque::new();
        for entry in table.range(lo.as_slice()..=hi.as_slice())? {
            let (_, value) = entry?;
            let candle: PriceCandle = rmp_serde::from_slice(value.value())?;
            if from_ms.is_some_and(|from| candle.bucket_start_ms < from)
                || to_ms.is_some_and(|to| candle.bucket_start_ms > to)
                || before_ms.is_some_and(|before| candle.bucket_start_ms >= before)
            {
                continue;
            }
            if candles.len() == limit.saturating_add(1) {
                candles.pop_front();
            }
            candles.push_back(candle);
        }
        let mut candles: Vec<_> = candles.into_iter().collect();
        let next_before_ms = if candles.len() > limit {
            candles.remove(0);
            candles.first().map(|candle| candle.bucket_start_ms)
        } else {
            None
        };
        Ok(PriceCandlePage {
            resolution_secs,
            candles,
            next_before_ms,
            retention_min_bucket_ms,
        })
    }

    /// Load the latest committed block witness, if the store has one.
    pub fn latest_block_witness(&self) -> Result<Option<BlockWitness>, StoreError> {
        let txn = self.db.begin_read()?;
        let Some(metadata) = read_recovery_metadata(&txn)? else {
            return Ok(None);
        };
        let table = txn.open_table(BLOCK_WITNESSES)?;
        table
            .get(metadata.height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }
    /// Append this block's equity points and history events as individual rows.
    /// Append-only; standalone version used by tests and as a fallback.
    pub fn append_offblock_rows(
        &self,
        equity: &[(AccountId, crate::aggregates::EquityPoint)],
        history: &[crate::aggregates::StoredHistoryEvent],
    ) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(EQUITY_POINTS)?;
            let mut t_by_time = txn.open_table(EQUITY_POINTS_BY_TIME)?;
            for (aid, p) in equity {
                let key = equity_key(*aid, p.height);
                let time_key = equity_by_time_key(p.timestamp_ms, *aid, p.height);
                let bytes = rmp_serde::to_vec(p)?;
                t.insert(key.as_slice(), bytes.as_slice())?;
                t_by_time.insert(time_key.as_slice(), 0)?;
            }
            let mut h = txn.open_table(HISTORY_EVENTS)?;
            let mut h_by_time = txn.open_table(HISTORY_EVENTS_BY_TIME)?;
            for ev in history {
                let key = history_event_key(AccountId(ev.account_id), ev.block_height, ev.seq);
                let time_key = history_event_by_time_key(
                    ev.timestamp_ms,
                    AccountId(ev.account_id),
                    ev.block_height,
                    ev.seq,
                );
                let bytes = rmp_serde::to_vec(ev)?;
                h.insert(key.as_slice(), bytes.as_slice())?;
                h_by_time.insert(time_key.as_slice(), 0)?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// Equity points for an account, oldest-first (matches `EquityTracker::series`),
    /// keeping points at/after `since_ms` plus the last point before the boundary
    /// as an opening anchor. Pass `since_ms == 0` for the full series. Points are
    /// keyed by height, so the timestamp range is applied while scanning.
    pub fn equity_series_page(
        &self,
        account_id: AccountId,
        since_ms: u64,
    ) -> Result<(Vec<crate::aggregates::EquityPoint>, usize), StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EQUITY_POINTS)?;
        let lo = equity_key(account_id, 0);
        let hi = equity_key(account_id, u64::MAX);
        let mut out = Vec::new();
        let mut opening_anchor = None;
        let mut source_points = 0usize;
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
            let (_k, v) = entry?;
            let point: crate::aggregates::EquityPoint = rmp_serde::from_slice(v.value())?;
            if point.timestamp_ms >= since_ms {
                out.push(point);
                source_points += 1;
            } else if since_ms > 0 {
                opening_anchor = Some(point);
            }
            if out.len() > Self::MAX_EQUITY_QUERY_POINTS * 2 {
                let latest = out.pop().expect("equity output is non-empty");
                out = out.into_iter().step_by(2).collect();
                out.push(latest);
            }
        }
        if let Some(anchor) = opening_anchor {
            out.insert(0, anchor);
            source_points += 1;
        }
        if out.len() > Self::MAX_EQUITY_QUERY_POINTS {
            let last = out.len() - 1;
            out = (0..Self::MAX_EQUITY_QUERY_POINTS)
                .map(|index| out[index * last / (Self::MAX_EQUITY_QUERY_POINTS - 1)])
                .collect();
        }
        debug_assert!(out.len() <= Self::MAX_EQUITY_QUERY_POINTS);
        Ok((out, source_points))
    }

    pub fn equity_series(
        &self,
        account_id: AccountId,
        since_ms: u64,
    ) -> Result<Vec<crate::aggregates::EquityPoint>, StoreError> {
        self.equity_series_page(account_id, since_ms)
            .map(|(points, _)| points)
    }

    /// Newest-first page of an account's history, replicating
    /// `AccountEventLog::query` (cursor `before = (block_height, seq)`,
    /// `category` filter via `HistoryKind::category`).
    pub fn account_events(
        &self,
        account_id: AccountId,
        limit: usize,
        before: Option<(u64, u64)>,
        category: Option<String>,
    ) -> Result<Vec<crate::aggregates::HistoryEvent>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY_EVENTS)?;
        let lo = history_event_key(account_id, 0, 0);
        let hi = history_event_key(account_id, u64::MAX, u64::MAX);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
            let (_k, v) = entry?;
            let stored: crate::aggregates::StoredHistoryEvent = rmp_serde::from_slice(v.value())?;
            if let Some((b, s)) = before {
                // Keep only events strictly before the cursor; skip the rest.
                if (stored.block_height, stored.seq) >= (b, s) {
                    continue;
                }
            }
            if let Some(ref c) = category
                && stored.kind.category() != c.as_str()
            {
                continue;
            }
            out.push(stored.into_event());
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Newest-first page of an account's fills from the durable store,
    /// replicating [`crate::fill_recorder::FillRecorder::account_fills`]: a fill
    /// matches `market_id_filter` if any of its `position_deltas` touches that
    /// market, then `offset`/`limit` page over the filtered, newest-first
    /// sequence.
    ///
    /// Reads the full persisted history, which outlives the bounded in-memory
    /// recorder — so `/v1/accounts/{id}/fills` stays populated even when the hot
    /// serving window is empty (e.g. prod retention caps). Stored keys sort
    /// ascending by `(block_height, order_id)`; we iterate in reverse to serve
    /// newest-first.
    pub fn account_fills(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(FILL_HISTORY)?;
        let (lo, hi) = fill_history_account_bounds(account_id);
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())?.rev() {
            let (_k, v) = entry?;
            let record: AccountFillRecord = rmp_serde::from_slice(v.value())?;
            let matches = market_id_filter
                .is_none_or(|mid| record.position_deltas.iter().any(|(m, _, _)| *m == mid));
            if !matches {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            out.push(record);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Oldest-first durable page of fills strictly after `after`, ordered by
    /// the stable `(block_height, order_id)` cursor.
    pub fn account_fills_after(
        &self,
        account_id: AccountId,
        market_id_filter: Option<MarketId>,
        after: Option<AccountFillCursor>,
        limit: usize,
    ) -> Result<Vec<AccountFillRecord>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(FILL_HISTORY)?;
        let (lo, hi) = fill_history_account_bounds(account_id);
        let mut out = Vec::new();
        for entry in table.range::<&[u8]>(lo.as_slice()..=hi.as_slice())? {
            let (_k, v) = entry?;
            let record: AccountFillRecord = rmp_serde::from_slice(v.value())?;
            if after.is_some_and(|cursor| AccountFillCursor::from_record(&record) <= cursor) {
                continue;
            }
            let matches = market_id_filter
                .is_none_or(|mid| record.position_deltas.iter().any(|(m, _, _)| *m == mid));
            if !matches {
                continue;
            }
            out.push(record);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
