use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{IndexerError, Result};

/// Persisted scan cursor, bound to one vault deployment and chain.
#[derive(Debug, Serialize, Deserialize)]
struct CursorState {
    next_from: u64,
    vault_address_hex: String,
    chain_id: u64,
}

/// Load the persisted scan cursor, or `None` if no file exists. Fails closed if
/// the file targets a different vault/chain than this run.
pub(super) fn load_cursor(path: &Path, vault_hex: &str, chain_id: u64) -> Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let state: CursorState = serde_json::from_str(&data)?;
    if state.vault_address_hex != vault_hex || state.chain_id != chain_id {
        return Err(IndexerError::CursorConfigMismatch {
            path: path.display().to_string(),
            stored_vault: state.vault_address_hex,
            stored_chain: state.chain_id,
            arg_vault: vault_hex.to_string(),
            arg_chain: chain_id,
        });
    }
    Ok(Some(state.next_from))
}

/// Persist the scan cursor durably (write-tmp-then-rename) so a crash cannot
/// leave a half-written file.
pub(super) fn save_cursor(
    path: &Path,
    next_from: u64,
    vault_hex: &str,
    chain_id: u64,
) -> Result<()> {
    let state = CursorState {
        next_from,
        vault_address_hex: vault_hex.to_string(),
        chain_id,
    };
    let data = serde_json::to_string_pretty(&state)?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, data)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
