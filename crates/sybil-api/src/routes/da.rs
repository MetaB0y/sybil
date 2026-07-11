use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;

use matching_sequencer::{DaArtifact, DaArtifactLookup, DaProviderRef};

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{DaManifestResponse, DaProviderRefResponse};

/// GET /v1/da/{height}/manifest
#[utoipa::path(
    get,
    path = "/v1/da/{height}/manifest",
    description = "Typed DA manifest for a retained canonical witness payload. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset there are no retained DA artifacts; with pruning disabled rows are retained until the store is reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root -> witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.",
    params(("height" = u64, Path, description = "Retained block height")),
    responses(
        (status = 200, description = "DA manifest", body = DaManifestResponse),
        (status = 404, description = "DA artifact not retained or unavailable for this height"),
        (status = 500, description = "Stored DA artifact failed integrity verification")
    )
)]
pub async fn get_da_manifest(
    State(state): State<AppState>,
    Path(height): Path<u64>,
) -> Result<Json<DaManifestResponse>, AppError> {
    let artifact = retained_da_artifact(state.sequencer.get_da_artifact(height).await?, height)?;
    verify_da_artifact(&artifact)?;
    Ok(Json(da_manifest_response(&artifact)))
}

/// GET /v1/da/{height}/payload
#[utoipa::path(
    get,
    path = "/v1/da/{height}/payload",
    description = "Canonical witness payload bytes for a retained height, served as application/octet-stream with Content-Length. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset there are no retained DA artifacts; with pruning disabled rows are retained until the store is reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root -> witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.",
    params(("height" = u64, Path, description = "Retained block height")),
    responses(
        (status = 200, description = "Canonical witness payload bytes", content_type = "application/octet-stream"),
        (status = 404, description = "DA artifact not retained or unavailable for this height"),
        (status = 500, description = "Stored DA artifact failed integrity verification")
    )
)]
pub async fn get_da_payload(
    State(state): State<AppState>,
    Path(height): Path<u64>,
) -> Result<Response, AppError> {
    let artifact = retained_da_artifact(state.sequencer.get_da_artifact(height).await?, height)?;
    verify_da_artifact(&artifact)?;
    let payload_len = artifact.payload.len();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, payload_len.to_string())
        .body(Body::from(artifact.payload))
        .map_err(|error| AppError::internal(format!("Build payload response: {error}")))
}

fn retained_da_artifact(lookup: DaArtifactLookup, height: u64) -> Result<DaArtifact, AppError> {
    lookup.artifact.ok_or_else(|| {
        let mut message = format!("DA artifact not retained for height {height}");
        if let Some(oldest) = lookup.oldest_retained_height {
            message.push_str(&format!("; oldest retained height is {oldest}"));
        }
        AppError::not_found(message)
    })
}

fn verify_da_artifact(artifact: &DaArtifact) -> Result<(), AppError> {
    artifact.verify_payload_integrity().map_err(|error| {
        tracing::error!(
            height = artifact.manifest.height,
            error = %error,
            "refusing to serve corrupt DA artifact"
        );
        AppError::internal("DA artifact integrity check failed")
    })
}

fn da_manifest_response(artifact: &DaArtifact) -> DaManifestResponse {
    let manifest = &artifact.manifest;
    DaManifestResponse {
        version: manifest.version,
        payload_kind: manifest.payload_kind.clone(),
        payload_encoding: manifest.payload_encoding.clone(),
        provider_refs_encoding: manifest.provider_refs_encoding.clone(),
        height: manifest.height,
        block_hash: hex32(manifest.block_hash),
        state_root: hex32(manifest.state_root),
        witness_root: hex32(manifest.witness_root),
        payload_root: hex32(manifest.payload_root),
        payload_len: manifest.payload_len,
        provider_refs_hash: hex32(manifest.provider_refs_hash),
        provider_refs: manifest
            .provider_refs
            .iter()
            .map(da_provider_ref_response)
            .collect(),
        da_commitment: hex32(manifest.da_commitment),
        public_input_hash: hex32(manifest.public_input_hash),
    }
}

fn da_provider_ref_response(provider_ref: &DaProviderRef) -> DaProviderRefResponse {
    DaProviderRefResponse {
        kind: provider_ref.kind.clone(),
        encoding: provider_ref.encoding.clone(),
        bytes: format!("0x{}", hex::encode(&provider_ref.bytes)),
        uri: provider_ref.uri.clone(),
        payload_root: provider_ref.payload_root.map(hex32),
        payload_len: provider_ref.payload_len,
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use matching_sequencer::{DaArtifact, DaArtifactIntegrityError};
    use sybil_verifier::{
        BlockWitness, DepositAccumulatorWitness, StateSidecarSnapshot, WitnessBlockHeader,
    };

    use super::verify_da_artifact;

    fn minimal_witness() -> BlockWitness {
        BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0; 32],
                state_root: [1; 32],
                events_root: sybil_verifier::event_commitment::empty_events_root(),
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1_000,
            },
            previous_header: None,
            genesis_hash: [0u8; 32],
            orders: Vec::new(),
            rejections: Vec::new(),
            system_events: Vec::new(),
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: Vec::new(),
            clearing_prices: std::collections::HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: Vec::new(),
            market_groups: Vec::new(),
            pre_state: Vec::new(),
            post_system_state: Vec::new(),
            post_state: Vec::new(),
            account_keys: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: Vec::new(),
        }
    }

    #[test]
    fn corrupt_payload_fails_closed_before_response_build() {
        let mut artifact = DaArtifact::from_witness(&minimal_witness());
        let first = artifact
            .payload
            .first_mut()
            .expect("canonical witness payload is non-empty");
        *first ^= 0x01;

        let error = artifact.verify_payload_integrity().unwrap_err();
        assert!(matches!(
            error,
            DaArtifactIntegrityError::PayloadRootMismatch { .. }
        ));
        let app_error = verify_da_artifact(&artifact).unwrap_err();
        assert_eq!(
            app_error.status,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
