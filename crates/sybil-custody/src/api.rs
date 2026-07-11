use std::path::Path;

use anyhow::{bail, Context, Result};
use matching_engine::MarketId;
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use sybil_api_types::{DaManifestResponse, HealthResponse, StateProofResponse};
use sybil_verifier::commitments::state_schema;

use crate::format::{CustodyManifest, CustodySnapshot, CUSTODY_SNAPSHOT_VERSION};
use crate::rpc::{fetch_root_record, validate_manifest_root_record};

const SNAPSHOT_RETRIES: usize = 20;

pub struct SnapshotRequest<'a> {
    pub api_url: &'a str,
    pub api_token: Option<&'a str>,
    pub account_id: u64,
    pub rpc_url: Option<&'a str>,
    pub settlement: Option<[u8; 20]>,
}

pub async fn collect_snapshot(
    request: SnapshotRequest<'_>,
) -> Result<(CustodySnapshot, CustodyManifest)> {
    if request.rpc_url.is_some() != request.settlement.is_some() {
        bail!("--rpc-url and --settlement must be provided together");
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build HTTP client")?;
    let base = request.api_url.trim_end_matches('/');

    for attempt in 1..=SNAPSHOT_RETRIES {
        match collect_once(&client, base, request.api_token, request.account_id).await {
            Ok((snapshot, manifest)) => {
                let root_record = if let (Some(rpc_url), Some(settlement)) =
                    (request.rpc_url, request.settlement)
                {
                    let record =
                        fetch_root_record(rpc_url, settlement, snapshot.block_height).await?;
                    validate_manifest_root_record(&manifest, &record)?;
                    Some(record)
                } else {
                    None
                };
                return Ok((
                    snapshot,
                    CustodyManifest {
                        version: CUSTODY_SNAPSHOT_VERSION,
                        manifest,
                        root_record,
                    },
                ));
            }
            Err(error) if attempt < SNAPSHOT_RETRIES && is_head_race(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded snapshot loop returns on its final attempt")
}

async fn collect_once(
    client: &Client,
    base: &str,
    token: Option<&str>,
    account_id: u64,
) -> Result<(CustodySnapshot, DaManifestResponse)> {
    let account_key = state_schema::account_leaf_key(account_id);
    let account_proof: StateProofResponse = get_json(
        client,
        &format!("{base}/v1/proofs/state/{}", hex::encode(account_key)),
        token,
    )
    .await?;
    require_inclusion(&account_proof, "account")?;
    let manifest: DaManifestResponse = get_json(
        client,
        &format!("{base}/v1/da/{}/manifest", account_proof.block_height),
        token,
    )
    .await?;
    let payload = fetch_payload(base, token, account_proof.block_height).await?;
    let witness = crate::reconstruct::decode_payload_against_manifest(&manifest, &payload)?;
    let account = witness
        .post_state
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
        .with_context(|| format!("account {account_id} missing from full snapshot"))?;
    if account.id != account_id {
        bail!(
            "account proof opened account {}, requested {account_id}",
            account.id
        );
    }

    let reservation_key = state_schema::account_reservation_leaf_key(account_id);
    let reservation_proof: StateProofResponse = get_json(
        client,
        &format!("{base}/v1/proofs/state/{}", hex::encode(reservation_key)),
        token,
    )
    .await?;
    let reservation = witness
        .state_sidecar
        .account_reservations
        .iter()
        .find(|reservation| reservation.account_id == account_id)
        .cloned();

    let mut market_ids = account
        .positions
        .iter()
        .filter_map(|(market_id, _, quantity)| (*quantity != 0).then_some(*market_id))
        .collect::<Vec<MarketId>>();
    market_ids.sort_by_key(|market| market.0);
    market_ids.dedup();
    let mut market_proofs = Vec::with_capacity(market_ids.len());
    let mut markets = Vec::with_capacity(market_ids.len());
    for market_id in market_ids {
        let market = witness
            .state_sidecar
            .markets
            .iter()
            .find(|market| market.market_id == market_id)
            .cloned()
            .with_context(|| format!("market {} missing from full snapshot", market_id.0))?;
        let key = state_schema::market_leaf_key(market_id);
        let proof: StateProofResponse = get_json(
            client,
            &format!("{base}/v1/proofs/state/{}", hex::encode(key)),
            token,
        )
        .await?;
        require_inclusion(&proof, "market")?;
        markets.push(market);
        market_proofs.push(proof);
    }

    let active_keys = witness
        .account_keys
        .iter()
        .find(|(id, _)| *id == account_id)
        .map(|(_, keys)| keys.clone())
        .context("account active key set missing from full snapshot")?;
    let health: HealthResponse = get_json(client, &format!("{base}/v1/health"), token).await?;

    let expected_height = account_proof.block_height;
    let expected_root = account_proof.state_root.as_str();
    for proof in std::iter::once(&reservation_proof).chain(market_proofs.iter()) {
        if proof.block_height != expected_height || proof.state_root != expected_root {
            bail!("snapshot head changed while collecting proofs; retry");
        }
        if !proof.verified {
            bail!("API returned a locally unverified qMDB proof");
        }
    }
    if manifest.height != expected_height || manifest.state_root != expected_root {
        bail!("snapshot head changed before DA manifest fetch; retry");
    }
    validate_opening_values(
        &account_proof,
        &state_schema::account_leaf_value(&account),
        "account",
    )?;
    match (&reservation, reservation_proof.proof_kind.as_str()) {
        (Some(reservation), "inclusion") => validate_opening_values(
            &reservation_proof,
            &state_schema::account_reservation_leaf_value(reservation),
            "account reservation",
        )?,
        (None, "exclusion") => {}
        _ => bail!("reservation proof does not match the full snapshot"),
    }
    for (market, proof) in markets.iter().zip(&market_proofs) {
        validate_opening_values(proof, &state_schema::market_leaf_value(market), "market")?;
    }
    if sybil_verifier::account_keys_digest(account_id, active_keys.iter().copied())
        != account.keys_digest
    {
        bail!("full-snapshot key set does not match account keys_digest");
    }
    let genesis_hash = health
        .genesis_hash
        .context("API health response omitted genesis_hash")?;

    Ok((
        CustodySnapshot {
            version: CUSTODY_SNAPSHOT_VERSION,
            account_id,
            block_height: expected_height,
            state_root: expected_root.to_string(),
            genesis_hash,
            account,
            account_proof,
            reservation,
            reservation_proof,
            markets,
            market_proofs,
            active_keys,
        },
        manifest,
    ))
}

fn is_head_race(error: &anyhow::Error) -> bool {
    error.to_string().contains("snapshot head changed")
}

fn require_inclusion(proof: &StateProofResponse, name: &str) -> Result<()> {
    if proof.proof_kind != "inclusion" {
        bail!("{name} state leaf is absent");
    }
    if !proof.verified {
        bail!("{name} state proof was not verified by the API");
    }
    Ok(())
}

fn decode_leaf_value(proof: &StateProofResponse, name: &str) -> Result<Vec<u8>> {
    let value = proof
        .leaf_value_hex
        .as_deref()
        .with_context(|| format!("{name} inclusion omitted leaf value"))?;
    hex::decode(value).with_context(|| format!("decode {name} leaf value"))
}

fn validate_opening_values(proof: &StateProofResponse, expected: &[u8], name: &str) -> Result<()> {
    if decode_leaf_value(proof, name)? != expected {
        bail!("{name} proof value does not match the canonical full snapshot");
    }
    Ok(())
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    token: Option<&str>,
) -> Result<T> {
    let response = with_bearer(client.get(url), token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("GET {url} returned {status}: {body}");
    }
    response
        .json()
        .await
        .with_context(|| format!("decode response from {url}"))
}

pub async fn fetch_payload(api_url: &str, token: Option<&str>, height: u64) -> Result<Vec<u8>> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("build HTTP client")?;
    let url = format!("{}/v1/da/{height}/payload", api_url.trim_end_matches('/'));
    let response = with_bearer(client.get(&url), token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("GET {url} returned {status}: {body}");
    }
    Ok(response
        .bytes()
        .await
        .context("read DA payload body")?
        .to_vec())
}

fn with_bearer(request: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("decode {}", path.display()))
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).context("encode JSON")?;
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}
