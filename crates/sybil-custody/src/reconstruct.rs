use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sybil_api_types::DaManifestResponse;
use sybil_escape_claim::compute_withdrawable_token_units;
use sybil_verifier::BlockWitness;
use sybil_verifier::commitments::witness_schema;

use crate::api::{SnapshotRequest, collect_snapshot, fetch_payload, read_json};
use crate::claim::snapshot_openings;
use crate::format::{CustodyManifest, CustodySnapshot, RootRecord};
use crate::rpc::{decode32, fetch_root_record, validate_manifest_root_record};

#[derive(Debug)]
pub struct ReconstructRequest<'a> {
    pub height: u64,
    pub account_id: u64,
    pub api_url: Option<&'a str>,
    pub api_token: Option<&'a str>,
    pub manifest_path: Option<&'a Path>,
    pub payload_path: Option<&'a Path>,
    pub snapshot_path: Option<&'a Path>,
    pub rpc_url: Option<&'a str>,
    pub settlement: Option<[u8; 20]>,
}

#[derive(Debug, Serialize)]
pub struct AccountSummary {
    pub height: u64,
    pub state_root: String,
    pub account_id: u64,
    pub balance_nanos: i64,
    pub total_deposited_nanos: i64,
    pub reserved_balance_nanos: i64,
    pub positions: Vec<(u32, u8, i64)>,
    pub withdrawable_token_units: u64,
    pub witness_accounts: usize,
    pub witness_markets: usize,
}

pub async fn reconstruct(request: ReconstructRequest<'_>) -> Result<AccountSummary> {
    let saved_manifest = request
        .manifest_path
        .map(read_json::<CustodyManifest>)
        .transpose()?;
    let manifest = match &saved_manifest {
        Some(saved) => {
            if saved.manifest.height != request.height {
                bail!("saved manifest height does not match --height");
            }
            saved.manifest.clone()
        }
        None => {
            fetch_manifest(
                request
                    .api_url
                    .context("--api-url is required without --manifest")?,
                request.api_token,
                request.height,
            )
            .await?
        }
    };
    let payload = match request.payload_path {
        Some(path) => {
            std::fs::read(path).with_context(|| format!("read DA payload {}", path.display()))?
        }
        None => {
            fetch_payload(
                request
                    .api_url
                    .context("--api-url is required without --payload")?,
                request.api_token,
                request.height,
            )
            .await?
        }
    };

    let root_record = resolve_root_record(
        &manifest,
        saved_manifest
            .as_ref()
            .and_then(|saved| saved.root_record.clone()),
        request.rpc_url,
        request.settlement,
    )
    .await?;
    let snapshot = match request.snapshot_path {
        Some(path) => read_json::<CustodySnapshot>(path)?,
        None => {
            collect_snapshot(SnapshotRequest {
                api_url: request
                    .api_url
                    .context("--api-url is required without --snapshot")?,
                api_token: request.api_token,
                account_id: request.account_id,
                rpc_url: None,
                settlement: None,
            })
            .await?
            .0
        }
    };
    if snapshot.block_height != request.height || snapshot.account_id != request.account_id {
        bail!("custody snapshot does not match reconstruction height/account");
    }
    verify_reconstruction(
        &manifest,
        &payload,
        &root_record,
        request.account_id,
        &snapshot,
    )
}

pub fn verify_reconstruction(
    manifest: &DaManifestResponse,
    payload: &[u8],
    root_record: &RootRecord,
    account_id: u64,
    snapshot: &CustodySnapshot,
) -> Result<AccountSummary> {
    validate_manifest_root_record(manifest, root_record)?;
    let witness = decode_payload_against_manifest(manifest, payload)?;
    let recomputed = witness.header.state_root;
    if recomputed != root_record.state_root {
        bail!("full typed state does not reproduce the L1-accepted state root");
    }

    let account = witness
        .post_state
        .iter()
        .find(|account| account.id == account_id)
        .with_context(|| {
            format!(
                "account {account_id} is absent at height {}",
                manifest.height
            )
        })?;
    let witness_reservation = witness
        .state_sidecar
        .account_reservations
        .iter()
        .find(|reservation| reservation.account_id == account_id);
    if account != &snapshot.account || witness_reservation != snapshot.reservation.as_ref() {
        bail!("custody openings do not match the authenticated full snapshot");
    }
    for market in &snapshot.markets {
        if !witness
            .state_sidecar
            .markets
            .iter()
            .any(|candidate| candidate == market)
        {
            bail!("custody market opening does not match the authenticated full snapshot");
        }
    }
    let (opened_account, reserved_balance, markets) = snapshot_openings(snapshot)?;
    let withdrawable = compute_withdrawable_token_units(
        &opened_account,
        reserved_balance,
        &markets,
        &root_record.state_root,
    )
    .context("compute canonical escape-claim valuation")?;

    Ok(AccountSummary {
        height: manifest.height,
        state_root: format!("0x{}", hex::encode(recomputed)),
        account_id,
        balance_nanos: account.balance,
        total_deposited_nanos: account.total_deposited,
        reserved_balance_nanos: reserved_balance,
        positions: account
            .positions
            .iter()
            .map(|(market, outcome, quantity)| (market.0, *outcome, *quantity))
            .collect(),
        withdrawable_token_units: withdrawable,
        witness_accounts: witness.post_state.len(),
        witness_markets: witness.state_sidecar.markets.len(),
    })
}

pub fn decode_payload_against_manifest(
    manifest: &DaManifestResponse,
    payload: &[u8],
) -> Result<BlockWitness> {
    let witness = witness_schema::decode_canonical_witness_bytes(payload)
        .context("decode canonical v9 witness payload")?;
    if witness.header.height != manifest.height {
        bail!("decoded witness height does not match manifest");
    }
    let provider_refs = manifest
        .provider_refs
        .iter()
        .map(|provider| {
            hex::decode(provider.bytes.trim_start_matches("0x"))
                .context("decode DA provider reference")
        })
        .collect::<Result<Vec<_>>>()?;
    let components = sybil_zk::da_commitment_components_from_payload_and_provider_refs(
        &witness,
        payload,
        &provider_refs,
    );
    for (name, actual, expected_hex) in [
        (
            "state_root",
            components.state_root,
            manifest.state_root.as_str(),
        ),
        (
            "witness_root",
            components.witness_root,
            manifest.witness_root.as_str(),
        ),
        (
            "payload_root",
            components.payload_root,
            manifest.payload_root.as_str(),
        ),
        (
            "provider_refs_hash",
            components.provider_refs_hash,
            manifest.provider_refs_hash.as_str(),
        ),
        (
            "da_commitment",
            components.da_commitment,
            manifest.da_commitment.as_str(),
        ),
    ] {
        if actual != decode32(name, expected_hex)? {
            bail!("reconstruction {name} mismatch");
        }
    }
    if components.payload_len != manifest.payload_len
        || payload.len() as u64 != manifest.payload_len
    {
        bail!("reconstruction payload length mismatch");
    }
    let recomputed = sybil_verifier::block::compute_state_root_with_sidecar(
        &witness.post_state,
        &witness.state_sidecar,
    );
    if recomputed != witness.header.state_root
        || recomputed != decode32("state_root", &manifest.state_root)?
    {
        bail!("full typed state does not reproduce the manifest state root");
    }
    Ok(witness)
}

async fn resolve_root_record(
    manifest: &DaManifestResponse,
    saved: Option<RootRecord>,
    rpc_url: Option<&str>,
    settlement: Option<[u8; 20]>,
) -> Result<RootRecord> {
    if rpc_url.is_some() != settlement.is_some() {
        bail!("--rpc-url and --settlement must be provided together");
    }
    if let (Some(rpc_url), Some(settlement)) = (rpc_url, settlement) {
        return fetch_root_record(rpc_url, settlement, manifest.height).await;
    }
    saved.context(
        "reconstruction needs an L1 RootRecord: pass --rpc-url/--settlement or use a manifest saved with them",
    )
}

async fn fetch_manifest(
    api_url: &str,
    token: Option<&str>,
    height: u64,
) -> Result<DaManifestResponse> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/da/{height}/manifest", api_url.trim_end_matches('/'));
    let mut request = client.get(&url);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        bail!("GET {url} returned {}", response.status());
    }
    response.json().await.context("decode DA manifest")
}
