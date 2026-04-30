use axum::extract::{Path, State};
use axum::Json;

use matching_sequencer::{
    AccountSnapshotSlot, QmdbStateExclusionProofParts, QmdbStateKeyValueProofParts,
    QmdbStateOperationProofParts, QmdbStateRangeProofParts, SequencerStateProof,
    SequencerStateProofKind, QMDB_STATE_MAX_KEY_BYTES,
};

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::*;

const STATE_PROOF_FORMAT: &str = "commonware-qmdb-current-ordered-sha256-mmr";

/// GET /v1/proofs/state/{leaf_key_hex}
#[utoipa::path(
    get,
    path = "/v1/proofs/state/{leaf_key_hex}",
    params(("leaf_key_hex" = String, Path, description = "Hex-encoded canonical state leaf key")),
    responses(
        (status = 200, description = "State leaf inclusion or exclusion proof", body = StateProofResponse),
        (status = 400, description = "Invalid state leaf key"),
        (status = 404, description = "No committed block yet"),
        (status = 503, description = "Proof store unavailable")
    )
)]
pub async fn get_state_proof(
    State(state): State<AppState>,
    Path(leaf_key_hex): Path<String>,
) -> Result<Json<StateProofResponse>, AppError> {
    let leaf_key = parse_leaf_key_hex(&leaf_key_hex)?;
    let proof = state.sequencer.get_state_proof(leaf_key).await?;
    Ok(Json(state_proof_response(proof)))
}

fn parse_leaf_key_hex(input: &str) -> Result<Vec<u8>, AppError> {
    let hex_input = input.strip_prefix("0x").unwrap_or(input);
    let leaf_key = hex::decode(hex_input)
        .map_err(|_| AppError::bad_request("Invalid hex-encoded state leaf key"))?;
    if leaf_key.is_empty() {
        return Err(AppError::bad_request("State leaf key cannot be empty"));
    }
    if leaf_key.len() > QMDB_STATE_MAX_KEY_BYTES {
        return Err(AppError::bad_request(format!(
            "State leaf key exceeds {QMDB_STATE_MAX_KEY_BYTES} bytes"
        )));
    }
    Ok(leaf_key)
}

fn state_proof_response(proof: SequencerStateProof) -> StateProofResponse {
    let leaf_key_ascii = printable_ascii(&proof.leaf_key);
    match proof.kind {
        SequencerStateProofKind::Inclusion {
            leaf_value,
            proof: inclusion,
        } => StateProofResponse {
            block_height: proof.block_height,
            state_root: hex::encode(proof.state_root),
            state_slot: state_slot(proof.slot).to_string(),
            leaf_key_hex: hex::encode(proof.leaf_key),
            leaf_key_ascii,
            proof_kind: "inclusion".to_string(),
            proof_format: STATE_PROOF_FORMAT.to_string(),
            verified: proof.verified,
            leaf_value_hex: Some(hex::encode(leaf_value)),
            inclusion_proof: Some(inclusion_response(inclusion)),
            exclusion_proof: None,
        },
        SequencerStateProofKind::Exclusion { proof: exclusion } => StateProofResponse {
            block_height: proof.block_height,
            state_root: hex::encode(proof.state_root),
            state_slot: state_slot(proof.slot).to_string(),
            leaf_key_hex: hex::encode(proof.leaf_key),
            leaf_key_ascii,
            proof_kind: "exclusion".to_string(),
            proof_format: STATE_PROOF_FORMAT.to_string(),
            verified: proof.verified,
            leaf_value_hex: None,
            inclusion_proof: None,
            exclusion_proof: Some(exclusion_response(exclusion)),
        },
    }
}

fn inclusion_response(proof: QmdbStateKeyValueProofParts) -> QmdbStateInclusionProofResponse {
    QmdbStateInclusionProofResponse {
        operation: operation_response(proof.operation),
        next_key_hex: hex::encode(proof.next_key),
    }
}

fn exclusion_response(proof: QmdbStateExclusionProofParts) -> QmdbStateExclusionProofResponse {
    match proof {
        QmdbStateExclusionProofParts::KeyValue {
            operation,
            span_key,
            span_value,
            span_next_key,
        } => QmdbStateExclusionProofResponse {
            variant: "key_value".to_string(),
            operation: operation_response(operation),
            span_key_hex: Some(hex::encode(span_key)),
            span_value_hex: Some(hex::encode(span_value)),
            span_next_key_hex: Some(hex::encode(span_next_key)),
            metadata_hex: None,
        },
        QmdbStateExclusionProofParts::Commit {
            operation,
            metadata,
        } => QmdbStateExclusionProofResponse {
            variant: "commit".to_string(),
            operation: operation_response(operation),
            span_key_hex: None,
            span_value_hex: None,
            span_next_key_hex: None,
            metadata_hex: metadata.map(hex::encode),
        },
    }
}

fn operation_response(proof: QmdbStateOperationProofParts) -> QmdbStateOperationProofResponse {
    QmdbStateOperationProofResponse {
        location: proof.location,
        activity_chunk_hex: hex::encode(proof.activity_chunk),
        range: range_response(proof.range),
    }
}

fn range_response(proof: QmdbStateRangeProofParts) -> QmdbStateRangeProofResponse {
    QmdbStateRangeProofResponse {
        leaves: proof.leaves,
        digests_hex: proof.digests.into_iter().map(hex::encode).collect(),
        pre_prefix_acc_hex: proof.pre_prefix_acc.map(hex::encode),
        unfolded_prefix_peaks_hex: proof
            .unfolded_prefix_peaks
            .into_iter()
            .map(hex::encode)
            .collect(),
        partial_chunk_digest_hex: proof.partial_chunk_digest.map(hex::encode),
        ops_root_hex: hex::encode(proof.ops_root),
    }
}

fn state_slot(slot: AccountSnapshotSlot) -> &'static str {
    match slot {
        AccountSnapshotSlot::A => "a",
        AccountSnapshotSlot::B => "b",
    }
}

fn printable_ascii(bytes: &[u8]) -> Option<String> {
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        String::from_utf8(bytes.to_vec()).ok()
    } else {
        None
    }
}
