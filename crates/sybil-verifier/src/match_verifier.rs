//! Layer 1: Fill-level and market-level match verification.
//!
//! Checks that every fill is consistent with its order and that
//! market-level invariants (UCP, complementarity, balance) hold.

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketId, Order, NANOS_PER_DOLLAR};

use crate::types::BlockWitness;
use crate::violations::{
    VerificationResult, VerificationStats, Violation, ViolationKind,
};

/// Verify all fill-level and market-level invariants.
pub fn verify_match(witness: &BlockWitness, strict: bool) -> VerificationResult {
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
        strict,
        witness.total_welfare,
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
    verify_uniform_clearing_prices(witness, &order_map, &mut violations);
    verify_price_complementarity(witness, &mut violations);
    verify_quantity_balance(witness, &order_map, &mut violations);
    verify_cash_conservation(witness, &order_map, &mut violations);
    verify_market_group_constraints(witness, &mut violations);
    verify_resolved_markets(witness, &order_map, &mut violations);
    verify_conditional_activation(witness, &order_map, &mut violations);

    stats.reported_welfare = witness.total_welfare;

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
    strict: bool,
    reported_welfare: i64,
    violations: &mut Vec<Violation>,
    stats: &mut VerificationStats,
) {
    let mut filled_orders: HashSet<u64> = HashSet::new();
    let mut computed_welfare: i64 = 0;

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

        if fill.fill_qty > 0 && fill.fill_qty < order.min_fill {
            violations.push(Violation {
                kind: ViolationKind::QuantityBelowMin,
                details: format!(
                    "Order {}: fill_qty {} < min_fill {} (AON violation)",
                    fill.order_id, fill.fill_qty, order.min_fill
                ),
            });
        }

        // 4. Zero fill (strict mode)
        if fill.fill_qty == 0 && strict {
            violations.push(Violation {
                kind: ViolationKind::ZeroQuantityFill,
                details: format!("Order {}: zero quantity fill", fill.order_id),
            });
        }

        // 5. Price constraint (seller-aware)
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
        let fill_welfare = fill.welfare(order);
        if fill_welfare < 0 {
            violations.push(Violation {
                kind: ViolationKind::NegativeWelfare,
                details: format!(
                    "Order {}: negative welfare {} (limit={}, fill_price={}, qty={})",
                    fill.order_id, fill_welfare, order.limit_price, fill.fill_price, fill.fill_qty
                ),
            });
        }
        computed_welfare += fill_welfare;
    }

    stats.orders_checked = order_map.len();
    stats.computed_welfare = computed_welfare;

    // 7. Welfare consistency
    let welfare_tolerance: i64 = if strict { 0 } else { 1_000 };
    let welfare_diff = (computed_welfare - reported_welfare).abs();
    if welfare_diff > welfare_tolerance {
        violations.push(Violation {
            kind: ViolationKind::WelfareMismatch,
            details: format!(
                "Computed welfare {} != reported welfare {} (diff={})",
                computed_welfare, reported_welfare, welfare_diff
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

        let mut mm_fills: HashMap<u64, (u64, u64)> = HashMap::new();
        for &order_id in &mm.order_ids {
            if let Some(fill) = fill_map.get(&order_id) {
                if fill.fill_qty > 0 {
                    mm_fills.insert(order_id, (fill.fill_price, fill.fill_qty));
                }
            }
        }

        let capital_used = mm.capital_used(&mm_fills);
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
        if fill.fill_qty == 0 {
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
        if prices.len() == 2 {
            let sum = prices[0] + prices[1];
            if sum != NANOS_PER_DOLLAR {
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

/// Check 12: Quantity balance — per market/outcome, total bought == total sold.
fn verify_quantity_balance(
    witness: &BlockWitness,
    order_map: &HashMap<u64, &Order>,
    violations: &mut Vec<Violation>,
) {
    // Track net position change per (market, outcome)
    let mut net_positions: HashMap<(MarketId, u8), i64> = HashMap::new();

    for fill in &witness.fills {
        if fill.fill_qty == 0 {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        let num_markets = order.num_markets as usize;
        let num_states = order.num_states as usize;

        if num_markets == 1 && num_states == 2 {
            let market = order.markets[0];
            let yes_payoff = order.payoffs[0] as i64;
            let no_payoff = order.payoffs[1] as i64;

            if yes_payoff != 0 {
                *net_positions.entry((market, 0)).or_insert(0) +=
                    yes_payoff * fill.fill_qty as i64;
            }
            if no_payoff != 0 {
                *net_positions.entry((market, 1)).or_insert(0) +=
                    no_payoff * fill.fill_qty as i64;
            }
        } else {
            // Multi-market: compute marginal position per market per outcome
            for m_idx in 0..num_markets {
                let market = order.markets[m_idx];
                let stride = 1usize << m_idx;

                let mut yes_sum: i64 = 0;
                let mut yes_count: usize = 0;
                let mut no_sum: i64 = 0;
                let mut no_count: usize = 0;

                for s in 0..num_states {
                    let outcome = (s / stride) % 2;
                    let payoff = order.payoffs[s] as i64;
                    if outcome == 0 {
                        yes_sum += payoff;
                        yes_count += 1;
                    } else {
                        no_sum += payoff;
                        no_count += 1;
                    }
                }

                if yes_count > 0 && yes_sum != 0 {
                    *net_positions.entry((market, 0)).or_insert(0) +=
                        yes_sum * fill.fill_qty as i64 / yes_count as i64;
                }
                if no_count > 0 && no_sum != 0 {
                    *net_positions.entry((market, 1)).or_insert(0) +=
                        no_sum * fill.fill_qty as i64 / no_count as i64;
                }
            }
        }
    }

    for (&(market, outcome), &net) in &net_positions {
        if net != 0 {
            violations.push(Violation {
                kind: ViolationKind::QuantityBalanceViolation,
                details: format!(
                    "Market {:?} outcome {}: net position change = {} (expected 0)",
                    market, outcome, net
                ),
            });
        }
    }
}

/// Check 13: Cash conservation — net cash flow across all fills == 0.
fn verify_cash_conservation(
    witness: &BlockWitness,
    order_map: &HashMap<u64, &Order>,
    violations: &mut Vec<Violation>,
) {
    let mut net_cash: i128 = 0;

    for fill in &witness.fills {
        if fill.fill_qty == 0 {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        // Each fill: buyer pays fill_price * fill_qty, seller receives it.
        // Net cash change for this order = -fill_price * fill_qty for buyers,
        // +fill_price * fill_qty for sellers.
        // The total across all fills should be zero.
        let cost = fill.fill_price as i128 * fill.fill_qty as i128;
        if order.is_seller() {
            // Seller receives (settle_fill adds to balance)
            // Actually in settle_generic: balance -= fill_price * fill_qty for everyone
            // So net is always negative. Let's think about this differently:
            // In settlement, every order debits balance by fill_price * fill_qty.
            // So the net cash is just the sum of all debits.
            // But that's not zero — positions are what balance it.
            //
            // Cash conservation means: sum of (fill_price * fill_qty) for buys ==
            // sum of (fill_price * fill_qty) for sells, when fills are matched.
            //
            // Actually, in the settlement model, EVERY fill debits fill_price * fill_qty
            // and credits positions. The cash conservation is actually about the
            // balance change being consistent. Let's skip this for now if the
            // settlement verification handles it.
        }

        // Simpler formulation: net cash flow = sum over all fills of
        // (fill_price * fill_qty) where buyer is negative and seller is positive.
        if order.is_seller() {
            net_cash += cost;
        } else {
            net_cash -= cost;
        }
    }

    if net_cash != 0 {
        violations.push(Violation {
            kind: ViolationKind::CashConservationViolation,
            details: format!("Net cash flow across all fills = {} (expected 0)", net_cash),
        });
    }
}

/// Check 14: Market group constraint — sum of YES clearing prices <= $1.
fn verify_market_group_constraints(witness: &BlockWitness, violations: &mut Vec<Violation>) {
    for group in &witness.market_groups {
        let mut sum: u64 = 0;
        for &market in &group.markets {
            if let Some(prices) = witness.clearing_prices.get(&market) {
                if let Some(&yes_price) = prices.first() {
                    sum += yes_price;
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
        if fill.fill_qty == 0 {
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
        if fill.fill_qty == 0 {
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

        let yes_price = prices.first().copied().unwrap_or(0);
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
    use matching_engine::{outcome_sell, simple_yes_buy, MarketSet, MmConstraint, MmId, MmSide};

    fn empty_header() -> WitnessBlockHeader {
        WitnessBlockHeader {
            height: 1,
            parent_hash: [0u8; 32],
            state_root: [0u8; 32],
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
        }
    }

    fn make_witness(orders: Vec<WitnessOrder>, fills: Vec<Fill>) -> BlockWitness {
        let total_welfare = {
            let order_map: HashMap<u64, &Order> =
                orders.iter().map(|wo| (wo.order.id, &wo.order)).collect();
            fills.iter().map(|f| {
                order_map.get(&f.order_id)
                    .map(|o| f.welfare(o))
                    .unwrap_or(0)
            }).sum()
        };

        BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders,
            rejections: vec![],
            fills,
            clearing_prices: HashMap::new(),
            total_welfare,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_state: vec![],
            resolved_markets: vec![],
        }
    }

    fn buy_order(markets: &MarketSet, id: u64, market: MarketId) -> WitnessOrder {
        WitnessOrder {
            order: simple_yes_buy(markets, id, market, 600_000_000, 100),
            account_id: 0,
            is_mm: false,
        }
    }

    fn sell_order(markets: &MarketSet, id: u64, market: MarketId) -> WitnessOrder {
        WitnessOrder {
            order: outcome_sell(markets, id, market, 0, 400_000_000, 100),
            account_id: 1,
            is_mm: false,
        }
    }

    #[test]
    fn test_valid_fills() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Balanced: buyer buys 50 YES, seller sells 50 YES at same price
        let orders = vec![
            buy_order(&markets, 1, m0),
            sell_order(&markets, 2, m0),
        ];
        let fills = vec![
            Fill::new(1, 50, 500_000_000),
            Fill::new(2, 50, 500_000_000),
        ];

        let mut witness = make_witness(orders, fills);
        witness.clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_order_not_found() {
        let witness = make_witness(
            vec![],
            vec![Fill::new(999, 50, 500_000_000)],
        );
        // Fix welfare since no orders exist
        let mut witness = witness;
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::OrderNotFound));
    }

    #[test]
    fn test_quantity_exceeds_max() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, 200, 500_000_000)]; // max_fill=100

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0; // welfare will be wrong due to overfill

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::QuantityExceedsMax));
    }

    #[test]
    fn test_duplicate_fill() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![
            Fill::new(1, 50, 500_000_000),
            Fill::new(1, 30, 500_000_000),
        ];

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::DuplicateFill));
    }

    #[test]
    fn test_price_exceeds_limit() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)]; // limit=600_000_000
        let fills = vec![Fill::new(1, 50, 700_000_000)]; // above limit

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::PriceExceedsLimit));
    }

    #[test]
    fn test_mm_budget_exceeded() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![
            buy_order(&markets, 1, m0),
            buy_order(&markets, 2, m0),
        ];
        let fills = vec![
            Fill::new(1, 100, 500_000_000),
            Fill::new(2, 100, 500_000_000),
        ];

        let mm = MmConstraint::new(MmId(1), 10_000_000_000) // $10 budget
            .with_order(1, MmSide::SellYes)
            .with_order(2, MmSide::SellYes);

        let mut witness = make_witness(orders, fills);
        witness.mm_constraints = vec![mm];

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::MmBudgetExceeded));
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
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::DuplicateOrderId));
    }

    #[test]
    fn test_price_complementarity_valid() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let mut witness = make_witness(vec![], vec![]);
        witness.clearing_prices.insert(m0, vec![600_000_000, 400_000_000]);

        let result = verify_match(&witness, false);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_price_complementarity_violated() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let mut witness = make_witness(vec![], vec![]);
        witness.clearing_prices.insert(m0, vec![600_000_000, 500_000_000]); // sum > $1

        let result = verify_match(&witness, false);
        assert!(!result.valid);
        assert!(result.violations.iter().any(|v| v.kind == ViolationKind::PriceComplementarityViolation));
    }

    #[test]
    fn test_zero_fill_strict_mode() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let orders = vec![buy_order(&markets, 1, m0)];
        let fills = vec![Fill::new(1, 0, 500_000_000)];

        let mut witness = make_witness(orders, fills);
        witness.total_welfare = 0;

        let lenient = verify_match(&witness, false);
        assert!(lenient.valid);

        let strict = verify_match(&witness, true);
        assert!(!strict.valid);
        assert!(strict.violations.iter().any(|v| v.kind == ViolationKind::ZeroQuantityFill));
    }
}
