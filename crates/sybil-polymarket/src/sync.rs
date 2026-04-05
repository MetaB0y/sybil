use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::Config;
use crate::feed::FeedMessage;
use crate::mapping::{GroupInfo, MappingStore};
use crate::mm::MmMessage;
use crate::polymarket::gamma::GammaClient;
use crate::sybil::client::SybilClient;
use crate::sybil::types::*;

/// Market sync actor. Polls Polymarket for new events, creates corresponding
/// markets on Sybil, and notifies Feed and MM actors.
pub struct SyncActor {
    config: Config,
    gamma_client: GammaClient,
    sybil_client: SybilClient,
    mapping: MappingStore,
    feed_tx: mpsc::Sender<FeedMessage>,
    mm_tx: mpsc::Sender<MmMessage>,
}

impl SyncActor {
    pub fn new(
        config: Config,
        gamma_client: GammaClient,
        sybil_client: SybilClient,
        mapping: MappingStore,
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
            if let Err(e) = self.mapping.save() {
                warn!(error = %e, "failed to save mapping");
            }

            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("SyncActor shutting down");
                    let _ = self.mapping.save();
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
                self.config.min_volume_usd,
            )
            .await?;

        info!(
            events = events.len(),
            synced = self.mapping.event_count(),
            "fetched events from Polymarket"
        );

        let mut new_token_ids = Vec::new();

        for event in &events {
            if self.mapping.is_event_synced(&event.id) {
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
                    category: None,
                    tags: Some(vec!["polymarket".to_string()]),
                    resolution_criteria: poly_market.resolution_source.clone(),
                    expiry_timestamp_ms: None,
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

                        self.mapping.register_market(
                            poly_market.condition_id.clone(),
                            token_ids.clone(),
                            sybil_id,
                        );
                        sybil_market_ids.push(sybil_id);

                        // Notify MM about the new market
                        let initial_mid = poly_market.yes_price().unwrap_or(0.5);
                        let _ = self
                            .mm_tx
                            .send(MmMessage::MarketMirrored {
                                sybil_market_id: sybil_id,
                                yes_token_id: token_ids[0].clone(),
                                initial_mid,
                                in_group: event.is_neg_risk(),
                            })
                            .await;

                        // Collect token IDs for Feed subscription
                        new_token_ids.extend(token_ids);
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
                        self.mapping.register_event(
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
            } else {
                self.mapping.mark_event_synced(&event.id);
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

    /// Consume the actor and return the mapping store (for persistence on shutdown).
    pub fn into_mapping(self) -> MappingStore {
        self.mapping
    }
}
