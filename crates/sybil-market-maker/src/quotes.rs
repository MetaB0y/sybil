use std::collections::HashMap;

use sybil_api_types::{NANOS_PER_DOLLAR, OrderSpec};

use super::{MmMode, QuoteRange, shares_to_qty_units, whole_shares_to_qty_units};

// Pure pricing logic, independent of discovery and reference-price providers.
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
    pub quote_range: Option<QuoteRange>,
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
    pub mode: MmMode,
}

/// Generate orders for one market. Pure function — no IO, no state mutation.
pub fn generate_quotes(input: &QuoteInput, config: &QuoteConfig) -> Vec<OrderSpec> {
    let generated = generate_market_quotes(input, config);
    let mut orders = Vec::new();
    if let Some(compaction) = generated.compaction {
        orders.extend(compaction);
    }
    orders.extend(generated.baseline);
    orders.extend(generated.inventory);
    orders
}

struct MarketQuotes {
    baseline: Vec<OrderSpec>,
    inventory: Vec<OrderSpec>,
    compaction: Option<[OrderSpec; 2]>,
}

fn generate_market_quotes(input: &QuoteInput, config: &QuoteConfig) -> MarketQuotes {
    let mut baseline = Vec::new();
    let mut inventory = Vec::new();
    let (reservation_min, reservation_max, yes_order_min, yes_order_max) =
        if let Some(range) = input.quote_range {
            (range.min, range.max, range.min, range.max)
        } else {
            (0.02, 0.98, 0.01, 0.99)
        };
    let no_order_min = 1.0 - yes_order_max;
    let no_order_max = 1.0 - yes_order_min;

    // Avellaneda-Stoikov reservation price
    let r = (input.mid - input.net_inventory * config.gamma * input.sigma_sq)
        .clamp(reservation_min, reservation_max);

    // Adaptive spread: wider when volatile
    let vol_spread = config.base_spread * (1.0 + input.sigma_sq * 200.0);
    let edge_room = if input.quote_range.is_some() {
        (r - yes_order_min).min(yes_order_max - r)
    } else {
        r.min(1.0 - r)
    };
    let half_spread =
        vol_spread.clamp(config.min_spread, (edge_room - 0.01).max(config.min_spread));

    // A matched YES+NO pair is a complete set worth exactly $1 in every
    // outcome. It carries no directional risk and is compacted below. The
    // absolute per-token position cap still applies until that burn lands.
    let yes_position = input.yes_position.max(0);
    let no_position = input.no_position.max(0);
    let paired_position = yes_position.min(no_position);
    let yes_residual = yes_position.saturating_sub(no_position);
    let no_residual = no_position.saturating_sub(yes_position);
    let max_position_units = whole_shares_to_qty_units(config.max_position);
    let yes_buy_room = max_position_units.saturating_sub(yes_position) as u64;
    let no_buy_room = max_position_units.saturating_sub(no_position) as u64;

    // Inventory-adjusted sizing
    let inv_ratio = (input.net_inventory.abs() / config.max_position as f64).min(1.0);
    let buy_size = config.quote_size_dollars * (1.0 - inv_ratio * 0.8);
    let sell_size = config.quote_size_dollars * (1.0 + inv_ratio * 0.5);

    // ── YES side ──
    let yes_bid = r - half_spread;
    let yes_ask = r + half_spread;

    let yes_buy_cap = match config.mode {
        MmMode::Normal => yes_buy_room,
        MmMode::ReduceOnly if no_residual > 0 => yes_buy_room.min(no_residual as u64),
        MmMode::ReduceOnly => 0,
    };
    if yes_buy_cap > 0 && price_in_band(yes_bid, yes_order_min, yes_order_max) {
        let quantity = shares_to_qty_units(buy_size / yes_bid).min(yes_buy_cap);
        if quantity > 0 {
            baseline.push(OrderSpec::BuyYes {
                market_id: input.market_id,
                limit_price_nanos: (yes_bid * NANOS_PER_DOLLAR as f64) as u64,
                quantity,
            });
        }
    }

    if yes_residual > 0 && price_in_band(yes_ask, yes_order_min, yes_order_max) {
        let max_sell = yes_residual as u64;
        let desired = shares_to_qty_units(sell_size / yes_ask);
        inventory.push(OrderSpec::SellYes {
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

    let no_buy_cap = match config.mode {
        MmMode::Normal => no_buy_room,
        MmMode::ReduceOnly if yes_residual > 0 => no_buy_room.min(yes_residual as u64),
        MmMode::ReduceOnly => 0,
    };
    if no_buy_cap > 0 && price_in_band(no_bid, no_order_min, no_order_max) {
        let quantity = shares_to_qty_units(buy_size / no_bid).min(no_buy_cap);
        if quantity > 0 {
            baseline.push(OrderSpec::BuyNo {
                market_id: input.market_id,
                limit_price_nanos: (no_bid * NANOS_PER_DOLLAR as f64) as u64,
                quantity,
            });
        }
    }

    if no_residual > 0 && price_in_band(no_ask, no_order_min, no_order_max) {
        let max_sell = no_residual as u64;
        let desired = shares_to_qty_units(sell_size / no_ask);
        inventory.push(OrderSpec::SellNo {
            market_id: input.market_id,
            limit_price_nanos: (no_ask * NANOS_PER_DOLLAR as f64) as u64,
            quantity: desired.min(max_sell),
        });
    }

    // Selling a complete set with limits summing to one dollar minus one nano
    // is always jointly executable at a valid binary clearing price. The
    // sequencer burns the matched YES+NO pair and returns its $1 collateral.
    // Keeping the pair atomic avoids accidentally selling only one outcome.
    let compaction = (paired_position > 0).then(|| {
        let yes_limit = (input.mid.clamp(0.0, 1.0) * NANOS_PER_DOLLAR as f64)
            .floor()
            .clamp(1.0, (NANOS_PER_DOLLAR - 2) as f64) as u64;
        let no_limit = NANOS_PER_DOLLAR - 1 - yes_limit;
        [
            OrderSpec::SellYes {
                market_id: input.market_id,
                limit_price_nanos: yes_limit,
                quantity: paired_position as u64,
            },
            OrderSpec::SellNo {
                market_id: input.market_id,
                limit_price_nanos: no_limit,
                quantity: paired_position as u64,
            },
        ]
    });

    MarketQuotes {
        baseline,
        inventory,
        compaction,
    }
}

fn price_in_band(price: f64, min: f64, max: f64) -> bool {
    price > 0.01 && price < 0.99 && price >= min && price <= max
}

/// Select a bounded, rotating quote batch for one block.
///
/// Cash-backed baseline quotes are selected for the whole eligible catalog
/// before complete-set compaction and extra inventory depth consume spare
/// capacity. Rotation remains deterministic overflow behavior when even the
/// baseline cannot fit or when optional work exceeds the remaining capacity.
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
    let mut group_quotes = HashMap::<String, GroupQuoteState>::new();
    let generated: Vec<_> = quote_inputs
        .iter()
        .map(|input| generate_market_quotes(input, quote_config))
        .collect();

    // Baseline pass: protect catalog-wide liquidity from accumulated
    // inventory and compaction work.
    for offset in 0..quote_inputs.len() {
        let idx = (start + offset) % quote_inputs.len();
        let input = &quote_inputs[idx];
        let mut market_orders = generated[idx].baseline.clone();
        if input.group_key.is_some() {
            market_orders.sort_by_key(|order| match order {
                OrderSpec::BuyNo { .. } => 0,
                OrderSpec::BuyYes { .. } => 1,
                _ => 2,
            });
        }

        for order in market_orders {
            if orders.len() >= max_orders {
                return (orders, idx);
            }
            if would_self_cross_group(input, &order, &group_quotes) {
                continue;
            }
            record_group_quote(input, &order, &mut group_quotes);
            orders.push(order);
        }
    }

    // Compaction pass: complete sets are always selected as an atomic pair.
    for offset in 0..quote_inputs.len() {
        let idx = (start + offset) % quote_inputs.len();
        let Some(compaction) = generated[idx].compaction.clone() else {
            continue;
        };
        if orders.len().saturating_add(compaction.len()) > max_orders {
            return (orders, idx);
        }
        orders.extend(compaction);
    }

    // Inventory pass: residual sells add depth only after every baseline quote
    // and every collateral-releasing compaction pair that fits.
    for offset in 0..quote_inputs.len() {
        let idx = (start + offset) % quote_inputs.len();
        let input = &quote_inputs[idx];
        for order in generated[idx].inventory.iter().cloned() {
            if orders.len() >= max_orders {
                return (orders, idx);
            }
            record_group_quote(input, &order, &mut group_quotes);
            orders.push(order);
        }
    }

    (orders, start)
}

#[derive(Clone, Default)]
struct GroupQuoteState {
    yes_limits: HashMap<u32, u64>,
    no_limits: HashMap<u32, u64>,
}

fn would_self_cross_group(
    input: &QuoteInput,
    order: &OrderSpec,
    groups: &HashMap<String, GroupQuoteState>,
) -> bool {
    let Some(group_key) = &input.group_key else {
        return false;
    };
    let group_size = input.group_size;
    if group_size < 2 {
        return false;
    }
    let mut candidate = groups.get(group_key).cloned().unwrap_or_default();
    record_limit(&mut candidate, order);

    match order {
        OrderSpec::BuyYes { market_id, .. } => {
            complementary_limits_cross(&candidate, *market_id)
                || (candidate.yes_limits.len() >= group_size
                    && candidate
                        .yes_limits
                        .values()
                        .copied()
                        .map(u128::from)
                        .sum::<u128>()
                        >= u128::from(NANOS_PER_DOLLAR))
        }
        OrderSpec::BuyNo { market_id, .. } => complementary_limits_cross(&candidate, *market_id),
        _ => false,
    }
}

fn record_group_quote(
    input: &QuoteInput,
    order: &OrderSpec,
    groups: &mut HashMap<String, GroupQuoteState>,
) {
    let Some(group_key) = &input.group_key else {
        return;
    };
    record_limit(groups.entry(group_key.clone()).or_default(), order);
}

fn record_limit(state: &mut GroupQuoteState, order: &OrderSpec) {
    match order {
        OrderSpec::BuyYes {
            market_id,
            limit_price_nanos,
            ..
        } => {
            state
                .yes_limits
                .entry(*market_id)
                .and_modify(|current| *current = (*current).max(*limit_price_nanos))
                .or_insert(*limit_price_nanos);
        }
        OrderSpec::BuyNo {
            market_id,
            limit_price_nanos,
            ..
        } => {
            state
                .no_limits
                .entry(*market_id)
                .and_modify(|current| *current = (*current).max(*limit_price_nanos))
                .or_insert(*limit_price_nanos);
        }
        _ => {}
    }
}

fn complementary_limits_cross(state: &GroupQuoteState, market_id: u32) -> bool {
    match (
        state.yes_limits.get(&market_id),
        state.no_limits.get(&market_id),
    ) {
        (Some(yes), Some(no)) => u128::from(*yes) + u128::from(*no) >= u128::from(NANOS_PER_DOLLAR),
        _ => false,
    }
}

// --------------------------------------------------------------------------- //
