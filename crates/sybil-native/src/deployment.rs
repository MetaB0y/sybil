use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sybil_api_types::{CreateMarketGroupRequest, ExtendMarketGroupRequest};
use sybil_client::SybilClient;

use crate::{
    Error, NativeMarketCatalog, NativeMarketSpec, NativeQuoteRange, native_group_key,
    native_market_creation_key,
};

pub const NATIVE_DEPLOYMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeDeployment {
    pub schema_version: u32,
    pub genesis_hash: String,
    pub markets: Vec<DeployedNativeMarket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeployedNativeMarket {
    pub template_id: String,
    pub market_key: String,
    pub market_id: u32,
    pub quote_range: NativeQuoteRange,
    pub group_key: Option<String>,
    pub group_size: usize,
}

impl NativeDeployment {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let deployment: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if deployment.schema_version != NATIVE_DEPLOYMENT_SCHEMA_VERSION {
            return Err(Error::Deployment(format!(
                "unsupported native deployment schema {}",
                deployment.schema_version
            )));
        }
        if deployment.genesis_hash.trim().is_empty() {
            return Err(Error::Deployment(
                "native deployment has no genesis hash".to_string(),
            ));
        }
        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for market in &deployment.markets {
            if !keys.insert(&market.market_key) || !ids.insert(market.market_id) {
                return Err(Error::Deployment(
                    "native deployment contains duplicate market identity".to_string(),
                ));
            }
        }
        Ok(deployment)
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
}

pub async fn apply_catalog(
    client: &SybilClient,
    catalog: &NativeMarketCatalog,
) -> Result<NativeDeployment, Error> {
    let health = client.health().await?;
    let genesis_hash = health
        .genesis_hash
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if genesis_hash.len() != 64 || genesis_hash.chars().all(|character| character == '0') {
        return Err(Error::Deployment(
            "native catalog apply requires a committed nonzero genesis hash".to_string(),
        ));
    }

    let specs = catalog.enabled_market_specs();
    let mut deployed = Vec::with_capacity(specs.len());

    // Live creation keys, read from the chain rather than the local manifest:
    // the manifest lives on a container volume that can be lost while chain
    // state survives, and a missing manifest must not look like an empty
    // deployment. Markets absent here are created; markets present here are
    // reconciled, because `create_market` rejects a key whose creation fields
    // drifted (`MarketCreationKeyConflict`) instead of quietly updating.
    let live = client.list_markets().await?;
    let live_by_creation_key: BTreeMap<&str, &sybil_api_types::MarketResponse> = live
        .iter()
        .filter_map(|market| Some((market.creation_key.as_deref()?, market)))
        .collect();

    for spec in &specs {
        let creation_key = native_market_creation_key(&spec.market_key);
        let market_id = match live_by_creation_key.get(creation_key.as_str()) {
            Some(existing) => {
                let response = client
                    .update_market_content(existing.market_id, &spec.update_request())
                    .await?;
                if response.updated {
                    tracing::info!(
                        market_id = existing.market_id,
                        native_market_key = %spec.market_key,
                        "rewrote native market content from catalog"
                    );
                }
                existing.market_id
            }
            None => {
                client
                    .create_market(&spec.create_request())
                    .await?
                    .market_id
            }
        };
        client
            .set_market_metadata(market_id, &spec.metadata_request())
            .await?;
        deployed.push(DeployedNativeMarket {
            template_id: spec.template_id.clone(),
            market_key: spec.market_key.clone(),
            market_id,
            quote_range: spec.quote_range,
            group_key: spec.group_key.clone(),
            group_size: spec.group_size,
        });
    }

    close_retired(client, catalog, &live).await?;
    ensure_groups(client, &specs, &deployed).await?;
    deployed.sort_by(|left, right| left.market_key.cmp(&right.market_key));
    Ok(NativeDeployment {
        schema_version: NATIVE_DEPLOYMENT_SCHEMA_VERSION,
        genesis_hash,
        markets: deployed,
    })
}

/// Take every market of a retired template off the board.
///
/// `closed` is display state, so this withdraws the event without deciding it:
/// the markets page drops closed cards, and an operator can still settle them
/// on their merits. Deleting is not on the table — a market is committed state
/// that positions, fills and DA witnesses reference by id. A retired template
/// also removed from `markets` is simply never created again from genesis,
/// which is the only way one truly disappears.
async fn close_retired(
    client: &SybilClient,
    catalog: &NativeMarketCatalog,
    live: &[sybil_api_types::MarketResponse],
) -> Result<(), Error> {
    if catalog.retired.is_empty() {
        return Ok(());
    }
    for market in live {
        let Some(creation_key) = market.creation_key.as_deref() else {
            continue;
        };
        if !catalog.is_retired_creation_key(creation_key) || market.closed == Some(true) {
            continue;
        }
        client
            .set_market_metadata(
                market.market_id,
                &sybil_api_types::SetMarketMetadataRequest {
                    closed: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        tracing::info!(
            market_id = market.market_id,
            creation_key,
            "closed retired native market"
        );
    }
    Ok(())
}

async fn ensure_groups(
    client: &SybilClient,
    specs: &[NativeMarketSpec],
    deployed: &[DeployedNativeMarket],
) -> Result<(), Error> {
    let mut by_template = BTreeMap::<&str, Vec<&DeployedNativeMarket>>::new();
    for market in deployed.iter().filter(|market| market.group_key.is_some()) {
        by_template
            .entry(&market.template_id)
            .or_default()
            .push(market);
    }
    let mut groups = client.list_market_groups().await?;
    for (template_id, markets) in by_template {
        let target: BTreeSet<u32> = markets.iter().map(|market| market.market_id).collect();
        let group_name = specs
            .iter()
            .find(|spec| spec.template_id == template_id)
            .map(NativeMarketSpec::group_name)
            .ok_or_else(|| Error::Deployment(format!("missing template {template_id}")))?;
        let creation_key = native_group_key(template_id);
        let keyed: Vec<_> = groups
            .iter()
            .filter(|group| group.creation_key.as_deref() == Some(creation_key.as_str()))
            .collect();
        if keyed.len() > 1 {
            return Err(Error::Deployment(format!(
                "multiple market groups have native creation key {creation_key:?}"
            )));
        }
        if let Some(group) = keyed.first() {
            let current: BTreeSet<u32> = group.market_ids.iter().copied().collect();
            if !current.is_subset(&target) {
                return Err(Error::Deployment(format!(
                    "native group {template_id} contains markets outside the catalog"
                )));
            }
            let missing: Vec<u32> = target.difference(&current).copied().collect();
            let group_id = group.group_id;
            for market_id in missing {
                client
                    .extend_market_group(group_id, &ExtendMarketGroupRequest { market_id })
                    .await?;
            }
        } else {
            let response = client
                .create_market_group(&CreateMarketGroupRequest {
                    name: group_name.to_string(),
                    creation_key: Some(creation_key),
                    market_ids: target.iter().copied().collect(),
                })
                .await?;
            groups.push(response);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_roundtrip_is_versioned_and_unique() {
        let path = std::env::temp_dir().join(format!(
            "sybil-native-deployment-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let deployment = NativeDeployment {
            schema_version: 1,
            genesis_hash: "a".repeat(64),
            markets: vec![DeployedNativeMarket {
                template_id: "template".to_string(),
                market_key: "template:yes".to_string(),
                market_id: 7,
                quote_range: NativeQuoteRange {
                    min: 0.1,
                    max: 0.9,
                    initial: 0.4,
                },
                group_key: None,
                group_size: 0,
            }],
        };
        deployment.save(&path).unwrap();
        assert_eq!(NativeDeployment::load(&path).unwrap(), deployment);
        let _ = std::fs::remove_file(path);
    }
}
