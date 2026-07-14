use std::collections::BTreeMap;
use std::path::Path;

use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
    WriteTransaction,
};
use sybil_proof_protocol::{
    EpochId, EpochTransitionAccumulator, ProofEnvelope, ProofKind, StateTransitionProofJob,
    build_state_transition_guest_input, proof_job_transport_digest,
};
use uuid::Uuid;

use super::DaemonError;
use super::model::{
    ArtifactRecord, AttemptOutcome, AttemptRecord, AuditRecord, DAEMON_STORE_VERSION, DaemonStatus,
    EpochPolicy, EpochRecord, EpochState, IngestAck, JobRecord, Lease, ProofBackendKind,
};

const JOBS: TableDefinition<u64, &[u8]> = TableDefinition::new("proof_jobs");
const EPOCHS: TableDefinition<u64, &[u8]> = TableDefinition::new("epochs");
const ATTEMPTS: TableDefinition<&str, &[u8]> = TableDefinition::new("proof_attempts");
const ARTIFACTS: TableDefinition<&str, &[u8]> = TableDefinition::new("proof_artifacts");
const AUDIT: TableDefinition<u64, &[u8]> = TableDefinition::new("audit_log");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

const POLICY_KEY: &str = "epoch_policy";
const STORE_VERSION_KEY: &str = "store_version";
const AUDIT_SEQUENCE_KEY: &str = "audit_sequence";

#[derive(Debug)]
pub struct ClaimedEpoch {
    pub epoch: EpochRecord,
    pub jobs: Vec<JobRecord>,
}

pub struct DaemonStore {
    db: Database,
}

impl DaemonStore {
    pub fn open(
        path: &Path,
        target_blocks: u64,
        max_attempts: u32,
        retry_base_ms: u64,
    ) -> Result<Self, DaemonError> {
        if target_blocks == 0 || target_blocks > sybil_zk::MAX_EPOCH_BLOCKS {
            return Err(DaemonError::Config(format!(
                "epoch block target must be in 1..={}, got {target_blocks}",
                sybil_zk::MAX_EPOCH_BLOCKS
            )));
        }
        if max_attempts == 0 {
            return Err(DaemonError::Config(
                "maximum proof attempts must be non-zero".to_string(),
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::create(path)?;
        let txn = db.begin_write()?;
        txn.open_table(JOBS)?;
        txn.open_table(EPOCHS)?;
        txn.open_table(ATTEMPTS)?;
        txn.open_table(ARTIFACTS)?;
        txn.open_table(AUDIT)?;
        txn.open_table(META)?;
        {
            let mut meta = txn.open_table(META)?;
            let stored_version = meta
                .get(STORE_VERSION_KEY)?
                .map(|value| decode::<u8>(value.value()))
                .transpose()?;
            match stored_version {
                Some(actual) => {
                    if actual != DAEMON_STORE_VERSION {
                        return Err(DaemonError::Config(format!(
                            "unsupported prover store version: expected {DAEMON_STORE_VERSION}, got {actual}"
                        )));
                    }
                }
                None => {
                    let bytes = encode(&DAEMON_STORE_VERSION)?;
                    meta.insert(STORE_VERSION_KEY, bytes.as_slice())?;
                }
            }
            let mut policy = match meta.get(POLICY_KEY)? {
                Some(value) => decode::<EpochPolicy>(value.value())?,
                None => EpochPolicy::new(target_blocks, max_attempts, retry_base_ms),
            };
            // Policy changes apply only to future, not already assembled, epochs.
            policy.target_blocks = target_blocks;
            policy.max_attempts = max_attempts;
            policy.retry_base_ms = retry_base_ms;
            let bytes = encode(&policy)?;
            meta.insert(POLICY_KEY, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    pub fn ingest(&self, bytes: Vec<u8>, now_ms: u64) -> Result<IngestAck, DaemonError> {
        let job: StateTransitionProofJob = rmp_serde::from_slice(&bytes)?;
        let guest_input = build_state_transition_guest_input(job.clone())?;
        sybil_zk::verify_state_transition_input(&guest_input)?;
        let id = job.id();
        let digest = proof_job_transport_digest(&bytes);
        let bytes_len = u64::try_from(bytes.len())
            .map_err(|_| DaemonError::Config("proof job is too large".to_string()))?;

        // Verify the exact cross-block chain before making the candidate durable.
        if let Some(prior_height) = id.block_height.checked_sub(1)
            && let Some(prior) = self.read_job(prior_height)?
        {
            let prior_job: StateTransitionProofJob = rmp_serde::from_slice(&prior.bytes)?;
            let prior_input = build_state_transition_guest_input(prior_job)?;
            let mut chain = EpochTransitionAccumulator::new();
            chain.push(&prior_input)?;
            chain.push(&guest_input)?;
        }

        let txn = self.db.begin_write()?;
        let mut duplicate = false;
        {
            let mut jobs = txn.open_table(JOBS)?;
            if let Some(existing) = jobs.get(id.block_height)? {
                let existing: JobRecord = decode(existing.value())?;
                if existing.transport_digest != digest || existing.bytes != bytes {
                    return Err(DaemonError::Conflict(format!(
                        "different proof job already committed at height {}",
                        id.block_height
                    )));
                }
                duplicate = true;
            } else {
                let mut policy = read_policy(&txn)?;
                if let Some(frontier) = policy.ingested_frontier {
                    let expected = frontier.checked_add(1).ok_or_else(|| {
                        DaemonError::Conflict("ingested frontier overflow".to_string())
                    })?;
                    if id.block_height != expected {
                        return Err(DaemonError::Gap {
                            expected,
                            actual: id.block_height,
                        });
                    }
                }
                let record = JobRecord {
                    format_version: DAEMON_STORE_VERSION,
                    id,
                    transport_digest: digest,
                    bytes_len,
                    received_at_ms: now_ms,
                    bytes,
                };
                let encoded = encode(&record)?;
                jobs.insert(id.block_height, encoded.as_slice())?;
                policy.ingested_frontier = Some(id.block_height);
                policy.next_epoch_start.get_or_insert(id.block_height);
                write_policy(&txn, &policy)?;
            }
        }
        append_audit(
            &txn,
            now_ms,
            "sequencer",
            if duplicate {
                "job_duplicate"
            } else {
                "job_ingested"
            },
            &format!(
                "height={} digest=0x{}",
                id.block_height,
                hex::encode(digest)
            ),
        )?;
        txn.commit()?;
        Ok(IngestAck {
            height: id.block_height,
            transport_digest: format!("0x{}", hex::encode(digest)),
            durable: true,
            duplicate,
        })
    }

    pub fn assemble_next(
        &self,
        proof_kind: ProofKind,
        force_partial: bool,
        now_ms: u64,
        actor: &str,
    ) -> Result<Option<EpochRecord>, DaemonError> {
        let policy = self.policy()?;
        let Some(first_height) = policy.next_epoch_start else {
            return Ok(None);
        };
        let available = policy
            .ingested_frontier
            .and_then(|frontier| frontier.checked_sub(first_height))
            .and_then(|delta| delta.checked_add(1))
            .unwrap_or(0);
        if available == 0 || (!force_partial && available < policy.target_blocks) {
            return Ok(None);
        }
        let count = available.min(policy.target_blocks);
        let last_height = first_height
            .checked_add(count - 1)
            .ok_or_else(|| DaemonError::Conflict("epoch height overflow".to_string()))?;

        let mut jobs = Vec::with_capacity(count as usize);
        let mut inputs = Vec::with_capacity(count as usize);
        let mut accumulator = EpochTransitionAccumulator::new();
        for height in first_height..=last_height {
            let record = self.read_job_required(height)?;
            let job: StateTransitionProofJob = rmp_serde::from_slice(&record.bytes)?;
            let input = build_state_transition_guest_input(job)?;
            accumulator.push(&input)?;
            jobs.push(record);
            inputs.push(input);
        }
        let public_inputs = accumulator.finish()?;
        let epoch_id = EpochId::from_public_inputs(&public_inputs);
        let record = EpochRecord {
            format_version: DAEMON_STORE_VERSION,
            first_block_height: first_height,
            last_block_height: last_height,
            job_heights: jobs.iter().map(|job| job.id.block_height).collect(),
            job_transport_digests: jobs.iter().map(|job| job.transport_digest).collect(),
            epoch_id,
            public_inputs,
            proof_kind,
            state: EpochState::Ready,
            attempt_count: 0,
            manual_seal: force_partial && count < policy.target_blocks,
            assembled_at_ms: now_ms,
            updated_at_ms: now_ms,
            last_error: None,
            artifact: None,
        };

        let txn = self.db.begin_write()?;
        {
            let current = read_policy(&txn)?;
            if current.next_epoch_start != Some(first_height) {
                return Ok(None);
            }
            let mut epochs = txn.open_table(EPOCHS)?;
            if let Some(existing) = epochs.get(first_height)? {
                let existing: EpochRecord = decode(existing.value())?;
                if existing.epoch_id != epoch_id {
                    return Err(DaemonError::Conflict(format!(
                        "different epoch already assembled at block {first_height}"
                    )));
                }
                return Ok(Some(existing));
            }
            let bytes = encode(&record)?;
            epochs.insert(first_height, bytes.as_slice())?;
            let mut updated = current;
            updated.assembled_frontier = Some(last_height);
            updated.next_epoch_start = last_height.checked_add(1);
            write_policy(&txn, &updated)?;
        }
        append_audit(
            &txn,
            now_ms,
            actor,
            if record.manual_seal {
                "epoch_partial_seal"
            } else {
                "epoch_assembled"
            },
            &format!(
                "first_block={} last_block={} epoch=0x{}",
                first_height,
                last_height,
                hex::encode(epoch_id.0)
            ),
        )?;
        txn.commit()?;
        drop(inputs);
        Ok(Some(record))
    }

    pub fn claim_next(
        &self,
        owner: Uuid,
        lease_ms: u64,
        now_ms: u64,
    ) -> Result<Option<ClaimedEpoch>, DaemonError> {
        // Only the first non-proven epoch may run. A poisoned, leased, or
        // backoff-delayed epoch is a hard contiguous-frontier barrier.
        let candidate = self
            .list_epochs()?
            .into_iter()
            .find_map(|epoch| match epoch.state {
                EpochState::Proven => None,
                EpochState::Ready => Some(Some(epoch)),
                EpochState::RetryWait { retry_at_ms } if retry_at_ms <= now_ms => Some(Some(epoch)),
                _ => Some(None),
            })
            .flatten();
        let Some(mut epoch) = candidate else {
            return Ok(None);
        };
        let policy = self.policy()?;
        epoch.attempt_count = epoch.attempt_count.saturating_add(1);
        let lease = Lease {
            owner,
            attempt: epoch.attempt_count,
            acquired_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(lease_ms),
        };
        epoch.state = EpochState::Proving {
            lease: lease.clone(),
        };
        epoch.updated_at_ms = now_ms;
        epoch.last_error = None;
        let attempt = AttemptRecord {
            format_version: DAEMON_STORE_VERSION,
            first_block_height: epoch.first_block_height,
            epoch_id: epoch.epoch_id,
            proof_kind: epoch.proof_kind,
            owner,
            attempt: epoch.attempt_count,
            started_at_ms: now_ms,
            finished_at_ms: None,
            outcome: AttemptOutcome::Running,
            error: None,
        };

        let txn = self.db.begin_write()?;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            let current: EpochRecord = match epochs.get(epoch.first_block_height)? {
                Some(value) => decode(value.value())?,
                None => {
                    return Err(DaemonError::Conflict(
                        "claimed epoch disappeared".to_string(),
                    ));
                }
            };
            if !matches!(
                current.state,
                EpochState::Ready | EpochState::RetryWait { .. }
            ) {
                return Ok(None);
            }
            // A manual retry moves a terminal epoch back to Ready without
            // erasing its monotonic attempt number or durable history.
            if current.attempt_count >= policy.max_attempts
                && !matches!(current.state, EpochState::Ready)
            {
                return Ok(None);
            }
            let bytes = encode(&epoch)?;
            epochs.insert(epoch.first_block_height, bytes.as_slice())?;
        }
        {
            let mut attempts = txn.open_table(ATTEMPTS)?;
            let key = attempt_key(epoch.first_block_height, epoch.attempt_count);
            let bytes = encode(&attempt)?;
            attempts.insert(key.as_str(), bytes.as_slice())?;
        }
        append_audit(
            &txn,
            now_ms,
            &owner.to_string(),
            "proof_claimed",
            &format!(
                "first_block={} attempt={}",
                epoch.first_block_height, epoch.attempt_count
            ),
        )?;
        txn.commit()?;

        let jobs = epoch
            .job_heights
            .iter()
            .map(|height| self.read_job_required(*height))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ClaimedEpoch { epoch, jobs }))
    }

    pub fn renew_lease(
        &self,
        first_height: u64,
        owner: Uuid,
        attempt: u32,
        lease_ms: u64,
        now_ms: u64,
    ) -> Result<(), DaemonError> {
        let txn = self.db.begin_write()?;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            let mut epoch: EpochRecord = match epochs.get(first_height)? {
                Some(value) => decode(value.value())?,
                None => {
                    return Err(DaemonError::Conflict(
                        "leased epoch disappeared".to_string(),
                    ));
                }
            };
            match &mut epoch.state {
                EpochState::Proving { lease }
                    if lease.owner == owner && lease.attempt == attempt =>
                {
                    lease.expires_at_ms = now_ms.saturating_add(lease_ms);
                    epoch.updated_at_ms = now_ms;
                }
                _ => {
                    return Err(DaemonError::LeaseLost {
                        first_height,
                        attempt,
                    });
                }
            }
            let bytes = encode(&epoch)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn complete_attempt(
        &self,
        first_height: u64,
        owner: Uuid,
        attempt: u32,
        artifact: ArtifactRecord,
        now_ms: u64,
    ) -> Result<EpochRecord, DaemonError> {
        artifact.envelope.validate()?;
        let txn = self.db.begin_write()?;
        let mut completed;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            completed = match epochs.get(first_height)? {
                Some(value) => decode::<EpochRecord>(value.value())?,
                None => {
                    return Err(DaemonError::Conflict(
                        "completed epoch disappeared".to_string(),
                    ));
                }
            };
            require_lease(&completed, owner, attempt)?;
            if completed.epoch_id != artifact.envelope.epoch_id
                || completed.public_inputs != artifact.envelope.public_inputs
                || completed.proof_kind != artifact.envelope.proof_kind
            {
                return Err(DaemonError::Conflict(
                    "proof artifact does not match claimed epoch".to_string(),
                ));
            }
            completed.state = EpochState::Proven;
            completed.updated_at_ms = now_ms;
            completed.last_error = None;
            completed.artifact = Some(artifact.clone());
            let bytes = encode(&completed)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        {
            let mut artifacts = txn.open_table(ARTIFACTS)?;
            let key = artifact_key(completed.epoch_id, completed.proof_kind);
            let bytes = encode(&artifact)?;
            artifacts.insert(key.as_str(), bytes.as_slice())?;
        }
        finish_attempt_record(
            &txn,
            first_height,
            attempt,
            now_ms,
            AttemptOutcome::Proven,
            None,
        )?;
        let mut policy = read_policy(&txn)?;
        if policy.proven_frontier.is_none()
            || policy
                .proven_frontier
                .and_then(|height| height.checked_add(1))
                == Some(first_height)
        {
            policy.proven_frontier = Some(completed.last_block_height);
            write_policy(&txn, &policy)?;
        }
        append_audit(
            &txn,
            now_ms,
            &owner.to_string(),
            "proof_proven",
            &format!(
                "first_block={} attempt={} epoch=0x{}",
                first_height,
                attempt,
                hex::encode(completed.epoch_id.0)
            ),
        )?;
        txn.commit()?;
        Ok(completed)
    }

    pub fn fail_attempt(
        &self,
        first_height: u64,
        owner: Uuid,
        attempt: u32,
        error: &str,
        permanent: bool,
        now_ms: u64,
    ) -> Result<EpochRecord, DaemonError> {
        let txn = self.db.begin_write()?;
        let policy = read_policy(&txn)?;
        let mut failed;
        let mut outcome = AttemptOutcome::RetryableFailure;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            failed = match epochs.get(first_height)? {
                Some(value) => decode::<EpochRecord>(value.value())?,
                None => {
                    return Err(DaemonError::Conflict(
                        "failed epoch disappeared".to_string(),
                    ));
                }
            };
            require_lease(&failed, owner, attempt)?;
            let exhausted = failed.attempt_count >= policy.max_attempts;
            if permanent || exhausted {
                failed.state = EpochState::FailedPermanent;
                outcome = AttemptOutcome::PermanentFailure;
            } else {
                failed.state = EpochState::RetryWait {
                    retry_at_ms: now_ms.saturating_add(retry_delay_ms(
                        policy.retry_base_ms,
                        failed.epoch_id,
                        failed.attempt_count,
                    )),
                };
            }
            failed.updated_at_ms = now_ms;
            failed.last_error = Some(bounded_error(error));
            let bytes = encode(&failed)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        finish_attempt_record(&txn, first_height, attempt, now_ms, outcome, Some(error))?;
        append_audit(
            &txn,
            now_ms,
            &owner.to_string(),
            if outcome == AttemptOutcome::PermanentFailure {
                "proof_failed_permanent"
            } else {
                "proof_retry_scheduled"
            },
            &format!(
                "first_block={first_height} attempt={attempt} error={}",
                bounded_error(error)
            ),
        )?;
        txn.commit()?;
        Ok(failed)
    }

    pub fn recover_expired(&self, now_ms: u64) -> Result<u64, DaemonError> {
        let policy = self.policy()?;
        let expired = self
            .list_epochs()?
            .into_iter()
            .filter_map(|epoch| {
                let lease = match &epoch.state {
                    EpochState::Proving { lease } if lease.expires_at_ms <= now_ms => lease.clone(),
                    _ => return None,
                };
                Some((epoch, lease))
            })
            .collect::<Vec<_>>();
        let mut recovered = 0;
        for (mut epoch, lease) in expired {
            let txn = self.db.begin_write()?;
            {
                let mut epochs = txn.open_table(EPOCHS)?;
                let current: EpochRecord = match epochs.get(epoch.first_block_height)? {
                    Some(value) => decode(value.value())?,
                    None => continue,
                };
                let still_expired = matches!(
                    &current.state,
                    EpochState::Proving { lease: current_lease }
                        if current_lease.owner == lease.owner
                            && current_lease.attempt == lease.attempt
                            && current_lease.expires_at_ms <= now_ms
                );
                if !still_expired {
                    continue;
                }
                let exhausted = current.attempt_count >= policy.max_attempts;
                epoch.state = if exhausted {
                    EpochState::FailedPermanent
                } else {
                    EpochState::RetryWait {
                        retry_at_ms: now_ms,
                    }
                };
                epoch.updated_at_ms = now_ms;
                epoch.last_error = Some(if exhausted {
                    "recovered expired proof lease; automatic attempt limit reached".to_string()
                } else {
                    "recovered expired proof lease".to_string()
                });
                let bytes = encode(&epoch)?;
                epochs.insert(epoch.first_block_height, bytes.as_slice())?;
            }
            finish_attempt_record(
                &txn,
                epoch.first_block_height,
                lease.attempt,
                now_ms,
                AttemptOutcome::RecoveredExpired,
                Some("daemon recovered an expired proof lease"),
            )?;
            append_audit(
                &txn,
                now_ms,
                "recovery",
                if epoch.state == EpochState::FailedPermanent {
                    "lease_expired_permanent"
                } else {
                    "lease_expired"
                },
                &format!(
                    "first_block={} attempt={} owner={}",
                    epoch.first_block_height, lease.attempt, lease.owner
                ),
            )?;
            txn.commit()?;
            recovered += 1;
        }
        Ok(recovered)
    }

    pub fn adopt_artifact(
        &self,
        first_height: u64,
        artifact: ArtifactRecord,
        now_ms: u64,
    ) -> Result<(), DaemonError> {
        let txn = self.db.begin_write()?;
        let mut adopted;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            adopted = match epochs.get(first_height)? {
                Some(value) => decode::<EpochRecord>(value.value())?,
                None => {
                    return Err(DaemonError::Conflict(
                        "orphan epoch disappeared".to_string(),
                    ));
                }
            };
            if adopted.epoch_id != artifact.envelope.epoch_id
                || adopted.public_inputs != artifact.envelope.public_inputs
                || adopted.proof_kind != artifact.envelope.proof_kind
            {
                return Err(DaemonError::Conflict(
                    "orphan artifact does not match epoch".to_string(),
                ));
            }
            adopted.state = EpochState::Proven;
            adopted.updated_at_ms = now_ms;
            adopted.last_error = None;
            adopted.artifact = Some(artifact.clone());
            let bytes = encode(&adopted)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        {
            let mut artifacts = txn.open_table(ARTIFACTS)?;
            let key = artifact_key(adopted.epoch_id, adopted.proof_kind);
            let bytes = encode(&artifact)?;
            artifacts.insert(key.as_str(), bytes.as_slice())?;
        }
        let mut policy = read_policy(&txn)?;
        if policy.proven_frontier.is_none()
            || policy
                .proven_frontier
                .and_then(|height| height.checked_add(1))
                == Some(first_height)
        {
            policy.proven_frontier = Some(adopted.last_block_height);
            write_policy(&txn, &policy)?;
        }
        append_audit(
            &txn,
            now_ms,
            "recovery",
            "artifact_adopted",
            &format!("first_block={first_height}"),
        )?;
        txn.commit()?;
        Ok(())
    }

    pub fn invalidate_artifact(
        &self,
        first_height: u64,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), DaemonError> {
        let txn = self.db.begin_write()?;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            let mut epoch: EpochRecord = match epochs.get(first_height)? {
                Some(value) => decode(value.value())?,
                None => {
                    return Err(DaemonError::Conflict("epoch disappeared".to_string()));
                }
            };
            epoch.state = EpochState::RetryWait {
                retry_at_ms: now_ms,
            };
            epoch.updated_at_ms = now_ms;
            epoch.last_error = Some(bounded_error(reason));
            epoch.artifact = None;
            let bytes = encode(&epoch)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        append_audit(
            &txn,
            now_ms,
            "recovery",
            "artifact_invalid",
            &format!(
                "first_block={first_height} reason={}",
                bounded_error(reason)
            ),
        )?;
        txn.commit()?;
        Ok(())
    }

    pub fn manual_retry(
        &self,
        first_height: u64,
        actor: &str,
        now_ms: u64,
    ) -> Result<EpochRecord, DaemonError> {
        let txn = self.db.begin_write()?;
        let mut epoch;
        {
            let mut epochs = txn.open_table(EPOCHS)?;
            epoch = match epochs.get(first_height)? {
                Some(value) => decode::<EpochRecord>(value.value())?,
                None => return Err(DaemonError::NotFound(format!("epoch {first_height}"))),
            };
            if matches!(epoch.state, EpochState::Proving { .. } | EpochState::Proven) {
                return Err(DaemonError::Conflict(format!(
                    "epoch {first_height} cannot be retried from state {}",
                    epoch.state.label()
                )));
            }
            epoch.state = EpochState::Ready;
            epoch.updated_at_ms = now_ms;
            epoch.last_error = None;
            let bytes = encode(&epoch)?;
            epochs.insert(first_height, bytes.as_slice())?;
        }
        append_audit(
            &txn,
            now_ms,
            actor,
            "manual_retry",
            &format!("first_block={first_height}"),
        )?;
        txn.commit()?;
        Ok(epoch)
    }

    pub fn read_epoch(&self, first_height: u64) -> Result<Option<EpochRecord>, DaemonError> {
        let txn = self.db.begin_read()?;
        let epochs = txn.open_table(EPOCHS)?;
        epochs
            .get(first_height)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    pub fn list_epochs(&self) -> Result<Vec<EpochRecord>, DaemonError> {
        let txn = self.db.begin_read()?;
        let epochs = txn.open_table(EPOCHS)?;
        let mut records = Vec::new();
        for row in epochs.iter()? {
            let (_, value) = row?;
            records.push(decode(value.value())?);
        }
        Ok(records)
    }

    pub fn policy(&self) -> Result<EpochPolicy, DaemonError> {
        let txn = self.db.begin_read()?;
        let meta = txn.open_table(META)?;
        let value = meta
            .get(POLICY_KEY)?
            .ok_or_else(|| DaemonError::Config("prover epoch policy is missing".to_string()))?;
        decode(value.value())
    }

    pub fn status(
        &self,
        owner: Uuid,
        backend: ProofBackendKind,
        ready: bool,
    ) -> Result<DaemonStatus, DaemonError> {
        let txn = self.db.begin_read()?;
        let jobs = txn.open_table(JOBS)?;
        let epochs = txn.open_table(EPOCHS)?;
        let mut bytes = 0u64;
        for row in jobs.iter()? {
            let (_, value) = row?;
            bytes = bytes.saturating_add(decode::<JobRecord>(value.value())?.bytes_len);
        }
        let mut epoch_states = BTreeMap::new();
        for row in epochs.iter()? {
            let (_, value) = row?;
            let epoch: EpochRecord = decode(value.value())?;
            *epoch_states
                .entry(epoch.state.label().to_string())
                .or_insert(0) += 1;
        }
        Ok(DaemonStatus {
            ready,
            owner,
            backend,
            policy: self.policy()?,
            jobs: jobs.len()?,
            epochs: epochs.len()?,
            epoch_states,
            queued_job_bytes: bytes,
        })
    }

    fn read_job(&self, height: u64) -> Result<Option<JobRecord>, DaemonError> {
        let txn = self.db.begin_read()?;
        let jobs = txn.open_table(JOBS)?;
        jobs.get(height)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    fn read_job_required(&self, height: u64) -> Result<JobRecord, DaemonError> {
        self.read_job(height)?.ok_or_else(|| DaemonError::Gap {
            expected: height,
            actual: height.saturating_add(1),
        })
    }
}

fn require_lease(epoch: &EpochRecord, owner: Uuid, attempt: u32) -> Result<(), DaemonError> {
    match &epoch.state {
        EpochState::Proving { lease } if lease.owner == owner && lease.attempt == attempt => Ok(()),
        _ => Err(DaemonError::LeaseLost {
            first_height: epoch.first_block_height,
            attempt,
        }),
    }
}

fn retry_delay_ms(base_ms: u64, epoch_id: EpochId, attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(10);
    let scaled = base_ms.saturating_mul(1u64 << exponent);
    let jitter_cap = base_ms / 4;
    let mut jitter_bytes = [0u8; 8];
    jitter_bytes.copy_from_slice(&epoch_id.0[..8]);
    let jitter_seed = u64::from_le_bytes(jitter_bytes) ^ u64::from(attempt);
    let jitter = if jitter_cap == 0 {
        0
    } else {
        jitter_seed % jitter_cap
    };
    scaled.saturating_add(jitter)
}

fn bounded_error(error: &str) -> String {
    const MAX_ERROR_BYTES: usize = 2_048;
    if error.len() <= MAX_ERROR_BYTES {
        return error.to_string();
    }
    let mut end = MAX_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &error[..end])
}

fn finish_attempt_record(
    txn: &WriteTransaction,
    first_height: u64,
    attempt: u32,
    now_ms: u64,
    outcome: AttemptOutcome,
    error: Option<&str>,
) -> Result<(), DaemonError> {
    let mut attempts = txn.open_table(ATTEMPTS)?;
    let key = attempt_key(first_height, attempt);
    let mut record: AttemptRecord = match attempts.get(key.as_str())? {
        Some(value) => decode(value.value())?,
        None => {
            return Err(DaemonError::Conflict(format!(
                "attempt record {key} is missing"
            )));
        }
    };
    record.finished_at_ms = Some(now_ms);
    record.outcome = outcome;
    record.error = error.map(bounded_error);
    let bytes = encode(&record)?;
    attempts.insert(key.as_str(), bytes.as_slice())?;
    Ok(())
}

fn read_policy(txn: &WriteTransaction) -> Result<EpochPolicy, DaemonError> {
    let meta = txn.open_table(META)?;
    let value = meta
        .get(POLICY_KEY)?
        .ok_or_else(|| DaemonError::Config("prover epoch policy is missing".to_string()))?;
    decode(value.value())
}

fn write_policy(txn: &WriteTransaction, policy: &EpochPolicy) -> Result<(), DaemonError> {
    let mut meta = txn.open_table(META)?;
    let bytes = encode(policy)?;
    meta.insert(POLICY_KEY, bytes.as_slice())?;
    Ok(())
}

fn append_audit(
    txn: &WriteTransaction,
    at_ms: u64,
    actor: &str,
    action: &str,
    detail: &str,
) -> Result<(), DaemonError> {
    let sequence = {
        let mut meta = txn.open_table(META)?;
        let current = meta
            .get(AUDIT_SEQUENCE_KEY)?
            .map(|value| decode::<u64>(value.value()))
            .transpose()?
            .unwrap_or(0);
        let next = current
            .checked_add(1)
            .ok_or_else(|| DaemonError::Conflict("audit sequence overflow".to_string()))?;
        let bytes = encode(&next)?;
        meta.insert(AUDIT_SEQUENCE_KEY, bytes.as_slice())?;
        next
    };
    let record = AuditRecord {
        format_version: DAEMON_STORE_VERSION,
        sequence,
        at_ms,
        actor: actor.to_string(),
        action: action.to_string(),
        detail: bounded_error(detail),
    };
    let mut audit = txn.open_table(AUDIT)?;
    let bytes = encode(&record)?;
    audit.insert(sequence, bytes.as_slice())?;
    Ok(())
}

fn attempt_key(first_height: u64, attempt: u32) -> String {
    format!("{first_height:020}:{attempt:010}")
}

fn artifact_key(epoch_id: EpochId, kind: ProofKind) -> String {
    format!("{}:{kind:?}", hex::encode(epoch_id.0))
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, DaemonError> {
    Ok(rmp_serde::to_vec_named(value)?)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, DaemonError> {
    Ok(rmp_serde::from_slice(bytes)?)
}

pub fn envelope_digest(envelope: &ProofEnvelope) -> Result<[u8; 32], DaemonError> {
    let bytes = encode(envelope)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/proof-envelope-artifact/v1");
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(&bytes);
    Ok(*hasher.finalize().as_bytes())
}
