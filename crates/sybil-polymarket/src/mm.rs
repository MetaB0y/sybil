use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::feed::PriceSnapshot;
use crate::sybil::client::SybilClient;
use crate::sybil::types::*;

/// Message from SyncActor to MmActor.
#[derive(Debug)]
pub enum MmMessage {
    /// A new market was mirrored onto Sybil.
    MarketMirrored {
        sybil_market_id: u32,
        /// Polymarket YES token ID (used to look up reference price).
        yes_token_id: String,
        /// Initial midpoint from Polymarket.
        initial_mid: f64,
    },
}

/// Tracks a market the MM is quoting.
struct ActiveMarket {
    sybil_market_id: u32,
    yes_token_id: String,
}

/// Market maker actor. Listens to Sybil's SSE block stream and submits
/// orders each block using Polymarket reference prices.
pub struct MmActor {
    config: Config,
    sybil_client: SybilClient,
    account_id: u64,
    price_rx: watch::Receiver<PriceSnapshot>,
    mm_rx: mpsc::Receiver<MmMessage>,
    active_markets: Vec<ActiveMarket>,
}

impl MmActor {
    pub fn new(
        config: Config,
        sybil_client: SybilClient,
        account_id: u64,
        price_rx: watch::Receiver<PriceSnapshot>,
        mm_rx: mpsc::Receiver<MmMessage>,
    ) -> Self {
        Self {
            config,
            sybil_client,
            account_id,
            price_rx,
            mm_rx,
            active_markets: Vec::new(),
        }
    }

    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        info!(account_id = self.account_id, "MmActor started");

        loop {
            // Wait for at least one market to be mirrored
            if self.active_markets.is_empty() {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("MmActor shutting down");
                        return;
                    }
                    msg = self.mm_rx.recv() => {
                        if let Some(msg) = msg {
                            self.handle_message(msg);
                        } else {
                            return;
                        }
                    }
                }
                continue;
            }

            // Connect to SSE block stream
            info!(
                markets = self.active_markets.len(),
                "connecting to block stream"
            );
            let block_stream = match self.sybil_client.stream_blocks().await {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "failed to connect block stream, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            tokio::pin!(block_stream);

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("MmActor shutting down");
                        return;
                    }
                    msg = self.mm_rx.recv() => {
                        match msg {
                            Some(msg) => self.handle_message(msg),
                            None => return,
                        }
                    }
                    block = block_stream.next() => {
                        match block {
                            Some(Ok(block)) => {
                                self.on_block(&block).await;
                            }
                            Some(Err(e)) => {
                                warn!(error = %e, "block stream error");
                                break; // Reconnect
                            }
                            None => {
                                info!("block stream ended");
                                break; // Reconnect
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_message(&mut self, msg: MmMessage) {
        match msg {
            MmMessage::MarketMirrored {
                sybil_market_id,
                yes_token_id,
                initial_mid,
            } => {
                info!(
                    sybil_market_id,
                    yes_token_id,
                    initial_mid,
                    "MM tracking new market"
                );
                self.active_markets.push(ActiveMarket {
                    sybil_market_id,
                    yes_token_id,
                });
            }
        }
    }

    async fn on_block(&self, block: &BlockResponse) {
        let snapshot = self.price_rx.borrow().clone();
        let now = now_ms();
        let stale_threshold_ms = 30_000;

        let mut orders = Vec::new();
        let half_spread = self.config.mm_half_spread;
        let quote_size_dollars = self.config.mm_quote_size_dollars;

        for market in &self.active_markets {
            // Get reference price from Polymarket
            let mid = match snapshot.midpoints.get(&market.yes_token_id) {
                Some(&p) if p > 0.0 && p < 1.0 => p,
                _ => continue, // No price or invalid
            };

            // Check staleness
            if now.saturating_sub(snapshot.last_updated_ms) > stale_threshold_ms {
                debug!(
                    market_id = market.sybil_market_id,
                    "skipping stale price"
                );
                continue;
            }

            // Compute bid prices
            let yes_bid = mid - half_spread;
            let no_bid = (1.0 - mid) - half_spread;

            // BuyYes
            if (0.01..=0.99).contains(&yes_bid) {
                let price_nanos = (yes_bid * NANOS_PER_DOLLAR as f64) as u64;
                let qty = (quote_size_dollars / yes_bid).max(1.0) as u64;
                orders.push(OrderSpec::BuyYes {
                    market_id: market.sybil_market_id,
                    limit_price_nanos: price_nanos,
                    quantity: qty,
                });
            }

            // BuyNo
            if (0.01..=0.99).contains(&no_bid) {
                let price_nanos = (no_bid * NANOS_PER_DOLLAR as f64) as u64;
                let qty = (quote_size_dollars / no_bid).max(1.0) as u64;
                orders.push(OrderSpec::BuyNo {
                    market_id: market.sybil_market_id,
                    limit_price_nanos: price_nanos,
                    quantity: qty,
                });
            }
        }

        if orders.is_empty() {
            return;
        }

        let budget_nanos = (self.config.mm_budget_dollars * NANOS_PER_DOLLAR as f64) as u64;
        let req = SubmitOrderRequest {
            account_id: self.account_id,
            orders: orders.clone(),
            mm_budget_nanos: Some(budget_nanos),
        };

        match self.sybil_client.submit_orders(&req).await {
            Ok(accepted) => {
                debug!(
                    block = block.height,
                    order_count = req.orders.len(),
                    accepted,
                    "submitted MM orders"
                );
            }
            Err(e) => {
                warn!(block = block.height, error = %e, "order submission failed");
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
