use super::*;

impl Store {
    pub fn latest_proof_job_outbox_entry(&self) -> Result<Option<ProofJobOutboxEntry>, StoreError> {
        let txn = self.db.begin_read()?;
        let jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
        let acks = txn.open_table(PROOF_JOB_ACKS)?;
        let Some(row) = jobs.iter()?.next_back() else {
            return Ok(None);
        };
        let (height, value) = row?;
        let height = height.value();
        let bytes = value.value().to_vec();
        let digest = sybil_proof_protocol::proof_job_transport_digest(&bytes);
        let acknowledged = acks
            .get(height)?
            .is_some_and(|stored| stored.value() == digest);
        Ok(Some(ProofJobOutboxEntry {
            height,
            digest,
            bytes,
            acknowledged,
        }))
    }

    /// Read portable proof jobs in strictly increasing committed-height order.
    /// Rows remain readable after acknowledgement until a separate retention
    /// policy safely prunes them.
    pub fn proof_job_outbox_page(
        &self,
        after_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<ProofJobOutboxEntry>, StoreError> {
        if limit == 0 || after_height == Some(u64::MAX) {
            return Ok(Vec::new());
        }
        let start = after_height.map_or(0, |height| height + 1);
        let txn = self.db.begin_read()?;
        let jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
        let acks = txn.open_table(PROOF_JOB_ACKS)?;
        let mut entries = Vec::with_capacity(limit);
        for row in jobs.range(start..)?.take(limit) {
            let (height, value) = row?;
            let height = height.value();
            let bytes = value.value().to_vec();
            let digest = sybil_proof_protocol::proof_job_transport_digest(&bytes);
            let acknowledged = acks
                .get(height)?
                .is_some_and(|stored| stored.value() == digest);
            entries.push(ProofJobOutboxEntry {
                height,
                digest,
                bytes,
                acknowledged,
            });
        }
        Ok(entries)
    }

    /// Record that the prover made these exact bytes durable. A wrong digest
    /// fails closed and cannot acknowledge a conflicting/corrupt payload.
    pub async fn acknowledge_proof_job(
        &self,
        height: u64,
        digest: [u8; 32],
    ) -> Result<(), StoreError> {
        self.redb_write(move |db| {
            let txn = db.begin_write()?;
            {
                let jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
                let job = jobs.get(height)?.ok_or_else(|| {
                    StoreError::ProofJob(format!(
                        "cannot acknowledge missing proof job at height {height}"
                    ))
                })?;
                let expected = sybil_proof_protocol::proof_job_transport_digest(job.value());
                if digest != expected {
                    return Err(StoreError::ProofJob(format!(
                        "ack digest does not match proof job at height {height}"
                    )));
                }
            }
            {
                let mut acks = txn.open_table(PROOF_JOB_ACKS)?;
                if let Some(existing) = acks.get(height)?
                    && existing.value() != digest
                {
                    return Err(StoreError::ProofJob(format!(
                        "conflicting ack digest already exists at height {height}"
                    )));
                }
                acks.insert(height, digest.as_slice())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await
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

    /// Load a persisted latest-only recovery witness by height.
    pub fn block_witness(&self, height: u64) -> Result<Option<BlockWitness>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BLOCK_WITNESSES)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Load a canonical API replay block from the bounded local archive.
    pub async fn load_block(&self, height: u64) -> Result<Option<SealedBlock>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CANONICAL_BLOCK_ARCHIVE)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Load a newest-first page of canonical API replay blocks. When
    /// `before_height` is present, only lower heights are returned.
    pub async fn load_block_page(
        &self,
        before_height: Option<u64>,
        limit: usize,
    ) -> Result<Vec<SealedBlock>, StoreError> {
        if limit == 0 || before_height == Some(0) {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read()?;
        let table = txn.open_table(CANONICAL_BLOCK_ARCHIVE)?;
        let mut blocks = Vec::new();
        match before_height {
            Some(before) => {
                for entry in table.range(0..before)?.rev().take(limit) {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                }
            }
            None => {
                for entry in table.iter()?.rev().take(limit) {
                    let (_, value) = entry?;
                    blocks.push(rmp_serde::from_slice(value.value())?);
                }
            }
        }
        Ok(blocks)
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
}
