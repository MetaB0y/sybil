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
    /// token_id -> timestamp of that token's last update (ms since epoch).
    /// Parallel to `midpoints`. Enables per-token staleness (PM-4) instead of
    /// a single global clock that a busy neighbour token keeps alive.
    pub token_updated_ms: HashMap<String, u64>,
    /// Timestamp of last update to ANY token (ms since epoch). Retained for
    /// coarse "feed is alive" diagnostics; per-token freshness lives in
    /// `token_updated_ms`.
    pub last_updated_ms: u64,
    /// Source of the most recent update.
    pub source: PriceSource,
}

impl PriceSnapshot {
    /// Record a fresh midpoint for `token_id`, stamping its per-token clock.
    pub fn record_midpoint(&mut self, token_id: String, price: f64, now_ms: u64) {
        self.token_updated_ms.insert(token_id.clone(), now_ms);
        self.midpoints.insert(token_id, price);
    }

    /// True when `token_id` has not updated within `max_age_ms` of `now_ms`
    /// (or was never seen). A stale token is neither quoted nor pushed as a
    /// live reference price.
    pub fn token_is_stale(&self, token_id: &str, now_ms: u64, max_age_ms: u64) -> bool {
        match self.token_updated_ms.get(token_id) {
            Some(&ts) => now_ms.saturating_sub(ts) > max_age_ms,
            None => true,
        }
    }
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

        'feed: loop {
            // Check for new token subscriptions (non-blocking drain)
            while let Ok(msg) = self.feed_rx.try_recv() {
                self.handle_message(msg);
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
                                self.handle_message(msg);
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
            self.poll_rest_once().await;
            // Clone the connection inputs so the `feed_rx.recv()` branch below
            // can take `&mut self` to fold in a new subscription and reconnect
            // immediately, instead of waiting for the current WebSocket to drop.
            let ws_url = self.config.ws_url.clone();
            let token_ids = self.token_ids.clone();
            let price_tx = self.price_tx.clone();
            let ws_feed = ws::run_ws_feed(&ws_url, &token_ids, &price_tx);
            tokio::pin!(ws_feed);
            let mut rest_refresh = tokio::time::interval(Duration::from_secs(
                self.config.rest_poll_interval_secs.max(1),
            ));
            // The pre-connect snapshot above is the immediate refresh; consume
            // interval's immediate first tick so the next one observes the
            // configured cadence.
            rest_refresh.tick().await;

            let ws_result = loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("FeedActor shutting down");
                        return;
                    }
                    msg = self.feed_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                let added = self.handle_message(msg);
                                if added > 0 {
                                    info!(
                                        added,
                                        tokens = self.token_ids.len(),
                                        "token subscription changed; reconnecting WebSocket"
                                    );
                                    backoff_secs = 1;
                                    continue 'feed;
                                }
                            }
                            None => return,
                        }
                    }
                    _ = rest_refresh.tick() => {
                        // WebSocket messages are change-driven. Quiet order
                        // books still need fresh timestamps so unchanged prices
                        // do not become falsely stale.
                        self.poll_rest_once().await;
                    }
                    result = &mut ws_feed => break result,
                }
            };

            match ws_result {
                Ok(()) => {
                    // Clean disconnect (proactive reconnect or server close)
                    backoff_secs = 1;
                }
                Err(e) => {
                    if e.is_expected_websocket_disconnect() {
                        info!(error = %e, "WebSocket disconnected, refreshing via REST before reconnect");
                    } else {
                        warn!(error = %e, "WebSocket failed, falling back to REST");
                    }
                    self.poll_rest_once().await;
                    backoff_secs = (backoff_secs * 2).min(60);
                }
            }

            // Drain any new subscriptions before reconnecting
            while let Ok(msg) = self.feed_rx.try_recv() {
                self.handle_message(msg);
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
                if prices.is_empty() && !self.token_ids.is_empty() {
                    warn!(
                        tokens = self.token_ids.len(),
                        "REST midpoints returned no prices"
                    );
                }
                let mut snapshot = self.price_tx.borrow().clone();
                let now = now_ms();
                for (token_id, price) in prices {
                    snapshot.record_midpoint(token_id, price, now);
                }
                snapshot.last_updated_ms = now;
                snapshot.source = PriceSource::RestFallback;
                let _ = self.price_tx.send(snapshot);
            }
            Err(e) => {
                warn!(error = %e, "REST midpoints fallback failed");
            }
        }
    }

    /// Fold a subscription message into `token_ids`, returning how many token
    /// IDs were newly added (0 when every id was already subscribed).
    fn handle_message(&mut self, msg: FeedMessage) -> usize {
        let FeedMessage::SubscribeTokens(ids) = msg;
        let mut added = 0;
        for id in ids {
            if !self.token_ids.contains(&id) {
                self.token_ids.push(id);
                added += 1;
            }
        }
        added
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_token_staleness_is_independent() {
        let mut snap = PriceSnapshot::default();
        // "hot" updated at t=10_000, "frozen" updated long ago at t=0.
        snap.record_midpoint("hot".into(), 0.6, 10_000);
        snap.record_midpoint("frozen".into(), 0.4, 0);

        let now = 20_000;
        let threshold = 30_000;
        // Even though a neighbour ("hot") is fresh, "frozen" is judged on its
        // own clock: 20s old, still inside the 30s window here.
        assert!(!snap.token_is_stale("frozen", now, threshold));

        // Push time forward: "frozen" crosses the threshold while "hot" would
        // not — the global-clock bug (PM-4) cannot mask it.
        let now = 35_000;
        assert!(snap.token_is_stale("frozen", now, threshold));
        assert!(!snap.token_is_stale("hot", now, threshold));
    }

    #[test]
    fn unknown_token_is_stale() {
        let snap = PriceSnapshot::default();
        assert!(snap.token_is_stale("never-seen", 1_000, 30_000));
    }
}
