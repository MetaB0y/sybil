use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sybil_proof_protocol::{
    EpochId, EpochTransitionPublicInputs, ProofEnvelope, ProofKind, StateTransitionProofJobId,
};
use uuid::Uuid;

pub const DAEMON_STORE_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ProofBackendKind {
    Mock,
    Stark,
    Evm,
}

impl ProofBackendKind {
    pub const fn proof_kind(self) -> ProofKind {
        match self {
            Self::Mock => ProofKind::Mock,
            Self::Stark => ProofKind::OpenVmStark,
            Self::Evm => ProofKind::OpenVmEvm,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub format_version: u8,
    pub id: StateTransitionProofJobId,
    pub transport_digest: [u8; 32],
    pub bytes_len: u64,
    pub received_at_ms: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EpochState {
    Ready,
    Proving { lease: Lease },
    RetryWait { retry_at_ms: u64 },
    FailedPermanent,
    Proven,
}

impl EpochState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Proving { .. } => "proving",
            Self::RetryWait { .. } => "retry_wait",
            Self::FailedPermanent => "failed_permanent",
            Self::Proven => "proven",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub owner: Uuid,
    pub attempt: u32,
    pub acquired_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochRecord {
    pub format_version: u8,
    /// First block job in the epoch. The public statement starts one height earlier.
    pub first_block_height: u64,
    pub last_block_height: u64,
    pub job_heights: Vec<u64>,
    pub job_transport_digests: Vec<[u8; 32]>,
    pub epoch_id: EpochId,
    pub public_inputs: EpochTransitionPublicInputs,
    pub proof_kind: ProofKind,
    pub state: EpochState,
    pub attempt_count: u32,
    pub manual_seal: bool,
    pub assembled_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_error: Option<String>,
    pub artifact: Option<ArtifactRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub relative_dir: PathBuf,
    pub envelope: ProofEnvelope,
    pub envelope_digest: [u8; 32],
    pub published_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub format_version: u8,
    pub first_block_height: u64,
    pub epoch_id: EpochId,
    pub proof_kind: ProofKind,
    pub owner: Uuid,
    pub attempt: u32,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub outcome: AttemptOutcome,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Running,
    RetryableFailure,
    PermanentFailure,
    Proven,
    RecoveredExpired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochPolicy {
    pub format_version: u8,
    pub target_blocks: u64,
    pub next_epoch_start: Option<u64>,
    pub ingested_frontier: Option<u64>,
    pub assembled_frontier: Option<u64>,
    pub proven_frontier: Option<u64>,
    pub max_attempts: u32,
    pub retry_base_ms: u64,
}

impl EpochPolicy {
    pub fn new(target_blocks: u64, max_attempts: u32, retry_base_ms: u64) -> Self {
        Self {
            format_version: DAEMON_STORE_VERSION,
            target_blocks,
            next_epoch_start: None,
            ingested_frontier: None,
            assembled_frontier: None,
            proven_frontier: None,
            max_attempts,
            retry_base_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub format_version: u8,
    pub sequence: u64,
    pub at_ms: u64,
    pub actor: String,
    pub action: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IngestAck {
    pub height: u64,
    pub transport_digest: String,
    pub durable: bool,
    pub duplicate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DaemonStatus {
    pub ready: bool,
    pub owner: Uuid,
    pub backend: ProofBackendKind,
    pub policy: EpochPolicy,
    pub jobs: u64,
    pub epochs: u64,
    pub epoch_states: std::collections::BTreeMap<String, u64>,
    pub queued_job_bytes: u64,
}
