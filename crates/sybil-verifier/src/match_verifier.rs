//! Layer 1: Fill-level and market-level match verification.
//!
//! Checks that every fill is consistent with its order and that market-level
//! price invariants (UCP, complementarity, market groups) hold. Position
//! solvency is checked by settlement verification through the MINT account.

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketId, Order, NANOS_PER_DOLLAR};

use crate::arithmetic::{checked_price_qty, checked_welfare};
use crate::types::BlockWitness;
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify all fill-level and market-level invariants.
///
/// Core checks (ZK invariants) always run. Diagnostic checks (quality metrics)
/// only run when `diagnostics` is true.
pub fn verify_match(witness: &BlockWitness, diagnostics: bool) -> VerificationResult {
    let mut violations = Vec::new();
    let mut stats = VerificationStats::default();

    let order_map: HashMap<u64, &Order> = witness
        .orders
        .iter()
        .map(|wo| (wo.order.id, &wo.order))
        .collect();

    // --- Per-fill checks (migrated from old verifier) ---
    verify_fills(
        &witness.fills,
        &order_map,
        diagnostics,
        witness.total_welfare,
        witness.minting_cost,
        &mut violations,
        &mut stats,
    );

    // --- MM budget constraints ---
    verify_mm_constraints(
        &witness.fills,
        &witness.mm_constraints,
        &mut violations,
        &mut stats,
    );

    // --- Market-level checks ---
    verify_order_id_uniqueness(witness, &mut violations);
    verify_price_complementarity(witness, &mut violations);
    verify_resolved_markets(witness, &order_map, &mut violations);
    verify_conditional_activation(witness, &order_map, &mut violations);

    // UCP: The solver enforces uniform clearing prices by re-pricing all
    // single-market fills at the final clearing price after iteration completes.
    verify_uniform_clearing_prices(witness, &order_map, &mut violations);

    // Diagnostic-only checks:
    //
    // Market group constraint: With finite liquidity, clearing prices in a market
    // group may sum > $1 (or < $1). This represents unexploited arbitrage that
    // the solver couldn't close due to insufficient liquidity, not a correctness
    // bug. Use verify_match stats to check avg |sum - 1| instead.
    if diagnostics {
        verify_market_group_constraints(witness, &mut violations);
    }

    stats.reported_welfare = witness.total_welfare;

    // Compute market group price quality metric: avg |sum_YES_prices - $1|
    if !witness.market_groups.is_empty() {
        let mut total_delta: u64 = 0;
        for group in &witness.market_groups {
            let mut sum: u64 = 0;
            for &market in &group.markets {
                if let Some(prices) = witness.clearing_prices.get(&market) {
                    if let Some(&yes_price) = prices.first() {
                        sum = sum.saturating_add(yes_price.0);
                    }
                }
            }
            let delta = sum.abs_diff(NANOS_PER_DOLLAR);
            total_delta = total_delta.saturating_add(delta);
        }
        stats.market_group_avg_delta = Some(total_delta / witness.market_groups.len() as u64);
    }

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats,
    }
}

/// Per-fill checks: order exists, qty, price, welfare, duplicates.
fn verify_fills(
    fills: &[Fill],
    order_map: &HashMap<u64, &Order>,
    diagnostics: bool,
    reported_welfare: i64,
    minting_cost: i64,
    violations: &mut Vec<Violation>,
    stats: &mut VerificationStats,
) {
    let mut filled_orders: HashSet<u64> = HashSet::new();
    let mut computed_gross_welfare: i64 = 0;

    for fill in fills {
        stats.fills_checked += 1;

        // 1. Order exists
        let Some(order) = order_map.get(&fill.order_id) else {
            violations.push(Violation {
                kind: ViolationKind::OrderNotFound,
                details: format!("Order {} not found in witness", fill.order_id),
            });
            continue;
        };

        // 2. Duplicate fills
        if filled_orders.contains(&fill.order_id) {
            violations.push(Violation {
                kind: ViolationKind::DuplicateFill,
                details: format!("Order {} filled multiple times", fill.order_id),
            });
        }
        filled_orders.insert(fill.order_id);

        // 3. Quantity constraints
        if fill.fill_qty > order.max_fill {
            violations.push(Violation {
                kind: ViolationKind::QuantityExceedsMax,
                details: format!(
                    "Order {}: fill_qty {} > max_fill {}",
                    fill.order_id, fill.fill_qty, order.max_fill
                ),
            });
        }

        // 4. Zero fill (diagnostic only)
        if fill.fill_qty == matching_engine::Qty::ZERO && diagnostics {
            violations.push(Violation {
                kind: ViolationKind::ZeroQuantityFill,
                details: format!("Order {}: zero quantity fill", fill.order_id),
            });
        }

        // 5. Price constraint (seller-aware)
        if order.limit_price.0 > NANOS_PER_DOLLAR {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: limit_price {} exceeds NANOS_PER_DOLLAR {}",
                    order.id, order.limit_price, NANOS_PER_DOLLAR
                ),
            });
        }
        if fill.fill_price.0 > NANOS_PER_DOLLAR {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: fill_price {} exceeds NANOS_PER_DOLLAR {}",
                    fill.order_id, fill.fill_price, NANOS_PER_DOLLAR
                ),
            });
        }

        let price_violated = if order.is_seller() {
            fill.fill_price < order.limit_price
        } else {
            fill.fill_price > order.limit_price
        };
        if price_violated {
            let dir = if order.is_seller() { "<" } else { ">" };
            violations.push(Violation {
                kind: ViolationKind::PriceExceedsLimit,
                details: format!(
                    "Order {} ({}): fill_price {} {} limit_price {}",
                    fill.order_id,
                    if order.is_seller() { "sell" } else { "buy" },
                    fill.fill_price,
                    dir,
                    order.limit_price
                ),
            });
        }

        // 6. Per-fill welfare
        let Some(fill_welfare) = checked_welfare(
            order.limit_price,
            fill.fill_price,
            fill.fill_qty,
            order.is_seller(),
        ) else {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: welfare overflow (limit={}, fill_price={}, qty={})",
                    fill.order_id, order.limit_price, fill.fill_price, fill.fill_qty
                ),
            });
            continue;
        };
        if fill_welfare < 0 {
            violations.push(Violation {
                kind: ViolationKind::NegativeWelfare,
                details: format!(
                    "Order {}: negative welfare {} (limit={}, fill_price={}, qty={})",
                    fill.order_id, fill_welfare, order.limit_price, fill.fill_price, fill.fill_qty
                ),
            });
        }
        let Some(gross_value) = checked_price_qty(order.limit_price, fill.fill_qty) else {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: gross welfare price*quantity overflow",
                    fill.order_id
                ),
            });
            continue;
        };
        let gross_contribution = if order.is_seller() {
            match gross_value.checked_neg() {
                Some(value) => value,
                None => {
                    violations.push(Violation {
                        kind: ViolationKind::SettlementOverflow,
                        details: format!("Order {}: gross welfare overflow", fill.order_id),
                    });
                    continue;
                }
            }
        } else {
            gross_value
        };
        let Some(updated_gross_welfare) = computed_gross_welfare.checked_add(gross_contribution)
        else {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: accumulated gross welfare overflow",
                    fill.order_id
                ),
            });
            continue;
        };
        computed_gross_welfare = updated_gross_welfare;
    }

    stats.orders_checked = order_map.len();
    let Some(expected_welfare) = computed_gross_welfare.checked_sub(minting_cost) else {
        violations.push(Violation {
            kind: ViolationKind::SettlementOverflow,
            details: format!(
                "Computed gross welfare {} - minting_cost {} overflowed",
                computed_gross_welfare, minting_cost
            ),
        });
        return;
    };
    stats.computed_welfare = expected_welfare;

    // 7a. Minting cost must be non-negative (can only reduce welfare, never inflate it)
    if minting_cost < 0 {
        violations.push(Violation {
            kind: ViolationKind::WelfareMismatch,
            details: format!(
                "Minting cost {} is negative — cannot inflate welfare beyond fill-level",
                minting_cost
            ),
        });
    }

    // 7b. Welfare consistency: total_welfare = gross_order_value - minting_cost
    let Some(welfare_delta) = expected_welfare.checked_sub(reported_welfare) else {
        violations.push(Violation {
            kind: ViolationKind::SettlementOverflow,
            details: format!(
                "Expected welfare {} - reported welfare {} overflowed",
                expected_welfare, reported_welfare
            ),
        });
        return;
    };
    let welfare_diff = welfare_delta.unsigned_abs();
    if welfare_diff > 0 {
        violations.push(Violation {
            kind: ViolationKind::WelfareMismatch,
            details: format!(
                "Computed gross welfare {} - minting_cost {} = {} != reported welfare {} (diff={})",
                computed_gross_welfare,
                minting_cost,
                expected_welfare,
                reported_welfare,
                welfare_diff
            ),
        });
    }
}

/// Verify market maker budget constraints.
fn verify_mm_constraints(
    fills: &[Fill],
    mm_constraints: &[matching_engine::MmConstraint],
    violations: &mut Vec<Violation>,
    stats: &mut VerificationStats,
) {
    let fill_map: HashMap<u64, &Fill> = fills.iter().map(|f| (f.order_id, f)).collect();

    for mm in mm_constraints {
        stats.mm_constraints_checked += 1;

        let mut mm_fills: HashMap<u64, (matching_engine::Nanos, matching_engine::Qty)> =
            HashMap::new();
        for &order_id in &mm.order_ids {
            if let Some(fill) = fill_map.get(&order_id) {
                if fill.fill_qty.0 > 0 {
                    mm_fills.insert(order_id, (fill.fill_price, fill.fill_qty));
                }
            }
        }

        let Some(capital_used) = mm.checked_capital_used(&mm_fills) else {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!("MM {:?}: capital_used overflowed", mm.mm_id),
            });
            continue;
        };
        if capital_used > mm.max_capital {
            violations.push(Violation {
                kind: ViolationKind::MmBudgetExceeded,
                details: format!(
                    "MM {:?}: capital_used {} > budget {} (overflow by {})",
                    mm.mm_id,
                    capital_used,
                    mm.max_capital,
                    capital_used - mm.max_capital
                ),
            });
        }
    }
}

/// Check 16: All order IDs in the witness are distinct.
fn verify_order_id_uniqueness(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    let mut seen: HashSet<u64> = HashSet::new();
    for wo in &witness.orders {
        if !seen.insert(wo.order.id) {
            violations.push(Violation {
                kind: ViolationKind::DuplicateOrderId,
                details: format!("Duplicate order ID {} in witness", wo.order.id),
            });
        }
    }
}

/// Check 10: Uniform clearing price — single-market fills must match clearing prices.
fn verify_uniform_clearing_prices(
    witness: &BlockWitness,
    order_map: &HashMap<u64, &Order>,
    violations: &mut Vec<Violation>,
) {
    for fill in &witness.fills {
        if fill.fill_qty == matching_engine::Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        // Only check single-market, single-outcome orders
        if order.num_markets != 1 {
            continue;
        }

        let market = order.markets[0];
        let Some(prices) = witness.clearing_prices.get(&market) else {
            continue;
        };

        // Determine which outcome this order is for
        let num_states = order.num_states as usize;
        if num_states != 2 {
            continue;
        }

        // Find which outcome has non-zero payoff
        let yes_payoff = order.payoffs[0];
        let no_payoff = order.payoffs[1];

        let expected_price = if yes_payoff > 0 && no_payoff == 0 {
            // Buy YES → should be filled at YES clearing price
            prices.first().copied()
        } else if yes_payoff == 0 && no_payoff > 0 {
            // Buy NO → should be filled at NO clearing price
            prices.get(1).copied()
        } else if yes_payoff < 0 && no_payoff == 0 {
            // Sell YES → should be filled at YES clearing price
            prices.first().copied()
        } else if yes_payoff == 0 && no_payoff < 0 {
            // Sell NO → should be filled at NO clearing price
            prices.get(1).copied()
        } else {
            // Mixed payoffs (e.g. spread on a single market) — skip
            None
        };

        if let Some(expected) = expected_price {
            if fill.fill_price != expected {
                violations.push(Violation {
                    kind: ViolationKind::UniformClearingPriceViolation,
                    details: format!(
                        "Order {} on market {:?}: fill_price {} != clearing_price {}",
                        fill.order_id, market, fill.fill_price, expected
                    ),
                });
            }
        }
    }
}

/// Check 11: Price complementarity — YES + NO = $1 per binary market.
fn verify_price_complementarity(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    for (&market, prices) in &witness.clearing_prices {
        for (outcome, price) in prices.iter().enumerate() {
            if price.0 > NANOS_PER_DOLLAR {
                violations.push(Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details: format!(
                        "Market {:?} outcome {}: clearing price {} exceeds NANOS_PER_DOLLAR {}",
                        market, outcome, price, NANOS_PER_DOLLAR
                    ),
                });
            }
        }
        if prices.len() == 2 {
            let Some(sum) = prices[0].checked_add(prices[1]) else {
                violations.push(Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details: format!(
                        "Market {:?}: P(YES)={} + P(NO)={} overflowed",
                        market, prices[0], prices[1]
                    ),
                });
                continue;
            };
            if sum.0 != NANOS_PER_DOLLAR {
                violations.push(Violation {
                    kind: ViolationKind::PriceComplementarityViolation,
                    details: format!(
                        "Market {:?}: P(YES)={} + P(NO)={} = {} != {}",
                        market, prices[0], prices[1], sum, NANOS_PER_DOLLAR
                    ),
                });
            }
        }
    }
}

/// Check 14: Market group constraint — sum of YES clearing prices <= $1.
fn verify_market_group_constraints(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    for group in &witness.market_groups {
        let mut sum: u64 = 0;
        for &market in &group.markets {
            if let Some(prices) = witness.clearing_prices.get(&market) {
                if let Some(&yes_price) = prices.first() {
                    let Some(updated) = sum.checked_add(yes_price.0) else {
                        violations.push(Violation {
                            kind: ViolationKind::SettlementOverflow,
                            details: format!(
                                "Market group {}: YES price sum overflowed",
                                group.name
                            ),
                        });
                        continue;
                    };
                    sum = updated;
                }
            }
        }
        if sum > NANOS_PER_DOLLAR {
            violations.push(Violation {
                kind: ViolationKind::MarketGroupConstraintViolation,
                details: format!(
                    "Group '{}': sum of YES prices = {} > {}",
                    group.name, sum, NANOS_PER_DOLLAR
                ),
            });
        }
    }
}

/// Check 15: No fills/orders on resolved markets.
fn verify_resolved_markets(
    witness: &BlockWitness,
    order_map: &HashMap<u64, &Order>,
    violations: &mut Vec<Violation>,
) {
    if witness.resolved_markets.is_empty() {
        return;
    }
    let resolved: HashSet<MarketId> = witness.resolved_markets.iter().copied().collect();

    for fill in &witness.fills {
        if fill.fill_qty == matching_engine::Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        for market in order.active_markets() {
            if resolved.contains(&market) {
                violations.push(Violation {
                    kind: ViolationKind::ResolvedMarketViolation,
                    details: format!(
                        "Fill for order {} references resolved market {:?}",
                        fill.order_id, market
                    ),
                });
            }
        }
    }
}

/// Check 17: Conditional order activation — clearing prices must satisfy condition.
fn verify_conditional_activation(
    witness: &BlockWitness,
    order_map: &HashMap<u64, &Order>,
    violations: &mut Vec<Violation>,
) {
    for fill in &witness.fills {
        if fill.fill_qty == matching_engine::Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        let Some(ref condition) = order.condition else {
            continue;
        };

        // Get the YES clearing price for the condition market
        let Some(prices) = witness.clearing_prices.get(&condition.market) else {
            // No clearing price — condition cannot be verified as satisfied
            violations.push(Violation {
                kind: ViolationKind::ConditionalActivationViolation,
                details: format!(
                    "Order {} has condition on market {:?} but no clearing price available",
                    fill.order_id, condition.market
                ),
            });
            continue;
        };

        let yes_price = prices
            .first()
            .copied()
            .unwrap_or(matching_engine::Nanos::ZERO);
        let condition_met = match condition.direction {
            matching_engine::ConditionDir::Above => yes_price > condition.threshold,
            matching_engine::ConditionDir::Below => yes_price < condition.threshold,
        };

        if !condition_met {
            violations.push(Violation {
                kind: ViolationKind::ConditionalActivationViolation,
                details: format!(
                    "Order {}: condition {:?} not met (clearing_price={}, threshold={})",
                    fill.order_id, condition.direction, yes_price, condition.threshold
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WitnessBlockHeader, WitnessOrder};
    use matching_engine::{
        gross_welfare_from_fills, minting_cost_from_fills, net_welfare, outcome_sell,
        shares_to_qty, simple_no_buy, simple_yes_buy, MarketSet, MmConstraint, MmId, MmSide, Nanos,
        Qty,
    };

    fn empty_header() -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            events_root: crate::event_commitment::empty_events_root(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
        }
    }

    fn make_witness(orders: Vec<WitnessOrder>, fills: Vec<Fill>) -> BlockWitness {
        let total_welfare = {
            let orders = orders.iter().map(|wo| &wo.order);
            gross_welfare_from_fills(orders, &fills)
        };

        BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders,
            rejections: vec![],
            system_events: vec![],
            l1_deposits: vec![],
            fills,
            clearing_prices: HashMap::new(),
            total_welfare,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        }
    }

    fn refresh_welfare(witness: &mut BlockWitness) {
        let orders: Vec<&Order> = witness.orders.iter().map(|wo| &wo.order).collect();
        let gross_welfare = gross_welfare_from_fills(orders.iter().copied(), &witness.fills);
        let minting_cost = minting_cost_from_fills(
            orders.iter().copied(),
            &witness.fills,
            &witness.clearing_prices,
        );
        witness.minting_cost = minting_cost;
        witness.total_welfare = net_welfare(gross_welfare, minting_cost);
    }

    fn buy_order(markets: &MarketSet, id: u64, market: MarketId) -> WitnessOrder {
        WitnessOrder {
            order: simple_yes_buy(markets, id, market, 600_000_000, shares_to_qty(100).0),
            account_id: 0,
            is_mm: false,
        }
    }

    fn sell_order(markets: &MarketSet, id: u64, market: MarketId) -> WitnessOrder {
        WitnessOrder {
            order: outcome_sell(markets, id, market, 0, 400_000_000, shares_to_qty(100).0),
            account_id: 1,
            is_mm: false,
        }
    }

    #[test]
    fn test_valid_fills() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Balanced: buyer buys 50 YES, seller sells 50 YES at same price
        let orders = vec![buy_order(&markets, 1, m0), sell_order(&markets, 2, m0)];
        let fills = vec![
            Fill::new(1, shares_to_qty(50), Nanos(500_000_000)),
            Fill::new(2, shares_to_qty(50), Nanos(500_000_000)),
        ];

        let mut witness = make_witness(orders, fills);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(500_000_000), Nanos(500_000_000)]);
        refresh_welfare(&mut witness);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_order_not_found() {
        let witness = make_witness(
            vec![],
            vec![Fill::new(999, shares_to_qty(50), Nanos(500_000_000))],
        );
        // Fix welfare since no orders exist
        let mut witness = witness;
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::OrderNotFound));
    }

    #[test]
    fn test_quantity_exceeds_max() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, shares_to_qty(200), Nanos(500_000_000))]; // max_fill=100 shares

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0; // welfare will be wrong due to overfill

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::QuantityExceedsMax));
    }

    #[test]
    fn test_duplicate_fill() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![
            Fill::new(1, shares_to_qty(50), Nanos(500_000_000)),
            Fill::new(1, shares_to_qty(30), Nanos(500_000_000)),
        ];

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::DuplicateFill));
    }

    #[test]
    fn test_price_exceeds_limit() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)]; // limit=600_000_000
        let fills = vec![Fill::new(1, shares_to_qty(50), Nanos(700_000_000))]; // above limit

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::PriceExceedsLimit));
    }

    #[test]
    fn limit_price_above_one_dollar_is_settlement_overflow() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let mut order = buy_order(&markets, 1, m0);
        order.order.limit_price = Nanos(NANOS_PER_DOLLAR + 1);
        let fills = vec![Fill::new(1, shares_to_qty(50), Nanos(500_000_000))];

        let mut witness = make_witness(vec![order], fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SettlementOverflow));
    }

    #[test]
    fn fill_price_above_one_dollar_is_settlement_overflow() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, shares_to_qty(50), Nanos(NANOS_PER_DOLLAR + 1))];

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SettlementOverflow));
    }

    #[test]
    fn test_mm_budget_exceeded() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0), buy_order(&markets, 2, m0)];
        let fills = vec![
            Fill::new(1, shares_to_qty(100), Nanos(500_000_000)),
            Fill::new(2, shares_to_qty(100), Nanos(500_000_000)),
        ];

        let mm = MmConstraint::new(MmId(1), Nanos(10_000_000_000)) // $10 budget
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes);

        let mut witness = make_witness(orders, fills);
        witness.mm_constraints = vec![mm];

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::MmBudgetExceeded));
    }

    #[test]
    fn test_duplicate_order_id() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![
            buy_order(&markets, 1, m0),
            buy_order(&markets, 1, m0), // duplicate
        ];

        let witness = make_witness(orders, vec![]);
        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::DuplicateOrderId));
    }

    #[test]
    fn test_price_complementarity_valid() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let mut witness = make_witness(vec![], vec![]);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(600_000_000), Nanos(400_000_000)]);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_price_complementarity_violated() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let mut witness = make_witness(vec![], vec![]);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(600_000_000), Nanos(500_000_000)]); // sum > $1

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::PriceComplementarityViolation));
    }

    #[test]
    fn test_zero_fill_diagnostic_mode() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, Qty(0), Nanos(500_000_000))];

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let core_only = verify_match(&witness, false);
        assert!(core_only.valid);

        let with_diagnostics = verify_match(&witness, true);
        assert!(!with_diagnostics.valid);
        assert!(with_diagnostics
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::ZeroQuantityFill));
    }

    #[test]
    fn test_position_balance_valid() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Balanced: 50 YES bought + 50 YES sold = net 0
        let orders = vec![buy_order(&markets, 1, m0), sell_order(&markets, 2, m0)];
        let fills = vec![
            Fill::new(1, shares_to_qty(50), Nanos(500_000_000)),
            Fill::new(2, shares_to_qty(50), Nanos(500_000_000)),
        ];

        let mut witness = make_witness(orders, fills);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(500_000_000), Nanos(500_000_000)]);
        refresh_welfare(&mut witness);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_match_layer_allows_mint_backed_position_imbalance() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // A one-sided buy creates position imbalance at the fill layer, but
        // this is valid when settlement derives the MINT counterparty.
        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, shares_to_qty(50), Nanos(500_000_000))];

        let mut witness = make_witness(orders, fills);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(500_000_000), Nanos(500_000_000)]);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_position_balance_minting() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Minting: buyer buys YES, another buyer buys NO → balanced (creates a complete set)
        let wo_yes = WitnessOrder {
            order: simple_yes_buy(&markets, 1, m0, 600_000_000, shares_to_qty(50).0),
            account_id: 0,
            is_mm: false,
        };
        let wo_no = WitnessOrder {
            order: simple_no_buy(&markets, 2, m0, 600_000_000, shares_to_qty(50).0),
            account_id: 1,
            is_mm: false,
        };

        let orders = vec![wo_yes, wo_no];
        let fills = vec![
            Fill::new(1, shares_to_qty(50), Nanos(500_000_000)),
            Fill::new(2, shares_to_qty(50), Nanos(500_000_000)),
        ];

        let mut witness = make_witness(orders, fills);
        witness
            .clearing_prices
            .insert(m0, vec![Nanos(500_000_000), Nanos(500_000_000)]);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }
}
