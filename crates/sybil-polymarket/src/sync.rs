use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::categorize::derive_categories;
use crate::config::Config;
use crate::feed::FeedMessage;
use crate::mapping::{GroupInfo, MappingStore};
use crate::mm::MmMessage;
use crate::polymarket::gamma::GammaClient;
use crate::polymarket::types::{parse_iso8601_to_ms, GammaEvent, GammaMarket};
use sybil_api_types::*;
use sybil_client::SybilClient;

/// Market sync actor. Polls Polymarket for new events, creates corresponding
/// markets on Sybil, and notifies Feed and MM actors.
pub struct SyncActor {
    config: Config,
    gamma_client: GammaClient,
    sybil_client: SybilClient,
    mapping: Arc<RwLock<MappingStore>>,
    feed_tx: mpsc::Sender<FeedMessage>,
    mm_tx: mpsc::Sender<MmMessage>,
    /// Live count of markets the MM is actively quoting (PM-8). Admission to MM
    /// is gated on this, not the monotonic mapped-market count, so slots freed
    /// by resolved/untracked markets are recycled for fresh events.
    mm_live_rx: watch::Receiver<usize>,
    /// Re-push off-block metadata for all mapped markets on the first cycle
    /// after start, so schema additions backfill onto existing markets
    /// without wiping `market_ref_data.json`.
    first_sync: bool,
    /// Curated Polymarket event ids to mirror (SYB-150). When non-empty the
    /// sync fetches exactly these events by id instead of the volume-ranked
    /// category scan.
    curated_event_ids: Vec<String>,
}

struct PendingMmNotification {
    sybil_market_id: u32,
    condition_id: String,
    yes_token_id: String,
    initial_mid: f64,
}

#[derive(Debug, PartialEq, Eq)]
enum NegRiskGroupAction {
    Create(Vec<u32>),
    Extend {
        missing_market_ids: Vec<u32>,
        existing_group_market_ids: Vec<u32>,
    },
    None,
}

fn plan_negrisk_group_action(
    event: &GammaEvent,
    active_mapped_ids: &[u32],
    existing_group: Option<&GroupInfo>,
) -> NegRiskGroupAction {
    if !event.is_neg_risk() || active_mapped_ids.len() <= 1 {
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

fn matching_sybil_group_id(
    groups: &[MarketGroupResponse],
    existing_group: &GroupInfo,
) -> Option<u64> {
    groups
        .iter()
        .filter(|group| group.name == existing_group.group_name)
        .filter_map(|group| {
            let overlap = group
                .market_ids
                .iter()
                .filter(|id| existing_group.sybil_market_ids.contains(id))
                .count();
            if overlap == 0 {
                return None;
            }

            let stored_subset_of_server = existing_group
                .sybil_market_ids
                .iter()
                .all(|id| group.market_ids.contains(id));
            let server_subset_of_stored = group
                .market_ids
                .iter()
                .all(|id| existing_group.sybil_market_ids.contains(id));
            (stored_subset_of_server || server_subset_of_stored)
                .then_some((group.group_id, overlap))
        })
        .max_by_key(|(_, overlap)| *overlap)
        .map(|(group_id, _)| group_id)
}

fn mm_group_membership(
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

impl SyncActor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        gamma_client: GammaClient,
        sybil_client: SybilClient,
        mapping: Arc<RwLock<MappingStore>>,
        feed_tx: mpsc::Sender<FeedMessage>,
        mm_tx: mpsc::Sender<MmMessage>,
        mm_live_rx: watch::Receiver<usize>,
        curated_event_ids: Vec<String>,
    ) -> Self {
        Self {
            config,
            gamma_client,
            sybil_client,
            mapping,
            feed_tx,
            mm_tx,
            mm_live_rx,
            first_sync: true,
            curated_event_ids,
        }
    }

    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        info!("SyncActor started");

        loop {
            if let Err(e) = self.sync_once().await {
                warn!(error = %e, "sync cycle failed");
            }

            // Save mapping after each cycle
            if let Err(e) = self.mapping.read().await.save() {
                warn!(error = %e, "failed to save mapping");
            }

            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("SyncActor shutting down");
                    let _ = self.mapping.read().await.save();
                    return;
                }
                _ = tokio::time::sleep(Duration::from_secs(self.config.sync_interval_secs)) => {}
            }
        }
    }

    async fn sync_once(&mut self) -> Result<(), crate::error::Error> {
        // Curated mode (SYB-150): mirror exactly the allowlisted events by id.
        // Otherwise fall back to the volume-ranked category scan.
        let events = if self.curated_event_ids.is_empty() {
            self.gamma_client
                .fetch_active_events(
                    self.config.max_events,
                    &self.config.mirror_categories,
                    &self.config.mirror_excluded_categories,
                    self.config.min_volume_usd,
                )
                .await?
        } else {
            self.gamma_client
                .fetch_curated_events(&self.curated_event_ids)
                .await?
        };

        // Baseline live-set size for MM admission this cycle (PM-8). Copied out
        // of the watch so we never hold the borrow across an await, and bumped
        // locally as we admit so a single cycle cannot overshoot the cap before
        // the MM has processed our messages.
        let mut mm_live = *self.mm_live_rx.borrow();

        let synced_before = self.mapping.read().await.event_count();
        info!(
            events = events.len(),
            synced = synced_before,
            "fetched events from Polymarket"
        );

        // Push the full event JSON to sybil-api so the FE can read it. We
        // already hold the parsed event (no extra Polymarket fetch); this is
        // an idempotent upsert each cycle, so the folder self-heals after a
        // restart of either process. Only events with a tradeable market.
        for event in &events {
            if !event.markets.iter().any(|m| m.active && !m.closed) {
                continue;
            }
            match serde_json::to_value(event) {
                Ok(value) => {
                    if let Err(e) = self.sybil_client.put_event_raw(&event.id, &value).await {
                        warn!(event_id = &event.id, error = %e, "failed to push event snapshot");
                    }
                }
                Err(e) => {
                    warn!(event_id = &event.id, error = %e, "failed to serialize event snapshot")
                }
            }
        }

        // One-time backfill: re-push off-block metadata for all already-mapped
        // markets on the first cycle after start, so schema additions land on
        // existing markets without wiping market_ref_data.json. Collect under
        // the lock, then POST after releasing it.
        if self.first_sync {
            // Fully-closed events drop out of the active fetch, so pull the
            // closed-event list once too — that's how their markets get flagged
            // `closed` for the frontend to hide.
            let closed_events = self
                .gamma_client
                .fetch_closed_events(self.config.max_events)
                .await
                .unwrap_or_else(|e| {
                    warn!(error = %e, "failed to fetch closed events for backfill");
                    Vec::new()
                });
            let refresh: Vec<(u32, SetMarketMetadataRequest)> = {
                let map = self.mapping.read().await;
                // Re-push display metadata for every mapped market — active
                // events (open + closed children) AND fully-closed events. No
                // active/closed filter: `filter_map` already gates on the market
                // being mapped, the POST is idempotent, and closed markets MUST
                // be included so they receive `closed: true` + `group_item_title`.
                events
                    .iter()
                    .chain(closed_events.iter())
                    .flat_map(|event| {
                        event
                            .markets
                            .iter()
                            .filter_map(|m| {
                                map.sybil_market_id(&m.condition_id)
                                    .map(|sid| (sid, build_metadata_request(event, m)))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            info!(
                count = refresh.len(),
                "backfilling market metadata (one-time)"
            );
            for (sid, req) in refresh {
                if let Err(e) = self.sybil_client.set_market_metadata(sid, &req).await {
                    warn!(sybil_id = sid, error = %e, "metadata backfill failed");
                }
            }
            self.first_sync = false;
        }

        let mut new_token_ids = Vec::new();

        for event in &events {
            let active_markets: Vec<_> = event
                .markets
                .iter()
                .filter(|m| m.active && !m.closed)
                .collect();

            if active_markets.is_empty() {
                continue;
            }

            let (event_synced, existing_group, mapped_count, unmapped_count) = {
                let map = self.mapping.read().await;
                let mapped_count = active_markets
                    .iter()
                    .filter(|m| map.sybil_market_id(&m.condition_id).is_some())
                    .count();
                (
                    map.is_event_synced(&event.id),
                    map.event_group(&event.id),
                    mapped_count,
                    active_markets.len().saturating_sub(mapped_count),
                )
            };
            let needs_group_retry =
                event.is_neg_risk() && existing_group.is_none() && mapped_count > 1;
            if event_synced && unmapped_count == 0 && !needs_group_retry {
                continue;
            }

            info!(
                event_id = &event.id,
                title = &event.title,
                markets = active_markets.len(),
                unmapped = unmapped_count,
                neg_risk = event.is_neg_risk(),
                already_synced = event_synced,
                "syncing polymarket event"
            );

            let mut pending_mm = Vec::new();

            for poly_market in &active_markets {
                if self
                    .mapping
                    .read()
                    .await
                    .sybil_market_id(&poly_market.condition_id)
                    .is_some()
                {
                    continue;
                }

                let token_ids = match poly_market.parsed_token_ids() {
                    Ok(ids) if ids.len() >= 2 => ids,
                    Ok(_) => {
                        warn!(
                            condition_id = &poly_market.condition_id,
                            "skipping market with < 2 token IDs"
                        );
                        continue;
                    }
                    Err(e) => {
                        warn!(
                            condition_id = &poly_market.condition_id,
                            error = %e,
                            "failed to parse token IDs"
                        );
                        continue;
                    }
                };

                // Build the market name
                let name = if event.is_neg_risk() {
                    // NegRisk: use groupItemTitle or question
                    format!(
                        "{}: {}",
                        event.title,
                        poly_market
                            .group_item_title
                            .as_deref()
                            .unwrap_or(&poly_market.question)
                    )
                } else {
                    poly_market.question.clone()
                };

                // Create market on Sybil
                let req = CreateMarketRequest {
                    name: name.clone(),
                    description: poly_market.description.clone(),
                    category: event.primary_category(),
                    tags: Some({
                        let mut tags = vec!["polymarket".to_string()];
                        tags.extend(event.tag_labels());
                        tags
                    }),
                    resolution_criteria: poly_market.resolution_source.clone(),
                    expiry_timestamp_ms: None,
                    resolution_template: if self.config.signer_key_path.is_empty() {
                        None
                    } else {
                        Some("polymarket_mirror".to_string())
                    },
                };

                match self.sybil_client.create_market(&req).await {
                    Ok(resp) => {
                        let sybil_id = resp.market_id;
                        info!(
                            sybil_id,
                            condition_id = &poly_market.condition_id,
                            name,
                            "created market"
                        );

                        self.mapping.write().await.register_market(
                            poly_market.condition_id.clone(),
                            token_ids.clone(),
                            sybil_id,
                        );

                        // Push off-block metadata (event id/title, images, end
                        // dates, category) so the frontend can render real
                        // chrome instead of mocks. Failure is non-fatal: the
                        // market itself was created successfully, and the next
                        // sync cycle is idempotent on the API side. We
                        // deliberately do NOT alter NegRisk MarketGroup
                        // semantics here.
                        let metadata_req = build_metadata_request(event, poly_market);
                        if let Err(e) = self
                            .sybil_client
                            .set_market_metadata(sybil_id, &metadata_req)
                            .await
                        {
                            warn!(
                                sybil_id,
                                condition_id = &poly_market.condition_id,
                                error = %e,
                                "failed to set market metadata (non-fatal; will retry next cycle)"
                            );
                        }

                        pending_mm.push(PendingMmNotification {
                            sybil_market_id: sybil_id,
                            condition_id: poly_market.condition_id.clone(),
                            yes_token_id: token_ids[0].clone(),
                            initial_mid: poly_market.yes_price().unwrap_or(0.5),
                        });
                    }
                    Err(e) => {
                        warn!(
                            condition_id = &poly_market.condition_id,
                            error = %e,
                            "failed to create market on Sybil"
                        );
                    }
                }
            }

            let (active_mapped_ids, unmapped_after) = {
                let map = self.mapping.read().await;
                let mapped: Vec<u32> = active_markets
                    .iter()
                    .filter_map(|m| map.sybil_market_id(&m.condition_id))
                    .collect();
                let unmapped_after = active_markets.len().saturating_sub(mapped.len());
                (mapped, unmapped_after)
            };

            let existing_group = self.mapping.read().await.event_group(&event.id);
            let group_action = if event.is_neg_risk()
                && existing_group.is_none()
                && active_mapped_ids.len() > 1
                && unmapped_after > 0
            {
                warn!(
                    event_id = &event.id,
                    mapped = active_mapped_ids.len(),
                    unmapped = unmapped_after,
                    "not creating partial NegRisk MarketGroup until all active child markets are mapped"
                );
                NegRiskGroupAction::None
            } else {
                plan_negrisk_group_action(event, &active_mapped_ids, existing_group.as_ref())
            };
            match group_action {
                NegRiskGroupAction::Create(market_ids) => {
                    let group_req = CreateMarketGroupRequest {
                        name: event.title.clone(),
                        market_ids: market_ids.clone(),
                    };
                    match self.sybil_client.create_market_group(&group_req).await {
                        Ok(_) => {
                            info!(
                                event_id = &event.id,
                                markets = market_ids.len(),
                                "created market group"
                            );
                            self.mapping.write().await.register_event(
                                event.id.clone(),
                                GroupInfo {
                                    group_name: event.title.clone(),
                                    sybil_market_ids: market_ids,
                                    neg_risk: true,
                                },
                            );
                        }
                        Err(e) => {
                            warn!(
                                event_id = &event.id,
                                error = %e,
                                "failed to create market group"
                            );
                        }
                    }
                }
                NegRiskGroupAction::Extend {
                    missing_market_ids,
                    existing_group_market_ids,
                } => {
                    let groups = match self.sybil_client.list_market_groups().await {
                        Ok(groups) => groups,
                        Err(e) => {
                            warn!(
                                event_id = &event.id,
                                missing_market_ids = ?missing_market_ids,
                                error = %e,
                                "failed to list Sybil market groups before extension"
                            );
                            Vec::new()
                        }
                    };
                    let Some(existing_group) = existing_group.as_ref() else {
                        warn!(
                            event_id = &event.id,
                            missing_market_ids = ?missing_market_ids,
                            "planned NegRisk group extension without a local group mapping"
                        );
                        continue;
                    };
                    let Some(group_id) = matching_sybil_group_id(&groups, existing_group) else {
                        warn!(
                            event_id = &event.id,
                            missing_market_ids = ?missing_market_ids,
                            existing_group_market_ids = ?existing_group_market_ids,
                            "could not locate current Sybil MarketGroup for NegRisk extension"
                        );
                        continue;
                    };

                    let mut extended = Vec::new();
                    for market_id in &missing_market_ids {
                        let req = ExtendMarketGroupRequest {
                            market_id: *market_id,
                        };
                        match self.sybil_client.extend_market_group(group_id, &req).await {
                            Ok(group) => {
                                info!(
                                    event_id = &event.id,
                                    group_id,
                                    market_id,
                                    members = group.market_ids.len(),
                                    "extended NegRisk market group"
                                );
                                extended.push(*market_id);
                            }
                            Err(e) => {
                                warn!(
                                    event_id = &event.id,
                                    group_id,
                                    market_id,
                                    error = %e,
                                    "failed to extend NegRisk market group"
                                );
                            }
                        }
                    }
                    if !extended.is_empty() {
                        self.mapping
                            .write()
                            .await
                            .extend_event_group(&event.id, &extended);
                    }
                }
                NegRiskGroupAction::None => {}
            }

            if (!event.is_neg_risk() || active_mapped_ids.len() <= 1)
                && !active_mapped_ids.is_empty()
                && unmapped_after == 0
            {
                self.mapping.write().await.mark_event_synced(&event.id);
            } else if active_mapped_ids.is_empty() {
                warn!(
                    event_id = &event.id,
                    "no Sybil markets mapped; leaving event unsynced for retry"
                );
            }

            let group_after_sync = self.mapping.read().await.event_group(&event.id);
            for pending in pending_mm {
                let (group_key, group_size) = mm_group_membership(
                    &event.id,
                    pending.sybil_market_id,
                    group_after_sync.as_ref(),
                );

                if event.is_neg_risk() && group_key.is_none() && group_after_sync.is_some() {
                    warn!(
                        event_id = &event.id,
                        sybil_id = pending.sybil_market_id,
                        condition_id = &pending.condition_id,
                        "skipping NegRisk MM notification until group extension is reflected in local mapping"
                    );
                    continue;
                }

                if self.config.mm_max_markets == 0 || mm_live < self.config.mm_max_markets {
                    match self
                        .mm_tx
                        .send(MmMessage::MarketMirrored {
                            sybil_market_id: pending.sybil_market_id,
                            yes_token_id: pending.yes_token_id.clone(),
                            initial_mid: pending.initial_mid,
                            group_key,
                            group_size,
                        })
                        .await
                    {
                        Ok(()) => {
                            mm_live += 1;
                            new_token_ids.push(pending.yes_token_id);
                        }
                        Err(e) => {
                            warn!(
                                sybil_id = pending.sybil_market_id,
                                error = %e,
                                "failed to notify MM about mirrored market"
                            );
                        }
                    }
                } else {
                    info!(
                        sybil_id = pending.sybil_market_id,
                        limit = self.config.mm_max_markets,
                        live = mm_live,
                        "created market but skipped live MM tracking (cap reached)"
                    );
                }
            }
        }

        // Notify Feed about new tokens to subscribe
        if !new_token_ids.is_empty() {
            if let Err(e) = self
                .feed_tx
                .send(FeedMessage::SubscribeTokens(new_token_ids))
                .await
            {
                warn!(error = %e, "failed to notify feed about new token subscriptions");
            }
        }

        Ok(())
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
fn build_metadata_request(event: &GammaEvent, market: &GammaMarket) -> SetMarketMetadataRequest {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn market(condition_id: &str, yes_token: &str, no_token: &str, title: &str) -> GammaMarket {
        GammaMarket {
            condition_id: condition_id.to_string(),
            question: format!("{title}?"),
            outcomes: serde_json::to_string(&["Yes", "No"]).unwrap(),
            outcome_prices: serde_json::to_string(&["0.50", "0.50"]).unwrap(),
            clob_token_ids: serde_json::to_string(&[yes_token, no_token]).unwrap(),
            active: true,
            closed: false,
            neg_risk: true,
            group_item_title: Some(title.to_string()),
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            volume: None,
            liquidity: None,
            slug: None,
            description: None,
            start_date: None,
            end_date: None,
            resolution_source: None,
            image: None,
            icon: None,
            umared: None,
            resolved_by: None,
            extra: Default::default(),
        }
    }

    fn negrisk_event(markets: Vec<GammaMarket>) -> GammaEvent {
        GammaEvent {
            id: "event-1".to_string(),
            title: "Election".to_string(),
            description: String::new(),
            slug: "election".to_string(),
            active: true,
            closed: false,
            enable_neg_risk: true,
            neg_risk: true,
            markets,
            tags: Vec::new(),
            volume: None,
            liquidity: None,
            start_date: None,
            end_date: None,
            created_at: None,
            image: None,
            icon: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn synced_negrisk_event_with_late_child_plans_group_extension() {
        let event = negrisk_event(vec![
            market("cond-1", "yes-1", "no-1", "A"),
            market("cond-2", "yes-2", "no-2", "B"),
            market("cond-3", "yes-3", "no-3", "C"),
        ]);
        let existing_group = GroupInfo {
            group_name: "Election".to_string(),
            sybil_market_ids: vec![0, 1],
            neg_risk: true,
        };

        let action = plan_negrisk_group_action(&event, &[0, 1, 2], Some(&existing_group));
        assert_eq!(
            action,
            NegRiskGroupAction::Extend {
                missing_market_ids: vec![2],
                existing_group_market_ids: vec![0, 1],
            }
        );
    }

    #[test]
    fn late_negrisk_child_uses_group_membership_after_extension() {
        let mut existing_group = GroupInfo {
            group_name: "Election".to_string(),
            sybil_market_ids: vec![0, 1],
            neg_risk: true,
        };
        existing_group.sybil_market_ids.push(2);

        assert_eq!(
            mm_group_membership("event-1", 0, Some(&existing_group)),
            (Some("event-1".to_string()), 3)
        );
        assert_eq!(
            mm_group_membership("event-1", 2, Some(&existing_group)),
            (Some("event-1".to_string()), 3)
        );
    }

    #[test]
    fn sybil_group_lookup_handles_h13_shrink_and_prior_extension() {
        let existing_group = GroupInfo {
            group_name: "Election".to_string(),
            sybil_market_ids: vec![0, 1, 2],
            neg_risk: true,
        };
        let groups = vec![
            MarketGroupResponse {
                group_id: 7,
                name: "Other".to_string(),
                market_ids: vec![0, 1],
            },
            MarketGroupResponse {
                group_id: 8,
                name: "Election".to_string(),
                market_ids: vec![0, 1],
            },
        ];
        assert_eq!(matching_sybil_group_id(&groups, &existing_group), Some(8));

        let groups = vec![MarketGroupResponse {
            group_id: 9,
            name: "Election".to_string(),
            market_ids: vec![0, 1, 2, 3],
        }];
        assert_eq!(matching_sybil_group_id(&groups, &existing_group), Some(9));
    }

    #[test]
    fn synced_event_with_new_unmapped_child_is_detected_for_resync() {
        let event = negrisk_event(vec![
            market("cond-1", "yes-1", "no-1", "A"),
            market("cond-2", "yes-2", "no-2", "B"),
            market("cond-3", "yes-3", "no-3", "C"),
        ]);
        let mut mapping = MappingStore::new(None);
        mapping.register_market("cond-1".to_string(), vec!["yes-1".into(), "no-1".into()], 0);
        mapping.register_market("cond-2".to_string(), vec!["yes-2".into(), "no-2".into()], 1);
        mapping.register_event(
            "event-1".to_string(),
            GroupInfo {
                group_name: "Election".to_string(),
                sybil_market_ids: vec![0, 1],
                neg_risk: true,
            },
        );

        let unmapped: Vec<_> = event
            .markets
            .iter()
            .filter(|market| mapping.sybil_market_id(&market.condition_id).is_none())
            .map(|market| market.condition_id.as_str())
            .collect();

        assert_eq!(unmapped, vec!["cond-3"]);
    }
}
