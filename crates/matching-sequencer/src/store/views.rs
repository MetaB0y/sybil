use super::*;

impl Store {
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
