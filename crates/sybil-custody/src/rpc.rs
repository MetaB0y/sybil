use alloy::primitives::{Address, Bytes};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result, bail};
use sybil_api_types::DaManifestResponse;
use sybil_l1_abi::{RootRecord as AbiRootRecord, SybilSettlement};

use crate::format::RootRecord;

pub async fn chain_id(rpc_url: &str) -> Result<u64> {
    provider(rpc_url)?
        .get_chain_id()
        .await
        .context("RPC eth_chainId")
}

pub async fn latest_height(rpc_url: &str, settlement: [u8; 20]) -> Result<u64> {
    let output = contract_call(rpc_url, settlement, SybilSettlement::latestHeightCall {}).await?;
    Ok(output)
}

pub async fn fetch_root_record(
    rpc_url: &str,
    settlement: [u8; 20],
    height: u64,
) -> Result<RootRecord> {
    let output = contract_call(rpc_url, settlement, SybilSettlement::rootAtCall { height }).await?;
    let AbiRootRecord {
        height,
        stateRoot,
        previousStateRoot,
        blockHash,
        eventsRoot,
        witnessRoot,
        daCommitment,
        depositRoot,
        depositCount,
        verifiedAt,
        verifierVersion,
    } = output;
    Ok(RootRecord {
        height,
        state_root: stateRoot.into(),
        previous_state_root: previousStateRoot.into(),
        block_hash: blockHash.into(),
        events_root: eventsRoot.into(),
        witness_root: witnessRoot.into(),
        da_commitment: daCommitment.into(),
        deposit_root: depositRoot.into(),
        deposit_count: depositCount,
        verified_at: verifiedAt,
        verifier_version: verifierVersion,
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

fn provider(rpc_url: &str) -> Result<DynProvider> {
    let url = rpc_url.parse().context("parse Ethereum RPC URL")?;
    Ok(ProviderBuilder::new().connect_http(url).erased())
}

async fn contract_call<C: SolCall>(rpc_url: &str, to: [u8; 20], call: C) -> Result<C::Return> {
    let request = TransactionRequest::default()
        .to(Address::from(to))
        .input(Bytes::from(call.abi_encode()).into());
    let output = provider(rpc_url)?
        .call(request)
        .latest()
        .await
        .context("RPC eth_call")?;
    C::abi_decode_returns_validate(&output).context("decode eth_call ABI result")
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
