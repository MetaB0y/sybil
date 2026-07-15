use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sybil_l1_protocol::Bytes32;

use super::{IndexerError, Result};

const CURSOR_SCHEMA_VERSION: u32 = 2;

/// Canonical hash of the last fully processed L1 block.
///
/// That header commits the whole ancestor chain, so validating this one hash
/// before the next poll detects a rewrite anywhere in the processed prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BlockCheckpoint {
    pub(super) block_number: u64,
    pub(super) block_hash_hex: String,
}

impl BlockCheckpoint {
    pub(super) fn new(block_number: u64, block_hash: Bytes32) -> Self {
        Self {
            block_number,
            block_hash_hex: hex::encode(block_hash),
        }
    }

    pub(super) fn block_hash(&self, path: &Path) -> Result<Bytes32> {
        let bytes = hex::decode(&self.block_hash_hex).map_err(|error| {
            IndexerError::CursorCheckpointInvalid {
                path: path.display().to_string(),
                message: format!("checkpoint block hash is not hex: {error}"),
            }
        })?;
        bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| IndexerError::CursorCheckpointInvalid {
                path: path.display().to_string(),
                message: format!(
                    "checkpoint block hash must be 32 bytes, got {}",
                    bytes.len()
                ),
            })
    }
}

/// Durable evidence that a processed L1 prefix no longer matches the RPC's
/// canonical chain. Restarts refuse this state until operator recovery replaces
/// the whole deployment-bound cursor after preserving the incident artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReorgIncident {
    pub(super) context: String,
    pub(super) block_number: u64,
    pub(super) expected_hash_hex: String,
    pub(super) observed_hash_hex: String,
}

impl ReorgIncident {
    pub(super) fn new(
        context: &'static str,
        block_number: u64,
        expected: Bytes32,
        observed: Bytes32,
    ) -> Self {
        Self {
            context: context.to_string(),
            block_number,
            expected_hash_hex: hex::encode(expected),
            observed_hash_hex: hex::encode(observed),
        }
    }
}

/// Persisted scan cursor, canonical checkpoint, and optional fail-stop latch,
/// bound to one vault deployment and chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CursorState {
    #[serde(default)]
    schema_version: u32,
    pub(super) next_from: u64,
    vault_address_hex: String,
    chain_id: u64,
    #[serde(default)]
    pub(super) checkpoint: Option<BlockCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reorg_incident: Option<ReorgIncident>,
}

impl CursorState {
    pub(super) fn active(
        next_from: u64,
        vault_address_hex: &str,
        chain_id: u64,
        checkpoint: BlockCheckpoint,
    ) -> Self {
        Self {
            schema_version: CURSOR_SCHEMA_VERSION,
            next_from,
            vault_address_hex: vault_address_hex.to_string(),
            chain_id,
            checkpoint: Some(checkpoint),
            reorg_incident: None,
        }
    }

    pub(super) fn halted(
        next_from: u64,
        vault_address_hex: &str,
        chain_id: u64,
        checkpoint: Option<BlockCheckpoint>,
        incident: ReorgIncident,
    ) -> Self {
        Self {
            schema_version: CURSOR_SCHEMA_VERSION,
            next_from,
            vault_address_hex: vault_address_hex.to_string(),
            chain_id,
            checkpoint,
            reorg_incident: Some(incident),
        }
    }
}

/// Load the persisted scan cursor, or `None` if no file exists. Fails closed on
/// a different deployment, unsupported legacy schema, invalid checkpoint, or
/// previously latched reorg incident.
pub(super) fn load_cursor(
    path: &Path,
    vault_hex: &str,
    chain_id: u64,
) -> Result<Option<CursorState>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let state: CursorState = serde_json::from_str(&data)?;
    if state.schema_version != CURSOR_SCHEMA_VERSION {
        return Err(IndexerError::CursorSchemaMismatch {
            path: path.display().to_string(),
            stored: state.schema_version,
            expected: CURSOR_SCHEMA_VERSION,
        });
    }
    if state.vault_address_hex != vault_hex || state.chain_id != chain_id {
        return Err(IndexerError::CursorConfigMismatch {
            path: path.display().to_string(),
            stored_vault: state.vault_address_hex,
            stored_chain: state.chain_id,
            arg_vault: vault_hex.to_string(),
            arg_chain: chain_id,
        });
    }
    if let Some(incident) = state.reorg_incident.as_ref() {
        return Err(IndexerError::ReorgIncidentLatched {
            path: path.display().to_string(),
            context: incident.context.clone(),
            block_number: incident.block_number,
            expected: incident.expected_hash_hex.clone(),
            observed: incident.observed_hash_hex.clone(),
        });
    }
    let checkpoint =
        state
            .checkpoint
            .as_ref()
            .ok_or_else(|| IndexerError::CursorCheckpointInvalid {
                path: path.display().to_string(),
                message: "active cursor is missing its canonical block checkpoint".to_string(),
            })?;
    checkpoint.block_hash(path)?;
    if checkpoint.block_number.saturating_add(1) != state.next_from {
        return Err(IndexerError::CursorCheckpointInvalid {
            path: path.display().to_string(),
            message: format!(
                "checkpoint block {} does not precede next_from {}",
                checkpoint.block_number, state.next_from
            ),
        });
    }
    Ok(Some(state))
}

/// Persist the scan state with a synced temp file, atomic rename, and parent
/// directory sync so a crash cannot acknowledge a range while losing its
/// checkpoint update.
pub(super) fn save_cursor(path: &Path, state: &CursorState) -> Result<()> {
    let data = serde_json::to_string_pretty(state)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = PathBuf::from(tmp_name);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp_path)?;
    file.write_all(data.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp_path, path)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
