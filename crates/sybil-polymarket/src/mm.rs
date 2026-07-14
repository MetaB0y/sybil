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

use quotes::quoteable_bounds;
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
        /// Threshold-ladder coherence only; never used as protocol group
        /// membership or complete-set coverage.
        coherence_key: Option<String>,
        coherence_rank: usize,
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
    coherence_key: Option<String>,
    coherence_rank: usize,
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
        /// Robust Sybil actor mark, seeded from the catalog.
        current_mid: f64,
        qualifying_observations: VecDeque<(f64, u64, u64)>,
        last_qualifying_height: Option<u64>,
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
            coherence_key: None,
            coherence_rank: 0,
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
        coherence_key: Option<String>,
        coherence_rank: usize,
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
                qualifying_observations: VecDeque::new(),
                last_qualifying_height: None,
            },
            group_key,
            group_size,
            coherence_key,
            coherence_rank,
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
        let yes_excess = self.yes_position.saturating_sub(self.no_position).max(0);
        let no_excess = self.no_position.saturating_sub(self.yes_position).max(0);
        qty_units_to_shares(yes_excess) * mid + qty_units_to_shares(no_excess) * (1.0 - mid)
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

fn weighted_median(observations: &VecDeque<(f64, u64, u64)>, weight_cap_nanos: u64) -> Option<f64> {
    let mut rows = observations
        .iter()
        .map(|(price, notional, _)| (*price, (*notional).min(weight_cap_nanos).max(1)))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|left, right| left.0.total_cmp(&right.0));
    let total = rows.iter().fold(0u128, |sum, (_, weight)| {
        sum.saturating_add(u128::from(*weight))
    });
    let threshold = total.div_ceil(2);
    let mut cumulative = 0u128;
    for (price, weight) in rows {
        cumulative = cumulative.saturating_add(u128::from(weight));
        if cumulative >= threshold {
            return Some(price);
        }
    }
    None
}

/// Euclidean projection onto a bounded simplex. This is off-protocol actor
/// state; submitted prices are still landed as integer nanos.
fn project_bounded_simplex(values: &[f64], lower: &[f64], upper: &[f64]) -> Option<Vec<f64>> {
    if values.len() != lower.len() || values.len() != upper.len() || values.is_empty() {
        return None;
    }
    let lower_sum: f64 = lower.iter().sum();
    let upper_sum: f64 = upper.iter().sum();
    if lower_sum > 1.0 + 1e-9 || upper_sum < 1.0 - 1e-9 {
        return None;
    }
    let mut lambda_low = values
        .iter()
        .zip(upper)
        .map(|(value, max)| value - max)
        .fold(f64::INFINITY, f64::min);
    let mut lambda_high = values
        .iter()
        .zip(lower)
        .map(|(value, min)| value - min)
        .fold(f64::NEG_INFINITY, f64::max);
    for _ in 0..96 {
        let lambda = (lambda_low + lambda_high) / 2.0;
        let sum: f64 = values
            .iter()
            .zip(lower)
            .zip(upper)
            .map(|((value, min), max)| (value - lambda).clamp(*min, *max))
            .sum();
        if sum > 1.0 {
            lambda_low = lambda;
        } else {
            lambda_high = lambda;
        }
    }
    let lambda = (lambda_low + lambda_high) / 2.0;
    let mut projected = values
        .iter()
        .zip(lower)
        .zip(upper)
        .map(|((value, min), max)| (value - lambda).clamp(*min, *max))
        .collect::<Vec<_>>();
    let residual = 1.0 - projected.iter().sum::<f64>();
    if residual.abs() > 1e-10
        && let Some((index, _)) = projected.iter().enumerate().find(|(index, value)| {
            let candidate = **value + residual;
            candidate >= lower[*index] - 1e-12 && candidate <= upper[*index] + 1e-12
        })
    {
        projected[index] += residual;
    }
    Some(projected)
}

fn project_bounded_nonincreasing(values: &[f64], lower: &[f64], upper: &[f64]) -> Option<Vec<f64>> {
    if values.len() != lower.len() || values.len() != upper.len() || values.is_empty() {
        return None;
    }
    if lower.windows(2).any(|pair| pair[0] + 1e-12 < pair[1])
        || upper.windows(2).any(|pair| pair[0] + 1e-12 < pair[1])
    {
        return None;
    }
    #[derive(Clone, Copy)]
    struct Block {
        start: usize,
        end: usize,
        sum: f64,
        weight: usize,
    }
    let mut blocks = Vec::<Block>::new();
    for (index, value) in values.iter().enumerate() {
        blocks.push(Block {
            start: index,
            end: index,
            sum: *value,
            weight: 1,
        });
        while blocks.len() >= 2 {
            let right = blocks[blocks.len() - 1];
            let left = blocks[blocks.len() - 2];
            if left.sum / left.weight as f64 >= right.sum / right.weight as f64 {
                break;
            }
            blocks.pop();
            blocks.pop();
            blocks.push(Block {
                start: left.start,
                end: right.end,
                sum: left.sum + right.sum,
                weight: left.weight + right.weight,
            });
        }
    }
    let mut projected = vec![0.0; values.len()];
    for block in blocks {
        let mean = block.sum / block.weight as f64;
        for index in block.start..=block.end {
            projected[index] = mean.clamp(lower[index], upper[index]);
        }
    }
    // Coherent bounds make the clamp preserve order. Keep the explicit check
    // so catalog mistakes fail closed instead of leaking crossed ladders.
    projected
        .windows(2)
        .all(|pair| pair[0] + 1e-12 >= pair[1])
        .then_some(projected)
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
                coherence_key,
                coherence_rank,
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
                        coherence_key,
                        coherence_rank,
                        quote_range,
                        self.config.mm_vol_window,
                    ),
                );
                self.publish_live_count();
            }
        }
    }

    // ----- Position sync -------------------------------------------------- //

    async fn sync_positions(&mut self) -> bool {
        let account = match self.sybil_client.get_account(self.account_id).await {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "position sync failed");
                return false;
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
        true
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

    /// Let native markets discover prices from qualifying organic Sybil flow.
    /// MM/noise-only fills and zero-volume clearing vectors have exactly zero
    /// weight, preventing actor feedback from walking marks to a guardrail.
    fn update_native_midpoints(&mut self, block: &PublicBlockResponse) {
        let min_organic_notional_nanos = (self.config.mm_native_min_organic_notional_dollars
            * NANOS_PER_DOLLAR as f64)
            .round() as u64;
        let observation_weight_cap_nanos = (self.config.mm_native_observation_weight_cap_dollars
            * NANOS_PER_DOLLAR as f64)
            .round() as u64;
        let observation_window = self.config.mm_native_observation_window;
        let max_step = self.config.mm_native_max_step;
        let ewma_weight = self.config.mm_native_ewma_weight;
        let seed_reversion = self.config.mm_native_seed_reversion;
        let quote_config = QuoteConfig {
            gamma: self.config.mm_gamma,
            base_spread: self.config.mm_half_spread,
            min_spread: self.config.mm_min_spread,
            max_position: self.config.mm_max_position as i64,
            quote_size_dollars: self.config.mm_quote_size_dollars,
        };
        for market in self.state.markets.values_mut() {
            let updated_mid = {
                let PriceSource::Native {
                    quote_range,
                    current_mid,
                    qualifying_observations,
                    last_qualifying_height,
                } = &mut market.price_source
                else {
                    continue;
                };
                let stats = block.by_market.get(&market.sybil_market_id.to_string());
                let qualifies = stats.is_some_and(|stats| {
                    stats.organic_matched_orders > 0
                        && stats.organic_fill_notional_nanos >= min_organic_notional_nanos
                        && stats.volume_nanos > 0
                });
                let observed = qualifies
                    .then(|| {
                        block
                            .clearing_prices_nanos
                            .get(&market.sybil_market_id.to_string())
                            .and_then(|prices| prices.first())
                            .map(|yes_nanos| *yes_nanos as f64 / NANOS_PER_DOLLAR as f64)
                            .filter(|mid| mid.is_finite() && *mid > 0.0 && *mid < 1.0)
                    })
                    .flatten();
                if let Some(observed) = observed {
                    let notional = stats
                        .map(|stats| stats.organic_fill_notional_nanos)
                        .unwrap_or_default();
                    qualifying_observations.push_back((observed, notional, block.height));
                    while qualifying_observations.len() > observation_window {
                        qualifying_observations.pop_front();
                    }
                    *last_qualifying_height = Some(block.height);
                    let candidate =
                        weighted_median(qualifying_observations, observation_weight_cap_nanos)
                            .unwrap_or(observed);
                    let capped_delta = (candidate - *current_mid).clamp(-max_step, max_step);
                    *current_mid += ewma_weight * capped_delta;
                } else {
                    *current_mid += seed_reversion * (quote_range.initial - *current_mid);
                }
                if let Some((min, max)) = quoteable_bounds(*quote_range, &quote_config) {
                    *current_mid = (*current_mid).clamp(min, max);
                } else {
                    *current_mid = quote_range.initial.clamp(quote_range.min, quote_range.max);
                }
                *current_mid
            };
            market.push_price(updated_mid);
        }

        // Mutually-exclusive native groups must remain coherent even when
        // organic evidence touches only one child. Project the whole actor-mark
        // vector onto its bounded simplex after individual robust updates.
        let mut native_groups = HashMap::<String, Vec<u32>>::new();
        for market in self.state.markets.values() {
            if matches!(&market.price_source, PriceSource::Native { .. })
                && let Some(group_key) = &market.group_key
            {
                native_groups
                    .entry(group_key.clone())
                    .or_default()
                    .push(market.sybil_market_id);
            }
        }
        for market_ids in native_groups.values_mut() {
            market_ids.sort_unstable();
            let mut values = Vec::with_capacity(market_ids.len());
            let mut lower = Vec::with_capacity(market_ids.len());
            let mut upper = Vec::with_capacity(market_ids.len());
            for market_id in market_ids.iter() {
                let Some(market) = self.state.markets.get(market_id) else {
                    continue;
                };
                let PriceSource::Native {
                    quote_range,
                    current_mid,
                    ..
                } = &market.price_source
                else {
                    continue;
                };
                let Some((min, max)) = quoteable_bounds(*quote_range, &quote_config) else {
                    continue;
                };
                values.push(*current_mid);
                lower.push(min);
                upper.push(max);
            }
            let Some(projected) = project_bounded_simplex(&values, &lower, &upper) else {
                warn!(market_ids = ?market_ids, "native actor-mark simplex is infeasible");
                continue;
            };
            for (market_id, projected_mid) in market_ids.iter().zip(projected) {
                if let Some(market) = self.state.markets.get_mut(market_id)
                    && let PriceSource::Native { current_mid, .. } = &mut market.price_source
                {
                    *current_mid = projected_mid;
                }
            }
        }

        let mut threshold_cohorts = HashMap::<String, Vec<(usize, u32)>>::new();
        for market in self.state.markets.values() {
            if let Some(coherence_key) = &market.coherence_key {
                threshold_cohorts
                    .entry(coherence_key.clone())
                    .or_default()
                    .push((market.coherence_rank, market.sybil_market_id));
            }
        }
        for cohort in threshold_cohorts.values_mut() {
            cohort.sort_unstable();
            let market_ids = cohort
                .iter()
                .map(|(_, market_id)| *market_id)
                .collect::<Vec<_>>();
            let mut values = Vec::with_capacity(market_ids.len());
            let mut lower = Vec::with_capacity(market_ids.len());
            let mut upper = Vec::with_capacity(market_ids.len());
            for market_id in &market_ids {
                let Some(market) = self.state.markets.get(market_id) else {
                    continue;
                };
                let PriceSource::Native {
                    quote_range,
                    current_mid,
                    ..
                } = &market.price_source
                else {
                    continue;
                };
                let Some((min, max)) = quoteable_bounds(*quote_range, &quote_config) else {
                    continue;
                };
                values.push(*current_mid);
                lower.push(min);
                upper.push(max);
            }
            let Some(projected) = project_bounded_nonincreasing(&values, &lower, &upper) else {
                warn!(market_ids = ?market_ids, "native threshold actor marks are infeasible");
                continue;
            };
            for (market_id, projected_mid) in market_ids.iter().zip(projected) {
                if let Some(market) = self.state.markets.get_mut(market_id)
                    && let PriceSource::Native { current_mid, .. } = &mut market.price_source
                {
                    *current_mid = projected_mid;
                }
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
        self.reconcile_complete_set_inventory().await;

        // 2. Dynamic budget
        let budget_nanos = self.compute_budget(&snapshot);
        if budget_nanos == 0 {
            debug!("budget exhausted (exposure at max), submitting typed all-market pause");
            let skip_reasons = self
                .state
                .markets
                .keys()
                .map(|market_id| (*market_id, "insufficient_budget".to_string()))
                .collect();
            let _ = self
                .submit_orders(&[], &skip_reasons, 0, block.height)
                .await;
            return;
        }

        // 3. Update state (mutation pass): push prices, collect reference prices.
        //    Staleness is now evaluated per token (PM-4): a single frozen token
        //    stops being quoted even while its neighbours keep updating.
        let staleness_ms = self.config.mm_staleness_ms;
        let hard_staleness_ms = self.config.mm_hard_staleness_ms;
        let mut ref_prices = HashMap::new();
        let mut quote_inputs = Vec::new();
        let mut skip_reasons = HashMap::new();

        for ms in self.state.markets.values_mut() {
            let (mid, quote_range, spread_multiplier, size_multiplier) = match &ms.price_source {
                PriceSource::Mirror { yes_token_id } => {
                    let Some(&mid) = snapshot.midpoints.get(yes_token_id) else {
                        // Never seen a price for this token; nothing to publish or quote.
                        skip_reasons
                            .insert(ms.sybil_market_id, "reference_never_observed".to_string());
                        continue;
                    };

                    if snapshot.token_is_stale(yes_token_id, now, hard_staleness_ms) {
                        // PM-6: a frozen token's reference price is evicted so downstream
                        // `--require-reference-prices` consumers stop trading on it
                        // rather than being picked off on the stale value.
                        ref_prices.insert(ms.sybil_market_id, REFERENCE_PRICE_EVICTION_SENTINEL);
                        skip_reasons.insert(ms.sybil_market_id, "reference_hard_stale".to_string());
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
                        skip_reasons.insert(ms.sybil_market_id, "reference_extreme".to_string());
                        continue;
                    }

                    if snapshot.token_is_stale(yes_token_id, now, staleness_ms) {
                        (
                            mid,
                            None,
                            self.config.mm_soft_stale_spread_multiplier,
                            self.config.mm_soft_stale_size_multiplier,
                        )
                    } else {
                        (mid, None, 1.0, 1.0)
                    }
                }
                PriceSource::Native {
                    quote_range,
                    current_mid,
                    ..
                } => (*current_mid, Some(*quote_range), 1.0, 1.0),
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
                spread_multiplier,
                size_multiplier,
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
        let mut orders = Vec::with_capacity(quote_inputs.len().saturating_mul(2));
        for input in &quote_inputs {
            let market_orders = generate_quotes(input, &quote_config);
            if market_orders.is_empty() {
                skip_reasons
                    .entry(input.market_id)
                    .or_insert_with(|| "inventory_or_quote_unavailable".to_string());
            } else {
                orders.extend(market_orders);
            }
        }

        // 5. Submit (IO). A whole-batch rejection that names a non-tradeable
        //    market lets us drop the poison defensively (PM-1 defence in depth)
        //    even if we never saw its `MarketResolved` (e.g. missed block, or a
        //    market that became untradeable for another reason).
        if let Some(poisoned) = self
            .submit_orders(&orders, &skip_reasons, budget_nanos, block.height)
            .await
        {
            self.untrack_market(poisoned, "batch_rejected_untradeable");
        }

        // 6. Push reference prices (IO)
        if !ref_prices.is_empty()
            && let Err(error) = self.sybil_client.set_reference_prices(&ref_prices).await
        {
            warn!(error = %error, prices = ref_prices.len(), "reference-price update failed");
        }
    }

    async fn maybe_sync_positions(&mut self, block_height: u64) {
        if (block_height.saturating_sub(self.state.last_sync_block)
            >= self.config.mm_sync_interval_blocks
            || self.state.last_sync_block == 0)
            && self.sync_positions().await
        {
            self.state.last_sync_block = block_height;
        }
    }

    async fn reconcile_complete_set_inventory(&mut self) {
        if self.config.mm_actor_token.trim().is_empty() {
            return;
        }
        let target = whole_shares_to_qty_units(
            self.config
                .mm_complete_set_target_shares
                .min(self.config.mm_max_position) as i64,
        );
        if target <= 0 {
            return;
        }
        let actions = self
            .state
            .markets
            .values()
            .filter_map(|market| {
                let neutral = market.yes_position.min(market.no_position).max(0);
                let deficit = target.saturating_sub(neutral);
                (deficit > 0).then_some(CompleteSetActionRequest::Collateralize {
                    market_id: market.sybil_market_id,
                    quantity: deficit as u64,
                })
            })
            .collect::<Vec<_>>();
        if actions.is_empty() {
            return;
        }
        let request = CompleteSetInventoryRequest { actions };
        match self
            .sybil_client
            .update_complete_set_inventory(&self.config.mm_actor_token, &request)
            .await
        {
            Ok(()) => {
                for action in request.actions {
                    let CompleteSetActionRequest::Collateralize {
                        market_id,
                        quantity,
                    } = action
                    else {
                        continue;
                    };
                    if let Some(market) = self.state.markets.get_mut(&market_id) {
                        let quantity = i64::try_from(quantity).unwrap_or(i64::MAX);
                        market.yes_position = market.yes_position.saturating_add(quantity);
                        market.no_position = market.no_position.saturating_add(quantity);
                    }
                }
            }
            Err(error) => warn!(%error, "complete-set inventory reconciliation failed"),
        }
    }

    /// Submit the IOC batch. Returns `Some(market_id)` when the whole batch was
    /// rejected because that market is non-tradeable, so the caller can untrack
    /// it (defence in depth for PM-1).
    async fn submit_orders(
        &self,
        orders: &[OrderSpec],
        skip_reasons: &HashMap<u32, String>,
        budget_nanos: u64,
        block_height: u64,
    ) -> Option<u32> {
        if !self.config.mm_actor_token.trim().is_empty() {
            let universe = match self
                .sybil_client
                .actor_universe(&self.config.mm_actor_token)
                .await
            {
                Ok(universe) if universe.actor_ready => universe,
                Ok(_) => {
                    warn!("MM actor submission paused: liquidity universe not committed");
                    return None;
                }
                Err(error) => {
                    warn!(%error, "MM actor submission paused: universe lookup failed");
                    return None;
                }
            };
            if universe.account_id != Some(self.account_id)
                || universe.actor_role != Some(ActorRole::MarketMaker)
            {
                warn!(
                    configured_account = self.account_id,
                    credential_account = universe.account_id,
                    credential_role = ?universe.actor_role,
                    "MM actor submission paused: credential binding mismatch"
                );
                return None;
            }
            let mut by_market = HashMap::<u32, Vec<OrderSpec>>::new();
            for order in orders {
                let market_id = match order {
                    OrderSpec::BuyYes { market_id, .. }
                    | OrderSpec::BuyNo { market_id, .. }
                    | OrderSpec::SellYes { market_id, .. }
                    | OrderSpec::SellNo { market_id, .. } => *market_id,
                };
                by_market.entry(market_id).or_default().push(order.clone());
            }
            let market_intents = universe
                .market_ids
                .iter()
                .map(|market_id| {
                    let market_orders = by_market.remove(market_id).unwrap_or_default();
                    ActorMarketIntent {
                        market_id: *market_id,
                        skip_reason: market_orders.is_empty().then(|| {
                            skip_reasons
                                .get(market_id)
                                .cloned()
                                .unwrap_or_else(|| "market_not_tracked".to_string())
                        }),
                        orders: market_orders,
                    }
                })
                .collect();
            let observed_at_ms = now_ms();
            let request = ActorEpochRequest {
                epoch_id: format!(
                    "mm-{}-{}",
                    universe.generation,
                    block_height.saturating_add(1)
                ),
                target_height: block_height.saturating_add(1),
                universe_generation: universe.generation,
                observed_at_ms,
                valid_until_ms: observed_at_ms.saturating_add(30_000),
                market_intents,
                mm_budget_nanos: Some(budget_nanos),
            };
            match self
                .sybil_client
                .submit_actor_epoch(&self.config.mm_actor_token, &request)
                .await
            {
                Ok(receipt) => {
                    debug!(
                        block = block_height,
                        markets = receipt.markets.len(),
                        orders = orders.len(),
                        "submitted all-market MM actor epoch"
                    );
                    return None;
                }
                Err(error) => {
                    warn!(block = block_height, %error, "MM actor epoch submission failed");
                    return None;
                }
            }
        }
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
            coherence_key: None,
            coherence_rank: 0,
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
        block.by_market.insert(
            "7".to_string(),
            BlockMarketStats {
                volume_nanos: 2 * NANOS_PER_DOLLAR,
                organic_matched_orders: 1,
                organic_fill_notional_nanos: 2 * NANOS_PER_DOLLAR,
                ..BlockMarketStats::default()
            },
        );
        actor.observe_block(&block);

        let market = actor.state.markets.get(&7).expect("tracked native market");
        assert_eq!(market.budget_mid(&PriceSnapshot::default()), 0.403);
    }

    #[test]
    fn native_midpoint_ignores_actor_only_flow() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut block = block_resolving(&[]);
        block
            .clearing_prices_nanos
            .insert("7".to_string(), vec![700_000_000, 300_000_000]);
        block.by_market.insert(
            "7".to_string(),
            BlockMarketStats {
                volume_nanos: 2 * NANOS_PER_DOLLAR,
                mm_matched_orders: 1,
                noise_matched_orders: 1,
                mm_fill_notional_nanos: 2 * NANOS_PER_DOLLAR,
                noise_fill_notional_nanos: 2 * NANOS_PER_DOLLAR,
                ..BlockMarketStats::default()
            },
        );
        actor.observe_block(&block);

        let market = actor.state.markets.get(&7).expect("tracked native market");
        assert_eq!(market.budget_mid(&PriceSnapshot::default()), 0.4);
    }

    #[test]
    fn native_midpoint_ignores_zero_volume_organic_clearing_vector() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut block = block_resolving(&[]);
        block
            .clearing_prices_nanos
            .insert("7".to_string(), vec![700_000_000, 300_000_000]);
        block.by_market.insert(
            "7".to_string(),
            BlockMarketStats {
                volume_nanos: 0,
                organic_matched_orders: 1,
                organic_fill_notional_nanos: 2 * NANOS_PER_DOLLAR,
                ..BlockMarketStats::default()
            },
        );
        actor.observe_block(&block);

        let market = actor.state.markets.get(&7).expect("tracked native market");
        assert_eq!(market.budget_mid(&PriceSnapshot::default()), 0.4);
    }

    #[test]
    fn native_midpoint_step_is_capped_and_quiet_blocks_revert_to_seed() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut organic = block_resolving(&[]);
        organic
            .clearing_prices_nanos
            .insert("7".to_string(), vec![900_000_000, 100_000_000]);
        organic.by_market.insert(
            "7".to_string(),
            BlockMarketStats {
                volume_nanos: 2 * NANOS_PER_DOLLAR,
                organic_matched_orders: 1,
                organic_fill_notional_nanos: 2 * NANOS_PER_DOLLAR,
                ..BlockMarketStats::default()
            },
        );
        actor.observe_block(&organic);
        let moved = actor
            .state
            .markets
            .get(&7)
            .expect("tracked native market")
            .budget_mid(&PriceSnapshot::default());
        assert!((moved - 0.403).abs() < 1e-12);

        let mut quiet = block_resolving(&[]);
        quiet.height = 2;
        actor.observe_block(&quiet);
        let reverted = actor
            .state
            .markets
            .get(&7)
            .expect("tracked native market")
            .budget_mid(&PriceSnapshot::default());
        assert!(reverted < moved);
        assert!(reverted > 0.4);
    }

    #[test]
    fn actor_only_flow_cannot_walk_native_mark_to_guardrail_after_ten_thousand_blocks() {
        let (live_tx, _live_rx) = watch::channel(0usize);
        let (mut actor, _price_tx) = test_actor(live_tx);
        track_native(&mut actor, 7, 0.4);

        let mut block = block_resolving(&[]);
        block
            .clearing_prices_nanos
            .insert("7".to_string(), vec![940_000_000, 60_000_000]);
        block.by_market.insert(
            "7".to_string(),
            BlockMarketStats {
                volume_nanos: 10 * NANOS_PER_DOLLAR,
                mm_matched_orders: 1,
                noise_matched_orders: 1,
                mm_fill_notional_nanos: 10 * NANOS_PER_DOLLAR,
                noise_fill_notional_nanos: 10 * NANOS_PER_DOLLAR,
                ..BlockMarketStats::default()
            },
        );
        for height in 1..=10_000 {
            block.height = height;
            actor.observe_block(&block);
        }

        let mid = actor
            .state
            .markets
            .get(&7)
            .expect("tracked native market")
            .budget_mid(&PriceSnapshot::default());
        assert_eq!(mid, 0.4);
        let mut input = default_input(mid);
        input.quote_range = Some(QuoteRange {
            min: 0.05,
            max: 0.95,
            initial: 0.4,
        });
        assert_eq!(generate_quotes(&input, &default_config()).len(), 2);
    }

    #[test]
    fn native_coherence_projections_are_bounded_and_ordered() {
        let simplex = project_bounded_simplex(&[0.8, 0.7, 0.6], &[0.1, 0.1, 0.1], &[0.8, 0.8, 0.8])
            .expect("feasible bounded simplex");
        assert!((simplex.iter().sum::<f64>() - 1.0).abs() < 1e-10);
        assert!(
            simplex
                .iter()
                .all(|value| (0.1 - 1e-12..=0.8 + 1e-12).contains(value))
        );

        let ladder = project_bounded_nonincreasing(
            &[0.2, 0.7, 0.3],
            &[0.15, 0.10, 0.05],
            &[0.90, 0.80, 0.70],
        )
        .expect("feasible bounded ladder");
        assert!(ladder.windows(2).all(|pair| pair[0] >= pair[1]));
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
            yes_position: q(100),
            no_position: q(100),
            group_key: None,
            group_size: 0,
            quote_range: None,
            spread_multiplier: 1.0,
            size_multiplier: 1.0,
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
        assert!(
            orders
                .iter()
                .any(|o| matches!(o, OrderSpec::SellYes { .. }))
        );
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
        assert!(
            !orders
                .iter()
                .any(|o| matches!(o, OrderSpec::BuyYes { .. } | OrderSpec::BuyNo { .. }))
        );
    }

    #[test]
    fn grouped_markets_quote_both_economic_sides_from_complete_sets() {
        let orders = generate_quotes(&grouped_input(0.7), &default_config());

        assert!(
            orders
                .iter()
                .any(|o| matches!(o, OrderSpec::SellYes { .. }))
        );
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
    }

    #[test]
    fn inventory_skews_reservation_price() {
        let config = default_config();
        // Long YES → reservation price below mid → tighter YES bid, wider YES ask
        let mut long_yes = default_input(0.5);
        long_yes.net_inventory = 1000.0;
        long_yes.yes_position = q(1000);
        let orders = generate_quotes(&long_yes, &config);

        let yes_ask = orders.iter().find_map(|o| match o {
            OrderSpec::SellYes {
                limit_price_nanos, ..
            } => Some(*limit_price_nanos),
            _ => None,
        });
        // With long directional YES inventory, the reservation and ask move lower.
        assert!(yes_ask.is_some());
        assert!(yes_ask.unwrap() < 520_000_000);
    }

    #[test]
    fn inventory_skews_both_complete_set_sell_quotes() {
        let config = default_config();
        let neutral_orders = generate_quotes(&default_input(0.5), &config);
        let mut long_yes = default_input(0.5);
        long_yes.net_inventory = 1000.0;
        long_yes.yes_position = q(1000);
        let long_yes_orders = generate_quotes(&long_yes, &config);

        let neutral_yes_ask = neutral_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::SellYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let long_yes_ask = long_yes_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::SellYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let neutral_no_ask = neutral_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::SellNo {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let long_yes_no_ask = long_yes_orders
            .iter()
            .find_map(|order| match order {
                OrderSpec::SellNo {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();

        assert!(long_yes_ask < neutral_yes_ask);
        assert!(long_yes_no_ask > neutral_no_ask);
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
    fn complete_inventory_never_generates_cash_buy_orders() {
        let config = default_config();
        let mut input = default_input(0.5);
        input.yes_position = q(5000); // at max_position
        let orders = generate_quotes(&input, &config);
        assert!(
            !orders
                .iter()
                .any(|o| matches!(o, OrderSpec::BuyYes { .. } | OrderSpec::BuyNo { .. }))
        );
    }

    #[test]
    fn sell_quantities_are_capped_to_held_complete_sets() {
        let mut config = default_config();
        config.max_position = 100;
        config.quote_size_dollars = 100.0;
        let mut input = default_input(0.5);
        input.yes_position = q(99);
        input.no_position = q(98);

        let orders = generate_quotes(&input, &config);
        let yes_quantity = orders.iter().find_map(|order| match order {
            OrderSpec::SellYes { quantity, .. } => Some(*quantity),
            _ => None,
        });
        let no_quantity = orders.iter().find_map(|order| match order {
            OrderSpec::SellNo { quantity, .. } => Some(*quantity),
            _ => None,
        });

        assert_eq!(yes_quantity, Some(q(99) as u64));
        assert_eq!(no_quantity, Some(q(98) as u64));
    }

    #[test]
    fn group_market_quotes_no_side() {
        let config = default_config();
        let input = grouped_input(0.5);
        let orders = generate_quotes(&input, &config);
        assert!(orders.iter().any(|o| matches!(o, OrderSpec::SellNo { .. })));
        assert!(!orders.iter().any(|o| matches!(o, OrderSpec::BuyNo { .. })));
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
            OrderSpec::SellYes { market_id, .. } => *market_id == 1,
            _ => false,
        }));
        assert!(orders.iter().any(|order| match order {
            OrderSpec::SellYes { market_id, .. } => *market_id == 3,
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
            OrderSpec::SellYes { market_id, .. } => *market_id == 4,
            _ => false,
        }));
        assert!(orders.iter().any(|order| match order {
            OrderSpec::SellYes { market_id, .. } => *market_id == 5,
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
        assert_eq!(orders.len(), 6);
        assert!(
            orders
                .iter()
                .all(|order| matches!(order, OrderSpec::SellYes { .. } | OrderSpec::SellNo { .. }))
        );
    }

    #[test]
    fn edge_price_retains_two_sided_quotes() {
        let config = default_config();
        // A near-edge reservation is recovered into the quoteable interior.
        let orders = generate_quotes(&default_input(0.005), &config);
        assert_eq!(orders.len(), 2);
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

        let low_ask = low_orders
            .iter()
            .find_map(|o| match o {
                OrderSpec::SellYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();
        let high_ask = high_orders
            .iter()
            .find_map(|o| match o {
                OrderSpec::SellYes {
                    limit_price_nanos, ..
                } => Some(*limit_price_nanos),
                _ => None,
            })
            .unwrap();

        // Higher volatility → wider spread → higher ask.
        assert!(high_ask > low_ask);
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
            coherence_key: None,
            coherence_rank: 0,
        });
        actor.state.markets.get_mut(&99).unwrap().yes_position = q(50);

        assert_eq!(*live_rx.borrow(), 1);
        let budget = actor.compute_budget(&PriceSnapshot::default());
        assert!(budget > 0, "native budget should not require feed prices");
    }
}
