use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use sybil_history_types::CommittedHistoryBatchV1;

use super::{
    KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS, KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES,
    PRODUCT_HISTORY_OUTBOX, PRODUCT_HISTORY_OUTBOX_META, Store, StoreError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductHistoryOutboxAck {
    pub height: u64,
    pub payload_hash: [u8; 32],
}

/// Cheap, exact logical stock metrics for the durable product-history source
/// outbox. `payload_bytes` counts encoded values only; filesystem capacity is
/// monitored separately because redb page and fragmentation overhead is not a
/// stable per-table quota.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProductHistoryOutboxStats {
    pub rows: u64,
    pub payload_bytes: u64,
    pub oldest_height: Option<u64>,
    pub newest_height: Option<u64>,
    pub oldest_committed_at_ms: Option<u64>,
}

fn encoded_len(bytes: &[u8]) -> Result<u64, StoreError> {
    u64::try_from(bytes.len()).map_err(|_| {
        StoreError::CorruptLayout("product-history outbox payload length exceeds u64".to_string())
    })
}

/// Add the payload-byte counter to stores created before the metric existed.
/// The one-time scan is performed during `Store::open`; normal reads and
/// updates remain O(log n) and never rescan an accumulated backlog.
pub(super) fn initialize_product_history_outbox_meta(
    db: &redb::Database,
) -> Result<(), StoreError> {
    let txn = db.begin_write()?;
    let (payload_bytes, oldest_committed_at_ms) = {
        let meta = txn.open_table(PRODUCT_HISTORY_OUTBOX_META)?;
        (
            meta.get(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES)?
                .map(|value| value.value()),
            meta.get(KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS)?
                .map(|value| value.value()),
        )
    };
    let outbox_empty = txn.open_table(PRODUCT_HISTORY_OUTBOX)?.is_empty()?;
    match (payload_bytes, oldest_committed_at_ms, outbox_empty) {
        (Some(0), None, true) => {}
        (Some(payload_bytes), Some(_), false) if payload_bytes > 0 => {}
        (None, None, _) => {
            let (payload_bytes, oldest_committed_at_ms) = {
                let table = txn.open_table(PRODUCT_HISTORY_OUTBOX)?;
                let mut total = 0u64;
                let mut oldest_committed_at_ms = None;
                for entry in table.iter()? {
                    let (_, value) = entry?;
                    if oldest_committed_at_ms.is_none() {
                        let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(value.value())?;
                        oldest_committed_at_ms = Some(batch.committed_at_ms);
                    }
                    total = total
                        .checked_add(encoded_len(value.value())?)
                        .ok_or_else(|| {
                            StoreError::CorruptLayout(
                                "product-history outbox payload-byte counter overflow".to_string(),
                            )
                        })?;
                }
                (total, oldest_committed_at_ms)
            };
            let mut meta = txn.open_table(PRODUCT_HISTORY_OUTBOX_META)?;
            meta.insert(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES, payload_bytes)?;
            if let Some(committed_at_ms) = oldest_committed_at_ms {
                meta.insert(
                    KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS,
                    committed_at_ms,
                )?;
            }
        }
        _ => {
            return Err(StoreError::CorruptLayout(
                "product-history outbox stock metadata is partially initialized".to_string(),
            ));
        }
    }
    txn.commit()?;
    Ok(())
}

impl Store {
    /// Return the oldest unacknowledged history batches. The outbox is ordered
    /// by block height and is the durable delivery source; actor/mailbox wakes
    /// are only hints.
    pub fn product_history_outbox_batches(
        &self,
        limit: usize,
    ) -> Result<Vec<CommittedHistoryBatchV1>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PRODUCT_HISTORY_OUTBOX)?;
        let mut batches = Vec::new();
        for entry in table.iter()?.take(limit) {
            let (_, value) = entry?;
            let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(value.value())?;
            batch.validate().map_err(|error| {
                StoreError::CorruptLayout(format!("invalid product-history outbox batch: {error}"))
            })?;
            batches.push(batch);
        }
        Ok(batches)
    }

    pub fn product_history_outbox_len(&self) -> Result<u64, StoreError> {
        let txn = self.db.begin_read()?;
        Ok(txn.open_table(PRODUCT_HISTORY_OUTBOX)?.len()?)
    }

    pub fn product_history_outbox_stats(&self) -> Result<ProductHistoryOutboxStats, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PRODUCT_HISTORY_OUTBOX)?;
        let rows = table.len()?;
        let oldest_height = table.first()?.map(|(key, _)| key.value());
        let newest_height = table.last()?.map(|(key, _)| key.value());
        let (payload_bytes, oldest_committed_at_ms) = {
            let meta = txn.open_table(PRODUCT_HISTORY_OUTBOX_META)?;
            let payload_bytes = meta
                .get(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES)?
                .map(|value| value.value())
                .ok_or_else(|| {
                    StoreError::CorruptLayout(
                        "missing product-history outbox payload-byte counter".to_string(),
                    )
                })?;
            let oldest_committed_at_ms = meta
                .get(KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS)?
                .map(|value| value.value());
            (payload_bytes, oldest_committed_at_ms)
        };
        let empty_metadata = oldest_height.is_none()
            && newest_height.is_none()
            && oldest_committed_at_ms.is_none()
            && payload_bytes == 0;
        let populated_metadata = oldest_height.is_some()
            && newest_height.is_some()
            && oldest_committed_at_ms.is_some()
            && payload_bytes > 0;
        if (rows == 0 && !empty_metadata) || (rows > 0 && !populated_metadata) {
            return Err(StoreError::CorruptLayout(
                "product-history outbox stock metadata is inconsistent".to_string(),
            ));
        }
        Ok(ProductHistoryOutboxStats {
            rows,
            payload_bytes,
            oldest_height,
            newest_height,
            oldest_committed_at_ms,
        })
    }

    /// Delete one row only after a consumer has durably committed the exact
    /// payload hash. A stale or cross-genesis acknowledgement cannot discard a
    /// different batch at the same height.
    pub fn acknowledge_product_history_batch(
        &self,
        ack: ProductHistoryOutboxAck,
    ) -> Result<bool, StoreError> {
        Ok(self.acknowledge_product_history_batches(&[ack])? == 1)
    }

    /// Delete a delivered prefix in one redb transaction. Catch-up can apply
    /// several batches remotely without forcing one local fsync per height.
    /// Every hash is verified before the transaction commits, so a bad ack
    /// cannot partially discard the prefix.
    pub fn acknowledge_product_history_batches(
        &self,
        acks: &[ProductHistoryOutboxAck],
    ) -> Result<usize, StoreError> {
        if acks.is_empty() {
            return Ok(0);
        }
        let txn = self.db.begin_write()?;
        let (removed, removed_payload_bytes, oldest_remaining_committed_at_ms) = {
            let mut table = txn.open_table(PRODUCT_HISTORY_OUTBOX)?;
            let mut removed = 0usize;
            let mut removed_payload_bytes = 0u64;
            for ack in acks {
                let Some(value) = table.get(ack.height)? else {
                    continue;
                };
                let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(value.value())?;
                if batch.payload_hash != ack.payload_hash {
                    return Err(StoreError::CorruptLayout(format!(
                        "product-history outbox acknowledgement hash mismatch at height {}",
                        ack.height
                    )));
                }
                let payload_bytes = encoded_len(value.value())?;
                drop(value);
                if table.remove(ack.height)?.is_some() {
                    removed += 1;
                    removed_payload_bytes = removed_payload_bytes
                        .checked_add(payload_bytes)
                        .ok_or_else(|| {
                            StoreError::CorruptLayout(
                                "product-history acknowledgement byte counter overflow".to_string(),
                            )
                        })?;
                }
            }
            let oldest_remaining_committed_at_ms = if removed > 0 {
                table
                    .first()?
                    .map(|(_, value)| {
                        rmp_serde::from_slice::<CommittedHistoryBatchV1>(value.value())
                            .map(|batch| batch.committed_at_ms)
                    })
                    .transpose()?
            } else {
                None
            };
            (
                removed,
                removed_payload_bytes,
                oldest_remaining_committed_at_ms,
            )
        };
        if removed_payload_bytes > 0 {
            let mut meta = txn.open_table(PRODUCT_HISTORY_OUTBOX_META)?;
            let current = meta
                .get(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES)?
                .map(|value| value.value())
                .ok_or_else(|| {
                    StoreError::CorruptLayout(
                        "missing product-history outbox payload-byte counter".to_string(),
                    )
                })?;
            let remaining = current.checked_sub(removed_payload_bytes).ok_or_else(|| {
                StoreError::CorruptLayout(format!(
                    "product-history outbox payload-byte counter underflow: current={current}, removed={removed_payload_bytes}"
                ))
            })?;
            meta.insert(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES, remaining)?;
            match oldest_remaining_committed_at_ms {
                Some(committed_at_ms) => {
                    meta.insert(
                        KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS,
                        committed_at_ms,
                    )?;
                }
                None => {
                    meta.remove(KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS)?;
                }
            }
        }
        txn.commit()?;
        Ok(removed)
    }
}
