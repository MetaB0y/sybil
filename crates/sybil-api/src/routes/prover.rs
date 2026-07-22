use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::types::error::AppError;

const JOB_HEIGHT_HEADER: HeaderName = HeaderName::from_static("x-sybil-proof-job-height");
const JOB_DIGEST_HEADER: HeaderName = HeaderName::from_static("x-sybil-proof-job-digest");

#[derive(Debug, Deserialize, ToSchema)]
pub struct ProofJobAckRequest {
    /// Exact transport digest returned by the pull response.
    pub transport_digest: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProofJobAckResponse {
    pub height: u64,
    pub transport_digest: String,
    pub acknowledged: bool,
}

/// GET /v1/prover/jobs/next
#[utoipa::path(
    tag = "routesprover",
    get,
    path = "/v1/prover/jobs/next",
    responses(
        (status = 200, description = "Oldest unacknowledged MessagePack proof job", body = Vec<u8>, content_type = "application/msgpack"),
        (status = 204, description = "No unacknowledged proof job"),
        (status = 503, description = "Persistent proof-job outbox unavailable")
    )
)]
pub async fn get_next_proof_job(State(state): State<AppState>) -> Result<Response<Body>, AppError> {
    let Some(job) = state.sequencer.oldest_unacknowledged_proof_job().await? else {
        return Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .map_err(|error| AppError::internal(error.to_string()));
    };
    let height = HeaderValue::from_str(&job.height.to_string())
        .map_err(|error| AppError::internal(error.to_string()))?;
    let digest = HeaderValue::from_str(&format!("0x{}", hex::encode(job.digest)))
        .map_err(|error| AppError::internal(error.to_string()))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CONTENT_LENGTH, job.bytes.len())
        .header(JOB_HEIGHT_HEADER, height)
        .header(JOB_DIGEST_HEADER, digest)
        .body(Body::from(job.bytes))
        .map_err(|error| AppError::internal(error.to_string()))
}

/// POST /v1/prover/jobs/{height}/ack
#[utoipa::path(
    tag = "routesprover",
    post,
    path = "/v1/prover/jobs/{height}/ack",
    params(("height" = u64, Path, description = "Committed block height")),
    request_body = ProofJobAckRequest,
    responses(
        (status = 200, description = "Exact proof-job bytes acknowledged", body = ProofJobAckResponse),
        (status = 400, description = "Malformed digest"),
        (status = 503, description = "Persistent proof-job outbox unavailable")
    )
)]
pub async fn acknowledge_proof_job(
    State(state): State<AppState>,
    Path(height): Path<u64>,
    Json(request): Json<ProofJobAckRequest>,
) -> Result<Json<ProofJobAckResponse>, AppError> {
    let digest = parse_digest(&request.transport_digest)?;
    state
        .sequencer
        .acknowledge_proof_job(height, digest)
        .await?;
    Ok(Json(ProofJobAckResponse {
        height,
        transport_digest: format!("0x{}", hex::encode(digest)),
        acknowledged: true,
    }))
}

fn parse_digest(value: &str) -> Result<[u8; 32], AppError> {
    let normalized = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(normalized)
        .map_err(|_| AppError::bad_request("Invalid proof-job transport digest"))?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        AppError::bad_request(format!(
            "Proof-job transport digest must be 32 bytes, got {}",
            bytes.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_digest_parser_is_exact() {
        assert_eq!(
            parse_digest(&format!("0x{}", "11".repeat(32))).unwrap(),
            [0x11; 32]
        );
        assert!(parse_digest("11").is_err());
        assert!(parse_digest("not-hex").is_err());
    }
}
