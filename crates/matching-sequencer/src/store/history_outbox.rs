use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use sybil_history_types::CommittedHistoryBatchV1;

use super::{HISTORY_OUTBOX, Store, StoreError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryOutboxAck {
    pub height: u64,
    pub payload_hash: [u8; 32],
}

impl Store {
    /// Return the oldest unacknowledged history batches. The outbox is ordered
    /// by block height and is the durable delivery source; actor/mailbox wakes
    /// are only hints.
    pub fn history_outbox_batches(
        &self,
        limit: usize,
    ) -> Result<Vec<CommittedHistoryBatchV1>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY_OUTBOX)?;
        let mut batches = Vec::new();
        for entry in table.iter()?.take(limit) {
            let (_, value) = entry?;
            let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(value.value())?;
            batch.validate().map_err(|error| {
                StoreError::CorruptLayout(format!("invalid history outbox batch: {error}"))
            })?;
            batches.push(batch);
        }
        Ok(batches)
    }

    pub fn history_outbox_len(&self) -> Result<u64, StoreError> {
        let txn = self.db.begin_read()?;
        Ok(txn.open_table(HISTORY_OUTBOX)?.len()?)
    }

    /// Delete one row only after a consumer has durably committed the exact
    /// payload hash. A stale or cross-genesis acknowledgement cannot discard a
    /// different batch at the same height.
    pub fn acknowledge_history_batch(&self, ack: HistoryOutboxAck) -> Result<bool, StoreError> {
        Ok(self.acknowledge_history_batches(&[ack])? == 1)
    }

    /// Delete a delivered prefix in one redb transaction. Catch-up can apply
    /// several batches remotely without forcing one local fsync per height.
    /// Every hash is verified before the transaction commits, so a bad ack
    /// cannot partially discard the prefix.
    pub fn acknowledge_history_batches(
        &self,
        acks: &[HistoryOutboxAck],
    ) -> Result<usize, StoreError> {
        if acks.is_empty() {
            return Ok(0);
        }
        let txn = self.db.begin_write()?;
        let removed = {
            let mut table = txn.open_table(HISTORY_OUTBOX)?;
            let mut removed = 0usize;
            for ack in acks {
                let Some(value) = table.get(ack.height)? else {
                    continue;
                };
                let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(value.value())?;
                if batch.payload_hash != ack.payload_hash {
                    return Err(StoreError::CorruptLayout(format!(
                        "history outbox acknowledgement hash mismatch at height {}",
                        ack.height
                    )));
                }
                drop(value);
                removed += usize::from(table.remove(ack.height)?.is_some());
            }
            removed
        };
        txn.commit()?;
        Ok(removed)
    }
}
