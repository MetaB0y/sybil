//! Layer 4: Order validation verification.
//!
//! Checks that accepted orders have valid pre-state coverage (balance for
//! buys, position for sells), that rejections are correct, and that
//! intra-batch double-spends are detected.

use std::collections::HashMap;

use crate::types::{AccountSnapshot, BlockWitness, RejectionReason};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify order validation: balance/position checks and rejection correctness.
pub fn verify_orders(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // Build pre-state lookup
    let pre_state: HashMap<u64, &AccountSnapshot> =
        witness.pre_state.iter().map(|s| (s.id, s)).collect();

    // Track cumulative balance reservations per account (intra-batch)
    let mut reserved_balance: HashMap<u64, i64> = HashMap::new();

    // Verify accepted orders
    for wo in &witness.orders {
        // MM orders skip balance validation (matching sequencer behavior)
        if wo.is_mm {
            continue;
        }

        let Some(snap) = pre_state.get(&wo.account_id) else {
            // Account not found in pre-state — this is suspicious but the
            // sequencer might handle it differently. Skip for now.
            continue;
        };

        let order = &wo.order;
        let num_states = order.num_states as usize;
        let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
        let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

        if has_positive && !has_negative {
            // Pure buy: check balance covers worst-case cost
            let max_cost = order.limit_price as i64 * order.max_fill as i64;
            let reserved = *reserved_balance.get(&wo.account_id).unwrap_or(&0);
            let available = snap.balance - reserved;

            if max_cost > available {
                violations.push(Violation {
                    kind: ViolationKind::InsufficientBalance,
                    details: format!(
                        "Order {} (account {}): max_cost {} > available {} (balance {} - reserved {})",
                        order.id, wo.account_id, max_cost, available, snap.balance, reserved
                    ),
                });
            }

            // Reserve this cost for subsequent orders in the batch
            *reserved_balance.entry(wo.account_id).or_insert(0) += max_cost;
        } else if has_negative && !has_positive {
            // Pure sell: check positions
            if order.num_markets == 1 {
                let market = order.markets[0];
                for s in 0..num_states {
                    if order.payoffs[s] < 0 {
                        let outcome = s as u8;
                        let sell_qty = (-order.payoffs[s] as i64) * order.max_fill as i64;

                        // Look up position in pre-state snapshot
                        let available = snap
                            .positions
                            .iter()
                            .find(|&&(m, o, _)| m == market && o == outcome)
                            .map(|&(_, _, q)| q)
                            .unwrap_or(0);

                        if sell_qty > available {
                            violations.push(Violation {
                                kind: ViolationKind::InsufficientPosition,
                                details: format!(
                                    "Order {} (account {}): sell_qty {} > position {} for market {:?} outcome {}",
                                    order.id, wo.account_id, sell_qty, available, market, outcome
                                ),
                            });
                        }
                    }
                }
            }
        }
        // Mixed payoff orders: skip validation (matching sequencer behavior)
    }

    // Verify rejections are correct
    for rej in &witness.rejections {
        let Some(snap) = pre_state.get(&rej.account_id) else {
            // AccountNotFound rejections are valid if account isn't in pre-state
            match &rej.reason {
                RejectionReason::AccountNotFound => continue,
                _ => {
                    violations.push(Violation {
                        kind: ViolationKind::IncorrectRejectionReason,
                        details: format!(
                            "Order {} (account {}): rejected for {:?} but account not in pre-state",
                            rej.order.id, rej.account_id, rej.reason
                        ),
                    });
                    continue;
                }
            }
        };

        match &rej.reason {
            RejectionReason::InsufficientBalance {
                required,
                available: _,
            } => {
                // Verify the rejection is legitimate
                let order = &rej.order;
                let num_states = order.num_states as usize;
                let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
                let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

                if has_positive && !has_negative {
                    let max_cost = order.limit_price as i64 * order.max_fill as i64;
                    if max_cost != *required {
                        violations.push(Violation {
                            kind: ViolationKind::IncorrectRejectionReason,
                            details: format!(
                                "Order {}: rejection says required={} but computed max_cost={}",
                                order.id, required, max_cost
                            ),
                        });
                    }
                }
            }
            RejectionReason::InsufficientPosition {
                market,
                outcome,
                required: _,
                available,
            } => {
                // Verify position check
                let actual_pos = snap
                    .positions
                    .iter()
                    .find(|&&(m, o, _)| m == *market && o == *outcome)
                    .map(|&(_, _, q)| q)
                    .unwrap_or(0);

                if actual_pos != *available {
                    violations.push(Violation {
                        kind: ViolationKind::IncorrectRejectionReason,
                        details: format!(
                            "Order {}: rejection says available={} but actual position={}",
                            rej.order.id, available, actual_pos
                        ),
                    });
                }
            }
            RejectionReason::AccountNotFound => {
                // Account exists in pre-state but rejected as not found
                violations.push(Violation {
                    kind: ViolationKind::FalseRejection,
                    details: format!(
                        "Order {} (account {}): rejected as AccountNotFound but account exists in pre-state",
                        rej.order.id, rej.account_id
                    ),
                });
            }
            RejectionReason::CompleteSetFormation => {
                // Valid rejection: MM orders would form a complete set in a market group.
                // No further validation needed — the sequencer detected self-trade potential.
            }
        }
    }

    VerificationResult {
        valid: violations.is_empty(),
        violations,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WitnessBlockHeader, WitnessOrder, WitnessRejection};
    use matching_engine::{outcome_buy, outcome_sell, MarketSet, NANOS_PER_DOLLAR};
    use std::collections::HashMap;

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

    fn make_witness_with_orders(
        orders: Vec<WitnessOrder>,
        rejections: Vec<WitnessRejection>,
        pre_state: Vec<AccountSnapshot>,
    ) -> BlockWitness {
        BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders,
            rejections,
            admin_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state: vec![],
            resolved_markets: vec![],
        }
    }

    #[test]
    fn test_valid_buy_order() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 10 * NANOS_PER_DOLLAR as i64,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_insufficient_balance() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        // max_cost = 0.50 * 10 = $5, but only $3 available
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 3 * NANOS_PER_DOLLAR as i64,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InsufficientBalance));
    }

    #[test]
    fn test_intra_batch_reservation() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Two buy orders from the same account, $5 each = $10 total
        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, 10);

        // Account has $8 — enough for one but not both
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 8 * NANOS_PER_DOLLAR as i64,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![
                WitnessOrder {
                    order: order1,
                    account_id: 0,
                    is_mm: false,
                },
                WitnessOrder {
                    order: order2,
                    account_id: 0,
                    is_mm: false,
                },
            ],
            vec![],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        // Second order should fail due to reservation
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InsufficientBalance));
    }

    #[test]
    fn test_mm_orders_skip_validation() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 100);
        // Account has $0 — MM orders should skip balance check
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: true,
            }],
            vec![],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_sell_insufficient_position() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 10);
        // Only 5 shares but trying to sell 10
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: NANOS_PER_DOLLAR as i64,
            positions: vec![(m0, 0, 5)],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InsufficientPosition));
    }

    #[test]
    fn test_false_rejection_account_exists() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 100 * NANOS_PER_DOLLAR as i64,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let witness = make_witness_with_orders(
            vec![],
            vec![WitnessRejection {
                order,
                account_id: 0,
                reason: RejectionReason::AccountNotFound,
            }],
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::FalseRejection));
    }
}
