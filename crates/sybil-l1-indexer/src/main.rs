use std::time::Duration;

use clap::Parser;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sybil_api_types::request::SubmitL1DepositRequest;
use sybil_api_types::response::{BridgeAccountKeyResponse, BridgeDepositResponse};
use sybil_client::SybilClient;
use sybil_l1_protocol::{
    deposit_received_topic0, parse_deposit_received_log, Bytes32, EthAddress, L1Log,
    L1ProtocolError,
};
use tokio::time::sleep;

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
    /// Maximum eth_getLogs block span per poll.
    #[arg(long, env = "SYBIL_L1_MAX_BLOCK_SPAN", default_value_t = 1_000)]
    max_block_span: u64,
    /// Poll interval in milliseconds.
    #[arg(long, env = "SYBIL_L1_POLL_MS", default_value_t = 1_000)]
    poll_ms: u64,
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
    #[error("Sybil API failed: {0}")]
    SybilApi(#[from] sybil_client::Error),
    #[error("L1 protocol error: {0}")]
    L1Protocol(#[from] L1ProtocolError),
    #[error(
        "deposit cursor gap: next Sybil deposit is {expected}, but L1 log has deposit {actual}"
    )]
    DepositGap { expected: u64, actual: u64 },
}

type Result<T> = std::result::Result<T, IndexerError>;

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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let vault_address = parse_hex_array::<20>(&args.vault_address, "vault_address")?;
    let http = reqwest::Client::new();
    let sybil = SybilClient::new(
        http.clone(),
        args.sybil_api_url.clone(),
        args.sybil_service_token.clone(),
    );
    let mut next_from = args.start_block;

    loop {
        match run_once(&http, &sybil, &args, vault_address, next_from).await {
            Ok(Some(next)) => next_from = next,
            Ok(None) => {}
            Err(error) => {
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

async fn run_once(
    l1_client: &reqwest::Client,
    sybil: &SybilClient,
    args: &Args,
    vault_address: EthAddress,
    next_from: u64,
) -> Result<Option<u64>> {
    let latest = eth_block_number(l1_client, &args.rpc_url).await?;
    if latest < next_from {
        return Ok(None);
    }

    let to = latest.min(next_from.saturating_add(args.max_block_span.saturating_sub(1)));
    let mut deposits = eth_get_deposit_logs(l1_client, &args.rpc_url, vault_address, next_from, to)
        .await?
        .into_iter()
        .map(indexed_deposit_from_log)
        .collect::<Result<Vec<_>>>()?;
    sort_deposits(&mut deposits);

    let status = sybil.bridge_status().await?;
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

        let account = resolve_bridge_account(sybil, deposit.event.sybil_account_key).await?;
        let response = submit_deposit(
            sybil,
            args.chain_id,
            vault_address,
            &deposit,
            account.account_id,
        )
        .await?;
        tracing::info!(
            deposit_id = response.deposit_id,
            account_id = response.account_id,
            balance_nanos = response.balance_nanos,
            tx = deposit.log.transaction_hash.as_deref().unwrap_or_default(),
            "l1.indexer.deposit_ingested"
        );
        cursor = deposit.event.deposit_id;
    }

    Ok(Some(to.saturating_add(1)))
}

async fn eth_block_number(client: &reqwest::Client, rpc_url: &str) -> Result<u64> {
    let value: String = rpc_call(client, rpc_url, "eth_blockNumber", json!([])).await?;
    parse_quantity(&value, "blockNumber")
}

async fn eth_get_deposit_logs(
    client: &reqwest::Client,
    rpc_url: &str,
    vault_address: EthAddress,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<EthLog>> {
    let topic0 = format!("0x{}", hex::encode(deposit_received_topic0()));
    let filter = json!({
        "fromBlock": quantity_hex(from_block),
        "toBlock": quantity_hex(to_block),
        "address": format!("0x{}", hex::encode(vault_address)),
        "topics": [topic0],
    });
    rpc_call(client, rpc_url, "eth_getLogs", json!([filter])).await
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

async fn resolve_bridge_account(
    sybil: &SybilClient,
    key: Bytes32,
) -> Result<BridgeAccountKeyResponse> {
    Ok(sybil.bridge_account_by_key(&hex::encode(key)).await?)
}

async fn submit_deposit(
    sybil: &SybilClient,
    chain_id: u64,
    vault_address: EthAddress,
    deposit: &IndexedDeposit,
    account_id: u64,
) -> Result<BridgeDepositResponse> {
    // TODO(SYB-188/SYB-178): this dev indexer trusts eth_getLogs from its RPC
    // and does not prove receipt inclusion/finality. Keep the API route
    // service-gated until deposit soundness is proof-backed.
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
    Ok(sybil.submit_l1_deposit(&body).await?)
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

    #[test]
    fn quantity_roundtrip() {
        assert_eq!(quantity_hex(31_337), "0x7a69");
        assert_eq!(parse_quantity("0x7a69", "chainId").unwrap(), 31_337);
    }

    #[test]
    fn parses_eth_deposit_log() {
        let token = [0x20; 20];
        let sender = [0x30; 20];
        let key = [0x44; 32];
        let root = [0x55; 32];
        let mut data = Vec::new();
        data.extend_from_slice(&abi_address_word(token));
        data.extend_from_slice(&abi_u64_word(1_000_000));
        data.extend_from_slice(&root);

        let log = EthLog {
            address: format!("0x{}", hex::encode([0x10; 20])),
            topics: vec![
                format!("0x{}", hex::encode(deposit_received_topic0())),
                format!("0x{}", hex::encode(abi_u64_word(7))),
                format!("0x{}", hex::encode(abi_address_word(sender))),
                format!("0x{}", hex::encode(key)),
            ],
            data: format!("0x{}", hex::encode(data)),
            block_number: Some("0x2".to_string()),
            transaction_hash: Some(format!("0x{}", hex::encode([0xaa; 32]))),
            log_index: Some("0x1".to_string()),
        };

        let indexed = indexed_deposit_from_log(log).unwrap();
        assert_eq!(indexed.event.deposit_id, 7);
        assert_eq!(indexed.event.sender, sender);
        assert_eq!(indexed.event.sybil_account_key, key);
        assert_eq!(indexed.event.token_address, token);
        assert_eq!(indexed.event.amount_token_units, 1_000_000);
        assert_eq!(indexed.event.deposit_root, root);
    }
}
