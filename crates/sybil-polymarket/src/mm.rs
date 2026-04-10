use std::collections::{HashMap, VecDeque};

use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::feed::PriceSnapshot;
use crate::sybil::client::SybilClient;
use sybil_api_types::*;

/// Default variance prior for markets with insufficient price history.
const DEFAULT_VARIANCE: f64 = 0.0005;

// --------------------------------------------------------------------------- //
// Messages
// --------------------------------------------------------------------------- //

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
        /// Whether this market is part of a NegRisk group.
        in_group: bool,
    },
}

// --------------------------------------------------------------------------- //
// Per-market state
// --------------------------------------------------------------------------- //

struct MarketState {
    sybil_market_id: u32,
    yes_token_id: String,
    in_group: bool,
    // Inventory (updated via periodic API sync)
    yes_position: i64,
    no_position: i64,
    // Price history for variance estimation
    price_history: VecDeque<f64>,
    vol_window: usize,
}

impl MarketState {
    fn new(sybil_market_id: u32, yes_token_id: String, in_group: bool, initial_mid: f64, vol_window: usize) -> Self {
        let mut price_history = VecDeque::with_capacity(vol_window + 1);
        price_history.push_back(initial_mid);
        Self {
            sybil_market_id,
            yes_token_id,
            in_group,
            yes_position: 0,
            no_position: 0,
            price_history,
            vol_window,
        }
    }

    /// Net inventory: positive = long YES, negative = long NO.
    fn net_inventory(&self) -> f64 {
        (self.yes_position - self.no_position) as f64
    }

    fn push_price(&mut self, mid: f64) {
        self.price_history.push_back(mid);
        while self.price_history.len() > self.vol_window {
            self.price_history.pop_front();
        }
    }

    /// Rolling variance of mid prices. Returns DEFAULT_VARIANCE if insufficient data.
    fn variance(&self) -> f64 {
        let n = self.price_history.len();
        if n < 3 {
            return DEFAULT_VARIANCE;
        }
        let mean: f64 = self.price_history.iter().sum::<f64>() / n as f64;
        let var = self.price_history.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        var.max(DEFAULT_VARIANCE)
    }

    /// Dollar exposure for this market given a reference mid price.
    fn exposure(&self, mid: f64) -> f64 {
        let yes_val = self.yes_position as f64 * mid;
        let no_val = self.no_position as f64 * (1.0 - mid);
        yes_val.abs() + no_val.abs()
    }
}

// --------------------------------------------------------------------------- //
// Aggregate MM state
// --------------------------------------------------------------------------- //

struct MmState {
    markets: HashMap<u32, MarketState>,
    last_sync_block: u64,
}

impl MmState {
    fn new() -> Self {
        Self {
            markets: HashMap::new(),
            last_sync_block: 0,
        }
    }
}

// --------------------------------------------------------------------------- //
// MmActor
// --------------------------------------------------------------------------- //

/// Inventory-aware market maker. Adapts Avellaneda-Stoikov for FBA:
/// - Reservation price skewed by inventory × γ × σ²
/// - Two-sided quotes (BuyYes + SellYes for groups, full four-sided for standalone)
/// - Dynamic budget that shrinks as exposure grows
/// - Position limits with unwind-only mode
pub struct MmActor {
    config: Config,
    sybil_client: SybilClient,
    account_id: u64,
    price_rx: watch::Receiver<PriceSnapshot>,
    mm_rx: mpsc::Receiver<MmMessage>,
    state: MmState,
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
            state: MmState::new(),
        }
    }

    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        info!(account_id = self.account_id, "MmActor started");

        loop {
            // Wait for at least one market to be mirrored
            if self.state.markets.is_empty() {
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
                markets = self.state.markets.len(),
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
                in_group,
            } => {
                info!(
                    sybil_market_id,
                    yes_token_id, initial_mid, in_group, "MM tracking new market"
                );
                self.state.markets.insert(
                    sybil_market_id,
                    MarketState::new(
                        sybil_market_id,
                        yes_token_id,
                        in_group,
                        initial_mid,
                        self.config.mm_vol_window,
                    ),
                );
            }
        }
    }

    // ----- Position sync -------------------------------------------------- //

    async fn sync_positions(&mut self) {
        let account = match self.sybil_client.get_account(self.account_id).await {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "position sync failed");
                return;
            }
        };

        // Reset all positions to 0, then fill from API response
        for ms in self.state.markets.values_mut() {
            ms.yes_position = 0;
            ms.no_position = 0;
        }
        for pos in &account.positions {
            if let Some(ms) = self.state.markets.get_mut(&pos.market_id) {
                match pos.outcome.as_str() {
                    "YES" => ms.yes_position = pos.quantity,
                    "NO" => ms.no_position = pos.quantity,
                    _ => {}
                }
            }
        }

        debug!(
            balance = account.balance_nanos as f64 / NANOS_PER_DOLLAR as f64,
            positions = account.positions.len(),
            "position sync complete"
        );
    }

    // ----- Budget computation --------------------------------------------- //

    fn compute_budget(&self, snapshot: &PriceSnapshot) -> u64 {
        let max_exposure = self.config.mm_max_exposure_dollars;
        if max_exposure <= 0.0 {
            return (self.config.mm_budget_dollars * NANOS_PER_DOLLAR as f64) as u64;
        }

        let total_exposure: f64 = self.state.markets.values().map(|ms| {
            let mid = snapshot.midpoints.get(&ms.yes_token_id).copied().unwrap_or(0.5);
            ms.exposure(mid)
        }).sum();

        let ratio = (total_exposure / max_exposure).min(1.0);
        let scale = (1.0 - ratio).powi(2); // Quadratic decay
        let budget = self.config.mm_budget_dollars * scale;

        (budget * NANOS_PER_DOLLAR as f64) as u64
    }

    // ----- Per-block quote generation ------------------------------------- //

    async fn on_block(&mut self, block: &BlockResponse) {
        let snapshot = self.price_rx.borrow().clone();
        let now = now_ms();
        let stale_threshold_ms = 30_000;

        // 1. Periodic position sync
        if block.height.saturating_sub(self.state.last_sync_block) >= self.config.mm_sync_interval_blocks
            || self.state.last_sync_block == 0
        {
            self.sync_positions().await;
            self.state.last_sync_block = block.height;
        }

        // 2. Dynamic budget
        let budget_nanos = self.compute_budget(&snapshot);
        if budget_nanos == 0 {
            debug!("budget exhausted (exposure at max), skipping block");
            return;
        }

        // 3. Build orders for each market
        let gamma = self.config.mm_gamma;
        let base_spread = self.config.mm_half_spread;
        let min_spread = self.config.mm_min_spread;
        let max_pos = self.config.mm_max_position as i64;
        let quote_size = self.config.mm_quote_size_dollars;

        let mut orders = Vec::new();
        let mut ref_prices = HashMap::new();

        // Collect market IDs to iterate (can't borrow self.state.markets mutably in loop)
        let market_ids: Vec<u32> = self.state.markets.keys().copied().collect();

        for market_id in market_ids {
            let ms = self.state.markets.get_mut(&market_id).unwrap();

            // Get reference price from Polymarket
            let mid = match snapshot.midpoints.get(&ms.yes_token_id) {
                Some(&p) if p > 0.01 && p < 0.99 => p,
                _ => continue,
            };

            // Reference price for Sybil display
            ref_prices.insert(ms.sybil_market_id, (mid * NANOS_PER_DOLLAR as f64) as u64);

            // Staleness check
            if now.saturating_sub(snapshot.last_updated_ms) > stale_threshold_ms {
                continue;
            }

            // Update price history
            ms.push_price(mid);

            // Variance
            let sigma_sq = ms.variance();

            // Reservation price (Avellaneda-Stoikov)
            let q = ms.net_inventory();
            let r = (mid - q * gamma * sigma_sq).clamp(0.02, 0.98);

            // Adaptive spread: wider when volatile
            let vol_spread = base_spread * (1.0 + sigma_sq * 200.0);
            let edge_room = r.min(1.0 - r);
            let half_spread = vol_spread.clamp(min_spread, (edge_room - 0.01).max(min_spread));

            // Position limits
            let at_yes_limit = ms.yes_position >= max_pos;
            let at_no_limit = ms.no_position >= max_pos;

            // Inventory-adjusted sizing: shrink buys when loaded, grow sells
            let inv_ratio = (q.abs() / max_pos as f64).min(1.0);
            let buy_size = quote_size * (1.0 - inv_ratio * 0.8);
            let sell_size = quote_size * (1.0 + inv_ratio * 0.5);

            // ----- YES side -----
            let yes_bid = r - half_spread;
            let yes_ask = r + half_spread;

            // BuyYes (bid)
            if !at_yes_limit && yes_bid > 0.01 && yes_bid < 0.99 {
                orders.push(OrderSpec::BuyYes {
                    market_id: ms.sybil_market_id,
                    limit_price_nanos: (yes_bid * NANOS_PER_DOLLAR as f64) as u64,
                    quantity: (buy_size / yes_bid).max(1.0) as u64,
                });
            }

            // SellYes (ask) — only to unwind existing YES inventory (no shorts)
            if ms.yes_position > 0 && yes_ask > 0.01 && yes_ask < 0.99 {
                let max_sell = ms.yes_position as u64;
                let desired = (sell_size / yes_ask).max(1.0) as u64;
                orders.push(OrderSpec::SellYes {
                    market_id: ms.sybil_market_id,
                    limit_price_nanos: (yes_ask * NANOS_PER_DOLLAR as f64) as u64,
                    quantity: desired.min(max_sell),
                });
            }

            // ----- NO side (standalone markets only) -----
            if !ms.in_group {
                let no_bid = (1.0 - r) - half_spread;
                let no_ask = (1.0 - r) + half_spread;

                if !at_no_limit && no_bid > 0.01 && no_bid < 0.99 {
                    orders.push(OrderSpec::BuyNo {
                        market_id: ms.sybil_market_id,
                        limit_price_nanos: (no_bid * NANOS_PER_DOLLAR as f64) as u64,
                        quantity: (buy_size / no_bid).max(1.0) as u64,
                    });
                }

                // SellNo — only to unwind existing NO inventory (no shorts)
                if ms.no_position > 0 && no_ask > 0.01 && no_ask < 0.99 {
                    let max_sell = ms.no_position as u64;
                    let desired = (sell_size / no_ask).max(1.0) as u64;
                    orders.push(OrderSpec::SellNo {
                        market_id: ms.sybil_market_id,
                        limit_price_nanos: (no_ask * NANOS_PER_DOLLAR as f64) as u64,
                        quantity: desired.min(max_sell),
                    });
                }
            }
        }

        if orders.is_empty() {
            return;
        }

        // 4. Submit
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
                    budget_dollars = budget_nanos as f64 / NANOS_PER_DOLLAR as f64,
                    "submitted MM orders"
                );
            }
            Err(e) => {
                warn!(block = block.height, error = %e, "order submission failed");
            }
        }

        // 5. Push reference prices
        if !ref_prices.is_empty() {
            let _ = self.sybil_client.set_reference_prices(&ref_prices).await;
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
