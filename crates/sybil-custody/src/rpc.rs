use alloy::primitives::{Address, Bytes};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result, bail};
use sybil_api_types::DaManifestResponse;
use sybil_escape_claim::EscapeClaimPublicInputs;
use sybil_l1_abi::{RootRecord as AbiRootRecord, SybilSettlement, SybilVault};

use crate::abi::abi_escape_inputs;
use crate::format::RootRecord;

pub async fn chain_id(rpc_url: &str) -> Result<u64> {
    provider(rpc_url)?
        .get_chain_id()
        .await
        .context("RPC eth_chainId")
}

pub async fn latest_height(rpc_url: &str, settlement: [u8; 20]) -> Result<u64> {
    SybilSettlement::new(Address::from(settlement), provider(rpc_url)?)
        .latestHeight()
        .call()
        .await
        .context("RPC SybilSettlement.latestHeight")
}

pub async fn fetch_root_record(
    rpc_url: &str,
    settlement: [u8; 20],
    height: u64,
) -> Result<RootRecord> {
    let output = SybilSettlement::new(Address::from(settlement), provider(rpc_url)?)
        .rootAt(height)
        .call()
        .await
        .context("RPC SybilSettlement.rootAt")?;
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

/// Sign and submit an escape claim using Alloy. This keeps custody submission
/// self-contained instead of requiring a Foundry `cast` executable at runtime.
pub async fn submit_escape_claim(
    rpc_url: &str,
    private_key: &str,
    vault: [u8; 20],
    inputs: &EscapeClaimPublicInputs,
    proof: &[u8],
) -> Result<String> {
    let signer: PrivateKeySigner = private_key.parse().context("parse Ethereum private key")?;
    let url = rpc_url.parse().context("parse Ethereum RPC URL")?;
    let provider = ProviderBuilder::new().wallet(signer).connect_http(url);
    let receipt = SybilVault::new(Address::from(vault), provider)
        .escapeClaim(abi_escape_inputs(inputs), Bytes::copy_from_slice(proof))
        .send()
        .await
        .context("submit SybilVault.escapeClaim")?
        .get_receipt()
        .await
        .context("wait for SybilVault.escapeClaim receipt")?;
    if !receipt.status() {
        bail!("SybilVault.escapeClaim transaction reverted");
    }
    Ok(format!("{:#x}", receipt.transaction_hash))
}

fn provider(rpc_url: &str) -> Result<DynProvider> {
    let url = rpc_url.parse().context("parse Ethereum RPC URL")?;
    Ok(ProviderBuilder::new().connect_http(url).erased())
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
