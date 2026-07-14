use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sybil_proof_protocol::{ProofEnvelope, ProofKind};
use uuid::Uuid;

use super::DaemonError;
use super::model::{ArtifactRecord, EpochRecord};
use super::store::envelope_digest;

const ENVELOPE_FILE: &str = "envelope.msgpack";
const PAYLOAD_FILE: &str = "proof.bin";

pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn open(root: PathBuf) -> Result<Self, DaemonError> {
        std::fs::create_dir_all(root.join("epochs"))?;
        std::fs::create_dir_all(root.join(".tmp"))?;
        std::fs::create_dir_all(root.join(".quarantine"))?;
        Ok(Self { root })
    }

    pub fn publish(
        &self,
        envelope: &ProofEnvelope,
        payload: &[u8],
        owner: Uuid,
        attempt: u32,
        published_at_ms: u64,
    ) -> Result<ArtifactRecord, DaemonError> {
        envelope.validate_payload(payload)?;
        let relative_dir = final_relative_dir(envelope);
        let final_dir = self.root.join(&relative_dir);
        if final_dir.exists() {
            return self.validate_dir(&relative_dir, Some(envelope));
        }

        let tmp_name = format!(
            "{}-{}-{attempt}-{}",
            hex::encode(envelope.epoch_id.0),
            proof_kind_label(envelope.proof_kind),
            owner
        );
        let tmp_dir = self.root.join(".tmp").join(tmp_name);
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir)?;
        }
        std::fs::create_dir(&tmp_dir)?;

        let envelope_bytes = rmp_serde::to_vec_named(envelope)?;
        write_sync(&tmp_dir.join(ENVELOPE_FILE), &envelope_bytes)?;
        write_sync(&tmp_dir.join(PAYLOAD_FILE), payload)?;
        sync_dir(&tmp_dir)?;

        let parent = final_dir
            .parent()
            .ok_or_else(|| DaemonError::Artifact("artifact path has no parent".to_string()))?;
        std::fs::create_dir_all(parent)?;
        sync_dir(parent)?;
        match std::fs::rename(&tmp_dir, &final_dir) {
            Ok(()) => sync_dir(parent)?,
            Err(_error) if final_dir.exists() => {
                std::fs::remove_dir_all(&tmp_dir)?;
                let existing = self.validate_dir(&relative_dir, Some(envelope))?;
                return Ok(existing);
            }
            Err(error) => return Err(error.into()),
        }

        let mut record = self.validate_dir(&relative_dir, Some(envelope))?;
        record.published_at_ms = published_at_ms;
        Ok(record)
    }

    pub fn validate(&self, record: &ArtifactRecord) -> Result<(), DaemonError> {
        let actual = self.validate_dir(&record.relative_dir, Some(&record.envelope))?;
        if actual.envelope_digest != record.envelope_digest {
            return Err(DaemonError::Artifact(
                "persisted envelope digest differs from artifact".to_string(),
            ));
        }
        Ok(())
    }

    pub fn find_valid(&self, epoch: &EpochRecord) -> Result<Option<ArtifactRecord>, DaemonError> {
        let kind_dir = self
            .root
            .join("epochs")
            .join(hex::encode(epoch.epoch_id.0))
            .join(proof_kind_label(epoch.proof_kind));
        let entries = match std::fs::read_dir(&kind_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut candidates = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        candidates.sort();
        for candidate in candidates {
            let Ok(relative) = candidate.strip_prefix(&self.root) else {
                continue;
            };
            let record = match self.validate_dir(relative, None) {
                Ok(record) => record,
                Err(_) => {
                    self.quarantine_path(&candidate, "invalid-final")?;
                    continue;
                }
            };
            if record.envelope.epoch_id == epoch.epoch_id
                && record.envelope.public_inputs == epoch.public_inputs
                && record.envelope.proof_kind == epoch.proof_kind
            {
                return Ok(Some(record));
            }
            self.quarantine_path(&candidate, "mismatched-final")?;
        }
        Ok(None)
    }

    pub fn quarantine_temporary(&self) -> Result<u64, DaemonError> {
        let tmp = self.root.join(".tmp");
        let mut quarantined = 0u64;
        for entry in std::fs::read_dir(&tmp)? {
            let entry = entry?;
            self.quarantine_path(&entry.path(), "interrupted-attempt")?;
            quarantined += 1;
        }
        Ok(quarantined)
    }

    fn validate_dir(
        &self,
        relative_dir: &Path,
        expected: Option<&ProofEnvelope>,
    ) -> Result<ArtifactRecord, DaemonError> {
        let dir = self.root.join(relative_dir);
        let envelope_bytes = std::fs::read(dir.join(ENVELOPE_FILE))?;
        let envelope: ProofEnvelope = rmp_serde::from_slice(&envelope_bytes)?;
        let payload = std::fs::read(dir.join(PAYLOAD_FILE))?;
        envelope.validate_payload(&payload)?;
        if expected.is_some_and(|expected| !same_artifact(expected, &envelope)) {
            return Err(DaemonError::Artifact(
                "published envelope differs from expected envelope".to_string(),
            ));
        }
        if final_relative_dir(&envelope) != relative_dir {
            return Err(DaemonError::Artifact(
                "artifact directory is not content-addressed by its envelope".to_string(),
            ));
        }
        Ok(ArtifactRecord {
            relative_dir: relative_dir.to_path_buf(),
            envelope_digest: envelope_digest(&envelope)?,
            published_at_ms: envelope.created_at_ms,
            envelope,
        })
    }

    fn quarantine_path(&self, path: &Path, reason: &str) -> Result<(), DaemonError> {
        if !path.exists() {
            return Ok(());
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact");
        let destination = self
            .root
            .join(".quarantine")
            .join(format!("{reason}-{name}-{}", Uuid::new_v4()));
        std::fs::rename(path, destination)?;
        Ok(())
    }
}

/// Wall-clock creation time is descriptive, not identity. Retrying the
/// deterministic mock backend may reproduce the exact payload later; every
/// trust-bearing field still has to match before the old artifact is reused.
fn same_artifact(expected: &ProofEnvelope, actual: &ProofEnvelope) -> bool {
    expected.format_version == actual.format_version
        && expected.proof_kind == actual.proof_kind
        && expected.epoch_id == actual.epoch_id
        && expected.public_inputs == actual.public_inputs
        && expected.public_input_hash == actual.public_input_hash
        && expected.app_exe_commit == actual.app_exe_commit
        && expected.app_vm_commit == actual.app_vm_commit
        && expected.payload_digest == actual.payload_digest
        && expected.payload_len == actual.payload_len
}

fn final_relative_dir(envelope: &ProofEnvelope) -> PathBuf {
    PathBuf::from("epochs")
        .join(hex::encode(envelope.epoch_id.0))
        .join(proof_kind_label(envelope.proof_kind))
        .join(hex::encode(envelope.payload_digest))
}

pub const fn proof_kind_label(kind: ProofKind) -> &'static str {
    match kind {
        ProofKind::Mock => "mock",
        ProofKind::OpenVmStark => "openvm-stark",
        ProofKind::OpenVmEvm => "openvm-evm",
    }
}

fn write_sync(path: &Path, bytes: &[u8]) -> Result<(), DaemonError> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn sync_dir(path: &Path) -> Result<(), DaemonError> {
    File::open(path)?.sync_all()?;
    Ok(())
}
