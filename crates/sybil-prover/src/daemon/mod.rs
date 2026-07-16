mod artifact;
mod backend;
mod http;
mod model;
mod source;
mod store;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use artifact::ArtifactStore;
use backend::{GuestPins, OpenVmStarkConfig, ProofBackend};
use clap::Args;
pub use model::{DaemonStatus, EpochRecord, EpochState, IngestAck, ProofBackendKind};
use source::ProofJobSource;
use store::{ClaimedEpoch, DaemonStore};
use sybil_proof_protocol::build_state_transition_guest_input;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Args, Clone, Debug)]
pub struct DaemonArgs {
    /// Durable redb authority for jobs, epochs, attempts, leases, and frontiers.
    #[arg(
        long,
        env = "SYBIL_PROVER_DB",
        default_value = "data/prover/prover.redb"
    )]
    pub db: PathBuf,
    /// Root for immutable proof payloads and envelopes.
    #[arg(
        long,
        env = "SYBIL_PROVER_ARTIFACTS",
        default_value = "data/prover/artifacts"
    )]
    pub artifacts_dir: PathBuf,
    /// HTTP bind address.
    #[arg(long, env = "SYBIL_PROVER_BIND", default_value = "127.0.0.1:3002")]
    pub bind: String,
    /// Bearer token required by ingest and administrative mutation endpoints.
    #[arg(long, env = "SYBIL_PROVER_AUTH_TOKEN", hide_env_values = true)]
    pub auth_token: String,
    /// Sybil API base URL providing the authenticated proof-job outbox.
    #[arg(long, env = "SYBIL_PROVER_SOURCE_URL")]
    pub source_url: Option<String>,
    /// Service bearer used to pull and acknowledge the sequencer outbox.
    #[arg(long, env = "SYBIL_PROVER_SOURCE_TOKEN", hide_env_values = true)]
    pub source_token: Option<String>,
    /// Backend. STARK is the deployment default; mock is for integration tests only.
    #[arg(
        long,
        env = "SYBIL_PROVER_PROOF_KIND",
        value_enum,
        default_value_t = ProofBackendKind::Stark
    )]
    pub proof_kind: ProofBackendKind,
    /// Fixed target for future epochs. Existing epochs are never reshaped.
    #[arg(long, env = "SYBIL_PROVER_EPOCH_BLOCKS", default_value_t = 4)]
    pub epoch_blocks: u64,
    /// Maximum automatic attempts before manual intervention is required.
    #[arg(long, default_value_t = 5)]
    pub max_attempts: u32,
    /// Base retry delay; retries use bounded exponential backoff plus deterministic jitter.
    #[arg(long, default_value_t = 5_000)]
    pub retry_base_ms: u64,
    /// Durable lease duration for one proof attempt.
    #[arg(long, default_value_t = 120_000)]
    pub lease_ms: u64,
    /// Scheduler and lease-renewal tick.
    #[arg(long, default_value_t = 1_000)]
    pub poll_ms: u64,
    /// Maximum accepted serialized proof job size.
    #[arg(long, default_value_t = 64 * 1024 * 1024)]
    pub max_job_bytes: usize,
    /// Committed guest pin JSON.
    #[arg(
        long,
        default_value = "zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json"
    )]
    pub guest_pins: PathBuf,
    /// OpenVM input encoder Cargo manifest.
    #[arg(long, default_value = "zk/openvm-tools/Cargo.toml")]
    pub openvm_tools_manifest: PathBuf,
    /// OpenVM guest Cargo manifest.
    #[arg(long, default_value = "zk/openvm-guest/Cargo.toml")]
    pub openvm_guest_manifest: PathBuf,
    /// OpenVM app configuration.
    #[arg(long, default_value = "zk/openvm-guest/openvm.toml")]
    pub openvm_config: PathBuf,
    /// Shared pinned OpenVM build/key output.
    #[arg(long, default_value = "target/openvm/sybil")]
    pub openvm_output_dir: PathBuf,
    /// Timeout for each input/prove/verify subprocess, in seconds.
    #[arg(long, default_value_t = 21_600)]
    pub command_timeout_secs: u64,
    /// Address-space limit for OpenVM subprocesses, in MiB. Zero disables the limit.
    #[arg(long, default_value_t = 0)]
    pub memory_limit_mib: u64,
    /// Reserved future switch. EVM remains fail-closed until #13 is implemented.
    #[arg(long, default_value_t = false, hide = true)]
    pub enable_evm: bool,
}

pub struct Runtime {
    store: Arc<DaemonStore>,
    artifacts: Arc<ArtifactStore>,
    backend: ProofBackend,
    backend_kind: ProofBackendKind,
    source: Option<ProofJobSource>,
    owner: Uuid,
    auth_token: Arc<str>,
    ready: AtomicBool,
    lease_ms: u64,
    poll_ms: u64,
    max_job_bytes: usize,
    metrics: RuntimeMetrics,
}

#[derive(Default)]
struct RuntimeMetrics {
    ingested_total: AtomicU64,
    duplicate_total: AtomicU64,
    proofs_total: AtomicU64,
    retryable_failures_total: AtomicU64,
    permanent_failures_total: AtomicU64,
    recovered_leases_total: AtomicU64,
    adopted_artifacts_total: AtomicU64,
    source_failures_total: AtomicU64,
    source_acks_total: AtomicU64,
    last_proof_at_ms: AtomicU64,
}

pub async fn run(args: DaemonArgs) -> Result<(), DaemonError> {
    if args.auth_token.is_empty() {
        return Err(DaemonError::Config(
            "prover authentication token must not be empty".to_string(),
        ));
    }
    if args.lease_ms < args.poll_ms.saturating_mul(3) {
        return Err(DaemonError::Config(
            "proof lease must be at least three scheduler ticks".to_string(),
        ));
    }
    let pins = GuestPins::read(&args.guest_pins)?;
    let store = Arc::new(DaemonStore::open(
        &args.db,
        args.epoch_blocks,
        args.max_attempts,
        args.retry_base_ms,
    )?);
    let artifacts = Arc::new(ArtifactStore::open(args.artifacts_dir.clone())?);
    let stark = OpenVmStarkConfig {
        pins,
        artifact_root: args.artifacts_dir,
        tools_manifest: args.openvm_tools_manifest,
        guest_manifest: args.openvm_guest_manifest,
        guest_config: args.openvm_config,
        output_dir: args.openvm_output_dir,
        command_timeout: Duration::from_secs(args.command_timeout_secs),
        memory_limit_mib: args.memory_limit_mib,
    };
    let backend = ProofBackend::new(args.proof_kind, pins, stark, args.enable_evm)?;
    let source = match args.source_url {
        Some(url) => {
            let token = args
                .source_token
                .filter(|token| !token.is_empty())
                .ok_or_else(|| {
                    DaemonError::Config(
                        "--source-token is required when --source-url is configured".to_string(),
                    )
                })?;
            Some(ProofJobSource::new(url, token, args.max_job_bytes)?)
        }
        None if args.source_token.is_some() => {
            return Err(DaemonError::Config(
                "--source-token has no effect without --source-url".to_string(),
            ));
        }
        None => None,
    };
    let runtime = Arc::new(Runtime {
        store,
        artifacts,
        backend,
        backend_kind: args.proof_kind,
        source,
        owner: Uuid::new_v4(),
        auth_token: Arc::from(args.auth_token),
        ready: AtomicBool::new(false),
        lease_ms: args.lease_ms,
        poll_ms: args.poll_ms,
        max_job_bytes: args.max_job_bytes,
        metrics: RuntimeMetrics::default(),
    });

    reconcile(&runtime)?;
    runtime.ready.store(true, Ordering::Release);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .map_err(|source| DaemonError::Bind {
            addr: args.bind.clone(),
            source,
        })?;
    println!("prover_daemon={}", args.bind);
    println!("prover_owner={}", runtime.owner);
    println!("proof_kind={:?}", runtime.backend_kind);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_runtime = Arc::clone(&runtime);
    let scheduler_shutdown = shutdown_rx.clone();
    let scheduler = tokio::spawn(async move {
        let result = scheduler_loop(Arc::clone(&scheduler_runtime), scheduler_shutdown).await;
        if let Err(error) = &result {
            scheduler_runtime.ready.store(false, Ordering::Release);
            eprintln!("prover scheduler stopped: {error}");
        }
        result
    });
    let source_runtime = Arc::clone(&runtime);
    let source_shutdown = shutdown_rx;
    let source_task = tokio::spawn(async move {
        let result = source_loop(Arc::clone(&source_runtime), source_shutdown).await;
        if let Err(error) = &result {
            source_runtime.ready.store(false, Ordering::Release);
            eprintln!("proof-job source stopped: {error}");
        }
        result
    });
    let app = http::router(Arc::clone(&runtime));
    let shutdown = async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    };
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    runtime.ready.store(false, Ordering::Release);
    scheduler.await??;
    source_task.await??;
    serve_result.map_err(DaemonError::Io)
}

fn reconcile(runtime: &Runtime) -> Result<(), DaemonError> {
    runtime.artifacts.quarantine_temporary()?;
    let recovered = runtime.store.recover_expired(now_ms())?;
    runtime
        .metrics
        .recovered_leases_total
        .fetch_add(recovered, Ordering::Relaxed);
    for epoch in runtime.store.list_epochs()? {
        match (&epoch.state, &epoch.artifact) {
            (EpochState::Proven, Some(artifact)) => {
                if let Err(error) = runtime.artifacts.validate(artifact) {
                    runtime.store.invalidate_artifact(
                        epoch.first_block_height,
                        &error.to_string(),
                        now_ms(),
                    )?;
                }
            }
            (EpochState::Proven, None) => {
                runtime.store.invalidate_artifact(
                    epoch.first_block_height,
                    "proven epoch is missing its artifact manifest",
                    now_ms(),
                )?;
            }
            _ => {
                if let Some(artifact) = runtime.artifacts.find_valid(&epoch)? {
                    runtime
                        .store
                        .adopt_artifact(epoch.first_block_height, artifact, now_ms())?;
                    runtime
                        .metrics
                        .adopted_artifacts_total
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    Ok(())
}

async fn scheduler_loop(
    runtime: Arc<Runtime>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), DaemonError> {
    let mut ticker = tokio::time::interval(Duration::from_millis(runtime.poll_ms));
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
            _ = ticker.tick() => {
                let recovered = runtime.store.recover_expired(now_ms())?;
                runtime
                    .metrics
                    .recovered_leases_total
                    .fetch_add(recovered, Ordering::Relaxed);
                while runtime.store.assemble_next(
                    runtime.backend_kind.proof_kind(),
                    false,
                    now_ms(),
                    "scheduler",
                )?.is_some() {}
                if let Some(claimed) = runtime.store.claim_next(
                    runtime.owner,
                    runtime.lease_ms,
                    now_ms(),
                )? {
                    process_claimed(&runtime, claimed, &mut shutdown).await?;
                }
            }
        }
    }
}

async fn source_loop(
    runtime: Arc<Runtime>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), DaemonError> {
    let Some(source) = &runtime.source else {
        while shutdown.changed().await.is_ok() {
            if *shutdown.borrow() {
                break;
            }
        }
        return Ok(());
    };
    let mut ticker = tokio::time::interval(Duration::from_millis(runtime.poll_ms));
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
            _ = ticker.tick() => {
                match source.pull_once(&runtime.store, now_ms()).await {
                    Ok(Some(ack)) => {
                        if ack.duplicate {
                            runtime.metrics.duplicate_total.fetch_add(1, Ordering::Relaxed);
                        } else {
                            runtime.metrics.ingested_total.fetch_add(1, Ordering::Relaxed);
                        }
                        runtime.metrics.source_acks_total.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(None) => {}
                    Err(error) if !error.permanent => {
                        runtime.metrics.source_failures_total.fetch_add(1, Ordering::Relaxed);
                        eprintln!("proof-job source retryable failure: {}", error.message);
                    }
                    Err(error) => return Err(DaemonError::Source(error.message)),
                }
            }
        }
    }
}

async fn process_claimed(
    runtime: &Arc<Runtime>,
    claimed: ClaimedEpoch,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), DaemonError> {
    let first_height = claimed.epoch.first_block_height;
    let attempt = claimed.epoch.attempt_count;
    let mut inputs = Vec::with_capacity(claimed.jobs.len());
    for (index, record) in claimed.jobs.iter().enumerate() {
        if claimed.epoch.job_transport_digests.get(index) != Some(&record.transport_digest) {
            runtime.store.fail_attempt(
                first_height,
                runtime.owner,
                attempt,
                "durable job digest differs from assembled epoch",
                true,
                now_ms(),
            )?;
            runtime
                .metrics
                .permanent_failures_total
                .fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        let job = match rmp_serde::from_slice(&record.bytes) {
            Ok(job) => job,
            Err(error) => {
                runtime.store.fail_attempt(
                    first_height,
                    runtime.owner,
                    attempt,
                    &format!("decode durable proof job: {error}"),
                    true,
                    now_ms(),
                )?;
                runtime
                    .metrics
                    .permanent_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
        };
        match build_state_transition_guest_input(job) {
            Ok(input) => inputs.push(input),
            Err(error) => {
                runtime.store.fail_attempt(
                    first_height,
                    runtime.owner,
                    attempt,
                    &format!("prepare durable proof job: {error}"),
                    true,
                    now_ms(),
                )?;
                runtime
                    .metrics
                    .permanent_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
        }
    }

    let proof = runtime
        .backend
        .prove(&claimed.epoch, &inputs, runtime.owner, attempt, now_ms());
    tokio::pin!(proof);
    let backend_result = loop {
        tokio::select! {
            result = &mut proof => break result,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    // The backend command is kill-on-drop. The durable lease makes this retryable.
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(Duration::from_millis((runtime.lease_ms / 3).max(1))) => {
                runtime.store.renew_lease(
                    first_height,
                    runtime.owner,
                    attempt,
                    runtime.lease_ms,
                    now_ms(),
                )?;
            }
        }
    };

    match backend_result {
        Ok(proof) => {
            if proof.envelope.proof_kind != claimed.epoch.proof_kind
                || proof.envelope.public_inputs != claimed.epoch.public_inputs
            {
                runtime.store.fail_attempt(
                    first_height,
                    runtime.owner,
                    attempt,
                    "proof backend changed the epoch statement or proof kind",
                    true,
                    now_ms(),
                )?;
                runtime
                    .metrics
                    .permanent_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            let published_at = now_ms();
            let artifact = runtime.artifacts.publish(
                &proof.envelope,
                &proof.payload,
                runtime.owner,
                attempt,
                published_at,
            )?;
            runtime.store.complete_attempt(
                first_height,
                runtime.owner,
                attempt,
                artifact,
                published_at,
            )?;
            runtime.metrics.proofs_total.fetch_add(1, Ordering::Relaxed);
            runtime
                .metrics
                .last_proof_at_ms
                .store(published_at, Ordering::Relaxed);
        }
        Err(error) => {
            runtime.store.fail_attempt(
                first_height,
                runtime.owner,
                attempt,
                &error.message,
                error.permanent,
                now_ms(),
            )?;
            if error.permanent {
                runtime
                    .metrics
                    .permanent_failures_total
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                runtime
                    .metrics
                    .retryable_failures_total
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("invalid prover configuration: {0}")]
    Config(String),
    #[error("durable prover conflict: {0}")]
    Conflict(String),
    #[error("proof-job height gap: expected {expected}, got {actual}")]
    Gap { expected: u64, actual: u64 },
    #[error("prover resource not found: {0}")]
    NotFound(String),
    #[error("proof lease lost for epoch starting at block {first_height}, attempt {attempt}")]
    LeaseLost { first_height: u64, attempt: u32 },
    #[error("proof artifact error: {0}")]
    Artifact(String),
    #[error("proof-job source failed closed: {0}")]
    Source(String),
    #[error("bind prover daemon at {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] redb::DatabaseError),
    #[error(transparent)]
    Transaction(#[from] redb::TransactionError),
    #[error(transparent)]
    Table(#[from] redb::TableError),
    #[error(transparent)]
    Storage(#[from] redb::StorageError),
    #[error(transparent)]
    Commit(#[from] redb::CommitError),
    #[error(transparent)]
    Encode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Decode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    ProofJob(#[from] sybil_proof_protocol::ProofJobError),
    #[error(transparent)]
    Zk(#[from] sybil_zk::ZkTransitionError),
    #[error(transparent)]
    Epoch(#[from] sybil_zk::EpochTransitionError),
    #[error(transparent)]
    Envelope(#[from] sybil_proof_protocol::ProofEnvelopeError),
    #[error("scheduler task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

#[cfg(all(test, feature = "sequencer-store"))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::Router;
    use axum::body::Body;
    use axum::extract::State as AxumState;
    use axum::http::{Response, StatusCode};
    use axum::routing::{get, post};
    use matching_engine::MarketSet;
    use matching_sequencer::store::Store;
    use matching_sequencer::{AccountStore, BlockSequencer, SequencerConfig};
    use sybil_proof_protocol::{ProofEnvelopeError, ProofKind, StateTransitionProofJob};
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::artifact::ArtifactStore;
    use super::backend::{GuestPins, ProofBackend};
    use super::model::EpochState;
    use super::source::ProofJobSource;
    use super::store::DaemonStore;

    async fn proof_jobs(count: usize) -> (TempDir, Vec<Vec<u8>>) {
        let temp = tempfile::tempdir().expect("tempdir");
        let sequencer_path = temp.path().join("sequencer.redb");
        let store = Store::open(&sequencer_path).expect("sequencer store");
        let mut sequencer = BlockSequencer::with_default_solver(
            AccountStore::new(),
            MarketSet::new(),
            vec![],
            SequencerConfig::default(),
        );
        for index in 0..count {
            let production = sequencer.produce_block(vec![], (index as u64 + 1) * 1_000);
            store
                .save_block_with_witness(sequencer.snapshot(), &production.witness)
                .await
                .expect("persist fixture block and proof job");
        }
        let jobs = store
            .proof_job_outbox_page(None, count + 1)
            .expect("read fixture outbox")
            .into_iter()
            .map(|entry| entry.bytes)
            .collect::<Vec<_>>();
        assert_eq!(jobs.len(), count);
        drop(store);
        (temp, jobs)
    }

    fn open_store(temp: &TempDir, target: u64) -> DaemonStore {
        DaemonStore::open(&temp.path().join("prover.redb"), target, 3, 10).expect("daemon store")
    }

    #[tokio::test]
    async fn ingest_is_exactly_idempotent_and_rejects_conflicts_and_gaps() {
        let (_fixture, jobs) = proof_jobs(3).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let store = open_store(&temp, 2);

        let first = store.ingest(jobs[0].clone(), 10).expect("first ingest");
        assert!(!first.duplicate);
        let duplicate = store.ingest(jobs[0].clone(), 11).expect("duplicate ingest");
        assert!(duplicate.duplicate);

        let decoded: StateTransitionProofJob =
            rmp_serde::from_slice(&jobs[0]).expect("decode fixture job");
        let alternate = rmp_serde::to_vec(&decoded).expect("alternate valid encoding");
        assert!(matches!(
            store.ingest(alternate, 12),
            Err(super::DaemonError::Conflict(_))
        ));
        assert!(matches!(
            store.ingest(jobs[2].clone(), 13),
            Err(super::DaemonError::Gap {
                expected: 2,
                actual: 3
            })
        ));
    }

    #[tokio::test]
    async fn epoch_assembly_is_deterministic_across_reopen() {
        let (_fixture, jobs) = proof_jobs(2).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let store = open_store(&temp, 2);
        store.ingest(jobs[0].clone(), 10).expect("job one");
        assert!(
            store
                .assemble_next(ProofKind::Mock, false, 11, "test")
                .expect("assemble before full")
                .is_none()
        );
        store.ingest(jobs[1].clone(), 12).expect("job two");
        let epoch = store
            .assemble_next(ProofKind::Mock, false, 13, "test")
            .expect("assemble")
            .expect("epoch");
        assert_eq!(epoch.public_inputs.block_count, 2);
        let epoch_id = epoch.epoch_id;
        drop(store);

        let reopened = open_store(&temp, 4);
        let persisted = reopened
            .read_epoch(1)
            .expect("read epoch")
            .expect("persisted epoch");
        assert_eq!(persisted.epoch_id, epoch_id);
        assert_eq!(persisted.job_transport_digests, epoch.job_transport_digests);
        assert!(
            reopened
                .assemble_next(ProofKind::Mock, false, 14, "test")
                .expect("idempotent assembly")
                .is_none()
        );
    }

    #[tokio::test]
    async fn expired_lease_retries_but_permanent_frontier_never_skips() {
        let (_fixture, jobs) = proof_jobs(2).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let store = open_store(&temp, 1);
        for (index, job) in jobs.into_iter().enumerate() {
            store.ingest(job, index as u64 + 1).expect("ingest");
            store
                .assemble_next(ProofKind::Mock, false, index as u64 + 10, "test")
                .expect("assemble")
                .expect("epoch");
        }
        let owner = Uuid::new_v4();
        let first = store
            .claim_next(owner, 10, 100)
            .expect("claim")
            .expect("first claim");
        drop(store);

        let reopened = open_store(&temp, 1);
        assert_eq!(reopened.recover_expired(111).expect("recover"), 1);
        let retried = reopened
            .claim_next(owner, 10, 111)
            .expect("retry claim")
            .expect("retryable epoch");
        assert_eq!(retried.epoch.first_block_height, 1);
        reopened
            .fail_attempt(
                1,
                owner,
                retried.epoch.attempt_count,
                "invalid witness",
                true,
                112,
            )
            .expect("permanent failure");
        assert!(
            reopened
                .claim_next(owner, 10, 113)
                .expect("blocked claim")
                .is_none()
        );
        assert_eq!(first.epoch.first_block_height, 1);
    }

    #[tokio::test]
    async fn expired_final_lease_is_terminal_and_manual_retry_is_monotonic() {
        let (_fixture, jobs) = proof_jobs(1).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("prover.redb");
        let store = DaemonStore::open(&path, 1, 1, 10).expect("store");
        store.ingest(jobs[0].clone(), 1).expect("ingest");
        store
            .assemble_next(ProofKind::Mock, false, 2, "test")
            .expect("assemble")
            .expect("epoch");
        let owner = Uuid::new_v4();
        let first = store
            .claim_next(owner, 10, 100)
            .expect("claim")
            .expect("first attempt");
        assert_eq!(first.epoch.attempt_count, 1);
        drop(store);

        let reopened = DaemonStore::open(&path, 1, 1, 10).expect("reopen");
        assert_eq!(reopened.recover_expired(111).expect("recover"), 1);
        let terminal = reopened.read_epoch(1).expect("read").expect("epoch");
        assert!(matches!(terminal.state, EpochState::FailedPermanent));
        assert_eq!(terminal.attempt_count, 1);
        assert!(
            reopened
                .claim_next(owner, 10, 112)
                .expect("terminal barrier")
                .is_none()
        );

        let retried = reopened
            .manual_retry(1, "test-admin", 113)
            .expect("manual retry");
        assert!(matches!(retried.state, EpochState::Ready));
        assert_eq!(retried.attempt_count, 1);
        let second = reopened
            .claim_next(owner, 10, 114)
            .expect("manual claim")
            .expect("second attempt");
        assert_eq!(second.epoch.attempt_count, 2);
    }

    #[tokio::test]
    async fn published_mock_artifact_is_adopted_after_db_commit_crash() {
        let (_fixture, jobs) = proof_jobs(2).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let store = open_store(&temp, 2);
        store.ingest(jobs[0].clone(), 1).expect("ingest one");
        store.ingest(jobs[1].clone(), 2).expect("ingest two");
        store
            .assemble_next(ProofKind::Mock, false, 3, "test")
            .expect("assemble")
            .expect("epoch");
        let owner = Uuid::new_v4();
        let claimed = store
            .claim_next(owner, 100, 4)
            .expect("claim")
            .expect("claimed epoch");
        let inputs = claimed
            .jobs
            .iter()
            .map(|record| {
                let job = rmp_serde::from_slice(&record.bytes).expect("decode job");
                sybil_proof_protocol::build_state_transition_guest_input(job).expect("prepare job")
            })
            .collect::<Vec<_>>();
        let pins = GuestPins {
            app_exe_commit: [7; 32],
            app_vm_commit: [8; 32],
        };
        let backend = ProofBackend::Mock { pins };
        let proof = backend
            .prove(
                &claimed.epoch,
                &inputs,
                owner,
                claimed.epoch.attempt_count,
                5,
            )
            .await
            .expect("mock proof");
        assert!(matches!(
            proof.envelope.require_l1_submittable(),
            Err(ProofEnvelopeError::NotL1Submittable {
                proof_kind: ProofKind::Mock
            })
        ));
        let artifacts = ArtifactStore::open(temp.path().join("artifacts")).expect("artifact store");
        let published = artifacts
            .publish(
                &proof.envelope,
                &proof.payload,
                owner,
                claimed.epoch.attempt_count,
                6,
            )
            .expect("atomic publish");
        artifacts.validate(&published).expect("valid artifact");

        // Simulate SIGKILL after atomic rename and before the redb state commit.
        drop(store);
        let reopened = open_store(&temp, 2);
        let epoch = reopened.read_epoch(1).expect("read epoch").expect("epoch");
        assert!(matches!(epoch.state, EpochState::Proving { .. }));
        let orphan = artifacts
            .find_valid(&epoch)
            .expect("scan final artifacts")
            .expect("orphan artifact");
        reopened
            .adopt_artifact(1, orphan, 7)
            .expect("adopt exact orphan");
        let adopted = reopened
            .read_epoch(1)
            .expect("read adopted")
            .expect("adopted epoch");
        assert!(matches!(adopted.state, EpochState::Proven));
        assert_eq!(reopened.policy().expect("policy").proven_frontier, Some(2));
    }

    #[derive(Clone)]
    struct FakeProofJobSource {
        bytes: Arc<Vec<u8>>,
        digest: [u8; 32],
        ack_attempts: Arc<AtomicU64>,
    }

    async fn fake_pull(AxumState(source): AxumState<FakeProofJobSource>) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/msgpack")
            .header("x-sybil-proof-job-height", "1")
            .header(
                "x-sybil-proof-job-digest",
                format!("0x{}", hex::encode(source.digest)),
            )
            .body(Body::from(source.bytes.as_ref().clone()))
            .expect("fake response")
    }

    async fn fake_ack(AxumState(source): AxumState<FakeProofJobSource>) -> StatusCode {
        if source.ack_attempts.fetch_add(1, Ordering::Relaxed) == 0 {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::OK
        }
    }

    #[tokio::test]
    async fn source_retries_ack_after_local_durability_without_duplicate_jobs() {
        let (_fixture, jobs) = proof_jobs(1).await;
        let bytes = jobs[0].clone();
        let fake = FakeProofJobSource {
            digest: sybil_proof_protocol::proof_job_transport_digest(&bytes),
            bytes: Arc::new(bytes),
            ack_attempts: Arc::new(AtomicU64::new(0)),
        };
        let app = Router::new()
            .route("/v1/prover/jobs/next", get(fake_pull))
            .route("/v1/prover/jobs/{height}/ack", post(fake_ack))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("local address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake source server");
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let store = open_store(&temp, 1);
        let source = ProofJobSource::new(
            format!("http://{address}"),
            "token".to_string(),
            64 * 1024 * 1024,
        )
        .expect("source client");
        let first = source
            .pull_once(&store, 1)
            .await
            .expect_err("first ack fails");
        assert!(!first.permanent);
        assert_eq!(store.policy().expect("policy").ingested_frontier, Some(1));
        let second = source
            .pull_once(&store, 2)
            .await
            .expect("ack retry")
            .expect("job result");
        assert!(second.duplicate);
        assert_eq!(fake.ack_attempts.load(Ordering::Relaxed), 2);
        assert_eq!(
            store
                .status(Uuid::new_v4(), super::ProofBackendKind::Mock, true)
                .unwrap()
                .jobs,
            1
        );
        server.abort();
    }
}
