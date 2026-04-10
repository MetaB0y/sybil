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
// QuoteEngine — pure pricing logic, no IO
// --------------------------------------------------------------------------- //

/// Inputs to the quoting engine for one market.
#[derive(Clone, Debug)]
pub struct QuoteInput {
    pub market_id: u32,
    pub mid: f64,
    pub sigma_sq: f64,
    pub net_inventory: f64,
    pub yes_position: i64,
    pub no_position: i64,
    pub in_group: bool,
}

/// Configuration for quote generation.
#[derive(Clone, Debug)]
pub struct QuoteConfig {
    pub gamma: f64,
    pub base_spread: f64,
    pub min_spread: f64,
    pub max_position: i64,
    pub quote_size_dollars: f64,
}

/// Generate orders for one market. Pure function — no IO, no state mutation.
pub fn generate_quotes(input: &QuoteInput, config: &QuoteConfig) -> Vec<OrderSpec> {
    let mut orders = Vec::new();

    // Avellaneda-Stoikov reservation price
    let r = (input.mid - input.net_inventory * config.gamma * input.sigma_sq).clamp(0.02, 0.98);

    // Adaptive spread: wider when volatile
    let vol_spread = config.base_spread * (1.0 + input.sigma_sq * 200.0);
    let edge_room = r.min(1.0 - r);
    let half_spread = vol_spread.clamp(config.min_spread, (edge_room - 0.01).max(config.min_spread));

    // Position limits
    let at_yes_limit = input.yes_position >= config.max_position;
    let at_no_limit = input.no_position >= config.max_position;

    // Inventory-adjusted sizing
    let inv_ratio = (input.net_inventory.abs() / config.max_position as f64).min(1.0);
    let buy_size = config.quote_size_dollars * (1.0 - inv_ratio * 0.8);
    let sell_size = config.quote_size_dollars * (1.0 + inv_ratio * 0.5);

    // ── YES side ──
    let yes_bid = r - half_spread;
    let yes_ask = r + half_spread;

    if !at_yes_limit && yes_bid > 0.01 && yes_bid < 0.99 {
        orders.push(OrderSpec::BuyYes {
            market_id: input.market_id,
            limit_price_nanos: (yes_bid * NANOS_PER_DOLLAR as f64) as u64,
            quantity: (buy_size / yes_bid).max(1.0) as u64,
        });
    }

    if input.yes_position > 0 && yes_ask > 0.01 && yes_ask < 0.99 {
        let max_sell = input.yes_position as u64;
        let desired = (sell_size / yes_ask).max(1.0) as u64;
        orders.push(OrderSpec::SellYes {
            market_id: input.market_id,
            limit_price_nanos: (yes_ask * NANOS_PER_DOLLAR as f64) as u64,
            quantity: desired.min(max_sell),
        });
    }

    // ── NO side (standalone markets only) ──
    if !input.in_group {
        let no_bid = (1.0 - r) - half_spread;
        let no_ask = (1.0 - r) + half_spread;

        if !at_no_limit && no_bid > 0.01 && no_bid < 0.99 {
            orders.push(OrderSpec::BuyNo {
                market_id: input.market_id,
                limit_price_nanos: (no_bid * NANOS_PER_DOLLAR as f64) as u64,
                quantity: (buy_size / no_bid).max(1.0) as u64,
            });
        }

        if input.no_position > 0 && no_ask > 0.01 && no_ask < 0.99 {
            let max_sell = input.no_position as u64;
            let desired = (sell_size / no_ask).max(1.0) as u64;
            orders.push(OrderSpec::SellNo {
                market_id: input.market_id,
                limit_price_nanos: (no_ask * NANOS_PER_DOLLAR as f64) as u64,
                quantity: desired.min(max_sell),
            });
        }
    }

    orders
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

        // 1. Periodic position sync
        self.maybe_sync_positions(block.height).await;

        // 2. Dynamic budget
        let budget_nanos = self.compute_budget(&snapshot);
        if budget_nanos == 0 {
            debug!("budget exhausted (exposure at max), skipping block");
            return;
        }

        // 3. Update state (mutation pass): push prices, collect reference prices
        let stale = now.saturating_sub(snapshot.last_updated_ms) > 30_000;
        let mut ref_prices = HashMap::new();
        let mut quote_inputs = Vec::new();

        for ms in self.state.markets.values_mut() {
            let mid = match snapshot.midpoints.get(&ms.yes_token_id) {
                Some(&p) if p > 0.01 && p < 0.99 => p,
                _ => continue,
            };

            ref_prices.insert(ms.sybil_market_id, (mid * NANOS_PER_DOLLAR as f64) as u64);

            if stale {
                continue;
            }

            ms.push_price(mid);

            quote_inputs.push(QuoteInput {
                market_id: ms.sybil_market_id,
                mid,
                sigma_sq: ms.variance(),
                net_inventory: ms.net_inventory(),
                yes_position: ms.yes_position,
                no_position: ms.no_position,
                in_group: ms.in_group,
            });
        }

        // 4. Generate quotes (pure pass): no mutation, no IO
        let quote_config = QuoteConfig {
            gamma: self.config.mm_gamma,
            base_spread: self.config.mm_half_spread,
            min_spread: self.config.mm_min_spread,
            max_position: self.config.mm_max_position as i64,
            quote_size_dollars: self.config.mm_quote_size_dollars,
        };
        let orders: Vec<OrderSpec> = quote_inputs
            .iter()
            .flat_map(|input| generate_quotes(input, &quote_config))
            .collect();

        // 5. Submit (IO)
        self.submit_orders(&orders, budget_nanos, block.height).await;

        // 6. Push reference prices (IO)
        if !ref_prices.is_empty() {
            let _ = self.sybil_client.set_reference_prices(&ref_prices).await;
        }
    }

    async fn maybe_sync_positions(&mut self, block_height: u64) {
        if block_height.saturating_sub(self.state.last_sync_block) >= self.config.mm_sync_interval_blocks
            || self.state.last_sync_block == 0
        {
            self.sync_positions().await;
            self.state.last_sync_block = block_height;
        }
    }

    async fn submit_orders(&self, orders: &[OrderSpec], budget_nanos: u64, block_height: u64) {
        if orders.is_empty() {
            return;
        }
        let req = SubmitOrderRequest {
            account_id: self.account_id,
            orders: orders.to_vec(),
            mm_budget_nanos: Some(budget_nanos),
        };
        match self.sybil_client.submit_orders(&req).await {
            Ok(accepted) => {
                debug!(
                    block = block_height,
                    order_count = orders.len(),
                    accepted,
                    budget_dollars = budget_nanos as f64 / NANOS_PER_DOLLAR as f64,
                    "submitted MM orders"
                );
            }
            Err(e) => {
                warn!(block = block_height, error = %e, "order submission failed");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> QuoteConfig {
        QuoteConfig {
            gamma: 0.05,
            base_spread: 0.02,
            min_spread: 0.005,
            max_position: 5000,
            quote_size_dollars: 100.0,
        }
    }

    fn default_input(mid: f64) -> QuoteInput {
        QuoteInput {
            market_id: 1,
            mid,
            sigma_sq: 0.0005,
            net_inventory: 0.0,
            yes_position: 0,
            no_position: 0,
            in_group: false,
        }
    }

    #[test]
    fn symmetric_quotes_at_midpoint() {
        let orders = generate_quotes(&default_input(0.5), &default_config());
        // Should have BuyYes + BuyNo (no sells since no position)
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellYes { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn inventory_skews_reservation_price() {
        let config = default_config();
        // Long YES → reservation price below mid → tighter YES bid, wider YES ask
        let mut long_yes = default_input(0.5);
        long_yes.net_inventory = 1000.0;
        long_yes.yes_position = 1000;
        let orders = generate_quotes(&long_yes, &config);

        let yes_bid = orders.iter().find_map(|o| match o {
            OrderSpec::BuyYes { limit_price_nanos, .. } => Some(*limit_price_nanos),
            _ => None,
        });
        // With long inventory, reservation price < mid, so bid should be below 0.48
        assert!(yes_bid.is_some());
        assert!(yes_bid.unwrap() < 480_000_000);
    }

    #[test]
    fn at_position_limit_no_buy() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = 5000; // at max_position
        let orders = generate_quotes(&input, &config);
        // At YES limit → no BuyYes
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
    }

    #[test]
    fn group_market_suppresses_no_side() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.in_group = true;
        let orders = generate_quotes(&input, &config);
        // In group → no BuyNo/SellNo
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn sell_only_when_holding_position() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = 100;
        input.no_position = 50;
        let orders = generate_quotes(&input, &config);
        // Should have SellYes (holding YES) and SellNo (holding NO, standalone)
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn sell_quantity_capped_to_position() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = 3; // very small position
        let orders = generate_quotes(&input, &config);
        let sell_qty = orders.iter().find_map(|o| match o {
            OrderSpec::SellYes { quantity, .. } => Some(*quantity),
            _ => None,
        });
        assert!(sell_qty.is_some());
        assert!(sell_qty.unwrap() <= 3);
    }

    #[test]
    fn edge_price_suppresses_yes_bid() {
        let config = default_config();
        // Price near 0 → reservation clamps to 0.02, YES bid likely below threshold
        let orders = generate_quotes(&default_input(0.005), &config);
        // YES bid should be suppressed (too low), but NO side may still generate
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
    }

    #[test]
    fn high_variance_widens_spread() {
        let config = default_config();
        let mut low_vol = default_input(0.5);
        low_vol.sigma_sq = 0.0001;
        let mut high_vol = default_input(0.5);
        high_vol.sigma_sq = 0.01;

        let low_orders = generate_quotes(&low_vol, &config);
        let high_orders = generate_quotes(&high_vol, &config);

        let low_bid = low_orders.iter().find_map(|o| match o {
            OrderSpec::BuyYes { limit_price_nanos, .. } => Some(*limit_price_nanos),
            _ => None,
        }).unwrap();
        let high_bid = high_orders.iter().find_map(|o| match o {
            OrderSpec::BuyYes { limit_price_nanos, .. } => Some(*limit_price_nanos),
            _ => None,
        }).unwrap();

        // Higher volatility → wider spread → lower bid
        assert!(high_bid < low_bid);
    }
}
