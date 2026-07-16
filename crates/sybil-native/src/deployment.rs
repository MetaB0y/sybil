use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sybil_api_types::{CreateMarketGroupRequest, ExtendMarketGroupRequest, MarketResponse};
use sybil_client::SybilClient;

use crate::{Error, NativeMarketCatalog, NativeMarketSpec, NativeQuoteRange, native_group_key};

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

    let existing = client.list_markets().await?;
    let specs = catalog.enabled_market_specs();
    let mut deployed = Vec::with_capacity(specs.len());

    for spec in &specs {
        let market_id = find_existing_market(&existing, spec)?;
        let market_id = match market_id {
            Some(market_id) => market_id,
            None => {
                let response = client.create_market(&spec.create_request()).await?;
                response.market_id
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

    ensure_groups(client, &specs, &deployed).await?;
    deployed.sort_by(|left, right| left.market_key.cmp(&right.market_key));
    Ok(NativeDeployment {
        schema_version: NATIVE_DEPLOYMENT_SCHEMA_VERSION,
        genesis_hash,
        markets: deployed,
    })
}

fn find_existing_market(
    markets: &[MarketResponse],
    spec: &NativeMarketSpec,
) -> Result<Option<u32>, Error> {
    let event_id = native_group_key(&spec.template_id);
    let exact: Vec<_> = markets
        .iter()
        .filter(|market| {
            market.name == spec.name
                && market.event_id.as_deref() == Some(&event_id)
                && market.polymarket_condition_id.is_none()
        })
        .collect();
    match exact.as_slice() {
        [market] => Ok(Some(market.market_id)),
        [_, _, ..] => Err(Error::Deployment(format!(
            "multiple native markets match catalog key {}",
            spec.market_key
        ))),
        [] => {
            // Creation commits the canonical `native` tag before the off-block
            // metadata write. This fallback lets a rerun recover from a crash
            // between those two calls without creating a duplicate contract.
            let tagged: Vec<_> = markets
                .iter()
                .filter(|market| {
                    market.name == spec.name
                        && market.polymarket_condition_id.is_none()
                        && market
                            .tags
                            .as_ref()
                            .is_some_and(|tags| tags.iter().any(|tag| tag == "native"))
                })
                .collect();
            match tagged.as_slice() {
                [] => Ok(None),
                [market] => Ok(Some(market.market_id)),
                _ => Err(Error::Deployment(format!(
                    "multiple tagged native markets match catalog key {}",
                    spec.market_key
                ))),
            }
        }
    }
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
        let named: Vec<_> = groups
            .iter()
            .filter(|group| group.name == group_name)
            .collect();
        if named.len() > 1 {
            return Err(Error::Deployment(format!(
                "multiple market groups match native template {template_id}"
            )));
        }
        if let Some(group) = named.first() {
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
