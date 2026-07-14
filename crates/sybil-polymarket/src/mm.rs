use std::collections::{HashMap, VecDeque};

use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::feed::PriceSnapshot;
use sybil_api_types::*;
use sybil_client::{PublicBlockStreamEvent, SybilClient};

mod quotes;

pub use quotes::{QuoteConfig, QuoteInput, generate_quotes, select_rotating_quotes};

/// Default variance prior for markets with insufficient price history.
const DEFAULT_VARIANCE: f64 = 0.0005;
const SHARE_SCALE: f64 = 1_000.0;
const SHARE_SCALE_I64: i64 = 1_000;

/// Reference price pushed for a market whose token has gone stale (PM-6). A 0
/// midpoint is not a legal in-band price (the MM only quotes `0.01 < p < 0.99`),
/// so downstream `reference_price_nanos > 0` guards read it as "no reference"
/// and stop trading rather than trading on a frozen value.
const REFERENCE_PRICE_EVICTION_SENTINEL: u64 = 0;

/// YES-price quote band for native markets. The MM seeds its midpoint from the
/// template and keeps generated YES orders inside this range; NO orders use the
/// complementary range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuoteRange {
    pub min: f64,
    pub max: f64,
    pub initial: f64,
}

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
#[derive(Clone, Debug)]
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
    /// A native Sybil market was created from the checked-in template catalog.
    MarketNative {
        sybil_market_id: u32,
        /// Stable native catalog child-market key.
        native_market_key: String,
        /// Template YES-price range used as the reference-free quoting source.
        quote_range: QuoteRange,
        /// Stable native group key for categorical markets.
        group_key: Option<String>,
        /// Number of markets in the categorical group. 0 for standalone binary.
        group_size: usize,
    },
}

// --------------------------------------------------------------------------- //
// Per-market state
// --------------------------------------------------------------------------- //

struct MarketState {
    sybil_market_id: u32,
    price_source: PriceSource,
    group_key: Option<String>,
    group_size: usize,
    // Inventory (updated via periodic API sync)
    yes_position: i64,
    no_position: i64,
    // Price history for variance estimation
    price_history: VecDeque<f64>,
    vol_window: usize,
}

enum PriceSource {
    Mirror {
        yes_token_id: String,
    },
    Native {
        quote_range: QuoteRange,
        /// Latest valid Sybil YES clearing price, seeded from the catalog.
        current_mid: f64,
    },
}

impl MarketState {
    fn new_mirror(
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
            price_source: PriceSource::Mirror { yes_token_id },
            group_key,
            group_size,
            yes_position: 0,
            no_position: 0,
            price_history,
            vol_window,
        }
    }

    fn new_native(
        sybil_market_id: u32,
        group_key: Option<String>,
        group_size: usize,
        quote_range: QuoteRange,
        vol_window: usize,
    ) -> Self {
        let mut price_history = VecDeque::with_capacity(vol_window + 1);
        price_history.push_back(quote_range.initial);
        Self {
            sybil_market_id,
            price_source: PriceSource::Native {
                quote_range,
                current_mid: quote_range.initial,
            },
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

    fn budget_mid(&self, snapshot: &PriceSnapshot) -> f64 {
        match &self.price_source {
            PriceSource::Mirror { yes_token_id } => {
                snapshot.midpoints.get(yes_token_id).copied().unwrap_or(0.5)
            }
            PriceSource::Native { current_mid, .. } => *current_mid,
        }
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
        let mut next_from_block: Option<u64> = None;

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

            // Connect to the first-party WebSocket block stream. On reconnect,
            // resume from the next height after the last block this actor saw.
            info!(
                markets = self.state.markets.len(),
                from_block = next_from_block,
                "connecting to block stream"
            );
            let block_stream = match self
                .sybil_client
                .stream_block_events_from_block(next_from_block)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "failed to connect block stream, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            tokio::pin!(block_stream);
            let mut replaying = next_from_block.is_some();

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
                    event = block_stream.next() => {
                        match event {
                            Some(Ok(PublicBlockStreamEvent::Block(block))) => {
                                next_from_block = Some(block.height.saturating_add(1));
                                if replaying {
                                    // Replayed blocks repair local lifecycle and native price
                                    // state only. Historical blocks are never fresh quote ticks.
                                    self.observe_block(&block);
                                } else {
                                    self.on_block(&block).await;
                                }
                            }
                            Some(Ok(PublicBlockStreamEvent::ReplayComplete { up_to_height })) => {
                                debug!(up_to_height, "block replay complete; following live stream");
                                self.sync_positions().await;
                                self.state.last_sync_block = up_to_height;
                                replaying = false;
                            }
                            Some(Err(sybil_client::Error::RetentionGap {
                                requested_height,
                                retention_min_height,
                                head_height,
                            })) => {
                                warn!(
                                    requested_height,
                                    retention_min_height,
                                    head_height,
                                    "block stream resume point is below retention floor; resyncing positions and resuming at floor"
                                );
                                self.sync_positions().await;
                                next_from_block = Some(retention_min_height);
                                break; // Reconnect from retained floor
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
                    MarketState::new_mirror(
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
            MmMessage::MarketNative {
                sybil_market_id,
                native_market_key,
                quote_range,
                group_key,
                group_size,
            } => {
                info!(
                    sybil_market_id,
                    native_market_key,
                    initial_mid = quote_range.initial,
                    min = quote_range.min,
                    max = quote_range.max,
                    group_key,
                    group_size,
                    "MM tracking native market"
                );
                self.state.markets.insert(
                    sybil_market_id,
                    MarketState::new_native(
                        sybil_market_id,
                        group_key,
                        group_size,
                        quote_range,
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
            .map(|ms| ms.exposure(ms.budget_mid(snapshot)))
            .sum();

        let ratio = (total_exposure / max_exposure).min(1.0);
        let scale = (1.0 - ratio).powi(2); // Quadratic decay
        let budget = self.config.mm_budget_dollars * scale;

        (budget * NANOS_PER_DOLLAR as f64) as u64
    }

    /// Untrack every market the block reports resolved. Pure state mutation
    /// (no IO) so it is unit-testable in isolation.
    fn untrack_resolved(&mut self, block: &PublicBlockResponse) {
        for market_id in &block.resolved_market_ids {
            self.untrack_market(*market_id, "market_resolved");
        }
    }

    /// Apply block state that matters during both replay and live following.
    /// Side effects that create new work (quoting and API writes) deliberately
    /// remain in `on_block` and therefore run only for live blocks.
    fn observe_block(&mut self, block: &PublicBlockResponse) {
        self.untrack_resolved(block);
        self.update_native_midpoints(block);
    }

    /// Let native markets discover prices on Sybil after their catalog seed.
    /// A clearing vector is `[YES, NO]`; invalid or terminal values are ignored.
    fn update_native_midpoints(&mut self, block: &PublicBlockResponse) {
        for market in self.state.markets.values_mut() {
            let PriceSource::Native { current_mid, .. } = &mut market.price_source else {
                continue;
            };
            let Some(yes_nanos) = block
                .clearing_prices_nanos
                .get(&market.sybil_market_id.to_string())
                .and_then(|prices| prices.first())
            else {
                continue;
            };
            let mid = *yes_nanos as f64 / NANOS_PER_DOLLAR as f64;
            if mid > 0.0 && mid < 1.0 {
                *current_mid = mid;
            }
        }
    }

    // ----- Per-block quote generation ------------------------------------- //

    async fn on_block(&mut self, block: &PublicBlockResponse) {
        let snapshot = self.price_rx.borrow().clone();
        let now = now_ms();

        // 0. Observe lifecycle and native clearing prices. The same state-only
        //    observation also runs during replay, while the quote side effects
        //    below are live-only.
        self.observe_block(block);

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
            let (mid, quote_range) = match &ms.price_source {
                PriceSource::Mirror { yes_token_id } => {
                    let Some(&mid) = snapshot.midpoints.get(yes_token_id) else {
                        // Never seen a price for this token; nothing to publish or quote.
                        continue;
                    };

                    if snapshot.token_is_stale(yes_token_id, now, staleness_ms) {
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

                    (mid, None)
                }
                PriceSource::Native {
                    quote_range,
                    current_mid,
                } => (*current_mid, Some(*quote_range)),
            };

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
                quote_range,
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
            Ok(order_ids) => {
                debug!(
                    block = block_height,
                    order_count = orders.len(),
                    order_ids = ?order_ids,
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

    fn track_native(actor: &mut MmActor, market_id: u32, initial_mid: f64) {
        actor.handle_message(MmMessage::MarketNative {
            sybil_market_id: market_id,
            native_market_key: format!("native-{market_id}"),
            quote_range: QuoteRange {
                min: 0.05,
                max: 0.95,
                initial: initial_mid,
            },
            group_key: None,
            group_size: 0,
        });
    }

    fn block_resolving(market_ids: &[u32]) -> PublicBlockResponse {
        PublicBlockResponse {
            height: 1,
            parent_hash: String::new(),
            state_root: String::new(),
            events_root: String::new(),
            order_count: 0,
            fill_count: 0,
            rejection_count: 0,
            timestamp_ms: 0,
            clearing_prices_nanos: Default::default(),
            bridge: Default::default(),
            resolved_market_ids: market_ids.to_vec(),
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
    fn native_midpoint_follows_latest_valid_clearing_price() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut block = block_resolving(&[]);
        block
            .clearing_prices_nanos
            .insert("7".to_string(), vec![700_000_000, 300_000_000]);
        actor.observe_block(&block);

        let market = actor.state.markets.get(&7).expect("tracked native market");
        assert_eq!(market.budget_mid(&PriceSnapshot::default()), 0.7);
    }

    #[test]
    fn native_midpoint_ignores_terminal_clearing_price() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut block = block_resolving(&[]);
        block
            .clearing_prices_nanos
            .insert("7".to_string(), vec![NANOS_PER_DOLLAR, 0]);
        actor.observe_block(&block);

        let market = actor.state.markets.get(&7).expect("tracked native market");
        assert_eq!(market.budget_mid(&PriceSnapshot::default()), 0.4);
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
            quote_range: None,
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
        assert!(
            !orders
                .iter()
                .any(|o| matches!(o, OrderSpec::SellYes { .. }))
        );
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn grouped_markets_quote_yes_and_no_from_cash() {
        let orders = generate_quotes(&grouped_input(0.7), &default_config());

        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyYes { .. })));
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
        assert!(
            !orders
                .iter()
                .any(|o| matches!(o, OrderSpec::SellYes { .. }))
        );
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
    fn inventory_skews_quotes_against_inventory_on_both_buy_sides() {
        let config = default_config();
        let neutral_orders = generate_quotes(&default_input(0.5), &config);
        let mut long_yes = default_input(0.5);
        long_yes.net_inventory = 1000.0;
        long_yes.yes_position = q(1000);
        let long_yes_orders = generate_quotes(&long_yes, &config);

        let neutral_yes_bid = neutral_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::BuyYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let long_yes_bid = long_yes_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::BuyYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let neutral_no_bid = neutral_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::BuyNo {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let long_yes_no_bid = long_yes_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::BuyNo {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();

        assert!(long_yes_bid < neutral_yes_bid);
        assert!(long_yes_no_bid > neutral_no_bid);
    }

    #[test]
    fn budget_decays_to_zero_at_max_exposure() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        actor.config.mm_max_exposure_dollars = 100.0;
        actor.config.mm_budget_dollars = 1000.0;
        track(&mut actor, 1);
        actor.state.markets.get_mut(&1).unwrap().yes_position = q(200);

        let mut snapshot = PriceSnapshot::default();
        snapshot.midpoints.insert("token-1".to_string(), 0.5);

        assert_eq!(actor.compute_budget(&snapshot), 0);
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
    fn buy_quantity_is_capped_to_remaining_position_room() {
        let mut config = default_config();
        config.max_position = 100;
        config.quote_size_dollars = 100.0;
        let mut input = default_input(0.5);
        input.yes_position = q(99);
        input.no_position = q(98);

        let orders = generate_quotes(&input, &config);
        let yes_quantity = orders.iter().find_map(|order| match order {
            OrderSpec::BuyYes { quantity, .. } => Some(*quantity),
            _ => None,
        });
        let no_quantity = orders.iter().find_map(|order| match order {
            OrderSpec::BuyNo { quantity, .. } => Some(*quantity),
            _ => None,
        });

        assert_eq!(yes_quantity, Some(q(1) as u64));
        assert_eq!(no_quantity, Some(q(2) as u64));
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
        assert!(
            orders
                .iter()
                .any(|o| matches!(o, OrderSpec::SellYes { .. }))
        );
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

    #[test]
    fn native_quote_range_bounds_yes_and_no_prices() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.quote_range = Some(QuoteRange {
            min: 0.45,
            max: 0.55,
            initial: 0.50,
        });

        let orders = generate_quotes(&input, &config);
        assert!(!orders.is_empty());
        for order in orders {
            match order {
                OrderSpec::BuyYes {
                    limit_price_nanos, ..
                }
                | OrderSpec::SellYes {
                    limit_price_nanos, ..
                } => {
                    assert!((450_000_000..=550_000_000).contains(&limit_price_nanos));
                }
                OrderSpec::BuyNo {
                    limit_price_nanos, ..
                }
                | OrderSpec::SellNo {
                    limit_price_nanos, ..
                } => {
                    assert!((450_000_000..=550_000_000).contains(&limit_price_nanos));
                }
            }
        }
    }

    #[test]
    fn native_market_budget_uses_template_mid_without_snapshot() {
        let (live_tx, live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        actor.config.mm_max_exposure_dollars = 100.0;
        actor.config.mm_budget_dollars = 1000.0;
        actor.handle_message(MmMessage::MarketNative {
            sybil_market_id: 99,
            native_market_key: "native:event".to_string(),
            quote_range: QuoteRange {
                min: 0.30,
                max: 0.70,
                initial: 0.40,
            },
            group_key: None,
            group_size: 0,
        });
        actor.state.markets.get_mut(&99).unwrap().yes_position = q(50);

        assert_eq!(*live_rx.borrow(), 1);
        let budget = actor.compute_budget(&PriceSnapshot::default());
        assert!(budget > 0, "native budget should not require feed prices");
    }
}
