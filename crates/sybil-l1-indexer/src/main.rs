use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use alloy::primitives::{Address, B256, Bytes};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::types::{BlockNumberOrTag, Filter, Log as EthLog, TransactionRequest};
use alloy::sol_types::SolCall;
use clap::Parser;
#[cfg(test)]
use sybil_api_types::request::BridgeWithdrawalL1Status;
use sybil_api_types::request::{
    ObserveL1HeightRequest, SubmitL1DepositRequest, SubmitL1WithdrawalEventRequest,
};
#[cfg(test)]
use sybil_api_types::response::BridgeWithdrawalResponse;
use sybil_api_types::response::{
    BridgeAccountKeyResponse, BridgeDepositResponse, BridgeStatusResponse,
    BridgeWithdrawalL1EventResponse, ObserveL1HeightResponse,
};
use sybil_client::SybilClient;
use sybil_l1_abi::SybilVault;
use sybil_l1_protocol::{
    Bytes32, EthAddress, L1ProtocolError, deposit_received_topic0, withdrawal_cancelled_topic0,
    withdrawal_finalized_topic0, withdrawal_queued_topic0,
};
use tokio::net::TcpListener;
use tokio::time::sleep;

mod cursor;
mod events;
mod monitoring;

use cursor::{BlockCheckpoint, CursorState, ReorgIncident, load_cursor, save_cursor};
use events::{
    IndexedDeposit, indexed_deposit_from_log, indexed_withdrawal_event_from_log, sort_deposits,
    sort_withdrawal_events, withdrawal_event_request,
};
use monitoring::IndexerMetrics;

/// Default L1 confirmation depth.
///
/// The indexer only credits deposits at or below `latest - CONFIRMATIONS`, so a
/// reorg shallower than this window is absorbed by re-scanning before anything
/// reaches the sequencer. `2` is chosen for local Anvil, where blocks are
/// effectively final on mine and a deep reorg cannot occur; it keeps the dev
/// loop responsive without waiting. Public-chain operation MUST raise this to
/// the repository policy of 64 (and set the matching minimum) because crediting
/// an event that a deeper reorg later drops or replaces has no automatic inverse
/// transition.
const DEFAULT_CONFIRMATIONS: u64 = 2;
/// Fail-closed minimum when operators omit `SYBIL_L1_MIN_CONFIRMATIONS`.
/// Local development can still opt out explicitly with `0`.
const DEFAULT_MIN_CONFIRMATIONS: u64 = 2;
/// Repository operating policy for public PoS chains. This is deliberately
/// conservative and still does not replace finalized-tag/receipt-proof trust.
const RECOMMENDED_PUBLIC_CONFIRMATIONS: u64 = 64;

#[derive(Debug, Parser)]
struct Args {
    /// Ethereum JSON-RPC URL. For local dev this is usually Anvil.
    #[arg(
        long,
        env = "SYBIL_L1_RPC_URL",
        default_value = "http://127.0.0.1:8545"
    )]
    rpc_url: String,
    /// Sybil API base URL.
    #[arg(long, env = "SYBIL_API_URL", default_value = "http://127.0.0.1:3001")]
    sybil_api_url: String,
    /// Service bearer token for bridge ops routes. Dev-mode sybil-api accepts None.
    #[arg(long, env = "SYBIL_SERVICE_TOKEN")]
    sybil_service_token: Option<String>,
    /// Hex-encoded SybilVault address to index.
    #[arg(long, env = "SYBIL_L1_VAULT")]
    vault_address: String,
    /// L1 chain id used in deposit leaf hashing.
    #[arg(long, env = "SYBIL_L1_CHAIN_ID", default_value_t = 31_337)]
    chain_id: u64,
    /// First L1 block to scan.
    #[arg(long, env = "SYBIL_L1_START_BLOCK", default_value_t = 0)]
    start_block: u64,
    /// L1 confirmation depth: only credit deposits at or below
    /// `latest - confirmations`. Defaults to a dev-Anvil value; use at least 64
    /// for public/mainnet-like chains. See
    /// `DEFAULT_CONFIRMATIONS`.
    #[arg(long, env = "SYBIL_L1_CONFIRMATIONS", default_value_t = DEFAULT_CONFIRMATIONS)]
    confirmations: u64,
    /// Minimum L1 confirmation depth enforced at startup. Defaults fail-closed
    /// at 2; explicit `0` disables the guard for local development. For
    /// public/mainnet-like chains, configure at least 64.
    #[arg(
        long,
        env = "SYBIL_L1_MIN_CONFIRMATIONS",
        default_value_t = DEFAULT_MIN_CONFIRMATIONS
    )]
    min_confirmations: u64,
    /// Maximum eth_getLogs block span per poll.
    #[arg(
        long,
        env = "SYBIL_L1_MAX_BLOCK_SPAN",
        default_value_t = 1_000,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    max_block_span: u64,
    /// Poll interval in milliseconds.
    #[arg(long, env = "SYBIL_L1_POLL_MS", default_value_t = 1_000)]
    poll_ms: u64,
    /// Required path for the deployment-bound scan cursor, canonical block-hash
    /// checkpoint, and durable deep-reorg fail-stop latch.
    #[arg(long, env = "SYBIL_L1_CURSOR_PATH")]
    cursor_path: PathBuf,
    /// Bind address for the independent Prometheus and health listener.
    #[arg(long, env = "SYBIL_L1_METRICS_BIND", default_value = "0.0.0.0:9102")]
    metrics_bind: SocketAddr,
    /// Scan once and exit.
    #[arg(long, env = "SYBIL_L1_INDEX_ONCE", default_value_t = false)]
    once: bool,
}

#[derive(Debug, thiserror::Error)]
enum IndexerError {
    #[error("invalid hex for {field}: {message}")]
    InvalidHex {
        field: &'static str,
        message: String,
    },
    #[error("invalid Ethereum RPC URL: {0}")]
    InvalidRpcUrl(String),
    #[error("Ethereum JSON-RPC failed: {0}")]
    Rpc(#[from] alloy::transports::TransportError),
    #[error("Ethereum ABI decode failed: {0}")]
    Abi(#[from] alloy::sol_types::Error),
    #[error("missing JSON-RPC result")]
    MissingRpcResult,
    #[error("JSON decode failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cursor state I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("metrics server I/O failed: {0}")]
    MetricsIo(std::io::Error),
    #[error("Sybil API failed: {0}")]
    SybilApi(#[from] sybil_client::Error),
    #[error("L1 protocol error: {0}")]
    L1Protocol(#[from] L1ProtocolError),
    #[error(
        "deposit cursor gap: next Sybil deposit is {expected}, but L1 log has deposit {actual}"
    )]
    DepositGap { expected: u64, actual: u64 },
    /// The cumulative deposit root carried by a confirmed `DepositReceived` log
    /// does not match the canonical on-chain `depositRootByCount(id)`. This
    /// means an L1 reorg replaced/dropped the deposit (or the RPC lied). Crediting
    /// it would be unrecoverable, so this is fatal and fail-closed.
    #[error(
        "deposit root mismatch for deposit {deposit_id}: on-chain depositRootByCount={onchain}, \
         log root={log}; refusing to credit (L1 reorg or corruption)"
    )]
    DepositRootMismatch {
        deposit_id: u64,
        onchain: String,
        log: String,
    },
    /// The persisted cursor file was written for a different vault/chain. Reusing
    /// it could credit against the wrong config, so refuse fail-closed.
    #[error(
        "cursor state at {path} is for vault={stored_vault} chain={stored_chain}, but this run \
         targets vault={arg_vault} chain={arg_chain}"
    )]
    CursorConfigMismatch {
        path: String,
        stored_vault: String,
        stored_chain: u64,
        arg_vault: String,
        arg_chain: u64,
    },
    #[error(
        "cursor state at {path} uses schema {stored}, expected {expected}; refusing an \
         uncheckpointed or unknown recovery state"
    )]
    CursorSchemaMismatch {
        path: String,
        stored: u32,
        expected: u32,
    },
    #[error("cursor checkpoint at {path} is invalid: {message}")]
    CursorCheckpointInvalid { path: String, message: String },
    #[error(
        "cursor state at {path} is fail-stop latched after {context} mismatch at L1 block \
         {block_number}: expected {expected}, observed {observed}; preserve the cursor and \
         sequencer store and follow the L1 reorg recovery runbook"
    )]
    ReorgIncidentLatched {
        path: String,
        context: String,
        block_number: u64,
        expected: String,
        observed: String,
    },
    #[error(
        "canonical L1 block hash mismatch during {context} at block {block_number}: expected \
         {expected}, observed {observed}; refusing further bridge input"
    )]
    CanonicalBlockHashMismatch {
        context: &'static str,
        block_number: u64,
        expected: String,
        observed: String,
    },
    #[error(
        "unsafe L1 confirmation configuration: confirmations={confirmations} is below \
         min_confirmations={min_confirmations}; deep reorgs can mis-credit already-processed blocks"
    )]
    UnsafeConfirmations {
        confirmations: u64,
        min_confirmations: u64,
    },
    #[error(
        "configured start block {start_block} is ahead of persisted cursor {persisted_cursor}; \
         refusing to skip unprocessed L1 blocks (preserve the cursor; a new cursor path is \
         allowed only under the documented deployment/reorg recovery procedure)"
    )]
    StartBlockAheadOfCursor {
        start_block: u64,
        persisted_cursor: u64,
    },
}

impl IndexerError {
    /// Fatal errors must stop the process rather than being retried on the next
    /// poll. A canonical hash mismatch is also persisted as a fail-stop latch.
    fn is_fatal(&self) -> bool {
        matches!(
            self,
            IndexerError::DepositRootMismatch { .. }
                | IndexerError::CursorConfigMismatch { .. }
                | IndexerError::CursorSchemaMismatch { .. }
                | IndexerError::CursorCheckpointInvalid { .. }
                | IndexerError::ReorgIncidentLatched { .. }
                | IndexerError::CanonicalBlockHashMismatch { .. }
        )
    }

    fn metric_kind(&self) -> &'static str {
        match self {
            Self::InvalidHex { .. }
            | Self::InvalidRpcUrl(_)
            | Self::UnsafeConfirmations { .. }
            | Self::StartBlockAheadOfCursor { .. } => "configuration",
            Self::Rpc(_) | Self::MissingRpcResult => "rpc",
            Self::Abi(_) | Self::L1Protocol(_) => "l1_decode",
            Self::Json(_)
            | Self::CursorConfigMismatch { .. }
            | Self::CursorSchemaMismatch { .. }
            | Self::CursorCheckpointInvalid { .. } => "cursor_invalid",
            Self::Io(_) => "cursor_io",
            Self::MetricsIo(_) => "metrics_io",
            Self::SybilApi(_) => "sybil_api",
            Self::DepositGap { .. } => "deposit_gap",
            Self::DepositRootMismatch { .. } => "deposit_root_mismatch",
            Self::ReorgIncidentLatched { .. } => "reorg_latched",
            Self::CanonicalBlockHashMismatch { .. } => "canonical_hash_mismatch",
        }
    }

    fn is_rpc_failure(&self) -> bool {
        matches!(self, Self::Rpc(_) | Self::MissingRpcResult)
    }

    fn is_latched_reorg(&self) -> bool {
        matches!(self, Self::ReorgIncidentLatched { .. })
    }
}

type Result<T> = std::result::Result<T, IndexerError>;

fn check_confirmation_safety(confirmations: u64, min_confirmations: u64) -> Result<()> {
    if min_confirmations > 0 && confirmations < min_confirmations {
        return Err(IndexerError::UnsafeConfirmations {
            confirmations,
            min_confirmations,
        });
    }
    Ok(())
}

fn warn_if_low_confirmation_depth(confirmations: u64) {
    if confirmations < RECOMMENDED_PUBLIC_CONFIRMATIONS {
        tracing::warn!(
            "L1 confirmation depth {} is below the public-chain policy of {}; deep reorgs can \
             mis-credit already-processed blocks",
            confirmations,
            RECOMMENDED_PUBLIC_CONFIRMATIONS
        );
    }
}

fn effective_scan_start(start_block: u64, persisted_cursor: Option<u64>) -> Result<u64> {
    match persisted_cursor {
        Some(cursor) if start_block > cursor => Err(IndexerError::StartBlockAheadOfCursor {
            start_block,
            persisted_cursor: cursor,
        }),
        Some(cursor) => Ok(cursor),
        None => Ok(start_block),
    }
}

fn persist_reorg_latch(
    path: &std::path::Path,
    next_from: u64,
    vault_hex: &str,
    chain_id: u64,
    checkpoint: Option<BlockCheckpoint>,
    error: &IndexerError,
) -> Result<bool> {
    let IndexerError::CanonicalBlockHashMismatch {
        context,
        block_number,
        expected,
        observed,
    } = error
    else {
        return Ok(false);
    };
    let expected_hash = parse_hex_array::<32>(expected, "expected_block_hash")?;
    let observed_hash = parse_hex_array::<32>(observed, "observed_block_hash")?;
    let halted = CursorState::halted(
        next_from,
        vault_hex,
        chain_id,
        checkpoint,
        ReorgIncident::new(context, *block_number, expected_hash, observed_hash),
    );
    save_cursor(path, &halted)?;
    Ok(true)
}

/// L1 JSON-RPC surface the indexer depends on. Abstracted so tests can drive the
/// reorg/confirmation logic without a network.
trait L1Rpc {
    async fn block_number(&self) -> Result<u64>;
    async fn block_hash(&self, block_number: u64) -> Result<Bytes32>;
    async fn deposit_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>>;
    async fn withdrawal_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>>;
    /// `depositRootByCount(count)` read via `eth_call`, pinned to `block` so the
    /// root is read at the same height as the logs it reconciles.
    async fn deposit_root_by_count(
        &self,
        vault: EthAddress,
        count: u64,
        block: u64,
    ) -> Result<Bytes32>;
}

/// Sequencer-side sink for credited deposits. Abstracted for the same reason.
trait DepositSink {
    async fn bridge_status(&self) -> Result<BridgeStatusResponse>;
    async fn bridge_account_by_key(&self, key_hex: &str) -> Result<BridgeAccountKeyResponse>;
    async fn submit_l1_deposit(
        &self,
        req: &SubmitL1DepositRequest,
    ) -> Result<BridgeDepositResponse>;
    async fn submit_l1_withdrawal_event(
        &self,
        req: &SubmitL1WithdrawalEventRequest,
    ) -> Result<BridgeWithdrawalL1EventResponse>;
    async fn observe_l1_height(
        &self,
        req: &ObserveL1HeightRequest,
    ) -> Result<ObserveL1HeightResponse>;
}

struct HttpL1Rpc {
    provider: DynProvider,
}

impl L1Rpc for HttpL1Rpc {
    async fn block_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    async fn block_hash(&self, block_number: u64) -> Result<Bytes32> {
        let block = self
            .provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number))
            .await?
            .ok_or(IndexerError::MissingRpcResult)?;
        Ok(block.hash().into())
    }

    async fn deposit_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>> {
        let filter = Filter::new()
            .from_block(from_block)
            .to_block(to_block)
            .address(Address::from(vault))
            .event_signature(B256::from(deposit_received_topic0()));
        Ok(self.provider.get_logs(&filter).await?)
    }

    async fn withdrawal_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>> {
        let topics = Vec::from([
            withdrawal_queued_topic0(),
            withdrawal_finalized_topic0(),
            withdrawal_cancelled_topic0(),
        ])
        .into_iter()
        .map(B256::from)
        .collect::<Vec<_>>();
        let filter = Filter::new()
            .from_block(from_block)
            .to_block(to_block)
            .address(Address::from(vault))
            .event_signature(topics);
        Ok(self.provider.get_logs(&filter).await?)
    }

    async fn deposit_root_by_count(
        &self,
        vault: EthAddress,
        count: u64,
        block: u64,
    ) -> Result<Bytes32> {
        let call = SybilVault::depositRootByCountCall { count };
        let request = TransactionRequest::default()
            .to(Address::from(vault))
            .input(Bytes::from(call.abi_encode()).into());
        let output = self.provider.call(request).number(block).await?;
        Ok(SybilVault::depositRootByCountCall::abi_decode_returns_validate(&output)?.into())
    }
}

impl DepositSink for SybilClient {
    async fn bridge_status(&self) -> Result<BridgeStatusResponse> {
        Ok(SybilClient::bridge_status(self).await?)
    }

    async fn bridge_account_by_key(&self, key_hex: &str) -> Result<BridgeAccountKeyResponse> {
        Ok(SybilClient::bridge_account_by_key(self, key_hex).await?)
    }

    async fn submit_l1_deposit(
        &self,
        req: &SubmitL1DepositRequest,
    ) -> Result<BridgeDepositResponse> {
        Ok(SybilClient::submit_l1_deposit(self, req).await?)
    }

    async fn submit_l1_withdrawal_event(
        &self,
        req: &SubmitL1WithdrawalEventRequest,
    ) -> Result<BridgeWithdrawalL1EventResponse> {
        Ok(SybilClient::submit_l1_withdrawal_event(self, req).await?)
    }

    async fn observe_l1_height(
        &self,
        req: &ObserveL1HeightRequest,
    ) -> Result<ObserveL1HeightResponse> {
        Ok(SybilClient::observe_l1_height(self, req).await?)
    }
}

struct RunFailure {
    error: IndexerError,
    fatal: bool,
}

impl RunFailure {
    fn fatal(error: IndexerError) -> Self {
        Self { error, fatal: true }
    }

    fn transient(error: IndexerError) -> Self {
        Self {
            error,
            fatal: false,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let args = Args::parse();
    let metrics = IndexerMetrics::new();
    let listener = TcpListener::bind(args.metrics_bind)
        .await
        .map_err(IndexerError::MetricsIo)?;
    tracing::info!(address = %args.metrics_bind, "l1.indexer.monitoring_listening");

    let server = monitoring::serve(listener, metrics.clone());
    let indexer = run_indexer(&args, &metrics);
    tokio::pin!(server);
    tokio::pin!(indexer);

    tokio::select! {
        outcome = &mut indexer => match outcome {
            Ok(()) => Ok(()),
            Err(failure) if !failure.fatal => Err(failure.error),
            Err(failure) => {
                metrics.record_fatal(
                    failure.error.metric_kind(),
                    failure.error.is_latched_reorg(),
                );
                tracing::error!(error = %failure.error, "l1.indexer.fatal_metrics_only_mode");
                // Keep only /metrics and /healthz alive so the first scrape sees
                // the nonzero fatal counter instead of losing it on process exit.
                server.await.map_err(IndexerError::MetricsIo)?;
                Err(failure.error)
            }
        },
        server_result = &mut server => server_result.map_err(IndexerError::MetricsIo),
    }
}

async fn run_indexer(args: &Args, metrics: &IndexerMetrics) -> std::result::Result<(), RunFailure> {
    warn_if_low_confirmation_depth(args.confirmations);
    if let Err(error) = check_confirmation_safety(args.confirmations, args.min_confirmations) {
        tracing::error!(%error, "l1.indexer.unsafe_confirmation_config");
        return Err(RunFailure::fatal(error));
    }

    let vault_address =
        parse_hex_array::<20>(&args.vault_address, "vault_address").map_err(RunFailure::fatal)?;
    let vault_hex = hex::encode(vault_address);
    let http = reqwest::Client::new();
    let rpc_url = reqwest::Url::parse(&args.rpc_url)
        .map_err(|error| IndexerError::InvalidRpcUrl(error.to_string()))
        .map_err(RunFailure::fatal)?;
    let l1 = HttpL1Rpc {
        provider: ProviderBuilder::new()
            .connect_reqwest(http.clone(), rpc_url)
            .erased(),
    };
    let sybil = SybilClient::new(
        http.clone(),
        args.sybil_api_url.clone(),
        args.sybil_service_token.clone(),
    );

    let persisted_cursor =
        load_cursor(&args.cursor_path, &vault_hex, args.chain_id).map_err(RunFailure::fatal)?;
    let mut next_from = effective_scan_start(
        args.start_block,
        persisted_cursor.as_ref().map(|state| state.next_from),
    )
    .map_err(RunFailure::fatal)?;
    let mut checkpoint = persisted_cursor
        .as_ref()
        .and_then(|state| state.checkpoint.clone());
    match persisted_cursor.as_ref() {
        Some(state) => tracing::info!(
            effective_start = next_from,
            persisted_cursor = state.next_from,
            checkpoint_block = state.checkpoint.as_ref().map(|value| value.block_number),
            configured_start = args.start_block,
            reason = "persisted cursor and canonical checkpoint",
            "l1.indexer.scan_start"
        ),
        None => tracing::info!(
            effective_start = next_from,
            configured_start = args.start_block,
            reason = "no persisted cursor; use configured start block",
            "l1.indexer.scan_start"
        ),
    }
    metrics.mark_ready(
        next_from,
        checkpoint.as_ref().map(|value| value.block_number),
    );

    loop {
        match poll_once(
            &l1,
            &sybil,
            args,
            vault_address,
            next_from,
            checkpoint.as_ref(),
        )
        .await
        {
            Ok(poll) => {
                if let Some(progress) = poll.progress {
                    let state = CursorState::active(
                        progress.next_from,
                        &vault_hex,
                        args.chain_id,
                        progress.checkpoint.clone(),
                    );
                    if let Err(error) = save_cursor(&args.cursor_path, &state) {
                        metrics.record_cursor_persistence_failure();
                        return Err(RunFailure::fatal(error));
                    }
                    next_from = progress.next_from;
                    checkpoint = Some(progress.checkpoint);
                }
                metrics.record_successful_poll(
                    poll.latest_block,
                    poll.confirmed_tip_block,
                    next_from,
                    checkpoint.as_ref().map(|value| value.block_number),
                );
            }
            Err(error) => {
                match persist_reorg_latch(
                    &args.cursor_path,
                    next_from,
                    &vault_hex,
                    args.chain_id,
                    checkpoint.clone(),
                    &error,
                ) {
                    Ok(true) => {
                        metrics.mark_reorg_latched();
                        tracing::error!(
                            path = %args.cursor_path.display(),
                            "l1.indexer.reorg_latched"
                        );
                    }
                    Ok(false) => {}
                    Err(save_error) => {
                        metrics.record_cursor_persistence_failure();
                        tracing::error!(
                            reorg_error = %error,
                            %save_error,
                            path = %args.cursor_path.display(),
                            "l1.indexer.reorg_latch_persist_failed"
                        );
                        return Err(RunFailure::fatal(save_error));
                    }
                }
                if error.is_fatal() {
                    tracing::error!(%error, "l1.indexer.fatal");
                    return Err(RunFailure::fatal(error));
                }
                metrics.record_poll_failure(error.metric_kind(), error.is_rpc_failure());
                if args.once {
                    return Err(RunFailure::transient(error));
                }
                tracing::warn!(%error, "l1.indexer.poll_failed");
            }
        }

        if args.once {
            return Ok(());
        }
        sleep(Duration::from_millis(args.poll_ms)).await;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScanProgress {
    next_from: u64,
    checkpoint: BlockCheckpoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PollResult {
    progress: Option<ScanProgress>,
    latest_block: u64,
    confirmed_tip_block: u64,
}

async fn poll_once<L: L1Rpc, S: DepositSink>(
    l1: &L,
    sink: &S,
    args: &Args,
    vault_address: EthAddress,
    next_from: u64,
    checkpoint: Option<&BlockCheckpoint>,
) -> Result<PollResult> {
    if let Some(checkpoint) = checkpoint {
        let expected = checkpoint.block_hash(&args.cursor_path)?;
        require_canonical_block_hash(
            l1,
            checkpoint.block_number,
            expected,
            "persisted scan checkpoint",
        )
        .await?;
    }

    let latest = l1.block_number().await?;
    // Confirmation depth: never look past `latest - confirmations`. A reorg
    // shallower than this window is absorbed by re-scanning before any deposit
    // is credited.
    let confirmed_tip = latest.saturating_sub(args.confirmations);
    if confirmed_tip < next_from {
        return Ok(PollResult {
            progress: None,
            latest_block: latest,
            confirmed_tip_block: confirmed_tip,
        });
    }

    let to = confirmed_tip.min(next_from.saturating_add(args.max_block_span.saturating_sub(1)));
    let range_tip_hash = l1.block_hash(to).await?;
    let mut deposits = l1
        .deposit_logs(vault_address, next_from, to)
        .await?
        .into_iter()
        .map(indexed_deposit_from_log)
        .collect::<Result<Vec<_>>>()?;
    let mut withdrawal_events = l1
        .withdrawal_logs(vault_address, next_from, to)
        .await?
        .into_iter()
        .map(indexed_withdrawal_event_from_log)
        .collect::<Result<Vec<_>>>()?;
    sort_deposits(&mut deposits);
    sort_withdrawal_events(&mut withdrawal_events);
    validate_log_block_hashes(l1, &deposits, &withdrawal_events).await?;

    let status = sink.bridge_status().await?;
    let mut cursor = status.deposit_cursor;

    for deposit in deposits {
        if deposit.event.deposit_id <= cursor {
            tracing::debug!(
                deposit_id = deposit.event.deposit_id,
                cursor,
                "l1.indexer.deposit_skip_already_consumed"
            );
            continue;
        }
        let expected = cursor.saturating_add(1);
        if deposit.event.deposit_id != expected {
            return Err(IndexerError::DepositGap {
                expected,
                actual: deposit.event.deposit_id,
            });
        }

        // Reorg safety: before crediting, reconcile the log's cumulative deposit
        // root against the canonical on-chain root at the confirmed height. A
        // mismatch means the deposit we are about to credit is not the one the
        // canonical chain recorded at this id (replaced/dropped by a reorg deeper
        // than the confirmation window, or an RPC lie). Fail closed: do not
        // credit, halt loudly. Crediting the wrong deposit is unrecoverable.
        let onchain_root = l1
            .deposit_root_by_count(vault_address, deposit.event.deposit_id, to)
            .await?;
        if onchain_root != deposit.event.deposit_root {
            return Err(IndexerError::DepositRootMismatch {
                deposit_id: deposit.event.deposit_id,
                onchain: hex::encode(onchain_root),
                log: hex::encode(deposit.event.deposit_root),
            });
        }

        let account_id = match resolve_bridge_account(sink, deposit.event.sybil_account_key).await {
            Ok(account) => Some(account.account_id),
            Err(error) if is_unresolvable_key(&error) => {
                tracing::warn!(
                    deposit_id = deposit.event.deposit_id,
                    sybil_account_key = %hex::encode(deposit.event.sybil_account_key),
                    "l1.indexer.deposit_quarantining"
                );
                None
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    deposit_id = deposit.event.deposit_id,
                    sybil_account_key = %hex::encode(deposit.event.sybil_account_key),
                    "l1.indexer.deposit_pipeline_stalled"
                );
                return Err(error);
            }
        };
        let ingestion =
            submit_deposit(sink, args.chain_id, vault_address, &deposit, account_id).await;
        let response = match ingestion {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    %error,
                    deposit_id = deposit.event.deposit_id,
                    sybil_account_key = %hex::encode(deposit.event.sybil_account_key),
                    "l1.indexer.deposit_pipeline_stalled"
                );
                return Err(error);
            }
        };
        tracing::info!(
            deposit_id = response.deposit_id,
            account_id = response.account_id,
            balance_nanos = response.balance_nanos,
            disposition = response.disposition,
            tx = ?deposit.log.transaction_hash,
            "l1.indexer.deposit_ingested"
        );
        cursor = deposit.event.deposit_id;
    }

    for event in withdrawal_events {
        let request = withdrawal_event_request(&event)?;
        let response = sink.submit_l1_withdrawal_event(&request).await?;
        tracing::info!(
            withdrawal_id = response.withdrawal.as_ref().map(|withdrawal| withdrawal.withdrawal_id),
            nullifier = request.nullifier_hex,
            l1_status = ?request.status,
            executable_at_unix = request.executable_at_unix,
            tx = ?event.log.transaction_hash,
            "l1.indexer.withdrawal_status_ingested"
        );
    }

    // Re-read the range-tip hash after applying every event. A mid-poll reorg is
    // fatal: the cursor is not advanced and the incident is latched for manual
    // recovery instead of replaying ambiguous already-acknowledged inputs.
    require_canonical_block_hash(l1, to, range_tip_hash, "scan range stability").await?;

    // The confirmed scan cursor is the bridge clock. Advance it only after
    // every event and both hash checks succeeded, so a failed observation or
    // unstable range cannot acquire an authoritative cursor checkpoint.
    sink.observe_l1_height(&ObserveL1HeightRequest {
        l1_block_height: to,
    })
    .await?;

    Ok(PollResult {
        progress: Some(ScanProgress {
            next_from: to.saturating_add(1),
            checkpoint: BlockCheckpoint::new(to, range_tip_hash),
        }),
        latest_block: latest,
        confirmed_tip_block: confirmed_tip,
    })
}

#[cfg(test)]
async fn run_once<L: L1Rpc, S: DepositSink>(
    l1: &L,
    sink: &S,
    args: &Args,
    vault_address: EthAddress,
    next_from: u64,
    checkpoint: Option<&BlockCheckpoint>,
) -> Result<Option<ScanProgress>> {
    Ok(
        poll_once(l1, sink, args, vault_address, next_from, checkpoint)
            .await?
            .progress,
    )
}

async fn require_canonical_block_hash<L: L1Rpc>(
    l1: &L,
    block_number: u64,
    expected: Bytes32,
    context: &'static str,
) -> Result<()> {
    let observed = l1.block_hash(block_number).await?;
    if observed != expected {
        return Err(IndexerError::CanonicalBlockHashMismatch {
            context,
            block_number,
            expected: hex::encode(expected),
            observed: hex::encode(observed),
        });
    }
    Ok(())
}

async fn validate_log_block_hashes<L: L1Rpc>(
    l1: &L,
    deposits: &[IndexedDeposit],
    withdrawals: &[events::IndexedWithdrawalEvent],
) -> Result<()> {
    for deposit in deposits {
        validate_log_block_hash(l1, &deposit.log, "confirmed deposit log").await?;
    }
    for withdrawal in withdrawals {
        validate_log_block_hash(l1, &withdrawal.log, "confirmed withdrawal log").await?;
    }
    Ok(())
}

async fn validate_log_block_hash<L: L1Rpc>(
    l1: &L,
    log: &EthLog,
    context: &'static str,
) -> Result<()> {
    let block_number = log.block_number.ok_or(IndexerError::MissingRpcResult)?;
    let expected: Bytes32 = log.block_hash.ok_or(IndexerError::MissingRpcResult)?.into();
    require_canonical_block_hash(l1, block_number, expected, context).await
}

async fn resolve_bridge_account<S: DepositSink>(
    sink: &S,
    key: Bytes32,
) -> Result<BridgeAccountKeyResponse> {
    sink.bridge_account_by_key(&hex::encode(key)).await
}

fn is_unresolvable_key(error: &IndexerError) -> bool {
    matches!(
        error,
        IndexerError::SybilApi(sybil_client::Error::Api { status: 404, .. })
    )
}

async fn submit_deposit<S: DepositSink>(
    sink: &S,
    chain_id: u64,
    vault_address: EthAddress,
    deposit: &IndexedDeposit,
    account_id: Option<u64>,
) -> Result<BridgeDepositResponse> {
    // The sequencer reconstructs this leaf's prefix root from its persisted
    // frontier. A same-id substitution that retains the canonical vault root
    // fails there; one that recomputes its root fails depositRootByCount above
    // and SybilSettlement's independent vault check. This indexer still trusts
    // one RPC for headers, logs, and eth_call rather than authenticating receipt
    // inclusion/finality, so keep the API route service-gated: a fully dishonest
    // RPC can create temporary unprovable off-chain state even though L1 cannot
    // accept its checkpoint.
    let body = SubmitL1DepositRequest {
        deposit_id: deposit.event.deposit_id,
        account_id,
        quarantine: account_id.is_none(),
        chain_id,
        vault_address_hex: hex::encode(vault_address),
        token_address_hex: hex::encode(deposit.event.token_address),
        sender_hex: hex::encode(deposit.event.sender),
        sybil_account_key_hex: Some(hex::encode(deposit.event.sybil_account_key)),
        amount_token_units: deposit.event.amount_token_units,
        deposit_root_hex: hex::encode(deposit.event.deposit_root),
    };
    sink.submit_l1_deposit(&body).await
}

fn parse_hex_array<const N: usize>(value: &str, field: &'static str) -> Result<[u8; N]> {
    let bytes = parse_hex_bytes(value, field)?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| IndexerError::InvalidHex {
            field,
            message: format!("expected {N} bytes, got {}", bytes.len()),
        })
}

fn parse_hex_bytes(value: &str, field: &'static str) -> Result<Vec<u8>> {
    hex::decode(strip_hex_prefix(value)).map_err(|error| IndexerError::InvalidHex {
        field,
        message: error.to_string(),
    })
}

fn strip_hex_prefix(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Log as PrimitiveLog, U256};
    use alloy::sol_types::SolEvent;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tracing::instrument::WithSubscriber as _;

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl LogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(Arc::clone(&self.0))
        }
    }

    /// Build a well-formed DepositReceived EthLog for `deposit_id` at `block`
    /// with the given cumulative `root`.
    fn test_block_hash(block: u64) -> Bytes32 {
        let mut hash = [0x42; 32];
        hash[24..].copy_from_slice(&block.to_be_bytes());
        hash
    }

    fn deposit_log(deposit_id: u64, block: u64, root: Bytes32, amount: u64) -> EthLog {
        let event = SybilVault::DepositReceived {
            depositId: deposit_id,
            sender: Address::from([0x30; 20]),
            sybilAccountKey: B256::from([0x44; 32]),
            token: Address::from([0x20; 20]),
            amount: U256::from_limbs([amount, 0, 0, 0]),
            depositRoot: B256::from(root),
        };
        EthLog {
            inner: PrimitiveLog {
                address: Address::from([0x10; 20]),
                data: event.encode_log_data(),
            },
            block_number: Some(block),
            block_hash: Some(B256::from(test_block_hash(block))),
            transaction_hash: Some(B256::from([0xaa; 32])),
            log_index: Some(1),
            ..Default::default()
        }
    }

    fn withdrawal_queued_log(nullifier: Bytes32, block: u64, log_index: u64) -> EthLog {
        let event = SybilVault::WithdrawalQueued {
            nullifier: B256::from(nullifier),
            recipient: Address::from([0x30; 20]),
            token: Address::from([0x20; 20]),
            amount: U256::from_limbs([1_000_000, 0, 0, 0]),
            stateRoot: B256::from([0x55; 32]),
            height: 42,
            requestedAt: 1_700_000_000,
            executableAt: 1_700_086_400,
        };
        EthLog {
            inner: PrimitiveLog {
                address: Address::from([0x10; 20]),
                data: event.encode_log_data(),
            },
            block_number: Some(block),
            block_hash: Some(B256::from(test_block_hash(block))),
            transaction_hash: Some(B256::from([0xbb; 32])),
            log_index: Some(log_index),
            ..Default::default()
        }
    }

    #[derive(Default)]
    struct FakeL1 {
        latest: u64,
        logs: Vec<EthLog>,
        withdrawal_logs: Vec<EthLog>,
        deposit_log_ranges: Mutex<Vec<(u64, u64)>>,
        withdrawal_log_ranges: Mutex<Vec<(u64, u64)>>,
        fail_deposit_logs_once_at: Option<u64>,
        deposit_log_failure_triggered: AtomicBool,
        /// deposit_id -> canonical on-chain root returned by depositRootByCount.
        onchain_roots: std::collections::HashMap<u64, Bytes32>,
        /// Records (count, block) each reconciliation queried.
        reconciled: Mutex<Vec<(u64, u64)>>,
        /// Explicit canonical block hashes; unspecified blocks use
        /// `test_block_hash(number)`.
        block_hashes: std::collections::HashMap<u64, Bytes32>,
        block_hash_queries: Mutex<Vec<u64>>,
    }

    impl L1Rpc for FakeL1 {
        async fn block_number(&self) -> Result<u64> {
            Ok(self.latest)
        }

        async fn block_hash(&self, block_number: u64) -> Result<Bytes32> {
            self.block_hash_queries.lock().unwrap().push(block_number);
            Ok(self
                .block_hashes
                .get(&block_number)
                .copied()
                .unwrap_or_else(|| test_block_hash(block_number)))
        }

        async fn deposit_logs(
            &self,
            _vault: EthAddress,
            from_block: u64,
            to_block: u64,
        ) -> Result<Vec<EthLog>> {
            self.deposit_log_ranges
                .lock()
                .unwrap()
                .push((from_block, to_block));
            if self.fail_deposit_logs_once_at == Some(from_block)
                && !self
                    .deposit_log_failure_triggered
                    .swap(true, Ordering::SeqCst)
            {
                return Err(IndexerError::MissingRpcResult);
            }
            Ok(self
                .logs
                .iter()
                .filter(|log| {
                    let block = log.block_number.unwrap_or(u64::MAX);
                    block >= from_block && block <= to_block
                })
                .cloned()
                .collect())
        }

        async fn withdrawal_logs(
            &self,
            _vault: EthAddress,
            from_block: u64,
            to_block: u64,
        ) -> Result<Vec<EthLog>> {
            self.withdrawal_log_ranges
                .lock()
                .unwrap()
                .push((from_block, to_block));
            Ok(self
                .withdrawal_logs
                .iter()
                .filter(|log| {
                    let block = log.block_number.unwrap_or(u64::MAX);
                    block >= from_block && block <= to_block
                })
                .cloned()
                .collect())
        }

        async fn deposit_root_by_count(
            &self,
            _vault: EthAddress,
            count: u64,
            block: u64,
        ) -> Result<Bytes32> {
            self.reconciled.lock().unwrap().push((count, block));
            Ok(self.onchain_roots.get(&count).copied().unwrap_or([0u8; 32]))
        }
    }

    #[derive(Default)]
    struct FakeSink {
        cursor: u64,
        submitted: Mutex<Vec<u64>>,
        withdrawal_statuses: Mutex<Vec<(String, BridgeWithdrawalL1Status)>>,
        fail_deposit_submit: bool,
        fail_withdrawal_submit: bool,
        observed_heights: Mutex<Vec<u64>>,
        unresolvable_key: bool,
        quarantine_submitted: AtomicBool,
    }

    impl DepositSink for FakeSink {
        async fn bridge_status(&self) -> Result<BridgeStatusResponse> {
            Ok(BridgeStatusResponse {
                deposit_cursor: self.cursor,
                deposit_root_hex: String::new(),
                observed_l1_height: 0,
                next_withdrawal_id: 0,
                withdrawal_count: 0,
                queued_withdrawal_count: 0,
                finalized_withdrawal_count: 0,
                cancelled_withdrawal_count: 0,
                refunded_withdrawal_count: 0,
                quarantine_ledger_size: 0,
                total_quarantined_nanos: 0,
            })
        }

        async fn bridge_account_by_key(&self, _key_hex: &str) -> Result<BridgeAccountKeyResponse> {
            if self.unresolvable_key {
                return Err(IndexerError::SybilApi(sybil_client::Error::Api {
                    status: 404,
                    body: "bridge key not found".to_string(),
                }));
            }
            Ok(BridgeAccountKeyResponse {
                account_id: 42,
                sybil_account_key_hex: String::new(),
            })
        }

        async fn submit_l1_deposit(
            &self,
            req: &SubmitL1DepositRequest,
        ) -> Result<BridgeDepositResponse> {
            self.submitted.lock().unwrap().push(req.deposit_id);
            if req.quarantine {
                self.quarantine_submitted.store(true, Ordering::SeqCst);
            }
            if self.fail_deposit_submit {
                return Err(IndexerError::MissingRpcResult);
            }
            Ok(BridgeDepositResponse {
                account_id: req.account_id,
                balance_nanos: Some(0),
                disposition: if req.quarantine {
                    "quarantined".to_string()
                } else {
                    "credited".to_string()
                },
                deposit_id: req.deposit_id,
                deposit_root_hex: req.deposit_root_hex.clone(),
            })
        }

        async fn submit_l1_withdrawal_event(
            &self,
            req: &SubmitL1WithdrawalEventRequest,
        ) -> Result<BridgeWithdrawalL1EventResponse> {
            self.withdrawal_statuses
                .lock()
                .unwrap()
                .push((req.nullifier_hex.clone(), req.status));
            if self.fail_withdrawal_submit {
                return Err(IndexerError::MissingRpcResult);
            }
            Ok(BridgeWithdrawalL1EventResponse {
                active_withdrawal_found: true,
                withdrawal: Some(BridgeWithdrawalResponse {
                    withdrawal_id: 7,
                    account_id: 42,
                    recipient_hex: String::new(),
                    token_hex: String::new(),
                    amount_token_units: 1_000_000,
                    amount_nanos: 1_000_000_000,
                    expiry_height: 100,
                    nullifier_hex: req.nullifier_hex.clone(),
                    withdrawal_leaf_hex: String::new(),
                    withdrawal_leaf_digest_hex: String::new(),
                    created_at_height: 1,
                    l1_status: req.status,
                    l1_requested_at_unix: Some(req.event_at_unix),
                    l1_executable_at_unix: req.executable_at_unix,
                    l1_finalized_at_unix: None,
                    l1_cancelled_at_unix: None,
                    l1_tx_hash_hex: req.tx_hash_hex.clone(),
                }),
            })
        }

        async fn observe_l1_height(
            &self,
            req: &ObserveL1HeightRequest,
        ) -> Result<ObserveL1HeightResponse> {
            self.observed_heights
                .lock()
                .unwrap()
                .push(req.l1_block_height);
            Ok(ObserveL1HeightResponse {
                observed_l1_height: req.l1_block_height,
                refunded_withdrawal_ids: Vec::new(),
            })
        }
    }

    fn test_args(confirmations: u64) -> Args {
        Args {
            rpc_url: String::new(),
            sybil_api_url: String::new(),
            sybil_service_token: None,
            vault_address: hex::encode([0x10; 20]),
            chain_id: 31_337,
            start_block: 0,
            confirmations,
            min_confirmations: 0,
            max_block_span: 1_000,
            poll_ms: 0,
            cursor_path: PathBuf::from("test-l1-cursor.json"),
            metrics_bind: "127.0.0.1:0".parse().unwrap(),
            once: true,
        }
    }

    #[test]
    fn confirmation_safety_rejects_depth_below_configured_minimum() {
        assert!(matches!(
            check_confirmation_safety(11, 12),
            Err(IndexerError::UnsafeConfirmations {
                confirmations: 11,
                min_confirmations: 12,
            })
        ));
    }

    #[test]
    fn minimum_confirmation_default_fails_closed() {
        assert_eq!(DEFAULT_MIN_CONFIRMATIONS, 2);
    }

    #[test]
    fn confirmation_safety_accepts_depth_at_or_above_configured_minimum() {
        assert!(check_confirmation_safety(12, 12).is_ok());
        assert!(check_confirmation_safety(32, 12).is_ok());
    }

    #[test]
    fn confirmation_safety_disabled_minimum_accepts_any_depth() {
        assert!(check_confirmation_safety(0, 0).is_ok());
        assert!(check_confirmation_safety(u64::MAX, 0).is_ok());
    }

    #[test]
    fn no_cursor_uses_configured_start_block() {
        assert_eq!(effective_scan_start(123, None).unwrap(), 123);
    }

    #[test]
    fn configured_start_behind_cursor_resumes_without_rescanning() {
        assert_eq!(effective_scan_start(100, Some(123)).unwrap(), 123);
        assert_eq!(effective_scan_start(123, Some(123)).unwrap(), 123);
    }

    #[test]
    fn configured_start_ahead_of_cursor_is_refused() {
        assert!(matches!(
            effective_scan_start(124, Some(123)),
            Err(IndexerError::StartBlockAheadOfCursor {
                start_block: 124,
                persisted_cursor: 123,
            })
        ));
    }

    #[test]
    fn low_confirmation_depth_emits_reorg_warning() {
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(logs.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || warn_if_low_confirmation_depth(63));

        let logs = logs.contents();
        assert!(logs.contains("WARN"));
        assert!(logs.contains(
            "L1 confirmation depth 63 is below the public-chain policy of 64; deep reorgs can \
             mis-credit already-processed blocks"
        ));
    }

    #[test]
    fn parses_eth_deposit_log() {
        let root = [0x55; 32];
        let indexed = indexed_deposit_from_log(deposit_log(7, 2, root, 1_000_000)).unwrap();
        assert_eq!(indexed.event.deposit_id, 7);
        assert_eq!(indexed.event.sender, [0x30; 20]);
        assert_eq!(indexed.event.sybil_account_key, [0x44; 32]);
        assert_eq!(indexed.event.token_address, [0x20; 20]);
        assert_eq!(indexed.event.amount_token_units, 1_000_000);
        assert_eq!(indexed.event.deposit_root, root);
    }

    #[tokio::test]
    async fn credits_deposit_when_onchain_root_matches() {
        let vault = [0x10; 20];
        let root = [0x55; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, root, 1_000_000)],
            onchain_roots: [(1u64, root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let next = run_once(&l1, &sink, &args, vault, 0, None).await.unwrap();

        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        // Reconciliation happened at the confirmed height (latest - confirmations = 8).
        assert_eq!(l1.reconciled.lock().unwrap().as_slice(), &[(1, 8)]);
        assert_eq!(next.map(|progress| progress.next_from), Some(9));
    }

    #[tokio::test]
    async fn indexes_withdrawal_queue_event() {
        let vault = [0x10; 20];
        let nullifier = [0xab; 32];
        let l1 = FakeL1 {
            latest: 10,
            withdrawal_logs: vec![withdrawal_queued_log(nullifier, 2, 3)],
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let next = run_once(&l1, &sink, &args, vault, 0, None).await.unwrap();

        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(hex::encode(nullifier), BridgeWithdrawalL1Status::Queued)]
        );
        assert_eq!(next.map(|progress| progress.next_from), Some(9));
    }

    #[tokio::test]
    async fn withdrawal_submit_error_returns_without_advancing_cursor() {
        let vault = [0x10; 20];
        let nullifier = [0xab; 32];
        let l1 = FakeL1 {
            latest: 10,
            withdrawal_logs: vec![withdrawal_queued_log(nullifier, 2, 3)],
            ..Default::default()
        };
        let sink = FakeSink {
            fail_withdrawal_submit: true,
            ..Default::default()
        };
        let args = test_args(2);
        let next_from = 0;

        let result = run_once(&l1, &sink, &args, vault, next_from, None).await;

        assert!(matches!(result, Err(IndexerError::MissingRpcResult)));
        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(hex::encode(nullifier), BridgeWithdrawalL1Status::Queued)]
        );
        // `run_once` returned no next cursor, so the caller retains `next_from`
        // and retries this block on its next poll.
        assert_eq!(next_from, 0);
    }

    #[tokio::test]
    async fn deposit_submit_error_returns_without_advancing_cursor_and_logs_stall() {
        let vault = [0x10; 20];
        let root = [0x55; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, root, 1_000_000)],
            onchain_roots: [(1u64, root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink {
            fail_deposit_submit: true,
            ..Default::default()
        };
        let args = test_args(2);
        let next_from = 0;
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(logs.clone())
            .finish();

        let result = run_once(&l1, &sink, &args, vault, next_from, None)
            .with_subscriber(subscriber)
            .await;

        assert!(matches!(result, Err(IndexerError::MissingRpcResult)));
        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert_eq!(next_from, 0);
        let logs = logs.contents();
        assert!(logs.contains("ERROR"));
        assert!(logs.contains("l1.indexer.deposit_pipeline_stalled"));
    }

    #[tokio::test]
    async fn unresolvable_deposit_is_quarantined_and_scan_cursor_advances_without_retry() {
        let vault = [0x10; 20];
        let root = [0x55; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, root, 1_000_000)],
            onchain_roots: [(1u64, root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink {
            unresolvable_key: true,
            ..Default::default()
        };
        let args = test_args(2);

        let next = run_once(&l1, &sink, &args, vault, 0, None).await.unwrap();

        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert!(sink.quarantine_submitted.load(Ordering::SeqCst));
        assert_eq!(
            next.map(|progress| progress.next_from),
            Some(9),
            "successful quarantine disposes the deposit and advances the scan cursor"
        );
    }

    #[tokio::test]
    async fn deposit_and_withdrawal_success_advance_cursor() {
        let vault = [0x10; 20];
        let root = [0x55; 32];
        let nullifier = [0xab; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, root, 1_000_000)],
            withdrawal_logs: vec![withdrawal_queued_log(nullifier, 3, 2)],
            onchain_roots: [(1u64, root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let next = run_once(&l1, &sink, &args, vault, 0, None).await.unwrap();

        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(hex::encode(nullifier), BridgeWithdrawalL1Status::Queued)]
        );
        assert_eq!(next.map(|progress| progress.next_from), Some(9));
    }

    #[tokio::test]
    async fn deep_reorg_after_credited_deposit_halts_before_future_credit() {
        let vault = [0x10; 20];
        let first_root = [0x55; 32];
        let first_l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, first_root, 1_000_000)],
            onchain_roots: [(1u64, first_root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let progress = run_once(&first_l1, &sink, &args, vault, 0, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert_eq!(progress.checkpoint.block_number, 8);

        let replacement_hash = [0x99; 32];
        let second_root = [0x66; 32];
        let reorged_l1 = FakeL1 {
            latest: 12,
            logs: vec![deposit_log(2, 9, second_root, 2_000_000)],
            onchain_roots: [(2u64, second_root)].into_iter().collect(),
            block_hashes: [(8u64, replacement_hash)].into_iter().collect(),
            ..Default::default()
        };

        let error = run_once(
            &reorged_l1,
            &sink,
            &args,
            vault,
            progress.next_from,
            Some(&progress.checkpoint),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            IndexerError::CanonicalBlockHashMismatch {
                context: "persisted scan checkpoint",
                block_number: 8,
                ..
            }
        ));
        assert!(error.is_fatal());
        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert!(reorged_l1.deposit_log_ranges.lock().unwrap().is_empty());
        assert!(reorged_l1.withdrawal_log_ranges.lock().unwrap().is_empty());
        assert_eq!(sink.observed_heights.lock().unwrap().as_slice(), &[8]);
    }

    #[tokio::test]
    async fn deep_reorg_after_withdrawal_event_halts_before_future_lifecycle_input() {
        let vault = [0x10; 20];
        let first_nullifier = [0xab; 32];
        let first_l1 = FakeL1 {
            latest: 10,
            withdrawal_logs: vec![withdrawal_queued_log(first_nullifier, 2, 3)],
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let progress = run_once(&first_l1, &sink, &args, vault, 0, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(
                hex::encode(first_nullifier),
                BridgeWithdrawalL1Status::Queued
            )]
        );

        let second_nullifier = [0xcd; 32];
        let reorged_l1 = FakeL1 {
            latest: 12,
            withdrawal_logs: vec![withdrawal_queued_log(second_nullifier, 9, 1)],
            block_hashes: [(8u64, [0x99; 32])].into_iter().collect(),
            ..Default::default()
        };

        let error = run_once(
            &reorged_l1,
            &sink,
            &args,
            vault,
            progress.next_from,
            Some(&progress.checkpoint),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            IndexerError::CanonicalBlockHashMismatch {
                context: "persisted scan checkpoint",
                block_number: 8,
                ..
            }
        ));
        assert_eq!(sink.withdrawal_statuses.lock().unwrap().len(), 1);
        assert!(reorged_l1.deposit_log_ranges.lock().unwrap().is_empty());
        assert!(reorged_l1.withdrawal_log_ranges.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn withdrawal_log_hash_mismatch_halts_before_sequencer_submission() {
        let vault = [0x10; 20];
        let nullifier = [0xab; 32];
        let l1 = FakeL1 {
            latest: 10,
            withdrawal_logs: vec![withdrawal_queued_log(nullifier, 2, 3)],
            block_hashes: [(2u64, [0x77; 32])].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let error = run_once(&l1, &sink, &args, vault, 0, None)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            IndexerError::CanonicalBlockHashMismatch {
                context: "confirmed withdrawal log",
                block_number: 2,
                ..
            }
        ));
        assert!(sink.withdrawal_statuses.lock().unwrap().is_empty());
        assert!(sink.observed_heights.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn malicious_rpc_same_id_substitution_with_forged_root_is_not_credited() {
        // The substituted amount needs its own self-consistent frontier root to
        // pass the sequencer. An honest canonical depositRootByCount view still
        // exposes the original leaf's different root, so the indexer fails
        // before service submission.
        let vault = [0x10; 20];
        let canonical_leaf = sybil_l1_protocol::DepositLeaf {
            chain_id: 31_337,
            vault_address: vault,
            deposit_id: 1,
            token_address: [0x20; 20],
            sender: [0x30; 20],
            sybil_account_key: [0x44; 32],
            amount_token_units: 1_000_000,
        };
        let mut substituted_leaf = canonical_leaf.clone();
        substituted_leaf.amount_token_units += 1;
        let canonical_root = sybil_l1_protocol::deposit_root_from_prefix(&[canonical_leaf]);
        let forged_root = sybil_l1_protocol::deposit_root_from_prefix(&[substituted_leaf]);
        assert_ne!(forged_root, canonical_root);
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, forged_root, 1_000_001)],
            onchain_roots: [(1u64, canonical_root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let err = run_once(&l1, &sink, &args, vault, 0, None)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            IndexerError::DepositRootMismatch { deposit_id: 1, .. }
        ));
        assert!(err.is_fatal());
        // Never credited.
        assert!(sink.submitted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reorg_dropped_deposit_has_zero_onchain_root_and_is_not_credited() {
        // Canonical chain has no root for this id (mapping default 0) -> mismatch.
        let vault = [0x10; 20];
        let log_root = [0x55; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, log_root, 1_000_000)],
            onchain_roots: std::collections::HashMap::new(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let err = run_once(&l1, &sink, &args, vault, 0, None)
            .await
            .unwrap_err();
        assert!(matches!(err, IndexerError::DepositRootMismatch { .. }));
        assert!(sink.submitted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn confirmation_window_excludes_unconfirmed_tip() {
        // Deposit sits at block 10 == latest, inside the confirmation window, so
        // it must not be scanned/credited yet; the cursor does not advance past it.
        let vault = [0x10; 20];
        let root = [0x55; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 10, root, 1_000_000)],
            onchain_roots: [(1u64, root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(3); // confirmed_tip = 7

        let next = run_once(&l1, &sink, &args, vault, 0, None).await.unwrap();

        assert!(sink.submitted.lock().unwrap().is_empty());
        // Scanned up to confirmed_tip = 7, so next_from advances to 8 but the
        // deposit at block 10 is left for a later poll once it is deep enough.
        assert_eq!(next.map(|progress| progress.next_from), Some(8));
    }

    #[tokio::test]
    async fn returns_none_when_nothing_is_confirmed_yet() {
        let vault = [0x10; 20];
        let l1 = FakeL1 {
            latest: 1,
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2); // confirmed_tip = 0, but next_from = 5

        let next = run_once(&l1, &sink, &args, vault, 5, None).await.unwrap();
        assert_eq!(next, None);
        assert!(sink.submitted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn wide_gap_is_scanned_in_bounded_chunks_and_resumes_after_failure() {
        let vault = [0x10; 20];
        let l1 = FakeL1 {
            latest: 8,
            fail_deposit_logs_once_at: Some(3),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let mut args = test_args(0);
        args.max_block_span = 3;
        let mut cursor = 0;
        let mut checkpoint = None;

        let progress = run_once(&l1, &sink, &args, vault, cursor, checkpoint.as_ref())
            .await
            .unwrap()
            .unwrap();
        cursor = progress.next_from;
        checkpoint = Some(progress.checkpoint);
        assert_eq!(cursor, 3);

        let failed_cursor = cursor;
        assert!(matches!(
            run_once(&l1, &sink, &args, vault, cursor, checkpoint.as_ref()).await,
            Err(IndexerError::MissingRpcResult)
        ));
        assert_eq!(cursor, failed_cursor);

        let progress = run_once(&l1, &sink, &args, vault, cursor, checkpoint.as_ref())
            .await
            .unwrap()
            .unwrap();
        cursor = progress.next_from;
        checkpoint = Some(progress.checkpoint);
        assert_eq!(cursor, 6);
        let progress = run_once(&l1, &sink, &args, vault, cursor, checkpoint.as_ref())
            .await
            .unwrap()
            .unwrap();
        cursor = progress.next_from;
        checkpoint = Some(progress.checkpoint);
        assert_eq!(cursor, 9);
        assert_eq!(
            run_once(&l1, &sink, &args, vault, cursor, checkpoint.as_ref())
                .await
                .unwrap(),
            None
        );

        let deposit_ranges = l1.deposit_log_ranges.lock().unwrap();
        assert_eq!(deposit_ranges.as_slice(), &[(0, 2), (3, 5), (3, 5), (6, 8)]);
        assert!(
            deposit_ranges
                .iter()
                .all(|(from, to)| from <= to && to - from < args.max_block_span)
        );
        let withdrawal_ranges = l1.withdrawal_log_ranges.lock().unwrap();
        assert_eq!(withdrawal_ranges.as_slice(), &[(0, 2), (3, 5), (6, 8)]);
        assert!(
            withdrawal_ranges
                .iter()
                .all(|(from, to)| from <= to && to - from < args.max_block_span)
        );
        assert_eq!(sink.observed_heights.lock().unwrap().as_slice(), &[2, 5, 8]);
    }

    #[test]
    fn cursor_persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("syb190-cursor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);

        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), None);

        let state = CursorState::active(
            123,
            &vault_hex,
            31_337,
            BlockCheckpoint::new(122, test_block_hash(122)),
        );
        save_cursor(&path, &state).unwrap();
        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), Some(state));

        // Overwrite with a later cursor.
        let later = CursorState::active(
            456,
            &vault_hex,
            31_337,
            BlockCheckpoint::new(455, test_block_hash(455)),
        );
        save_cursor(&path, &later).unwrap();
        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), Some(later));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cursor_rejects_mismatched_vault_or_chain() {
        let dir =
            std::env::temp_dir().join(format!("syb190-cursor-mismatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);
        let state = CursorState::active(
            7,
            &vault_hex,
            31_337,
            BlockCheckpoint::new(6, test_block_hash(6)),
        );
        save_cursor(&path, &state).unwrap();

        let other_vault = hex::encode([0x99; 20]);
        assert!(matches!(
            load_cursor(&path, &other_vault, 31_337),
            Err(IndexerError::CursorConfigMismatch { .. })
        ));
        assert!(matches!(
            load_cursor(&path, &vault_hex, 1),
            Err(IndexerError::CursorConfigMismatch { .. })
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_cursor_without_checkpoint_is_rejected() {
        let dir = std::env::temp_dir().join(format!("syb62-legacy-cursor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);
        std::fs::write(
            &path,
            format!("{{\"next_from\":7,\"vault_address_hex\":\"{vault_hex}\",\"chain_id\":31337}}"),
        )
        .unwrap();

        assert!(matches!(
            load_cursor(&path, &vault_hex, 31_337),
            Err(IndexerError::CursorSchemaMismatch {
                stored: 0,
                expected: 2,
                ..
            })
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn invalid_cursor_startup_exports_first_scrape_fatal_signal() {
        let dir = std::env::temp_dir().join(format!(
            "syb85-invalid-cursor-metrics-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        std::fs::write(
            &path,
            r#"{
                "next_from": 9,
                "vault_address_hex": "1010101010101010101010101010101010101010",
                "chain_id": 31337
            }"#,
        )
        .unwrap();

        let mut args = test_args(2);
        args.rpc_url = "http://127.0.0.1:8545".to_string();
        args.cursor_path = path;
        let metrics = IndexerMetrics::new();
        let failure = run_indexer(&args, &metrics).await.unwrap_err();
        assert!(failure.fatal);
        assert_eq!(failure.error.metric_kind(), "cursor_invalid");

        // This is the same transition main applies before awaiting the metrics
        // server forever in recovery-only mode.
        metrics.record_fatal(
            failure.error.metric_kind(),
            failure.error.is_latched_reorg(),
        );
        let rendered = metrics.render();
        assert!(rendered.contains("sybil_l1_indexer_ready 0"));
        assert!(
            rendered.contains("sybil_l1_indexer_fatal_failures_total{kind=\"cursor_invalid\"} 1")
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn persisted_reorg_incident_is_fail_stop_latched_across_restart() {
        let dir = std::env::temp_dir().join(format!("syb62-reorg-latch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);
        let expected = test_block_hash(8);
        let observed = [0x99; 32];
        let error = IndexerError::CanonicalBlockHashMismatch {
            context: "persisted scan checkpoint",
            block_number: 8,
            expected: hex::encode(expected),
            observed: hex::encode(observed),
        };
        assert!(
            persist_reorg_latch(
                &path,
                9,
                &vault_hex,
                31_337,
                Some(BlockCheckpoint::new(8, expected)),
                &error,
            )
            .unwrap()
        );

        assert!(matches!(
            load_cursor(&path, &vault_hex, 31_337),
            Err(IndexerError::ReorgIncidentLatched {
                context,
                block_number: 8,
                expected: expected_hex,
                observed: observed_hex,
                ..
            }) if context == "persisted scan checkpoint"
                && expected_hex == hex::encode(expected)
                && observed_hex == hex::encode(observed)
        ));

        let mut args = test_args(2);
        args.rpc_url = "http://127.0.0.1:8545".to_string();
        args.cursor_path = path;
        let metrics = IndexerMetrics::new();
        let failure = run_indexer(&args, &metrics).await.unwrap_err();
        assert!(failure.fatal);
        assert_eq!(failure.error.metric_kind(), "reorg_latched");
        assert!(failure.error.is_latched_reorg());

        metrics.record_fatal(
            failure.error.metric_kind(),
            failure.error.is_latched_reorg(),
        );
        let rendered = metrics.render();
        assert!(rendered.contains("sybil_l1_indexer_reorg_latched 1"));
        assert!(
            rendered.contains("sybil_l1_indexer_fatal_failures_total{kind=\"reorg_latched\"} 1")
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
