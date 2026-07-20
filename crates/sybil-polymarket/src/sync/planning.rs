use crate::categorize::derive_categories;
use crate::mapping::GroupInfo;
use crate::polymarket::types::{GammaEvent, GammaMarket, parse_iso8601_to_ms};
use sybil_api_types::{MarketGroupResponse, SetMarketMetadataRequest};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum NegRiskGroupAction {
    Create(Vec<u32>),
    Extend {
        missing_market_ids: Vec<u32>,
        existing_group_market_ids: Vec<u32>,
    },
    None,
}

/// Stable operator identity for a mirrored market.
///
/// The external condition id is normalized and hashed so malformed or
/// unexpectedly long provider input cannot violate the API's creation-key
/// alphabet/length contract. Retrying after an ambiguous response therefore
/// resolves to the original Sybil market instead of allocating a duplicate.
pub(super) fn polymarket_market_creation_key(condition_id: &str) -> String {
    let normalized = condition_id.trim().to_ascii_lowercase();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/polymarket-market-creation/v1");
    hasher.update(&(normalized.len() as u64).to_le_bytes());
    hasher.update(normalized.as_bytes());
    format!("polymarket:{}", hasher.finalize().to_hex())
}

/// Stable operator identity for a mirrored NegRisk event group.
pub(super) fn polymarket_group_creation_key(event_id: &str) -> String {
    let normalized = event_id.trim().to_ascii_lowercase();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/polymarket-group-creation/v1");
    hasher.update(&(normalized.len() as u64).to_le_bytes());
    hasher.update(normalized.as_bytes());
    format!("polymarket-group:{}", hasher.finalize().to_hex())
}

pub(super) fn plan_negrisk_group_action(
    event: &GammaEvent,
    active_mapped_ids: &[u32],
    existing_group: Option<&GroupInfo>,
) -> NegRiskGroupAction {
    if !event.is_neg_risk() || active_mapped_ids.len() <= 1 {
        return NegRiskGroupAction::None;
    }

    plan_market_group_action(active_mapped_ids, existing_group)
}

pub(super) fn plan_market_group_action(
    active_mapped_ids: &[u32],
    existing_group: Option<&GroupInfo>,
) -> NegRiskGroupAction {
    if active_mapped_ids.len() <= 1 {
        return NegRiskGroupAction::None;
    }

    if let Some(group) = existing_group {
        let missing_market_ids: Vec<u32> = active_mapped_ids
            .iter()
            .copied()
            .filter(|id| !group.sybil_market_ids.contains(id))
            .collect();
        if missing_market_ids.is_empty() {
            NegRiskGroupAction::None
        } else {
            NegRiskGroupAction::Extend {
                missing_market_ids,
                existing_group_market_ids: group.sybil_market_ids.clone(),
            }
        }
    } else {
        NegRiskGroupAction::Create(active_mapped_ids.to_vec())
    }
}

pub(super) fn matching_sybil_group_id(
    groups: &[MarketGroupResponse],
    creation_key: &str,
) -> Option<u64> {
    groups
        .iter()
        .find(|group| group.creation_key.as_deref() == Some(creation_key))
        .map(|group| group.group_id)
}

pub(super) fn mm_group_membership(
    event_id: &str,
    sybil_market_id: u32,
    group: Option<&GroupInfo>,
) -> (Option<String>, usize) {
    let in_group = group
        .as_ref()
        .is_some_and(|group| group.neg_risk && group.sybil_market_ids.contains(&sybil_market_id));
    if in_group {
        (
            Some(event_id.to_string()),
            group.map(|group| group.sybil_market_ids.len()).unwrap_or(0),
        )
    } else {
        (None, 0)
    }
}

/// Compose the off-block metadata POST payload from the Polymarket event +
/// market pair. Pure function — no I/O — to keep the call site clean.
///
/// - `event_id` / `event_title`: frontend grouping signal (independent of
///   NegRisk `MarketGroup` on the matching engine).
/// - Image / icon URLs: passed through verbatim; frontend uses image first
///   and falls back to icon on 404.
/// - End dates: parsed from ISO-8601 to epoch ms. Display only; matching
///   engine doesn't enforce trading cutoffs.
/// - `polymarket_tags`: raw `event.tags[].label` list. Frontend derives one
///   or more categories from these via its own priority table — moves
///   categorization out of the build/deploy loop.
/// - `category`: deliberately left `None` for mirrored markets; superseded
///   by `polymarket_tags` + frontend derivation.
/// - `external_url`: Polymarket event page (when slug present), for the
///   "view on Polymarket" link.
pub(super) fn build_metadata_request(
    event: &GammaEvent,
    market: &GammaMarket,
) -> SetMarketMetadataRequest {
    let event_end_date_ms = event
        .end_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());
    let market_end_date_ms = market
        .end_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());
    let event_start_date_ms = event
        .start_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());
    let market_start_date_ms = market
        .start_date
        .as_deref()
        .and_then(parse_iso8601_to_ms)
        .and_then(|ms| u64::try_from(ms).ok());

    let external_url = if event.slug.is_empty() {
        None
    } else {
        Some(format!("https://polymarket.com/event/{}", event.slug))
    };

    let categories = derive_categories(&event.tags);

    SetMarketMetadataRequest {
        external_url,
        event_id: Some(event.id.clone()),
        event_title: Some(event.title.clone()),
        event_image_url: event.image.clone(),
        event_icon_url: event.icon.clone(),
        event_end_date_ms,
        market_image_url: market.image.clone(),
        market_icon_url: market.icon.clone(),
        market_end_date_ms,
        // `category` (singular) is reserved for sybil-native markets; the
        // mirror ships `categories` (plural) and lets the frontend pick.
        category: None,
        categories: if categories.is_empty() {
            None
        } else {
            Some(categories)
        },
        polymarket_condition_id: Some(market.condition_id.clone()),
        event_start_date_ms,
        market_start_date_ms,
        group_item_title: market.group_item_title.clone(),
        closed: Some(market.closed),
    }
}
