//! Layer 4: Order validation verification.
//!
//! Checks that accepted orders have valid post-system coverage (balance for
//! buys, position for sells), that rejections are correct, and that
//! intra-batch double-spends are detected.

use std::collections::HashMap;

use matching_engine::NANOS_PER_DOLLAR;

use crate::arithmetic::{checked_position_qty, checked_price_qty_ceil};
use crate::types::{AccountSnapshot, BlockWitness, RejectionReason};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify order validation: balance/position checks and rejection correctness.
pub fn verify_orders(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let stats = VerificationStats::default();

    // Build post-system-state lookup. Orders are validated after system
    // events for the block have been applied, but before any fills settle.
    let post_system_state: HashMap<u64, &AccountSnapshot> = witness
        .post_system_state
        .iter()
        .map(|s| (s.id, s))
        .collect();

    // Track cumulative balance reservations per account (intra-batch)
    let mut reserved_balance: HashMap<u64, i64> = HashMap::new();

    // Verify accepted orders
    for wo in &witness.orders {
        let order = &wo.order;
        if order.limit_price.0 > NANOS_PER_DOLLAR {
            violations.push(Violation {
                kind: ViolationKind::SettlementOverflow,
                details: format!(
                    "Order {}: limit_price {} exceeds NANOS_PER_DOLLAR {}",
                    order.id, order.limit_price, NANOS_PER_DOLLAR
                ),
            });
            continue;
        }

        if let Err(reason) = order.validate_binary_one_hot() {
            violations.push(Violation {
                kind: ViolationKind::InvalidOrder,
                details: format!("Order {}: invalid order shape: {}", order.id, reason),
            });
            continue;
        }

        if let Some(expires_at_block) = order.expires_at_block {
            if expires_at_block < witness.header.height {
                violations.push(Violation {
                    kind: ViolationKind::OrderExpiryViolation,
                    details: format!(
                        "Order {}: expires_at_block {} < block height {}",
                        order.id, expires_at_block, witness.header.height
                    ),
                });
            }
        }

        // MM orders skip balance validation (matching sequencer behavior)
        if wo.is_mm {
            continue;
        }

        let Some(snap) = post_system_state.get(&wo.account_id) else {
            violations.push(Violation {
                kind: ViolationKind::AcceptedOrderMissingAccount,
                details: format!(
                    "Order {} (account {}): accepted order missing from post-system state",
                    wo.order.id, wo.account_id
                ),
            });
            continue;
        };

        let num_states = order.num_states as usize;
        let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
        let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

        if has_positive && !has_negative {
            // Pure buy: check balance covers worst-case cost
            let Some(max_cost) = checked_price_qty_ceil(order.limit_price, order.max_fill) else {
                violations.push(Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details: format!("Order {}: price*quantity overflow", order.id),
                });
                continue;
            };
            let reserved = *reserved_balance.get(&wo.account_id).unwrap_or(&0);
            let Some(available) = snap.balance.checked_sub(reserved) else {
                violations.push(Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details: format!(
                        "Order {} (account {}): balance {} - reserved {} overflowed",
                        order.id, wo.account_id, snap.balance, reserved
                    ),
                });
                continue;
            };

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
            let entry = reserved_balance.entry(wo.account_id).or_insert(0);
            let Some(updated) = entry.checked_add(max_cost) else {
                violations.push(Violation {
                    kind: ViolationKind::SettlementOverflow,
                    details: format!(
                        "Order {} (account {}): reserved balance {} + max_cost {} overflowed",
                        order.id, wo.account_id, *entry, max_cost
                    ),
                });
                continue;
            };
            *entry = updated;
        } else if has_negative && !has_positive {
            // Pure sell: check positions
            if order.num_markets == 1 {
                let market = order.markets[0];
                for s in 0..num_states {
                    if order.payoffs[s] < 0 {
                        let outcome = s as u8;
                        let Some(raw_sell_qty) =
                            checked_position_qty(order.payoffs[s], order.max_fill)
                        else {
                            violations.push(Violation {
                                kind: ViolationKind::SettlementOverflow,
                                details: format!("Order {}: sell quantity overflow", order.id),
                            });
                            continue;
                        };
                        let Some(sell_qty) = raw_sell_qty.checked_neg() else {
                            violations.push(Violation {
                                kind: ViolationKind::SettlementOverflow,
                                details: format!("Order {}: sell quantity overflow", order.id),
                            });
                            continue;
                        };

                        // Look up position in post-system-state snapshot
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
        let Some(snap) = post_system_state.get(&rej.account_id) else {
            // AccountNotFound rejections are valid if account isn't in post-system state
            match &rej.reason {
                RejectionReason::AccountNotFound => continue,
                _ => {
                    violations.push(Violation {
                        kind: ViolationKind::IncorrectRejectionReason,
                        details: format!(
                            "Order {} (account {}): rejected for {:?} but account not in post-system state",
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
                    let Some(max_cost) = checked_price_qty_ceil(order.limit_price, order.max_fill)
                    else {
                        violations.push(Violation {
                            kind: ViolationKind::SettlementOverflow,
                            details: format!("Order {}: price*quantity overflow", order.id),
                        });
                        continue;
                    };
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
                // Account exists in post-system state but rejected as not found
                violations.push(Violation {
                    kind: ViolationKind::FalseRejection,
                    details: format!(
                        "Order {} (account {}): rejected as AccountNotFound but account exists in post-system state",
                        rej.order.id, rej.account_id
                    ),
                });
            }
            RejectionReason::CompleteSetFormation => {
                // Valid rejection: MM orders would form a complete set in a market group.
                // No further validation needed — the sequencer detected self-trade potential.
            }
            RejectionReason::InvalidOrder(reason) => {
                if rej.order.validate_binary_one_hot().is_ok() {
                    violations.push(Violation {
                        kind: ViolationKind::FalseRejection,
                        details: format!(
                            "Order {}: rejected as invalid ({}) but shape is supported",
                            rej.order.id, reason
                        ),
                    });
                }
            }
            RejectionReason::Expired {
                current_block,
                expires_at_block,
            } => {
                if current_block <= expires_at_block {
                    violations.push(Violation {
                        kind: ViolationKind::IncorrectRejectionReason,
                        details: format!(
                            "Order {}: rejected as expired but current_block {} <= expires_at_block {}",
                            rej.order.id, current_block, expires_at_block
                        ),
                    });
                }
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
    use matching_engine::{
        notional_nanos_ceil, outcome_buy, outcome_sell, shares_to_qty, MarketSet, Nanos, Qty,
        NANOS_PER_DOLLAR,
    };
    use proptest::prelude::*;
    use std::collections::HashMap;

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

    fn make_witness_with_orders(
        orders: Vec<WitnessOrder>,
        rejections: Vec<WitnessRejection>,
        pre_state: Vec<AccountSnapshot>,
        post_system_state: Vec<AccountSnapshot>,
    ) -> BlockWitness {
        BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders,
            rejections,
            system_events: vec![],
            deposit_accumulator: crate::DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_system_state,
            post_state: vec![],
            account_keys: vec![],
            state_sidecar: Default::default(),

            pre_state_sidecar: Default::default(),

            resolved_markets: vec![],
        }
    }

    #[test]
    fn test_valid_buy_order() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, shares_to_qty(10).0);
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 10 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state.clone(),
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn accepted_expired_order_is_violation() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let mut order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 1);
        order.expires_at_block = Some(0);
        let account = AccountSnapshot {
            id: 0,
            balance: 10 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        };

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            vec![account.clone()],
            vec![account],
        );

        let result = verify_orders(&witness);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::OrderExpiryViolation));
    }

    #[test]
    fn expired_rejection_is_valid_only_after_expiry() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 1);
        let account = AccountSnapshot {
            id: 0,
            balance: 10 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        };

        let valid = make_witness_with_orders(
            vec![],
            vec![WitnessRejection {
                order: order.clone(),
                account_id: 0,
                reason: RejectionReason::Expired {
                    current_block: 2,
                    expires_at_block: 1,
                },
            }],
            vec![account.clone()],
            vec![account.clone()],
        );
        assert!(verify_orders(&valid).valid);

        let invalid = make_witness_with_orders(
            vec![],
            vec![WitnessRejection {
                order,
                account_id: 0,
                reason: RejectionReason::Expired {
                    current_block: 1,
                    expires_at_block: 1,
                },
            }],
            vec![account.clone()],
            vec![account],
        );
        assert!(verify_orders(&invalid)
            .violations
            .iter()
            .any(|v| { v.kind == ViolationKind::IncorrectRejectionReason }));
    }

    #[test]
    fn test_insufficient_balance() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, shares_to_qty(10).0);
        // max_cost = 0.50 * 10 = $5, but only $3 available
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 3 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state.clone(),
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
        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, shares_to_qty(10).0);
        let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, shares_to_qty(10).0);

        // Account has $8 — enough for one but not both
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 8 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
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
            pre_state.clone(),
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
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: true,
            }],
            vec![],
            pre_state.clone(),
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_validates_against_post_system_state() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let post_system_state = vec![AccountSnapshot {
            id: 0,
            balance: 10 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            vec![],
            post_system_state,
        );

        let result = verify_orders(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_missing_post_system_account_is_violation_for_accepted_order() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 42,
                is_mm: false,
            }],
            vec![],
            vec![],
            vec![],
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::AcceptedOrderMissingAccount));
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
            total_deposited: 0,
            positions: vec![(m0, 0, 5)],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            vec![],
            pre_state.clone(),
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
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
        }];

        let witness = make_witness_with_orders(
            vec![],
            vec![WitnessRejection {
                order,
                account_id: 0,
                reason: RejectionReason::AccountNotFound,
            }],
            pre_state.clone(),
            pre_state,
        );

        let result = verify_orders(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::FalseRejection));
    }

    proptest! {
        #[test]
        fn prop_buy_validation_is_monotone_in_balance(
            low_balance in 0i64..=5_000_000_000,
            extra_balance in 0i64..=5_000_000_000,
            limit_price in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            max_fill in 1u64..=10,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let order = outcome_buy(&markets, 1, m0, 0, limit_price, max_fill);

            let low_state = vec![AccountSnapshot {
                id: 0,
                balance: low_balance,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];
            let high_state = vec![AccountSnapshot {
                id: 0,
                balance: low_balance.saturating_add(extra_balance).saturating_add(1),
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];

            let low_result = verify_orders(&make_witness_with_orders(
                vec![WitnessOrder { order: order.clone(), account_id: 0, is_mm: false }],
                vec![],
                low_state.clone(),
                low_state,
            ));
            let high_result = verify_orders(&make_witness_with_orders(
                vec![WitnessOrder { order, account_id: 0, is_mm: false }],
                vec![],
                high_state.clone(),
                high_state,
            ));

            prop_assert!(!low_result.valid || high_result.valid);
        }

        #[test]
        fn prop_sell_validation_is_monotone_in_position(
            position in 0i64..=20,
            extra_position in 0i64..=20,
            max_fill in 1u64..=10,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, max_fill);

            let low_state = vec![AccountSnapshot {
                id: 0,
                balance: NANOS_PER_DOLLAR as i64,
                total_deposited: 0,
                positions: vec![(m0, 0, position)],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];
            let high_state = vec![AccountSnapshot {
                id: 0,
                balance: NANOS_PER_DOLLAR as i64,
                total_deposited: 0,
                positions: vec![(m0, 0, position + extra_position + 1)],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];

            let low_result = verify_orders(&make_witness_with_orders(
                vec![WitnessOrder { order: order.clone(), account_id: 0, is_mm: false }],
                vec![],
                low_state.clone(),
                low_state,
            ));
            let high_result = verify_orders(&make_witness_with_orders(
                vec![WitnessOrder { order, account_id: 0, is_mm: false }],
                vec![],
                high_state.clone(),
                high_state,
            ));

            prop_assert!(!low_result.valid || high_result.valid);
        }

        #[test]
        fn prop_reservation_can_only_reduce_later_buy_capacity(
            limit_price_1 in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            limit_price_2 in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            max_fill_1 in 1u64..=5,
            max_fill_2 in 1u64..=5,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let order1 = outcome_buy(&markets, 1, m0, 0, limit_price_1, max_fill_1);
            let order2 = outcome_buy(&markets, 2, m0, 0, limit_price_2, max_fill_2);

            let cost1 = notional_nanos_ceil(Nanos(limit_price_1), Qty(max_fill_1)).0 as i64;
            let cost2 = notional_nanos_ceil(Nanos(limit_price_2), Qty(max_fill_2)).0 as i64;
            let state = vec![AccountSnapshot {
                id: 0,
                balance: cost1 + cost2 - 1,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];

            let combined_result = verify_orders(&make_witness_with_orders(
                vec![
                    WitnessOrder { order: order1, account_id: 0, is_mm: false },
                    WitnessOrder { order: order2.clone(), account_id: 0, is_mm: false },
                ],
                vec![],
                state.clone(),
                state.clone(),
            ));
            let second_only_result = verify_orders(&make_witness_with_orders(
                vec![WitnessOrder { order: order2, account_id: 0, is_mm: false }],
                vec![],
                state.clone(),
                state,
            ));

            prop_assert!(second_only_result.valid);
            prop_assert!(combined_result.violations.iter().any(|v| v.kind == ViolationKind::InsufficientBalance));
        }

        #[test]
        fn prop_insufficient_position_rejection_metadata_matches_snapshot(
            available in 0i64..=20,
            max_fill in 1u64..=10,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, max_fill);
            let state = vec![AccountSnapshot {
                id: 0,
                balance: NANOS_PER_DOLLAR as i64,
                total_deposited: 0,
                positions: vec![(m0, 0, available)],
                events_digest: [0u8; 32],
            keys_digest: crate::empty_account_keys_digest(0),
            }];

            let result = verify_orders(&make_witness_with_orders(
                vec![],
                vec![WitnessRejection {
                    order,
                    account_id: 0,
                    reason: RejectionReason::InsufficientPosition {
                        market: m0,
                        outcome: 0,
                        required: max_fill as i64,
                        available,
                    },
                }],
                state.clone(),
                state,
            ));

            prop_assert!(result.valid, "violations: {:?}", result.violations);
        }
    }
}
