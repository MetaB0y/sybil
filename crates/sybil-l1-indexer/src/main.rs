use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sybil_api_types::request::{
    BridgeWithdrawalL1Status, SubmitL1DepositRequest, SubmitL1WithdrawalEventRequest,
};
use sybil_api_types::response::{
    BridgeAccountKeyResponse, BridgeDepositResponse, BridgeStatusResponse, BridgeWithdrawalResponse,
};
use sybil_client::SybilClient;
use sybil_l1_protocol::{
    deposit_received_topic0, deposit_root_by_count_calldata, parse_deposit_received_log,
    parse_withdrawal_event_log, withdrawal_cancelled_topic0, withdrawal_finalized_topic0,
    withdrawal_queued_topic0, Bytes32, EthAddress, L1Log, L1ProtocolError, WithdrawalEvent,
};
use tokio::time::sleep;

/// Default L1 confirmation depth.
///
/// The indexer only credits deposits at or below `latest - CONFIRMATIONS`, so a
/// reorg shallower than this window is absorbed by re-scanning before anything
/// reaches the sequencer. `2` is chosen for local Anvil, where blocks are
/// effectively final on mine and a deep reorg cannot occur; it keeps the dev
/// loop responsive without waiting. Production against a public chain MUST raise
/// this to something like 12-32 (e.g. `SYBIL_L1_CONFIRMATIONS=32`) because
/// crediting a deposit that a reorg later drops or replaces is unrecoverable
/// (`ingest_l1_deposit` mutates the deposit cursor/root irreversibly).
const DEFAULT_CONFIRMATIONS: u64 = 2;
/// Fail-closed minimum when operators omit `SYBIL_L1_MIN_CONFIRMATIONS`.
/// Local development can still opt out explicitly with `0`.
const DEFAULT_MIN_CONFIRMATIONS: u64 = 2;

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
    /// `latest - confirmations`. Defaults to a dev-Anvil value; use 12–32 for
    /// public/mainnet-like chains. See `DEFAULT_CONFIRMATIONS`.
    #[arg(long, env = "SYBIL_L1_CONFIRMATIONS", default_value_t = DEFAULT_CONFIRMATIONS)]
    confirmations: u64,
    /// Minimum L1 confirmation depth enforced at startup. Defaults fail-closed
    /// at 2; explicit `0` disables the guard for local development. For
    /// public/mainnet-like chains, configure a value in the recommended 12–32
    /// range.
    #[arg(
        long,
        env = "SYBIL_L1_MIN_CONFIRMATIONS",
        default_value_t = DEFAULT_MIN_CONFIRMATIONS
    )]
    min_confirmations: u64,
    /// Maximum eth_getLogs block span per poll.
    #[arg(long, env = "SYBIL_L1_MAX_BLOCK_SPAN", default_value_t = 1_000)]
    max_block_span: u64,
    /// Poll interval in milliseconds.
    #[arg(long, env = "SYBIL_L1_POLL_MS", default_value_t = 1_000)]
    poll_ms: u64,
    /// Optional path to persist the scan cursor (`next_from`) so restarts do not
    /// rescan from `start_block`. Lightweight JSON state file; no DB.
    #[arg(long, env = "SYBIL_L1_CURSOR_PATH")]
    cursor_path: Option<PathBuf>,
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
    #[error("invalid Ethereum quantity for {field}: {value}")]
    InvalidQuantity { field: &'static str, value: String },
    #[error("Ethereum JSON-RPC error {code}: {message}")]
    RpcError { code: i64, message: String },
    #[error("missing JSON-RPC result")]
    MissingRpcResult,
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON decode failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cursor state I/O failed: {0}")]
    Io(#[from] std::io::Error),
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
        "unsafe L1 confirmation configuration: confirmations={confirmations} is below \
         min_confirmations={min_confirmations}; deep reorgs can mis-credit already-processed blocks"
    )]
    UnsafeConfirmations {
        confirmations: u64,
        min_confirmations: u64,
    },
}

impl IndexerError {
    /// Fatal errors must stop the process rather than being retried on the next
    /// poll: a detected reorg (`DepositRootMismatch`) or a misconfigured cursor
    /// (`CursorConfigMismatch`) will not fix themselves, and continuing to poll
    /// risks crediting deposits built on a divergent chain view.
    fn is_fatal(&self) -> bool {
        matches!(
            self,
            IndexerError::DepositRootMismatch { .. } | IndexerError::CursorConfigMismatch { .. }
        )
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
    if confirmations < 12 {
        tracing::warn!(
            "L1 confirmation depth {} is below the recommended 12–32; deep reorgs can \
             mis-credit already-processed blocks",
            confirmations
        );
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct EthLog {
    address: String,
    topics: Vec<String>,
    data: String,
    block_number: Option<String>,
    transaction_hash: Option<String>,
    log_index: Option<String>,
}

#[derive(Clone, Debug)]
struct IndexedDeposit {
    log: EthLog,
    event: sybil_l1_protocol::DepositReceived,
}

#[derive(Clone, Debug)]
struct IndexedWithdrawalEvent {
    log: EthLog,
    event: WithdrawalEvent,
}

/// Persisted scan cursor. Stored alongside the targeted vault/chain so a cursor
/// left over from a different deployment is rejected rather than silently reused.
#[derive(Debug, Serialize, Deserialize)]
struct CursorState {
    next_from: u64,
    vault_address_hex: String,
    chain_id: u64,
}

/// L1 JSON-RPC surface the indexer depends on. Abstracted so tests can drive the
/// reorg/confirmation logic without a network.
trait L1Rpc {
    async fn block_number(&self) -> Result<u64>;
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
    ) -> Result<BridgeWithdrawalResponse>;
}

struct HttpL1Rpc {
    client: reqwest::Client,
    rpc_url: String,
}

impl L1Rpc for HttpL1Rpc {
    async fn block_number(&self) -> Result<u64> {
        let value: String =
            rpc_call(&self.client, &self.rpc_url, "eth_blockNumber", json!([])).await?;
        parse_quantity(&value, "blockNumber")
    }

    async fn deposit_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>> {
        let topic0 = format!("0x{}", hex::encode(deposit_received_topic0()));
        let filter = json!({
            "fromBlock": quantity_hex(from_block),
            "toBlock": quantity_hex(to_block),
            "address": format!("0x{}", hex::encode(vault)),
            "topics": [topic0],
        });
        rpc_call(&self.client, &self.rpc_url, "eth_getLogs", json!([filter])).await
    }

    async fn withdrawal_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EthLog>> {
        let topics = [
            withdrawal_queued_topic0(),
            withdrawal_finalized_topic0(),
            withdrawal_cancelled_topic0(),
        ]
        .map(|topic| format!("0x{}", hex::encode(topic)));
        let filter = json!({
            "fromBlock": quantity_hex(from_block),
            "toBlock": quantity_hex(to_block),
            "address": format!("0x{}", hex::encode(vault)),
            "topics": [topics],
        });
        rpc_call(&self.client, &self.rpc_url, "eth_getLogs", json!([filter])).await
    }

    async fn deposit_root_by_count(
        &self,
        vault: EthAddress,
        count: u64,
        block: u64,
    ) -> Result<Bytes32> {
        let calldata = deposit_root_by_count_calldata(count);
        let call = json!({
            "to": format!("0x{}", hex::encode(vault)),
            "data": format!("0x{}", hex::encode(calldata)),
        });
        let value: String = rpc_call(
            &self.client,
            &self.rpc_url,
            "eth_call",
            json!([call, quantity_hex(block)]),
        )
        .await?;
        parse_hex_array::<32>(&value, "depositRootByCount")
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
    ) -> Result<BridgeWithdrawalResponse> {
        Ok(SybilClient::submit_l1_withdrawal_event(self, req).await?)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let args = Args::parse();
    warn_if_low_confirmation_depth(args.confirmations);
    if let Err(error) = check_confirmation_safety(args.confirmations, args.min_confirmations) {
        tracing::error!(%error, "l1.indexer.unsafe_confirmation_config");
        return Err(error);
    }

    let vault_address = parse_hex_array::<20>(&args.vault_address, "vault_address")?;
    let vault_hex = hex::encode(vault_address);
    let http = reqwest::Client::new();
    let l1 = HttpL1Rpc {
        client: http.clone(),
        rpc_url: args.rpc_url.clone(),
    };
    let sybil = SybilClient::new(
        http.clone(),
        args.sybil_api_url.clone(),
        args.sybil_service_token.clone(),
    );

    let mut next_from = args.start_block;
    if let Some(path) = args.cursor_path.as_deref() {
        if let Some(persisted) = load_cursor(path, &vault_hex, args.chain_id)? {
            next_from = persisted.max(args.start_block);
            tracing::info!(next_from, path = %path.display(), "l1.indexer.cursor_restored");
        }
    }

    loop {
        match run_once(&l1, &sybil, &args, vault_address, next_from).await {
            Ok(Some(next)) => {
                next_from = next;
                if let Some(path) = args.cursor_path.as_deref() {
                    save_cursor(path, next_from, &vault_hex, args.chain_id)?;
                }
            }
            Ok(None) => {}
            Err(error) => {
                if error.is_fatal() {
                    tracing::error!(%error, "l1.indexer.fatal");
                    return Err(error);
                }
                if args.once {
                    return Err(error);
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

async fn run_once<L: L1Rpc, S: DepositSink>(
    l1: &L,
    sink: &S,
    args: &Args,
    vault_address: EthAddress,
    next_from: u64,
) -> Result<Option<u64>> {
    let latest = l1.block_number().await?;
    // Confirmation depth: never look past `latest - confirmations`. A reorg
    // shallower than this window is absorbed by re-scanning before any deposit
    // is credited.
    let confirmed_tip = latest.saturating_sub(args.confirmations);
    if confirmed_tip < next_from {
        return Ok(None);
    }

    let to = confirmed_tip.min(next_from.saturating_add(args.max_block_span.saturating_sub(1)));
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

        // The no-gap deposit cursor means this event cannot be skipped. The
        // vault integration must guarantee that a depositor's Sybil account key
        // is registered before accepting a deposit; otherwise account resolution
        // can never succeed and this deposit intentionally blocks all later ones.
        let ingestion = async {
            let account = resolve_bridge_account(sink, deposit.event.sybil_account_key).await?;
            submit_deposit(
                sink,
                args.chain_id,
                vault_address,
                &deposit,
                account.account_id,
            )
            .await
        }
        .await;
        let response = match ingestion {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    %error,
                    deposit_id = deposit.event.deposit_id,
                    sybil_account_key = %hex::encode(deposit.event.sybil_account_key),
                    "deposit pipeline stalled; refusing to skip the next required deposit"
                );
                return Err(error);
            }
        };
        tracing::info!(
            deposit_id = response.deposit_id,
            account_id = response.account_id,
            balance_nanos = response.balance_nanos,
            tx = deposit.log.transaction_hash.as_deref().unwrap_or_default(),
            "l1.indexer.deposit_ingested"
        );
        cursor = deposit.event.deposit_id;
    }

    for event in withdrawal_events {
        let request = withdrawal_event_request(&event);
        let response = sink.submit_l1_withdrawal_event(&request).await?;
        tracing::info!(
            withdrawal_id = response.withdrawal_id,
            nullifier = response.nullifier_hex,
            l1_status = ?request.status,
            executable_at_unix = request.executable_at_unix,
            tx = event.log.transaction_hash.as_deref().unwrap_or_default(),
            "l1.indexer.withdrawal_status_ingested"
        );
    }

    Ok(Some(to.saturating_add(1)))
}

async fn rpc_call<T: DeserializeOwned>(
    client: &reqwest::Client,
    rpc_url: &str,
    method: &'static str,
    params: Value,
) -> Result<T> {
    let response = client
        .post(rpc_url)
        .json(&JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        })
        .send()
        .await?;
    let body: JsonRpcResponse<T> = response.error_for_status()?.json().await?;
    if let Some(error) = body.error {
        return Err(IndexerError::RpcError {
            code: error.code,
            message: error.message,
        });
    }
    body.result.ok_or(IndexerError::MissingRpcResult)
}

async fn resolve_bridge_account<S: DepositSink>(
    sink: &S,
    key: Bytes32,
) -> Result<BridgeAccountKeyResponse> {
    sink.bridge_account_by_key(&hex::encode(key)).await
}

async fn submit_deposit<S: DepositSink>(
    sink: &S,
    chain_id: u64,
    vault_address: EthAddress,
    deposit: &IndexedDeposit,
    account_id: u64,
) -> Result<BridgeDepositResponse> {
    // TODO(SYB-188/SYB-178): this dev indexer trusts eth_getLogs from its RPC
    // and does not prove receipt inclusion/finality. The confirmation-depth +
    // on-chain root reconciliation added in SYB-190 defends against reorgs, but
    // the sequencer still credits the first deposit it sees for an id and cannot
    // reject a same-id replacement (`ingest_l1_deposit` only rejects
    // non-sequential ids). Keep the API route service-gated until deposit
    // soundness is proof-backed and the sequencer rejects same-id replacement.
    let body = SubmitL1DepositRequest {
        deposit_id: deposit.event.deposit_id,
        account_id,
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

fn indexed_deposit_from_log(log: EthLog) -> Result<IndexedDeposit> {
    let l1_log = L1Log {
        address: parse_hex_array(&log.address, "log.address")?,
        topics: log
            .topics
            .iter()
            .map(|topic| parse_hex_array(topic, "log.topic"))
            .collect::<Result<Vec<Bytes32>>>()?,
        data: parse_hex_bytes(&log.data, "log.data")?,
    };
    let event = parse_deposit_received_log(&l1_log)?;
    Ok(IndexedDeposit { log, event })
}

fn indexed_withdrawal_event_from_log(log: EthLog) -> Result<IndexedWithdrawalEvent> {
    let l1_log = L1Log {
        address: parse_hex_array(&log.address, "log.address")?,
        topics: log
            .topics
            .iter()
            .map(|topic| parse_hex_array(topic, "log.topic"))
            .collect::<Result<Vec<Bytes32>>>()?,
        data: parse_hex_bytes(&log.data, "log.data")?,
    };
    let event = parse_withdrawal_event_log(&l1_log)?;
    Ok(IndexedWithdrawalEvent { log, event })
}

fn sort_deposits(deposits: &mut [IndexedDeposit]) {
    deposits.sort_by_key(|deposit| {
        (
            deposit
                .log
                .block_number
                .as_deref()
                .and_then(|value| parse_quantity(value, "blockNumber").ok())
                .unwrap_or(u64::MAX),
            deposit
                .log
                .log_index
                .as_deref()
                .and_then(|value| parse_quantity(value, "logIndex").ok())
                .unwrap_or(u64::MAX),
        )
    });
}

fn sort_withdrawal_events(events: &mut [IndexedWithdrawalEvent]) {
    events.sort_by_key(|event| {
        (
            event
                .log
                .block_number
                .as_deref()
                .and_then(|value| parse_quantity(value, "blockNumber").ok())
                .unwrap_or(u64::MAX),
            event
                .log
                .log_index
                .as_deref()
                .and_then(|value| parse_quantity(value, "logIndex").ok())
                .unwrap_or(u64::MAX),
        )
    });
}

fn withdrawal_event_request(event: &IndexedWithdrawalEvent) -> SubmitL1WithdrawalEventRequest {
    let (nullifier, status, event_at_unix, executable_at_unix) = match &event.event {
        WithdrawalEvent::Queued(queued) => (
            queued.nullifier,
            BridgeWithdrawalL1Status::Queued,
            queued.requested_at_unix,
            Some(queued.executable_at_unix),
        ),
        WithdrawalEvent::Finalized(finalized) => (
            finalized.nullifier,
            BridgeWithdrawalL1Status::Finalized,
            finalized.finalized_at_unix,
            Some(finalized.executable_at_unix),
        ),
        WithdrawalEvent::Cancelled(cancelled) => (
            cancelled.nullifier,
            BridgeWithdrawalL1Status::Cancelled,
            cancelled.cancelled_at_unix,
            Some(cancelled.executable_at_unix),
        ),
    };
    SubmitL1WithdrawalEventRequest {
        nullifier_hex: hex::encode(nullifier),
        status,
        event_at_unix,
        executable_at_unix,
        tx_hash_hex: event.transaction_hash_hex(),
    }
}

impl IndexedWithdrawalEvent {
    fn transaction_hash_hex(&self) -> Option<String> {
        self.log
            .transaction_hash
            .as_deref()
            .map(strip_hex_prefix)
            .map(ToOwned::to_owned)
    }
}

/// Load the persisted scan cursor, or `None` if no file exists. Fails closed if
/// the file targets a different vault/chain than this run.
fn load_cursor(path: &Path, vault_hex: &str, chain_id: u64) -> Result<Option<u64>> {
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
fn save_cursor(path: &Path, next_from: u64, vault_hex: &str, chain_id: u64) -> Result<()> {
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

fn parse_quantity(value: &str, field: &'static str) -> Result<u64> {
    u64::from_str_radix(strip_hex_prefix(value), 16).map_err(|_| IndexerError::InvalidQuantity {
        field,
        value: value.to_string(),
    })
}

fn quantity_hex(value: u64) -> String {
    format!("0x{value:x}")
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
    use std::io::Write;
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

    fn abi_u64_word(value: u64) -> Bytes32 {
        let mut out = [0u8; 32];
        out[24..].copy_from_slice(&value.to_be_bytes());
        out
    }

    fn abi_address_word(value: EthAddress) -> Bytes32 {
        let mut out = [0u8; 32];
        out[12..].copy_from_slice(&value);
        out
    }

    /// Build a well-formed DepositReceived EthLog for `deposit_id` at `block`
    /// with the given cumulative `root`.
    fn deposit_log(deposit_id: u64, block: u64, root: Bytes32, amount: u64) -> EthLog {
        let token = [0x20; 20];
        let sender = [0x30; 20];
        let key = [0x44; 32];
        let mut data = Vec::new();
        data.extend_from_slice(&abi_address_word(token));
        data.extend_from_slice(&abi_u64_word(amount));
        data.extend_from_slice(&root);
        EthLog {
            address: format!("0x{}", hex::encode([0x10; 20])),
            topics: vec![
                format!("0x{}", hex::encode(deposit_received_topic0())),
                format!("0x{}", hex::encode(abi_u64_word(deposit_id))),
                format!("0x{}", hex::encode(abi_address_word(sender))),
                format!("0x{}", hex::encode(key)),
            ],
            data: format!("0x{}", hex::encode(data)),
            block_number: Some(quantity_hex(block)),
            transaction_hash: Some(format!("0x{}", hex::encode([0xaa; 32]))),
            log_index: Some("0x1".to_string()),
        }
    }

    fn withdrawal_queued_log(nullifier: Bytes32, block: u64, log_index: u64) -> EthLog {
        let token = [0x20; 20];
        let recipient = [0x30; 20];
        let mut data = Vec::new();
        data.extend_from_slice(&abi_address_word(token));
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&[0x55; 32]);
        data.extend_from_slice(&abi_u64_word(42));
        data.extend_from_slice(&abi_u64_word(1_700_000_000));
        data.extend_from_slice(&abi_u64_word(1_700_086_400));
        EthLog {
            address: format!("0x{}", hex::encode([0x10; 20])),
            topics: vec![
                format!("0x{}", hex::encode(withdrawal_queued_topic0())),
                format!("0x{}", hex::encode(nullifier)),
                format!("0x{}", hex::encode(abi_address_word(recipient))),
            ],
            data: format!("0x{}", hex::encode(data)),
            block_number: Some(quantity_hex(block)),
            transaction_hash: Some(format!("0x{}", hex::encode([0xbb; 32]))),
            log_index: Some(quantity_hex(log_index)),
        }
    }

    #[derive(Default)]
    struct FakeL1 {
        latest: u64,
        logs: Vec<EthLog>,
        withdrawal_logs: Vec<EthLog>,
        /// deposit_id -> canonical on-chain root returned by depositRootByCount.
        onchain_roots: std::collections::HashMap<u64, Bytes32>,
        /// Records (count, block) each reconciliation queried.
        reconciled: Mutex<Vec<(u64, u64)>>,
    }

    impl L1Rpc for FakeL1 {
        async fn block_number(&self) -> Result<u64> {
            Ok(self.latest)
        }

        async fn deposit_logs(
            &self,
            _vault: EthAddress,
            from_block: u64,
            to_block: u64,
        ) -> Result<Vec<EthLog>> {
            Ok(self
                .logs
                .iter()
                .filter(|log| {
                    let block = log
                        .block_number
                        .as_deref()
                        .and_then(|value| parse_quantity(value, "blockNumber").ok())
                        .unwrap_or(u64::MAX);
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
            Ok(self
                .withdrawal_logs
                .iter()
                .filter(|log| {
                    let block = log
                        .block_number
                        .as_deref()
                        .and_then(|value| parse_quantity(value, "blockNumber").ok())
                        .unwrap_or(u64::MAX);
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
    }

    impl DepositSink for FakeSink {
        async fn bridge_status(&self) -> Result<BridgeStatusResponse> {
            Ok(BridgeStatusResponse {
                deposit_cursor: self.cursor,
                deposit_root_hex: String::new(),
                next_withdrawal_id: 0,
                withdrawal_count: 0,
                queued_withdrawal_count: 0,
                finalized_withdrawal_count: 0,
                cancelled_withdrawal_count: 0,
            })
        }

        async fn bridge_account_by_key(&self, _key_hex: &str) -> Result<BridgeAccountKeyResponse> {
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
            if self.fail_deposit_submit {
                return Err(IndexerError::MissingRpcResult);
            }
            Ok(BridgeDepositResponse {
                account_id: req.account_id,
                balance_nanos: 0,
                deposit_id: req.deposit_id,
                deposit_root_hex: req.deposit_root_hex.clone(),
            })
        }

        async fn submit_l1_withdrawal_event(
            &self,
            req: &SubmitL1WithdrawalEventRequest,
        ) -> Result<BridgeWithdrawalResponse> {
            self.withdrawal_statuses
                .lock()
                .unwrap()
                .push((req.nullifier_hex.clone(), req.status));
            if self.fail_withdrawal_submit {
                return Err(IndexerError::MissingRpcResult);
            }
            Ok(BridgeWithdrawalResponse {
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
            cursor_path: None,
            once: true,
        }
    }

    #[test]
    fn quantity_roundtrip() {
        assert_eq!(quantity_hex(31_337), "0x7a69");
        assert_eq!(parse_quantity("0x7a69", "chainId").unwrap(), 31_337);
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
    fn low_confirmation_depth_emits_reorg_warning() {
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(logs.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || warn_if_low_confirmation_depth(11));

        let logs = logs.contents();
        assert!(logs.contains("WARN"));
        assert!(logs.contains(
            "L1 confirmation depth 11 is below the recommended 12–32; deep reorgs can \
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

        let next = run_once(&l1, &sink, &args, vault, 0).await.unwrap();

        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        // Reconciliation happened at the confirmed height (latest - confirmations = 8).
        assert_eq!(l1.reconciled.lock().unwrap().as_slice(), &[(1, 8)]);
        assert_eq!(next, Some(9));
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

        let next = run_once(&l1, &sink, &args, vault, 0).await.unwrap();

        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(hex::encode(nullifier), BridgeWithdrawalL1Status::Queued)]
        );
        assert_eq!(next, Some(9));
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

        let result = run_once(&l1, &sink, &args, vault, next_from).await;

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

        let result = run_once(&l1, &sink, &args, vault, next_from)
            .with_subscriber(subscriber)
            .await;

        assert!(matches!(result, Err(IndexerError::MissingRpcResult)));
        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert_eq!(next_from, 0);
        let logs = logs.contents();
        assert!(logs.contains("ERROR"));
        assert!(
            logs.contains("deposit pipeline stalled; refusing to skip the next required deposit")
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

        let next = run_once(&l1, &sink, &args, vault, 0).await.unwrap();

        assert_eq!(sink.submitted.lock().unwrap().as_slice(), &[1]);
        assert_eq!(
            sink.withdrawal_statuses.lock().unwrap().as_slice(),
            &[(hex::encode(nullifier), BridgeWithdrawalL1Status::Queued)]
        );
        assert_eq!(next, Some(9));
    }

    #[tokio::test]
    async fn reorg_replaced_tip_deposit_is_not_credited() {
        // Log carries the pre-reorg root, but the canonical chain now records a
        // different root at this deposit id (a replacement deposit). Fail closed.
        let vault = [0x10; 20];
        let log_root = [0x55; 32];
        let canonical_root = [0x66; 32];
        let l1 = FakeL1 {
            latest: 10,
            logs: vec![deposit_log(1, 2, log_root, 1_000_000)],
            onchain_roots: [(1u64, canonical_root)].into_iter().collect(),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let args = test_args(2);

        let err = run_once(&l1, &sink, &args, vault, 0).await.unwrap_err();

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

        let err = run_once(&l1, &sink, &args, vault, 0).await.unwrap_err();
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

        let next = run_once(&l1, &sink, &args, vault, 0).await.unwrap();

        assert!(sink.submitted.lock().unwrap().is_empty());
        // Scanned up to confirmed_tip = 7, so next_from advances to 8 but the
        // deposit at block 10 is left for a later poll once it is deep enough.
        assert_eq!(next, Some(8));
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

        let next = run_once(&l1, &sink, &args, vault, 5).await.unwrap();
        assert_eq!(next, None);
        assert!(sink.submitted.lock().unwrap().is_empty());
    }

    #[test]
    fn cursor_persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("syb190-cursor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);

        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), None);

        save_cursor(&path, 123, &vault_hex, 31_337).unwrap();
        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), Some(123));

        // Overwrite with a later cursor.
        save_cursor(&path, 456, &vault_hex, 31_337).unwrap();
        assert_eq!(load_cursor(&path, &vault_hex, 31_337).unwrap(), Some(456));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cursor_rejects_mismatched_vault_or_chain() {
        let dir =
            std::env::temp_dir().join(format!("syb190-cursor-mismatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cursor.json");
        let vault_hex = hex::encode([0x10; 20]);
        save_cursor(&path, 7, &vault_hex, 31_337).unwrap();

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
}
