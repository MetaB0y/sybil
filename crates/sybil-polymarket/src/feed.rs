use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::config::Config;
use crate::polymarket::gamma::GammaClient;
use crate::polymarket::ws;

/// Message from SyncActor to FeedActor.
#[derive(Debug)]
pub enum FeedMessage {
    /// New token IDs to subscribe to.
    SubscribeTokens(Vec<String>),
}

/// Source of the most recent price update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PriceSource {
    #[default]
    None,
    WebSocket,
    RestFallback,
}

/// Snapshot of reference prices from Polymarket.
#[derive(Debug, Clone, Default)]
pub struct PriceSnapshot {
    /// token_id -> midpoint price (0.0 to 1.0)
    pub midpoints: HashMap<String, f64>,
    /// Timestamp of last update (ms since epoch).
    pub last_updated_ms: u64,
    /// Source of the most recent update.
    pub source: PriceSource,
}

/// Price feed actor. Maintains a WebSocket connection to Polymarket CLOB,
/// falls back to REST polling on failure.
pub struct FeedActor {
    config: Config,
    gamma_client: GammaClient,
    price_tx: watch::Sender<PriceSnapshot>,
    feed_rx: mpsc::Receiver<FeedMessage>,
    /// All token IDs we should be subscribed to.
    token_ids: Vec<String>,
}

impl FeedActor {
    pub fn new(
        config: Config,
        gamma_client: GammaClient,
        price_tx: watch::Sender<PriceSnapshot>,
        feed_rx: mpsc::Receiver<FeedMessage>,
    ) -> Self {
        Self {
            config,
            gamma_client,
            price_tx,
            feed_rx,
            token_ids: Vec::new(),
        }
    }

    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        info!("FeedActor started");
        let mut backoff_secs = 1u64;

        loop {
            // Check for new token subscriptions (non-blocking drain)
            while let Ok(msg) = self.feed_rx.try_recv() {
                let FeedMessage::SubscribeTokens(ids) = msg;
                for id in ids {
                    if !self.token_ids.contains(&id) {
                        self.token_ids.push(id);
                    }
                }
            }

            if self.token_ids.is_empty() {
                // Nothing to subscribe to yet — wait for sync actor
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("FeedActor shutting down");
                        return;
                    }
                    msg = self.feed_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                let FeedMessage::SubscribeTokens(ids) = msg;
                                self.token_ids.extend(ids);
                            }
                            None => return, // Channel closed
                        }
                    }
                }
                continue;
            }

            // Try WebSocket
            info!(
                tokens = self.token_ids.len(),
                "attempting WebSocket connection"
            );
            let ws_result = tokio::select! {
                _ = cancel.cancelled() => {
                    info!("FeedActor shutting down");
                    return;
                }
                result = ws::run_ws_feed(&self.config.ws_url, &self.token_ids, &self.price_tx) => result,
            };

            match ws_result {
                Ok(()) => {
                    // Clean disconnect (proactive reconnect or server close)
                    backoff_secs = 1;
                }
                Err(e) => {
                    warn!(error = %e, "WebSocket failed, falling back to REST");
                    self.poll_rest_once().await;
                    backoff_secs = (backoff_secs * 2).min(60);
                }
            }

            // Drain any new subscriptions before reconnecting
            while let Ok(msg) = self.feed_rx.try_recv() {
                let FeedMessage::SubscribeTokens(ids) = msg;
                for id in ids {
                    if !self.token_ids.contains(&id) {
                        self.token_ids.push(id);
                    }
                }
            }

            // Backoff before reconnect
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("FeedActor shutting down");
                    return;
                }
                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
            }
        }
    }

    async fn poll_rest_once(&self) {
        match self.gamma_client.fetch_midpoints(&self.token_ids).await {
            Ok(prices) => {
                let mut snapshot = self.price_tx.borrow().clone();
                for (token_id, price) in prices {
                    snapshot.midpoints.insert(token_id, price);
                }
                snapshot.last_updated_ms = now_ms();
                snapshot.source = PriceSource::RestFallback;
                let _ = self.price_tx.send(snapshot);
            }
            Err(e) => {
                warn!(error = %e, "REST midpoints fallback failed");
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
