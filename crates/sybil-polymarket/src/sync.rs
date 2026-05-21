use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::categorize::derive_categories;
use crate::config::Config;
use crate::feed::FeedMessage;
use crate::mapping::{GroupInfo, MappingStore};
use crate::mm::MmMessage;
use crate::polymarket::gamma::GammaClient;
use crate::polymarket::types::{parse_iso8601_to_ms, GammaEvent, GammaMarket};
use crate::sybil::client::SybilClient;
use sybil_api_types::*;

/// Market sync actor. Polls Polymarket for new events, creates corresponding
/// markets on Sybil, and notifies Feed and MM actors.
pub struct SyncActor {
    config: Config,
    gamma_client: GammaClient,
    sybil_client: SybilClient,
    mapping: Arc<RwLock<MappingStore>>,
    feed_tx: mpsc::Sender<FeedMessage>,
    mm_tx: mpsc::Sender<MmMessage>,
}

impl SyncActor {
    pub fn new(
        config: Config,
        gamma_client: GammaClient,
        sybil_client: SybilClient,
        mapping: Arc<RwLock<MappingStore>>,
        feed_tx: mpsc::Sender<FeedMessage>,
        mm_tx: mpsc::Sender<MmMessage>,
    ) -> Self {
        Self {
            config,
            gamma_client,
            sybil_client,
            mapping,
            feed_tx,
            mm_tx,
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
        let events = self
            .gamma_client
            .fetch_active_events(
                self.config.max_events,
                &self.config.mirror_categories,
                &self.config.mirror_excluded_categories,
                self.config.min_volume_usd,
            )
            .await?;

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

        let mut new_token_ids = Vec::new();

        for event in &events {
            if self.mapping.read().await.is_event_synced(&event.id) {
                continue;
            }

            let active_markets: Vec<_> = event
                .markets
                .iter()
                .filter(|m| m.active && !m.closed)
                .collect();

            if active_markets.is_empty() {
                continue;
            }

            info!(
                event_id = &event.id,
                title = &event.title,
                markets = active_markets.len(),
                neg_risk = event.is_neg_risk(),
                "syncing new event"
            );

            let mut sybil_market_ids = Vec::new();

            for poly_market in &active_markets {
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
                        sybil_market_ids.push(sybil_id);

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

                        if self.config.mm_max_markets == 0
                            || self.mapping.read().await.market_count()
                                <= self.config.mm_max_markets
                        {
                            // Notify MM about the new market
                            let initial_mid = poly_market.yes_price().unwrap_or(0.5);
                            let _ = self
                                .mm_tx
                                .send(MmMessage::MarketMirrored {
                                    sybil_market_id: sybil_id,
                                    yes_token_id: token_ids[0].clone(),
                                    initial_mid,
                                    group_key: event.is_neg_risk().then(|| event.id.clone()),
                                    group_size: if event.is_neg_risk() {
                                        active_markets.len()
                                    } else {
                                        0
                                    },
                                })
                                .await;

                            // Collect token IDs for Feed subscription
                            new_token_ids.extend(token_ids);
                        } else {
                            info!(
                                sybil_id,
                                limit = self.config.mm_max_markets,
                                "created market but skipped live MM tracking"
                            );
                        }
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

            // Create market group for NegRisk events with multiple markets
            if event.is_neg_risk() && sybil_market_ids.len() > 1 {
                let group_req = CreateMarketGroupRequest {
                    name: event.title.clone(),
                    market_ids: sybil_market_ids.clone(),
                };
                match self.sybil_client.create_market_group(&group_req).await {
                    Ok(_) => {
                        info!(
                            event_id = &event.id,
                            markets = sybil_market_ids.len(),
                            "created market group"
                        );
                        self.mapping.write().await.register_event(
                            event.id.clone(),
                            GroupInfo {
                                group_name: event.title.clone(),
                                sybil_market_ids: sybil_market_ids.clone(),
                                neg_risk: true,
                            },
                        );
                    }
                    Err(e) => {
                        warn!(event_id = &event.id, error = %e, "failed to create market group");
                    }
                }
            } else if !sybil_market_ids.is_empty() {
                self.mapping.write().await.mark_event_synced(&event.id);
            } else {
                warn!(
                    event_id = &event.id,
                    "no Sybil markets created; leaving event unsynced for retry"
                );
            }
        }

        // Notify Feed about new tokens to subscribe
        if !new_token_ids.is_empty() {
            let _ = self
                .feed_tx
                .send(FeedMessage::SubscribeTokens(new_token_ids))
                .await;
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
    }
}
