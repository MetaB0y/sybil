use std::collections::{HashMap, VecDeque};

use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::feed::PriceSnapshot;
use sybil_api_types::*;
use sybil_client::SybilClient;

/// Default variance prior for markets with insufficient price history.
const DEFAULT_VARIANCE: f64 = 0.0005;
const SHARE_SCALE: f64 = 1_000.0;
const SHARE_SCALE_I64: i64 = 1_000;

/// Reference price pushed for a market whose token has gone stale (PM-6). A 0
/// midpoint is not a legal in-band price (the MM only quotes `0.01 < p < 0.99`),
/// so downstream `reference_price_nanos > 0` guards read it as "no reference"
/// and stop trading rather than trading on a frozen value.
const REFERENCE_PRICE_EVICTION_SENTINEL: u64 = 0;

fn shares_to_qty_units(shares: f64) -> u64 {
    if !shares.is_finite() || shares <= 0.0 {
        return 0;
    }
    (shares * SHARE_SCALE).floor().max(1.0) as u64
}

fn whole_shares_to_qty_units(shares: i64) -> i64 {
    shares.saturating_mul(SHARE_SCALE_I64)
}

fn qty_units_to_shares(qty_units: i64) -> f64 {
    qty_units as f64 / SHARE_SCALE
}

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
        /// Stable Polymarket event/group key for NegRisk groups.
        group_key: Option<String>,
        /// Number of markets in the NegRisk group. 0 for standalone markets.
        group_size: usize,
    },
}

// --------------------------------------------------------------------------- //
// Per-market state
// --------------------------------------------------------------------------- //

struct MarketState {
    sybil_market_id: u32,
    yes_token_id: String,
    group_key: Option<String>,
    group_size: usize,
    // Inventory (updated via periodic API sync)
    yes_position: i64,
    no_position: i64,
    // Price history for variance estimation
    price_history: VecDeque<f64>,
    vol_window: usize,
}

impl MarketState {
    fn new(
        sybil_market_id: u32,
        yes_token_id: String,
        group_key: Option<String>,
        group_size: usize,
        initial_mid: f64,
        vol_window: usize,
    ) -> Self {
        let mut price_history = VecDeque::with_capacity(vol_window + 1);
        price_history.push_back(initial_mid);
        Self {
            sybil_market_id,
            yes_token_id,
            group_key,
            group_size,
            yes_position: 0,
            no_position: 0,
            price_history,
            vol_window,
        }
    }

    /// Net inventory in full-share units: positive = long YES, negative = long NO.
    fn net_inventory(&self) -> f64 {
        qty_units_to_shares(self.yes_position - self.no_position)
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
        let var = self
            .price_history
            .iter()
            .map(|p| (p - mean).powi(2))
            .sum::<f64>()
            / (n - 1) as f64;
        var.max(DEFAULT_VARIANCE)
    }

    /// Dollar exposure for this market given a reference mid price.
    fn exposure(&self, mid: f64) -> f64 {
        let yes_val = qty_units_to_shares(self.yes_position) * mid;
        let no_val = qty_units_to_shares(self.no_position) * (1.0 - mid);
        yes_val.abs() + no_val.abs()
    }
}

// --------------------------------------------------------------------------- //
// Aggregate MM state
// --------------------------------------------------------------------------- //

struct MmState {
    markets: HashMap<u32, MarketState>,
    last_sync_block: u64,
    next_quote_index: usize,
}

impl MmState {
    fn new() -> Self {
        Self {
            markets: HashMap::new(),
            last_sync_block: 0,
            next_quote_index: 0,
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
    pub group_key: Option<String>,
    pub group_size: usize,
}

/// Configuration for quote generation.
#[derive(Clone, Debug)]
pub struct QuoteConfig {
    pub gamma: f64,
    pub base_spread: f64,
    pub min_spread: f64,
    /// Position cap in full shares, not protocol share-units.
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
    let half_spread =
        vol_spread.clamp(config.min_spread, (edge_room - 0.01).max(config.min_spread));

    // Position limits
    let max_position_units = whole_shares_to_qty_units(config.max_position);
    let at_yes_limit = input.yes_position >= max_position_units;
    let at_no_limit = input.no_position >= max_position_units;

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
            quantity: shares_to_qty_units(buy_size / yes_bid),
        });
    }

    if input.yes_position > 0 && yes_ask > 0.01 && yes_ask < 0.99 {
        let max_sell = input.yes_position as u64;
        let desired = shares_to_qty_units(sell_size / yes_ask);
        orders.push(OrderSpec::SellYes {
            market_id: input.market_id,
            limit_price_nanos: (yes_ask * NANOS_PER_DOLLAR as f64) as u64,
            quantity: desired.min(max_sell),
        });
    }

    // ── NO side ──
    //
    // Buying NO at price (1 - ask_yes) is the collateralized way to provide
    // the YES ask without requiring existing YES inventory. This matters most
    // for Polymarket NegRisk groups: disabling the NO side left the live MM as
    // a one-sided YES bidder on the mirrored multi-outcome markets.
    let no_bid = (1.0 - r) - half_spread;
    let no_ask = (1.0 - r) + half_spread;

    if !at_no_limit && no_bid > 0.01 && no_bid < 0.99 {
        orders.push(OrderSpec::BuyNo {
            market_id: input.market_id,
            limit_price_nanos: (no_bid * NANOS_PER_DOLLAR as f64) as u64,
            quantity: shares_to_qty_units(buy_size / no_bid),
        });
    }

    if input.no_position > 0 && no_ask > 0.01 && no_ask < 0.99 {
        let max_sell = input.no_position as u64;
        let desired = shares_to_qty_units(sell_size / no_ask);
        orders.push(OrderSpec::SellNo {
            market_id: input.market_id,
            limit_price_nanos: (no_ask * NANOS_PER_DOLLAR as f64) as u64,
            quantity: desired.min(max_sell),
        });
    }

    orders
}

/// Select a bounded, rotating slice of quotes for one block.
///
/// The live API intentionally caps orders per submission. When the mirror
/// tracks hundreds of markets, submitting every quote every block is both too
/// expensive and rejected by that guardrail. This preserves coverage by
/// rotating the starting market each block.
pub fn select_rotating_quotes(
    quote_inputs: &[QuoteInput],
    quote_config: &QuoteConfig,
    start_index: usize,
    max_orders: usize,
) -> (Vec<OrderSpec>, usize) {
    if quote_inputs.is_empty() || max_orders == 0 {
        return (Vec::new(), start_index);
    }

    let start = start_index % quote_inputs.len();
    let mut orders = Vec::new();
    let mut group_coverage = HashMap::<String, GroupQuoteCoverage>::new();
    let mut considered = 0;

    for offset in 0..quote_inputs.len() {
        let idx = (start + offset) % quote_inputs.len();
        let input = &quote_inputs[idx];
        let mut market_orders = generate_quotes(input, quote_config);
        if input.group_key.is_some() {
            market_orders.sort_by_key(|order| match order {
                OrderSpec::BuyNo { .. } => 0,
                OrderSpec::BuyYes { .. } => 1,
                _ => 2,
            });
        }
        considered = offset + 1;

        if market_orders.is_empty() {
            continue;
        }

        for order in market_orders {
            if orders.len() >= max_orders {
                break;
            }
            if would_complete_group_coverage(input, &order, &group_coverage) {
                continue;
            }
            record_group_coverage(input, &order, &mut group_coverage);
            orders.push(order);
        }

        if orders.len() >= max_orders {
            break;
        }
    }

    let next_index = (start + considered.max(1)) % quote_inputs.len();
    (orders, next_index)
}

#[derive(Default)]
struct GroupQuoteCoverage {
    buy_yes_markets: std::collections::HashSet<u32>,
    buy_no_markets: std::collections::HashSet<u32>,
}

fn would_complete_group_coverage(
    input: &QuoteInput,
    order: &OrderSpec,
    coverage: &HashMap<String, GroupQuoteCoverage>,
) -> bool {
    let Some(group_key) = &input.group_key else {
        return false;
    };
    let group_size = input.group_size;
    if group_size < 2 {
        return false;
    }
    let Some(existing) = coverage.get(group_key) else {
        return false;
    };
    match order {
        OrderSpec::BuyYes { market_id, .. } => {
            existing.buy_no_markets.contains(market_id)
                || existing.buy_yes_markets.len()
                    + usize::from(!existing.buy_yes_markets.contains(market_id))
                    >= group_size
        }
        OrderSpec::BuyNo { market_id, .. } => {
            existing.buy_yes_markets.contains(market_id)
                || existing
                    .buy_no_markets
                    .iter()
                    .any(|existing_market_id| existing_market_id != market_id)
        }
        _ => false,
    }
}

fn record_group_coverage(
    input: &QuoteInput,
    order: &OrderSpec,
    coverage: &mut HashMap<String, GroupQuoteCoverage>,
) {
    let Some(group_key) = &input.group_key else {
        return;
    };
    let entry = coverage
        .entry(group_key.clone())
        .or_insert_with(|| GroupQuoteCoverage {
            ..GroupQuoteCoverage::default()
        });
    match order {
        OrderSpec::BuyYes { market_id, .. } => {
            entry.buy_yes_markets.insert(*market_id);
        }
        OrderSpec::BuyNo { market_id, .. } => {
            entry.buy_no_markets.insert(*market_id);
        }
        _ => {}
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
    /// Publishes the count of markets the MM is actively quoting so SyncActor
    /// can recycle `mm_max_markets` slots as markets are untracked (PM-8).
    live_tx: watch::Sender<usize>,
    state: MmState,
}

impl MmActor {
    pub fn new(
        config: Config,
        sybil_client: SybilClient,
        account_id: u64,
        price_rx: watch::Receiver<PriceSnapshot>,
        mm_rx: mpsc::Receiver<MmMessage>,
        live_tx: watch::Sender<usize>,
    ) -> Self {
        Self {
            config,
            sybil_client,
            account_id,
            price_rx,
            mm_rx,
            live_tx,
            state: MmState::new(),
        }
    }

    /// Publish the current live-market count to SyncActor's watch channel.
    fn publish_live_count(&self) {
        let _ = self.live_tx.send(self.state.markets.len());
    }

    /// Stop quoting a market and free its live-set slot. Returns `true` if the
    /// market was tracked. Used by resolution untracking (PM-1 root fix) and by
    /// the batch-rejection defence below.
    fn untrack_market(&mut self, market_id: u32, reason: &str) -> bool {
        if self.state.markets.remove(&market_id).is_some() {
            info!(market_id, reason, "MM untracking market");
            self.publish_live_count();
            true
        } else {
            false
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
                group_key,
                group_size,
            } => {
                info!(
                    sybil_market_id,
                    yes_token_id, initial_mid, group_key, group_size, "MM tracking new market"
                );
                self.state.markets.insert(
                    sybil_market_id,
                    MarketState::new(
                        sybil_market_id,
                        yes_token_id,
                        group_key,
                        group_size,
                        initial_mid,
                        self.config.mm_vol_window,
                    ),
                );
                self.publish_live_count();
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

        let total_exposure: f64 = self
            .state
            .markets
            .values()
            .map(|ms| {
                let mid = snapshot
                    .midpoints
                    .get(&ms.yes_token_id)
                    .copied()
                    .unwrap_or(0.5);
                ms.exposure(mid)
            })
            .sum();

        let ratio = (total_exposure / max_exposure).min(1.0);
        let scale = (1.0 - ratio).powi(2); // Quadratic decay
        let budget = self.config.mm_budget_dollars * scale;

        (budget * NANOS_PER_DOLLAR as f64) as u64
    }

    /// Untrack every market the block reports resolved. Pure state mutation
    /// (no IO) so it is unit-testable in isolation.
    fn untrack_resolved(&mut self, block: &BlockResponse) {
        for event in &block.system_events {
            if let SystemEventResponse::MarketResolved { market_id, .. } = event {
                self.untrack_market(*market_id, "market_resolved");
            }
        }
    }

    // ----- Per-block quote generation ------------------------------------- //

    async fn on_block(&mut self, block: &BlockResponse) {
        let snapshot = self.price_rx.borrow().clone();
        let now = now_ms();

        // 0. Lifecycle: untrack markets the chain resolved this block (PM-1 root
        //    fix). The mirror already receives `MarketResolved` on the block
        //    stream it consumes; acting on it here stops a resolved market from
        //    poisoning the whole IOC batch and frees its live-set slot (PM-8).
        self.untrack_resolved(block);

        // 1. Periodic position sync
        self.maybe_sync_positions(block.height).await;

        // 2. Dynamic budget
        let budget_nanos = self.compute_budget(&snapshot);
        if budget_nanos == 0 {
            debug!("budget exhausted (exposure at max), skipping block");
            return;
        }

        // 3. Update state (mutation pass): push prices, collect reference prices.
        //    Staleness is now evaluated per token (PM-4): a single frozen token
        //    stops being quoted even while its neighbours keep updating.
        let staleness_ms = self.config.mm_staleness_ms;
        let mut ref_prices = HashMap::new();
        let mut quote_inputs = Vec::new();

        for ms in self.state.markets.values_mut() {
            let Some(&mid) = snapshot.midpoints.get(&ms.yes_token_id) else {
                // Never seen a price for this token; nothing to publish or quote.
                continue;
            };

            if snapshot.token_is_stale(&ms.yes_token_id, now, staleness_ms) {
                // PM-6: a frozen token's reference price is evicted so downstream
                // `--require-reference-prices` consumers stop trading on it
                // rather than being picked off on the stale value.
                ref_prices.insert(ms.sybil_market_id, REFERENCE_PRICE_EVICTION_SENTINEL);
                continue;
            }

            // Publish the *current* reference price even when it has drifted out
            // of the tradeable band (PM-6): the reference must track reality
            // instead of freezing at the last in-band value. Quoting is still
            // suppressed outside the band below.
            ref_prices.insert(
                ms.sybil_market_id,
                (mid.clamp(0.0, 1.0) * NANOS_PER_DOLLAR as f64) as u64,
            );

            if !(mid > 0.01 && mid < 0.99) {
                // Out of band: near-resolved, don't quote.
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
                group_key: ms.group_key.clone(),
                group_size: ms.group_size,
            });
        }
        quote_inputs.sort_by_key(|input| input.market_id);

        // 4. Generate quotes (pure pass): no mutation, no IO
        let quote_config = QuoteConfig {
            gamma: self.config.mm_gamma,
            base_spread: self.config.mm_half_spread,
            min_spread: self.config.mm_min_spread,
            max_position: self.config.mm_max_position as i64,
            quote_size_dollars: self.config.mm_quote_size_dollars,
        };
        let start_index = self.state.next_quote_index;
        let (orders, next_quote_index) = select_rotating_quotes(
            &quote_inputs,
            &quote_config,
            start_index,
            self.config.mm_max_orders_per_block,
        );
        self.state.next_quote_index = next_quote_index;

        // 5. Submit (IO). A whole-batch rejection that names a non-tradeable
        //    market lets us drop the poison defensively (PM-1 defence in depth)
        //    even if we never saw its `MarketResolved` (e.g. missed block, or a
        //    market that became untradeable for another reason).
        if let Some(poisoned) = self
            .submit_orders(&orders, budget_nanos, block.height)
            .await
        {
            self.untrack_market(poisoned, "batch_rejected_untradeable");
        }

        // 6. Push reference prices (IO)
        if !ref_prices.is_empty() {
            let _ = self.sybil_client.set_reference_prices(&ref_prices).await;
        }
    }

    async fn maybe_sync_positions(&mut self, block_height: u64) {
        if block_height.saturating_sub(self.state.last_sync_block)
            >= self.config.mm_sync_interval_blocks
            || self.state.last_sync_block == 0
        {
            self.sync_positions().await;
            self.state.last_sync_block = block_height;
        }
    }

    /// Submit the IOC batch. Returns `Some(market_id)` when the whole batch was
    /// rejected because that market is non-tradeable, so the caller can untrack
    /// it (defence in depth for PM-1).
    async fn submit_orders(
        &self,
        orders: &[OrderSpec],
        budget_nanos: u64,
        block_height: u64,
    ) -> Option<u32> {
        if orders.is_empty() {
            return None;
        }
        let req = SubmitOrderRequest {
            account_id: self.account_id,
            orders: orders.to_vec(),
            time_in_force: TimeInForce::Ioc,
            expires_at_block: None,
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
                None
            }
            Err(e) => {
                let e = Error::from(e);
                let poisoned = poisoned_market_from_error(&e);
                warn!(block = block_height, error = %e, poisoned, "order submission failed");
                poisoned
            }
        }
    }
}

/// Extract the non-tradeable market id from a whole-batch rejection.
///
/// sybil-api validates every order against the live market set and fails the
/// entire submission with `{"error":"Market <id> not found", ...}` (HTTP 400)
/// as soon as one order targets a market that is gone/untradeable. Parsing that
/// id out is the cleanest mechanism the current API surfaces — no probing or
/// per-market bisection needed — so we drop exactly that market and let the
/// next block re-form the batch without it.
fn poisoned_market_from_error(err: &Error) -> Option<u32> {
    let Error::SybilApi { status: 400, body } = err else {
        return None;
    };
    // The body is JSON (`{"error": "...", "code": "..."}`); fall back to the
    // raw text if it is not the shape we expect.
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.clone());

    if !message.contains("not found") {
        return None;
    }
    let rest = message.strip_prefix("Market ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
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
    use clap::Parser as _;

    fn sybil_api_error(body: &str) -> Error {
        Error::SybilApi {
            status: 400,
            body: body.to_string(),
        }
    }

    #[test]
    fn poisoned_market_parsed_from_json_rejection() {
        let err = sybil_api_error(r#"{"error":"Market 42 not found","code":"BAD_REQUEST"}"#);
        assert_eq!(poisoned_market_from_error(&err), Some(42));
    }

    #[test]
    fn poisoned_market_parsed_from_raw_message() {
        // Defensive: also handles a non-JSON body carrying the same message.
        let err = sybil_api_error("Market 7 not found");
        assert_eq!(poisoned_market_from_error(&err), Some(7));
    }

    #[test]
    fn poisoned_market_ignores_unrelated_rejections() {
        assert_eq!(
            poisoned_market_from_error(&sybil_api_error(
                r#"{"error":"Invalid price","code":"BAD_REQUEST"}"#
            )),
            None
        );
        // Non-400 statuses are never treated as poison.
        assert_eq!(
            poisoned_market_from_error(&Error::SybilApi {
                status: 500,
                body: "Market 3 not found".to_string(),
            }),
            None
        );
    }

    fn test_actor(live_tx: watch::Sender<usize>) -> (MmActor, watch::Sender<PriceSnapshot>) {
        let (price_tx, price_rx) = watch::channel(PriceSnapshot::default());
        let (_mm_tx, mm_rx) = mpsc::channel(16);
        let client = SybilClient::new(reqwest::Client::new(), "http://localhost".into(), None);
        let actor = MmActor::new(
            Config::parse_from(["sybil-polymarket"]),
            client,
            1,
            price_rx,
            mm_rx,
            live_tx,
        );
        (actor, price_tx)
    }

    fn track(actor: &mut MmActor, market_id: u32) {
        actor.handle_message(MmMessage::MarketMirrored {
            sybil_market_id: market_id,
            yes_token_id: format!("token-{market_id}"),
            initial_mid: 0.5,
            group_key: None,
            group_size: 0,
        });
    }

    fn block_resolving(market_ids: &[u32]) -> BlockResponse {
        BlockResponse {
            height: 1,
            parent_hash: String::new(),
            state_root: String::new(),
            events_root: String::new(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
            system_events: market_ids
                .iter()
                .map(|&market_id| SystemEventResponse::MarketResolved {
                    market_id,
                    payout_nanos: 0,
                    affected_accounts: Vec::new(),
                })
                .collect(),
            fills: Vec::new(),
            clearing_prices_nanos: Default::default(),
            rejections: Vec::new(),
            bridge: Default::default(),
            total_welfare_nanos: 0,
            total_volume_nanos: 0,
            orders_filled: 0,
            unique_placers: 0,
            by_market: Default::default(),
        }
    }

    #[test]
    fn resolved_market_is_untracked_and_frees_live_slot() {
        let (live_tx, live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);

        track(&mut actor, 10);
        track(&mut actor, 11);
        assert_eq!(*live_rx.borrow(), 2);
        assert!(actor.state.markets.contains_key(&10));

        actor.untrack_resolved(&block_resolving(&[10]));

        assert!(!actor.state.markets.contains_key(&10));
        assert!(actor.state.markets.contains_key(&11));
        // PM-8: the freed slot is published back to Sync.
        assert_eq!(*live_rx.borrow(), 1);
    }

    #[test]
    fn untrack_market_defensive_drop_publishes_live_count() {
        let (live_tx, live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track(&mut actor, 5);
        assert_eq!(*live_rx.borrow(), 1);

        assert!(actor.untrack_market(5, "batch_rejected_untradeable"));
        assert_eq!(*live_rx.borrow(), 0);
        // Dropping an already-gone market is a no-op.
        assert!(!actor.untrack_market(5, "batch_rejected_untradeable"));
    }

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
            group_key: None,
            group_size: 0,
        }
    }

    fn grouped_input(mid: f64) -> QuoteInput {
        QuoteInput {
            group_key: Some("group".to_string()),
            group_size: 3,
            ..default_input(mid)
        }
    }

    fn q(shares: i64) -> i64 {
        whole_shares_to_qty_units(shares)
    }

    #[test]
    fn symmetric_quotes_at_midpoint() {
        let orders = generate_quotes(&default_input(0.5), &default_config());
        // Should have BuyYes + BuyNo (no sells since no position)
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(!orders
            .iter()
            .any(|o| matches!(o, OrderSpec::SellYes { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn grouped_markets_quote_yes_and_no_from_cash() {
        let orders = generate_quotes(&grouped_input(0.7), &default_config());

        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(!orders
            .iter()
            .any(|o| matches!(o, OrderSpec::SellYes { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn inventory_skews_reservation_price() {
        let config = default_config();
        // Long YES → reservation price below mid → tighter YES bid, wider YES ask
        let mut long_yes = default_input(0.5);
        long_yes.net_inventory = 1000.0;
        long_yes.yes_position = q(1000);
        let orders = generate_quotes(&long_yes, &config);

        let yes_bid = orders.iter().find_map(|o| match o {
            OrderSpec::BuyYes {
                limit_price_nanos, ..
            } => Some(*limit_price_nanos),
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
        input.yes_position = q(5000); // at max_position
        let orders = generate_quotes(&input, &config);
        // At YES limit → no BuyYes
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
    }

    #[test]
    fn group_market_quotes_no_side() {
        let config = default_config();
        let input = grouped_input(0.5);
        let orders = generate_quotes(&input, &config);
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn sell_only_when_holding_position() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = q(100);
        input.no_position = q(50);
        let orders = generate_quotes(&input, &config);
        // Should have SellYes (holding YES) and SellNo (holding NO, standalone)
        assert!(orders
            .iter()
            .any(|o| matches!(o, OrderSpec::SellYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn sell_quantity_capped_to_position() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = q(3); // very small position
        let orders = generate_quotes(&input, &config);
        let sell_qty = orders.iter().find_map(|o| match o {
            OrderSpec::SellYes { quantity, .. } => Some(*quantity),
            _ => None,
        });
        assert!(sell_qty.is_some());
        assert!(sell_qty.unwrap() <= q(3) as u64);
    }

    #[test]
    fn rotating_selection_caps_orders_and_advances_cursor() {
        let config = default_config();
        let inputs: Vec<_> = (1..=10)
            .map(|market_id| {
                let mut input = default_input(0.5);
                input.market_id = market_id;
                input
            })
            .collect();

        let (orders, next_index) = select_rotating_quotes(&inputs, &config, 0, 6);

        assert_eq!(orders.len(), 6);
        assert_eq!(next_index, 3);
        assert!(orders.iter().any(|order| match order {
            OrderSpec::BuyYes { market_id, .. } => *market_id == 1,
            _ => false,
        }));
        assert!(orders.iter().any(|order| match order {
            OrderSpec::BuyYes { market_id, .. } => *market_id == 3,
            _ => false,
        }));
    }

    #[test]
    fn rotating_selection_resumes_from_cursor() {
        let config = default_config();
        let inputs: Vec<_> = (1..=10)
            .map(|market_id| {
                let mut input = default_input(0.5);
                input.market_id = market_id;
                input
            })
            .collect();

        let (orders, next_index) = select_rotating_quotes(&inputs, &config, 3, 4);

        assert_eq!(orders.len(), 4);
        assert_eq!(next_index, 5);
        assert!(orders.iter().any(|order| match order {
            OrderSpec::BuyYes { market_id, .. } => *market_id == 4,
            _ => false,
        }));
        assert!(orders.iter().any(|order| match order {
            OrderSpec::BuyYes { market_id, .. } => *market_id == 5,
            _ => false,
        }));
    }

    #[test]
    fn grouped_selection_filters_self_completing_quotes() {
        let config = default_config();
        let inputs: Vec<_> = (1..=3)
            .map(|market_id| {
                let mut input = grouped_input(0.5);
                input.market_id = market_id;
                input
            })
            .collect();

        let (orders, next_index) = select_rotating_quotes(&inputs, &config, 0, 12);

        assert_eq!(next_index, 0);
        assert_eq!(
            orders
                .iter()
                .filter(|order| matches!(order, OrderSpec::BuyNo { .. }))
                .count(),
            1
        );
        let buy_no_market = orders.iter().find_map(|order| match order {
            OrderSpec::BuyNo { market_id, .. } => Some(*market_id),
            _ => None,
        });
        assert!(buy_no_market.is_some());
        assert!(!orders.iter().any(|order| match order {
            OrderSpec::BuyYes { market_id, .. } => Some(*market_id) == buy_no_market,
            _ => false,
        }));
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

        let low_bid = low_orders
            .iter()
            .find_map(|o| match o {
                OrderSpec::BuyYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let high_bid = high_orders
            .iter()
            .find_map(|o| match o {
                OrderSpec::BuyYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();

        // Higher volatility → wider spread → lower bid
        assert!(high_bid < low_bid);
    }
}
