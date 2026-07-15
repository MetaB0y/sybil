use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Config;
use crate::feed::FeedMessage;
use crate::mapping::{GroupInfo, MappingStore};
use crate::mm::MmMessage;
use crate::native::{NativeMarketCatalog, NativeMarketSpec};
use crate::polymarket::gamma::GammaClient;
#[cfg(test)]
use crate::polymarket::types::{GammaEvent, GammaMarket};
use sybil_api_types::*;
use sybil_client::SybilClient;

mod planning;

use planning::*;

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
    /// Exact child conditions retained from fetched curated parent events.
    /// This is mirror policy only; it is not protocol-validity state.
    curated_condition_ids: HashSet<String>,
    /// Native market templates to ensure on Sybil before the mirror scan.
    native_catalog: NativeMarketCatalog,
}

struct PendingMmNotification {
    sybil_market_id: u32,
    condition_id: String,
    yes_token_id: String,
    initial_mid: f64,
}

struct PendingNativeMmNotification {
    sybil_market_id: u32,
    spec: NativeMarketSpec,
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
        curated_condition_ids: Vec<String>,
        native_catalog: NativeMarketCatalog,
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
            curated_condition_ids: curated_condition_ids.into_iter().collect(),
            native_catalog,
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
        // Native mode (SYB-151): ensure enabled checked-in templates exist
        // before any Polymarket fetch. This path never uses raw Polymarket JSON
        // and never writes `polymarket_condition_id`.
        let mut mm_live = *self.mm_live_rx.borrow();
        self.ensure_native_catalog(&mut mm_live).await?;

        // Curated mode (SYB-150): mirror exactly the allowlisted events by id.
        // Otherwise fall back to the volume-ranked category scan.
        let mut events = if self.curated_event_ids.is_empty() {
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
        if !self.curated_condition_ids.is_empty() {
            for event in &mut events {
                event.markets.retain(|market| {
                    self.curated_condition_ids
                        .contains(&market.condition_id.to_ascii_lowercase())
                });
            }
            events.retain(|event| !event.markets.is_empty());
        }

        // Baseline live-set size for MM admission this cycle (PM-8). Copied out
        // of the watch so we never hold the borrow across an await, and bumped
        // locally as we admit so a single cycle cannot overshoot the cap before
        // the MM has processed our messages.
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
        if !new_token_ids.is_empty()
            && let Err(e) = self
                .feed_tx
                .send(FeedMessage::SubscribeTokens(new_token_ids))
                .await
        {
            warn!(error = %e, "failed to notify feed about new token subscriptions");
        }

        Ok(())
    }

    async fn ensure_native_catalog(
        &mut self,
        mm_live: &mut usize,
    ) -> Result<(), crate::error::Error> {
        let specs = self.native_catalog.enabled_market_specs();
        if specs.is_empty() {
            return Ok(());
        }

        let mut seen_templates = HashSet::new();
        let template_ids: Vec<String> = specs
            .iter()
            .filter(|spec| seen_templates.insert(spec.template_id.clone()))
            .map(|spec| spec.template_id.clone())
            .collect();

        for template_id in template_ids {
            let template_specs: Vec<_> = specs
                .iter()
                .filter(|spec| spec.template_id == template_id)
                .cloned()
                .collect();
            self.ensure_native_template(&template_id, &template_specs, mm_live)
                .await?;
        }

        Ok(())
    }

    async fn ensure_native_template(
        &mut self,
        template_id: &str,
        specs: &[NativeMarketSpec],
        mm_live: &mut usize,
    ) -> Result<(), crate::error::Error> {
        if specs.is_empty() {
            return Ok(());
        }

        let mut pending_mm = Vec::new();

        for spec in specs {
            if let Some(sybil_id) = self.mapping.read().await.native_market_id(&spec.market_key) {
                if self.first_sync {
                    let metadata_req = spec.metadata_request();
                    if let Err(e) = self
                        .sybil_client
                        .set_market_metadata(sybil_id, &metadata_req)
                        .await
                    {
                        warn!(
                            sybil_id,
                            native_market_key = %spec.market_key,
                            error = %e,
                            "native metadata backfill failed"
                        );
                    }
                }
                continue;
            }

            let create_req = spec.create_request();
            match self.sybil_client.create_market(&create_req).await {
                Ok(resp) => {
                    let sybil_id = resp.market_id;
                    info!(
                        sybil_id,
                        native_market_key = %spec.market_key,
                        name = %spec.name,
                        "created native market"
                    );

                    {
                        let mut map = self.mapping.write().await;
                        map.register_native_market(spec.market_key.clone(), sybil_id);
                        if let Err(e) = map.save() {
                            warn!(
                                sybil_id,
                                native_market_key = %spec.market_key,
                                error = %e,
                                "failed to persist native market mapping"
                            );
                        }
                    }

                    let metadata_req = spec.metadata_request();
                    if let Err(e) = self
                        .sybil_client
                        .set_market_metadata(sybil_id, &metadata_req)
                        .await
                    {
                        warn!(
                            sybil_id,
                            native_market_key = %spec.market_key,
                            error = %e,
                            "failed to set native market metadata (non-fatal; will retry next cycle)"
                        );
                    }

                    pending_mm.push(PendingNativeMmNotification {
                        sybil_market_id: sybil_id,
                        spec: spec.clone(),
                    });
                }
                Err(e) => {
                    warn!(
                        native_market_key = %spec.market_key,
                        error = %e,
                        "failed to create native market on Sybil"
                    );
                }
            }
        }

        let active_mapped_ids: Vec<u32> = {
            let map = self.mapping.read().await;
            specs
                .iter()
                .filter_map(|spec| map.native_market_id(&spec.market_key))
                .collect()
        };
        let unmapped_after = specs.len().saturating_sub(active_mapped_ids.len());
        let is_grouped = specs.iter().any(|spec| spec.group_key.is_some());

        if is_grouped {
            let existing_group = self.mapping.read().await.native_group(template_id);
            let group_action = if existing_group.is_none()
                && active_mapped_ids.len() > 1
                && unmapped_after > 0
            {
                warn!(
                    template_id,
                    mapped = active_mapped_ids.len(),
                    unmapped = unmapped_after,
                    "not creating partial native MarketGroup until all enabled child markets are mapped"
                );
                NegRiskGroupAction::None
            } else {
                plan_market_group_action(&active_mapped_ids, existing_group.as_ref())
            };

            match group_action {
                NegRiskGroupAction::Create(market_ids) => {
                    let group_name = specs[0].group_name().to_string();
                    let group_req = CreateMarketGroupRequest {
                        name: group_name.clone(),
                        market_ids: market_ids.clone(),
                    };
                    match self.sybil_client.create_market_group(&group_req).await {
                        Ok(_) => {
                            info!(
                                template_id,
                                markets = market_ids.len(),
                                "created native market group"
                            );
                            let mut map = self.mapping.write().await;
                            map.register_native_group(
                                template_id.to_string(),
                                GroupInfo {
                                    group_name,
                                    sybil_market_ids: market_ids,
                                    neg_risk: true,
                                },
                            );
                            if let Err(e) = map.save() {
                                warn!(
                                    template_id,
                                    error = %e,
                                    "failed to persist native group mapping"
                                );
                            }
                        }
                        Err(e) => {
                            warn!(
                                template_id,
                                error = %e,
                                "failed to create native market group"
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
                                template_id,
                                missing_market_ids = ?missing_market_ids,
                                error = %e,
                                "failed to list Sybil market groups before native extension"
                            );
                            Vec::new()
                        }
                    };
                    let Some(existing_group) = existing_group.as_ref() else {
                        warn!(
                            template_id,
                            missing_market_ids = ?missing_market_ids,
                            "planned native group extension without a local group mapping"
                        );
                        return Ok(());
                    };
                    let Some(group_id) = matching_sybil_group_id(&groups, existing_group) else {
                        warn!(
                            template_id,
                            missing_market_ids = ?missing_market_ids,
                            existing_group_market_ids = ?existing_group_market_ids,
                            "could not locate current Sybil MarketGroup for native extension"
                        );
                        return Ok(());
                    };

                    let mut extended = Vec::new();
                    for market_id in &missing_market_ids {
                        let req = ExtendMarketGroupRequest {
                            market_id: *market_id,
                        };
                        match self.sybil_client.extend_market_group(group_id, &req).await {
                            Ok(group) => {
                                info!(
                                    template_id,
                                    group_id,
                                    market_id,
                                    members = group.market_ids.len(),
                                    "extended native market group"
                                );
                                extended.push(*market_id);
                            }
                            Err(e) => {
                                warn!(
                                    template_id,
                                    group_id,
                                    market_id,
                                    error = %e,
                                    "failed to extend native market group"
                                );
                            }
                        }
                    }
                    if !extended.is_empty() {
                        let mut map = self.mapping.write().await;
                        map.extend_native_group(template_id, &extended);
                        if let Err(e) = map.save() {
                            warn!(
                                template_id,
                                error = %e,
                                "failed to persist native group extension"
                            );
                        }
                    }
                }
                NegRiskGroupAction::None => {}
            }
        }

        let group_after_sync = self.mapping.read().await.native_group(template_id);
        for pending in pending_mm {
            let (group_key, group_size) = native_mm_group_membership(
                pending.sybil_market_id,
                pending.spec.group_key.clone(),
                group_after_sync.as_ref(),
            );

            if pending.spec.group_key.is_some() && group_key.is_none() {
                warn!(
                    template_id,
                    sybil_id = pending.sybil_market_id,
                    native_market_key = %pending.spec.market_key,
                    "skipping native MM notification until group is reflected in local mapping"
                );
                continue;
            }

            if self.config.mm_max_markets == 0 || *mm_live < self.config.mm_max_markets {
                match self
                    .mm_tx
                    .send(MmMessage::MarketNative {
                        sybil_market_id: pending.sybil_market_id,
                        native_market_key: pending.spec.market_key.clone(),
                        quote_range: to_mm_quote_range(pending.spec.quote_range),
                        group_key,
                        group_size,
                    })
                    .await
                {
                    Ok(()) => {
                        *mm_live += 1;
                    }
                    Err(e) => {
                        warn!(
                            sybil_id = pending.sybil_market_id,
                            native_market_key = %pending.spec.market_key,
                            error = %e,
                            "failed to notify MM about native market"
                        );
                    }
                }
            } else {
                info!(
                    sybil_id = pending.sybil_market_id,
                    native_market_key = %pending.spec.market_key,
                    limit = self.config.mm_max_markets,
                    live = *mm_live,
                    "created native market but skipped live MM tracking (cap reached)"
                );
            }
        }

        Ok(())
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
    fn native_group_with_late_child_plans_group_extension() {
        let existing_group = GroupInfo {
            group_name: "Native event".to_string(),
            sybil_market_ids: vec![10, 11],
            neg_risk: true,
        };

        let action = plan_market_group_action(&[10, 11, 12], Some(&existing_group));
        assert_eq!(
            action,
            NegRiskGroupAction::Extend {
                missing_market_ids: vec![12],
                existing_group_market_ids: vec![10, 11],
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
    fn native_mm_group_membership_requires_local_group_mapping() {
        assert_eq!(
            native_mm_group_membership(10, Some("native:event".to_string()), None),
            (None, 0)
        );

        let group = GroupInfo {
            group_name: "Native event".to_string(),
            sybil_market_ids: vec![10, 11],
            neg_risk: true,
        };
        assert_eq!(
            native_mm_group_membership(10, Some("native:event".to_string()), Some(&group)),
            (Some("native:event".to_string()), 2)
        );
        assert_eq!(
            native_mm_group_membership(12, Some("native:event".to_string()), Some(&group)),
            (None, 0)
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
