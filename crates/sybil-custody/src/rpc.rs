use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::{Value, json};
use sha3::{Digest as _, Keccak256};
use sybil_api_types::DaManifestResponse;

use crate::format::RootRecord;

pub async fn chain_id(rpc_url: &str) -> Result<u64> {
    let result = rpc(rpc_url, "eth_chainId", json!([])).await?;
    parse_quantity(result.as_str().context("eth_chainId was not a string")?)
}

pub async fn latest_height(rpc_url: &str, settlement: [u8; 20]) -> Result<u64> {
    let output = eth_call(rpc_url, settlement, &selector("latestHeight()"), None).await?;
    parse_word_u64(word(&output, 0)?)
}

pub async fn fetch_root_record(
    rpc_url: &str,
    settlement: [u8; 20],
    height: u64,
) -> Result<RootRecord> {
    let mut calldata = selector("rootAt(uint64)").to_vec();
    let mut height_word = [0u8; 32];
    height_word[24..].copy_from_slice(&height.to_be_bytes());
    calldata.extend_from_slice(&height_word);
    let output = eth_call(rpc_url, settlement, &calldata, None).await?;
    if output.len() != 11 * 32 {
        bail!(
            "rootAt({height}) returned {} bytes, expected 352",
            output.len()
        );
    }
    Ok(RootRecord {
        height: parse_word_u64(word(&output, 0)?)?,
        state_root: word(&output, 1)?.try_into().expect("word length"),
        previous_state_root: word(&output, 2)?.try_into().expect("word length"),
        block_hash: word(&output, 3)?.try_into().expect("word length"),
        events_root: word(&output, 4)?.try_into().expect("word length"),
        witness_root: word(&output, 5)?.try_into().expect("word length"),
        da_commitment: word(&output, 6)?.try_into().expect("word length"),
        deposit_root: word(&output, 7)?.try_into().expect("word length"),
        deposit_count: parse_word_u64(word(&output, 8)?)?,
        verified_at: parse_word_u64(word(&output, 9)?)?,
        verifier_version: u32::try_from(parse_word_u64(word(&output, 10)?)?)
            .context("verifier version exceeds u32")?,
    })
}

pub fn validate_manifest_root_record(
    manifest: &DaManifestResponse,
    record: &RootRecord,
) -> Result<()> {
    if record.height != manifest.height {
        bail!("L1 root height does not match DA manifest");
    }
    for (name, manifest_hex, actual) in [
        (
            "state_root",
            manifest.state_root.as_str(),
            record.state_root,
        ),
        (
            "block_hash",
            manifest.block_hash.as_str(),
            record.block_hash,
        ),
        (
            "witness_root",
            manifest.witness_root.as_str(),
            record.witness_root,
        ),
        (
            "da_commitment",
            manifest.da_commitment.as_str(),
            record.da_commitment,
        ),
    ] {
        if decode32(name, manifest_hex)? != actual {
            bail!("L1 RootRecord {name} does not match DA manifest");
        }
    }
    Ok(())
}

pub async fn send_raw_calldata_with_cast(
    rpc_url: &str,
    private_key: &str,
    to: [u8; 20],
    calldata: &[u8],
) -> Result<String> {
    let output = tokio::process::Command::new("cast")
        .args([
            "send",
            &format!("0x{}", hex::encode(to)),
            "--data",
            &format!("0x{}", hex::encode(calldata)),
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--json",
        ])
        .output()
        .await
        .context("run cast send")?;
    if !output.status.success() {
        bail!(
            "cast send failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn eth_call(
    rpc_url: &str,
    to: [u8; 20],
    calldata: &[u8],
    from: Option<[u8; 20]>,
) -> Result<Vec<u8>> {
    let mut call = serde_json::Map::new();
    call.insert(
        "to".to_string(),
        Value::String(format!("0x{}", hex::encode(to))),
    );
    call.insert(
        "data".to_string(),
        Value::String(format!("0x{}", hex::encode(calldata))),
    );
    if let Some(from) = from {
        call.insert(
            "from".to_string(),
            Value::String(format!("0x{}", hex::encode(from))),
        );
    }
    let result = rpc(rpc_url, "eth_call", json!([Value::Object(call), "latest"])).await?;
    let encoded = result
        .as_str()
        .context("eth_call result was not a string")?;
    hex::decode(encoded.trim_start_matches("0x")).context("decode eth_call result")
}

async fn rpc(rpc_url: &str, method: &str, params: Value) -> Result<Value> {
    let response = Client::new()
        .post(rpc_url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .with_context(|| format!("RPC {method}"))?;
    let status = response.status();
    let body: Value = response.json().await.context("decode RPC response")?;
    if !status.is_success() {
        bail!("RPC {method} returned HTTP {status}: {body}");
    }
    if let Some(error) = body.get("error") {
        bail!("RPC {method} failed: {error}");
    }
    body.get("result")
        .cloned()
        .with_context(|| format!("RPC {method} omitted result"))
}

fn selector(signature: &str) -> [u8; 4] {
    let hash = Keccak256::digest(signature.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

fn word(bytes: &[u8], index: usize) -> Result<&[u8]> {
    bytes
        .get(index * 32..(index + 1) * 32)
        .with_context(|| format!("missing ABI word {index}"))
}

fn parse_word_u64(word: &[u8]) -> Result<u64> {
    if word.len() != 32 || word[..24].iter().any(|byte| *byte != 0) {
        bail!("ABI uint does not fit u64");
    }
    Ok(u64::from_be_bytes(word[24..].try_into().expect("8 bytes")))
}

fn parse_quantity(value: &str) -> Result<u64> {
    u64::from_str_radix(value.trim_start_matches("0x"), 16).context("parse RPC quantity")
}

pub fn decode20(name: &str, value: &str) -> Result<[u8; 20]> {
    let bytes =
        hex::decode(value.trim_start_matches("0x")).with_context(|| format!("decode {name}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("{name} must be 20 bytes, got {}", bytes.len()))
}

pub fn decode32(name: &str, value: &str) -> Result<[u8; 32]> {
    let bytes =
        hex::decode(value.trim_start_matches("0x")).with_context(|| format!("decode {name}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("{name} must be 32 bytes, got {}", bytes.len()))
}
