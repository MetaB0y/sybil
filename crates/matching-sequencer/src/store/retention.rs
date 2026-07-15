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
        info!(height, "pruned non-current recovery rows from store");
    }
    Ok(pruned)
}

/// Bounded maintenance policy for canonical replay blocks and their paired DA
/// serving artifacts. Product/account history has a different owner:
/// `sybil-history`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalArchiveRetentionPolicy {
    /// Number of canonical heights to retain. Zero disables maintenance.
    pub retention_blocks: u64,
    /// Run one maintenance pass every N committed heights.
    pub maintenance_interval_blocks: u64,
    /// Maximum block or DA-artifact rows deleted in one pass.
    pub max_rows_per_pass: usize,
}

impl CanonicalArchiveRetentionPolicy {
    pub fn should_maintain_at(self, height: u64) -> bool {
        height > 0
            && self.retention_blocks > 0
            && self.maintenance_interval_blocks > 0
            && self.max_rows_per_pass > 0
            && height.is_multiple_of(self.maintenance_interval_blocks)
    }

    fn target_floor(self, head_height: u64) -> Option<u64> {
        if head_height == 0 || self.retention_blocks == 0 {
            return None;
        }
        Some(
            head_height
                .saturating_sub(self.retention_blocks.saturating_sub(1))
                .max(1),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalArchiveMeta {
    /// Oldest canonical replay block actually present after bounded pruning.
    pub oldest_retained_height: Option<u64>,
    pub last_maintenance_height: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalArchivePruneReport {
    pub replay_blocks_pruned: usize,
    pub da_artifacts_pruned: usize,
    pub meta: CanonicalArchiveMeta,
}

/// Bounded source-retention policy for proof jobs that the standalone prover
/// has already durably ingested and acknowledged by exact transport digest.
///
/// Unacknowledged jobs are never eligible: the sequencer remains their durable
/// owner until the prover completes the at-least-once handoff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcknowledgedProofJobRetentionPolicy {
    /// Number of acknowledged proof-job heights to retain as a safety window.
    /// Zero disables maintenance.
    pub retention_blocks: u64,
    /// Run one maintenance pass every N committed heights.
    pub maintenance_interval_blocks: u64,
    /// Maximum acknowledged job/ack pairs deleted in one pass.
    pub max_rows_per_pass: usize,
}

impl AcknowledgedProofJobRetentionPolicy {
    pub fn should_maintain_at(self, height: u64) -> bool {
        height > 0
            && self.retention_blocks > 0
            && self.maintenance_interval_blocks > 0
            && self.max_rows_per_pass > 0
            && height.is_multiple_of(self.maintenance_interval_blocks)
    }

    fn target_floor(self, head_height: u64) -> Option<u64> {
        if head_height == 0 || self.retention_blocks == 0 {
            return None;
        }
        Some(
            head_height
                .saturating_sub(self.retention_blocks.saturating_sub(1))
                .max(1),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcknowledgedProofJobPruneReport {
    pub jobs_pruned: usize,
    pub oldest_retained_height: Option<u64>,
}

fn read_canonical_archive_meta(db: &Database) -> Result<CanonicalArchiveMeta, StoreError> {
    let txn = db.begin_read()?;
    let table = txn.open_table(CANONICAL_ARCHIVE_META)?;
    Ok(CanonicalArchiveMeta {
        oldest_retained_height: table
            .get(KEY_CANONICAL_ARCHIVE_OLDEST_HEIGHT)?
            .map(|value| value.value()),
        last_maintenance_height: table
            .get(KEY_LAST_CANONICAL_ARCHIVE_MAINTENANCE_HEIGHT)?
            .map(|value| value.value()),
    })
}

fn prune_canonical_archive_redb(
    db: &Database,
    head_height: u64,
    target_floor: u64,
    max_rows: usize,
) -> Result<CanonicalArchivePruneReport, StoreError> {
    let txn = db.begin_write()?;
    let mut remaining = max_rows;
    let mut replay_blocks_pruned = 0usize;
    let mut da_artifacts_pruned = 0usize;

    if remaining > 0 {
        let mut table = txn.open_table(CANONICAL_BLOCK_ARCHIVE)?;
        let mut rows = table.extract_from_if(0..target_floor, |_, _| true)?;
        while remaining > 0 {
            let Some(_) = rows.next().transpose()? else {
                break;
            };
            replay_blocks_pruned += 1;
            remaining -= 1;
        }
    }

    if remaining > 0 {
        let mut artifacts = txn.open_table(DA_ARTIFACTS)?;
        let mut manifests = txn.open_table(DA_MANIFESTS)?;
        let mut rows = artifacts.extract_from_if(0..target_floor, |_, _| true)?;
        while remaining > 0 {
            let Some((height, _)) = rows.next().transpose()? else {
                break;
            };
            manifests.remove(height.value())?;
            da_artifacts_pruned += 1;
            remaining -= 1;
        }
    }

    let oldest_retained_height = {
        let table = txn.open_table(CANONICAL_BLOCK_ARCHIVE)?;
        table
            .iter()?
            .next()
            .transpose()?
            .map(|(height, _)| height.value())
    };
    {
        let mut meta = txn.open_table(CANONICAL_ARCHIVE_META)?;
        match oldest_retained_height {
            Some(height) => {
                meta.insert(KEY_CANONICAL_ARCHIVE_OLDEST_HEIGHT, height)?;
            }
            None => {
                meta.remove(KEY_CANONICAL_ARCHIVE_OLDEST_HEIGHT)?;
            }
        }
        meta.insert(KEY_LAST_CANONICAL_ARCHIVE_MAINTENANCE_HEIGHT, head_height)?;
    }
    txn.commit()?;

    Ok(CanonicalArchivePruneReport {
        replay_blocks_pruned,
        da_artifacts_pruned,
        meta: read_canonical_archive_meta(db)?,
    })
}

fn prune_acknowledged_proof_jobs_redb(
    db: &Database,
    target_floor: u64,
    max_rows: usize,
) -> Result<AcknowledgedProofJobPruneReport, StoreError> {
    let txn = db.begin_write()?;
    let candidates = {
        let jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
        let acks = txn.open_table(PROOF_JOB_ACKS)?;
        let mut candidates = Vec::with_capacity(max_rows);
        for row in jobs.range(0..target_floor)? {
            if candidates.len() >= max_rows {
                break;
            }
            let (height, job) = row?;
            let height = height.value();
            let Some(ack) = acks.get(height)? else {
                continue;
            };
            let expected = sybil_proof_protocol::proof_job_transport_digest(job.value());
            if ack.value() != expected {
                return Err(StoreError::ProofJob(format!(
                    "ack digest does not match retained proof job at height {height}"
                )));
            }
            candidates.push(height);
        }
        candidates
    };

    if !candidates.is_empty() {
        let mut jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
        let mut acks = txn.open_table(PROOF_JOB_ACKS)?;
        for height in &candidates {
            jobs.remove(*height)?;
            acks.remove(*height)?;
        }
    }

    let oldest_retained_height = {
        let jobs = txn.open_table(PROOF_JOB_OUTBOX)?;
        jobs.iter()?
            .next()
            .transpose()?
            .map(|(height, _)| height.value())
    };
    txn.commit()?;

    Ok(AcknowledgedProofJobPruneReport {
        jobs_pruned: candidates.len(),
        oldest_retained_height,
    })
}

impl Store {
    pub fn canonical_archive_meta(&self) -> Result<CanonicalArchiveMeta, StoreError> {
        read_canonical_archive_meta(&self.db)
    }

    /// Delete old canonical replay blocks and DA artifacts outside the commit
    /// fence. A small budget may require several passes; metadata always names
    /// the oldest replay block that is actually still present.
    pub async fn prune_canonical_archive(
        &self,
        head_height: u64,
        policy: CanonicalArchiveRetentionPolicy,
    ) -> Result<CanonicalArchivePruneReport, StoreError> {
        let Some(target_floor) = policy.target_floor(head_height) else {
            return Ok(CanonicalArchivePruneReport {
                meta: self.canonical_archive_meta()?,
                ..CanonicalArchivePruneReport::default()
            });
        };
        if policy.max_rows_per_pass == 0 {
            return Ok(CanonicalArchivePruneReport {
                meta: self.canonical_archive_meta()?,
                ..CanonicalArchivePruneReport::default()
            });
        }

        self.redb_write(move |db| {
            prune_canonical_archive_redb(&db, head_height, target_floor, policy.max_rows_per_pass)
        })
        .await
    }

    /// Delete only proof jobs whose exact bytes have already been durably
    /// acknowledged by the prover and whose height is outside the configured
    /// source safety window. The job and acknowledgement are removed in the
    /// same redb transaction.
    pub async fn prune_acknowledged_proof_jobs(
        &self,
        head_height: u64,
        policy: AcknowledgedProofJobRetentionPolicy,
    ) -> Result<AcknowledgedProofJobPruneReport, StoreError> {
        let Some(target_floor) = policy.target_floor(head_height) else {
            return Ok(AcknowledgedProofJobPruneReport::default());
        };
        if policy.max_rows_per_pass == 0 {
            return Ok(AcknowledgedProofJobPruneReport::default());
        }

        self.redb_write(move |db| {
            prune_acknowledged_proof_jobs_redb(&db, target_floor, policy.max_rows_per_pass)
        })
        .await
    }
}
