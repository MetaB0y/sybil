//! Layer 2: Settlement verification.
//!
//! Re-derives post-state from pre-state + fills and compares against
//! the claimed post-state. Uses the same settlement logic as the sequencer.

use std::collections::HashMap;

use matching_engine::{compute_fill_settlement, derive_minting, MarketId, Order};

use crate::types::{AccountSnapshot, BlockWitness};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

/// Verify that `pre_state + fills → post_state`.
pub fn verify_settlement(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let mut stats = VerificationStats::default();

    // Build order map
    let order_map: HashMap<u64, &Order> = witness
        .orders
        .iter()
        .map(|wo| (wo.order.id, &wo.order))
        .collect();

    // Build order→account mapping
    let order_account: HashMap<u64, u64> = witness
        .orders
        .iter()
        .map(|wo| (wo.order.id, wo.account_id))
        .collect();

    // Clone pre-state into working state
    let mut balances: HashMap<u64, i64> = HashMap::new();
    let mut positions: HashMap<u64, HashMap<(MarketId, u8), i64>> = HashMap::new();

    for snap in &witness.pre_state {
        balances.insert(snap.id, snap.balance);
        let mut pos_map: HashMap<(MarketId, u8), i64> = HashMap::new();
        for &(market, outcome, qty) in &snap.positions {
            pos_map.insert((market, outcome), qty);
        }
        positions.insert(snap.id, pos_map);
        stats.accounts_checked += 1;
    }

    // Apply each fill using the shared settlement function
    for fill in &witness.fills {
        if fill.fill_qty == 0 {
            continue;
        }

        let Some(&account_id) = order_account.get(&fill.order_id) else {
            continue;
        };
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        // Ensure account exists in our working state
        balances.entry(account_id).or_insert(0);
        positions.entry(account_id).or_default();

        if let Some(delta) = compute_fill_settlement(order, fill) {
            *balances.get_mut(&account_id).unwrap() += delta.balance_delta;
            let pos = positions.get_mut(&account_id).unwrap();
            for (market, outcome, qty_delta) in delta.position_deltas {
                *pos.entry((market, outcome)).or_insert(0) += qty_delta;
            }
        }
    }

    // Derive minting adjustments — shared pure function from matching-engine.
    //
    // Proof that MINT expected P&L = 0: see design/mint-pnl.typ and
    // lean/FisherClearing/Duality/MintingSimplex.lean (Theorem 1).
    {
        const MINT_ID: u64 = u64::MAX;

        let mint_in_witness = witness.pre_state.iter().any(|s| s.id == MINT_ID)
            || witness.post_state.iter().any(|s| s.id == MINT_ID);

        if mint_in_witness {
            // Collect all markets with any positions
            let all_markets: std::collections::HashSet<MarketId> = positions
                .values()
                .flat_map(|pm| pm.keys().map(|(m, _)| *m))
                .collect();

            let market_totals: Vec<(MarketId, i64, i64)> = all_markets
                .iter()
                .map(|&market_id| {
                    let total_yes: i64 = positions
                        .values()
                        .map(|pm| pm.get(&(market_id, 0)).copied().unwrap_or(0))
                        .sum();
                    let total_no: i64 = positions
                        .values()
                        .map(|pm| pm.get(&(market_id, 1)).copied().unwrap_or(0))
                        .sum();
                    (market_id, total_yes, total_no)
                })
                .collect();

            let adjustments = derive_minting(&market_totals, &witness.clearing_prices);

            if !adjustments.is_empty() {
                balances.entry(MINT_ID).or_insert(0);
                positions.entry(MINT_ID).or_default();

                // Check for missing clearing prices (balance_delta == 0 with non-zero position)
                for adj in &adjustments {
                    if adj.balance_delta == 0 {
                        let side = if adj.outcome == 0 { "YES" } else { "NO" };
                        violations.push(Violation {
                            kind: ViolationKind::MintingWithoutClearingPrice,
                            details: format!(
                                "Market {:?}: position imbalance {} but no {} clearing price",
                                adj.market_id, adj.position_delta.abs(), side
                            ),
                        });
                    }
                }

                let mint_balance = balances.get_mut(&MINT_ID).unwrap();
                let mint_positions = positions.get_mut(&MINT_ID).unwrap();
                for adj in &adjustments {
                    *mint_positions
                        .entry((adj.market_id, adj.outcome))
                        .or_insert(0) += adj.position_delta;
                    *mint_balance += adj.balance_delta;
                }
            }
        }
    }

    // Non-negative balance/position assertions (ZK invariants).
    // MINT (u64::MAX) is exempt — it holds short positions by design.
    const MINT_ID: u64 = u64::MAX;
    for (&account_id, &balance) in &balances {
        if balance < 0 && account_id != MINT_ID {
            violations.push(Violation {
                kind: ViolationKind::NegativeBalance,
                details: format!(
                    "Account {}: derived balance {} < 0 after settlement",
                    account_id, balance
                ),
            });
        }
    }
    for (&account_id, pos_map) in &positions {
        if account_id == MINT_ID {
            continue; // MINT holds short (negative) positions by design
        }
        for (&(market, outcome), &qty) in pos_map {
            if qty < 0 {
                violations.push(Violation {
                    kind: ViolationKind::NegativePosition,
                    details: format!(
                        "Account {} market {:?} outcome {}: derived position {} < 0 after settlement",
                        account_id, market, outcome, qty
                    ),
                });
            }
        }
    }

    // Compare derived state against claimed post-state
    let post_map: HashMap<u64, &AccountSnapshot> =
        witness.post_state.iter().map(|s| (s.id, s)).collect();

    // Check every account that should be in the post-state
    let all_ids: std::collections::HashSet<u64> = balances
        .keys()
        .chain(post_map.keys().copied().collect::<Vec<_>>().iter())
        .copied()
        .collect();

    for &account_id in &all_ids {
        let derived_balance = balances.get(&account_id).copied().unwrap_or(0);
        let derived_positions = positions.get(&account_id);

        if let Some(claimed) = post_map.get(&account_id) {
            // Check balance
            if derived_balance != claimed.balance {
                violations.push(Violation {
                    kind: ViolationKind::SettlementBalanceMismatch,
                    details: format!(
                        "Account {}: derived balance {} != claimed balance {}",
                        account_id, derived_balance, claimed.balance
                    ),
                });
            }

            // Check positions
            let empty_map = HashMap::new();
            let derived_pos = derived_positions.unwrap_or(&empty_map);

            // Build claimed positions map
            let claimed_pos: HashMap<(MarketId, u8), i64> = claimed
                .positions
                .iter()
                .map(|&(m, o, q)| ((m, o), q))
                .collect();

            // Check all derived positions
            let all_pos_keys: std::collections::HashSet<(MarketId, u8)> = derived_pos
                .keys()
                .chain(claimed_pos.keys())
                .copied()
                .collect();

            for key in all_pos_keys {
                let derived_qty = derived_pos.get(&key).copied().unwrap_or(0);
                let claimed_qty = claimed_pos.get(&key).copied().unwrap_or(0);

                // Skip zero positions (may not be present in either)
                if derived_qty == 0 && claimed_qty == 0 {
                    continue;
                }

                if derived_qty != claimed_qty {
                    violations.push(Violation {
                        kind: ViolationKind::SettlementPositionMismatch,
                        details: format!(
                            "Account {} market {:?} outcome {}: derived {} != claimed {}",
                            account_id, key.0, key.1, derived_qty, claimed_qty
                        ),
                    });
                }
            }
        } else {
            // Account in derived state but not in claimed post-state
            // Only flag if it has non-zero balance or positions
            let has_positions = derived_positions
                .map(|p| p.values().any(|&v| v != 0))
                .unwrap_or(false);

            if derived_balance != 0 || has_positions {
                violations.push(Violation {
                    kind: ViolationKind::SettlementAccountMismatch,
                    details: format!(
                        "Account {} exists in derived state but not in claimed post-state",
                        account_id
                    ),
                });
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
    use crate::types::{WitnessBlockHeader, WitnessOrder};
    use matching_engine::{outcome_buy, outcome_sell, Fill, MarketSet, NANOS_PER_DOLLAR};

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

    #[test]
    fn test_settlement_buy_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_cost = 500_000_000i64 * 10;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            positions: vec![],
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance - expected_cost,
            positions: vec![(m0, 0, 10)],
        }];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_settlement_sell_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, 5);
        let fill = Fill::new(2, 5, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_revenue = 500_000_000i64 * 5;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            positions: vec![(m0, 0, 10)],
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance + expected_revenue,
            positions: vec![(m0, 0, 5)],
        }];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_settlement_balance_mismatch() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            positions: vec![],
        }];

        // Wrong balance in post-state
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance, // Should be initial_balance - cost
            positions: vec![(m0, 0, 10)],
        }];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SettlementBalanceMismatch));
    }

    #[test]
    fn test_no_fills_settlement() {
        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: 100_000_000_000,
            positions: vec![],
        }];
        let post_state = pre_state.clone();

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_negative_balance_detected() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Account starts with $1, buys 10 YES @ $0.50 → cost = $5 → balance = -$4
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 1_000_000_000; // $1
        let expected_balance = initial_balance - 500_000_000i64 * 10; // -$4

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            positions: vec![],
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: expected_balance,
            positions: vec![(m0, 0, 10)],
        }];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::NegativeBalance));
    }

    #[test]
    fn test_negative_position_detected() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        // Account sells 5 YES without holding any → position = -5
        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 5);
        let fill = Fill::new(1, 5, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_revenue = 500_000_000i64 * 5;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            positions: vec![], // no YES position
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance + expected_revenue,
            positions: vec![(m0, 0, -5)],
        }];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::NegativePosition));
    }

    #[test]
    fn test_mint_derivation_buy_yes() {
        // When MINT is in the witness, the verifier derives minting adjustments.
        // Account 0 buys 10 YES at $0.50 → MINT shorts 10 YES, receives $5.
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = 500_000_000i64 * 10; // $5
        let mint_revenue = fill_cost; // MINT receives yes_price * diff

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let mint_id = u64::MAX;
        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                positions: vec![],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                positions: vec![],
            },
        ];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                positions: vec![(m0, 0, 10)],
            },
            AccountSnapshot {
                id: mint_id,
                balance: mint_revenue,
                positions: vec![(m0, 0, -10)],
            },
        ];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_mint_wrong_balance_detected() {
        // MINT with incorrect balance in post_state should fail verification.
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = 500_000_000i64 * 10;

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let mint_id = u64::MAX;
        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                positions: vec![],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                positions: vec![],
            },
        ];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                positions: vec![(m0, 0, 10)],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 999, // WRONG — should be fill_cost
                positions: vec![(m0, 0, -10)],
            },
        ];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SettlementBalanceMismatch));
    }

    #[test]
    fn test_minting_without_clearing_price() {
        // Position imbalance with no clearing prices → MintingWithoutClearingPrice
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let fill = Fill::new(1, 10, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = 500_000_000i64 * 10;
        let mint_id = u64::MAX;

        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                positions: vec![],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                positions: vec![],
            },
        ];

        // No clearing prices — verifier will flag MintingWithoutClearingPrice
        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                positions: vec![(m0, 0, 10)],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                positions: vec![(m0, 0, -10)],
            },
        ];

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![WitnessOrder {
                order,
                account_id: 0,
                is_mm: false,
            }],
            rejections: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(), // empty!
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state,
            post_state,
            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::MintingWithoutClearingPrice));
    }
}
