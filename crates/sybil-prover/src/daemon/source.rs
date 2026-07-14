use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use sybil_proof_protocol::proof_job_transport_digest;

use super::DaemonError;
use super::model::IngestAck;
use super::store::DaemonStore;

const JOB_HEIGHT_HEADER: &str = "x-sybil-proof-job-height";
const JOB_DIGEST_HEADER: &str = "x-sybil-proof-job-digest";

#[derive(Clone)]
pub struct ProofJobSource {
    client: reqwest::Client,
    base_url: String,
    token: String,
    max_job_bytes: usize,
}

#[derive(Debug)]
pub struct SourceFailure {
    pub message: String,
    pub permanent: bool,
}

impl ProofJobSource {
    pub fn new(base_url: String, token: String, max_job_bytes: usize) -> Result<Self, DaemonError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(DaemonError::Config(
                "proof-job source URL must use http:// or https://".to_string(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| DaemonError::Config(error.to_string()))?;
        Ok(Self {
            client,
            base_url,
            token,
            max_job_bytes,
        })
    }

    pub async fn pull_once(
        &self,
        store: &DaemonStore,
        now_ms: u64,
    ) -> Result<Option<IngestAck>, SourceFailure> {
        let mut response = self
            .client
            .get(format!("{}/v1/prover/jobs/next", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(retryable)?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(http_failure("pull proof job", response.status()));
        }
        let height = response
            .headers()
            .get(JOB_HEIGHT_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| permanent("proof-job source omitted a valid height header"))?;
        let claimed_digest = response
            .headers()
            .get(JOB_DIGEST_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| permanent("proof-job source omitted its transport digest header"))
            .and_then(parse_digest)?;
        if response
            .content_length()
            .is_some_and(|length| length > self.max_job_bytes as u64)
        {
            return Err(permanent(
                "proof-job source payload exceeds the configured limit",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(retryable)? {
            let next_len = bytes.len().saturating_add(chunk.len());
            if next_len > self.max_job_bytes {
                return Err(permanent(
                    "proof-job source payload exceeds the configured limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let actual_digest = proof_job_transport_digest(&bytes);
        if claimed_digest != actual_digest {
            return Err(permanent(
                "proof-job source transport digest does not match its response bytes",
            ));
        }
        let ack = store
            .ingest(bytes.to_vec(), now_ms)
            .map_err(store_failure)?;
        if ack.height != height
            || ack.transport_digest != format!("0x{}", hex::encode(actual_digest))
        {
            return Err(permanent(
                "proof-job source metadata does not match the decoded durable job",
            ));
        }

        let response = self
            .client
            .post(format!("{}/v1/prover/jobs/{height}/ack", self.base_url))
            .bearer_auth(&self.token)
            .json(&AckRequest {
                transport_digest: &ack.transport_digest,
            })
            .send()
            .await
            .map_err(retryable)?;
        if !response.status().is_success() {
            return Err(http_failure("acknowledge proof job", response.status()));
        }
        Ok(Some(ack))
    }
}

#[derive(Serialize)]
struct AckRequest<'a> {
    transport_digest: &'a str,
}

fn parse_digest(value: &str) -> Result<[u8; 32], SourceFailure> {
    let normalized = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(normalized)
        .map_err(|_| permanent("proof-job source returned an invalid transport digest"))?;
    bytes
        .try_into()
        .map_err(|_| permanent("proof-job source transport digest is not 32 bytes"))
}

fn store_failure(error: DaemonError) -> SourceFailure {
    let permanent = matches!(
        error,
        DaemonError::Conflict(_)
            | DaemonError::Gap { .. }
            | DaemonError::ProofJob(_)
            | DaemonError::Zk(_)
            | DaemonError::Epoch(_)
            | DaemonError::Decode(_)
    );
    SourceFailure {
        message: error.to_string(),
        permanent,
    }
}

fn http_failure(operation: &str, status: StatusCode) -> SourceFailure {
    SourceFailure {
        message: format!("{operation}: source returned HTTP {status}"),
        permanent: matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::UNPROCESSABLE_ENTITY
        ),
    }
}

fn retryable(error: reqwest::Error) -> SourceFailure {
    SourceFailure {
        message: error.to_string(),
        permanent: false,
    }
}

fn permanent(message: &str) -> SourceFailure {
    SourceFailure {
        message: message.to_string(),
        permanent: true,
    }
}
