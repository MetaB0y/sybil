//! Layer 2: Settlement verification.
//!
//! Re-derives post-state from post-system-state + fills and compares against
//! the claimed post-state. Uses the same settlement logic as the sequencer.

use std::collections::HashMap;

use matching_engine::{
    compute_fill_settlement, derive_minting, minting_cost_from_incremental_adjustments, Fill,
    MarketId, Order,
};

use crate::types::{AccountSnapshot, BlockWitness, WitnessOrder};
use crate::violations::{VerificationResult, VerificationStats, Violation, ViolationKind};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DerivedAccountState {
    balance: i64,
    positions: HashMap<(MarketId, u8), i64>,
}

#[derive(Clone, Debug, Default)]
struct DerivedSettlement {
    accounts: HashMap<u64, DerivedAccountState>,
    violations: Vec<Violation>,
    accounts_checked: usize,
    minting_cost: i64,
    fill_balance_delta: i64,
}

fn derive_post_state(
    post_system_state: &[AccountSnapshot],
    orders: &[WitnessOrder],
    fills: &[Fill],
    clearing_prices: &HashMap<MarketId, Vec<u64>>,
) -> DerivedSettlement {
    let mut result = DerivedSettlement::default();

    // Build order map
    let order_map: HashMap<u64, &Order> =
        orders.iter().map(|wo| (wo.order.id, &wo.order)).collect();

    // Build order→account mapping (fallback for fills with account_id == 0)
    let order_account: HashMap<u64, u64> = orders
        .iter()
        .map(|wo| (wo.order.id, wo.account_id))
        .collect();

    // Clone post-system state into working state
    for snap in post_system_state {
        let mut pos_map: HashMap<(MarketId, u8), i64> = HashMap::new();
        for &(market, outcome, qty) in &snap.positions {
            pos_map.insert((market, outcome), qty);
        }
        result.accounts.insert(
            snap.id,
            DerivedAccountState {
                balance: snap.balance,
                positions: pos_map,
            },
        );
        result.accounts_checked += 1;
    }

    let pre_market_totals = market_totals_from_accounts(&result.accounts);
    let pre_adjustments = derive_minting(&pre_market_totals, clearing_prices);

    // Apply each fill using the shared settlement function
    for fill in fills {
        if fill.fill_qty == 0 {
            continue;
        }

        // Prefer fill.account_id (enriched by sequencer), fall back to order map
        let account_id = if fill.account_id != 0 {
            fill.account_id
        } else {
            match order_account.get(&fill.order_id) {
                Some(&id) => id,
                None => continue,
            }
        };
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };

        // Ensure account exists in our working state
        let account = result.accounts.entry(account_id).or_default();

        if let Some(delta) = compute_fill_settlement(order, fill) {
            result.fill_balance_delta += delta.balance_delta;
            account.balance += delta.balance_delta;
            let pos = &mut account.positions;
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
        // Collect all markets with any positions after applying fills.
        let all_markets: std::collections::HashSet<MarketId> = result
            .accounts
            .values()
            .flat_map(|account| account.positions.keys().map(|(m, _)| *m))
            .collect();

        let market_totals: Vec<(MarketId, i64, i64)> = all_markets
            .iter()
            .map(|&market_id| {
                let total_yes: i64 = result
                    .accounts
                    .values()
                    .map(|account| account.positions.get(&(market_id, 0)).copied().unwrap_or(0))
                    .sum();
                let total_no: i64 = result
                    .accounts
                    .values()
                    .map(|account| account.positions.get(&(market_id, 1)).copied().unwrap_or(0))
                    .sum();
                (market_id, total_yes, total_no)
            })
            .collect();

        let adjustments = derive_minting(&market_totals, clearing_prices);
        result.minting_cost = minting_cost_from_incremental_adjustments(
            result.fill_balance_delta,
            &pre_adjustments,
            &adjustments,
        );

        if !adjustments.is_empty() {
            let mint = result.accounts.entry(MINT_ID).or_default();

            // Check for missing clearing prices (balance_delta == 0 with non-zero position)
            for adj in &adjustments {
                if adj.balance_delta == 0 {
                    let side = if adj.outcome == 0 { "YES" } else { "NO" };
                    result.violations.push(Violation {
                        kind: ViolationKind::MintingWithoutClearingPrice,
                        details: format!(
                            "Market {:?}: position imbalance {} but no {} clearing price",
                            adj.market_id,
                            adj.position_delta.abs(),
                            side
                        ),
                    });
                }
            }

            for adj in &adjustments {
                *mint
                    .positions
                    .entry((adj.market_id, adj.outcome))
                    .or_insert(0) += adj.position_delta;
                mint.balance += adj.balance_delta;
            }
        }
    }

    result
}

fn market_totals_from_accounts(
    accounts: &HashMap<u64, DerivedAccountState>,
) -> Vec<(MarketId, i64, i64)> {
    let all_markets: std::collections::HashSet<MarketId> = accounts
        .values()
        .flat_map(|account| account.positions.keys().map(|(m, _)| *m))
        .collect();

    all_markets
        .iter()
        .map(|&market_id| {
            let total_yes: i64 = accounts
                .values()
                .map(|account| account.positions.get(&(market_id, 0)).copied().unwrap_or(0))
                .sum();
            let total_no: i64 = accounts
                .values()
                .map(|account| account.positions.get(&(market_id, 1)).copied().unwrap_or(0))
                .sum();
            (market_id, total_yes, total_no)
        })
        .collect()
}

/// Verify that `post_system_state + fills → post_state`.
pub fn verify_settlement(witness: &BlockWitness) -> VerificationResult {
    let mut violations = Vec::new();
    let mut stats = VerificationStats::default();
    let derived = derive_post_state(
        &witness.post_system_state,
        &witness.orders,
        &witness.fills,
        &witness.clearing_prices,
    );
    stats.accounts_checked = derived.accounts_checked;
    violations.extend(derived.violations.clone());

    if derived.minting_cost != witness.minting_cost {
        violations.push(Violation {
            kind: ViolationKind::WelfareMismatch,
            details: format!(
                "Reported minting_cost {} != settlement-derived minting_cost {}",
                witness.minting_cost, derived.minting_cost
            ),
        });
    }

    // Non-negative balance/position assertions (ZK invariants).
    // MINT (u64::MAX) is exempt — it holds short positions by design.
    const MINT_ID: u64 = u64::MAX;
    for (&account_id, account) in &derived.accounts {
        if account.balance < 0 && account_id != MINT_ID {
            violations.push(Violation {
                kind: ViolationKind::NegativeBalance,
                details: format!(
                    "Account {}: derived balance {} < 0 after settlement",
                    account_id, account.balance
                ),
            });
        }
    }
    for (&account_id, account) in &derived.accounts {
        if account_id == MINT_ID {
            continue; // MINT holds short (negative) positions by design
        }
        for (&(market, outcome), &qty) in &account.positions {
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
    let all_ids: std::collections::HashSet<u64> = derived
        .accounts
        .keys()
        .chain(post_map.keys().copied().collect::<Vec<_>>().iter())
        .copied()
        .collect();

    for &account_id in &all_ids {
        let derived_account = derived.accounts.get(&account_id);
        let derived_balance = derived_account.map(|account| account.balance).unwrap_or(0);

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
            let derived_pos = derived_account
                .map(|account| &account.positions)
                .unwrap_or(&empty_map);

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
            let has_positions = derived_account
                .map(|account| account.positions.values().any(|&v| v != 0))
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
    use matching_engine::{
        notional_nanos, outcome_buy, outcome_sell, shares_to_qty, Fill, MarketSet, NANOS_PER_DOLLAR,
    };
    use proptest::prelude::*;

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

    fn derived_from_snapshots(
        post_system_state: &[AccountSnapshot],
    ) -> HashMap<u64, DerivedAccountState> {
        post_system_state
            .iter()
            .map(|snapshot| {
                (
                    snapshot.id,
                    DerivedAccountState {
                        balance: snapshot.balance,
                        positions: snapshot
                            .positions
                            .iter()
                            .map(|&(market, outcome, qty)| ((market, outcome), qty))
                            .collect(),
                    },
                )
            })
            .collect()
    }

    fn q(shares: u64) -> u64 {
        shares_to_qty(shares)
    }

    #[test]
    fn test_settlement_buy_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_cost = notional_nanos(500_000_000, qty) as i64;
        let mint_id = u64::MAX;
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - expected_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: expected_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: expected_cost,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_settlement_starts_from_post_system_state() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let mint_id = u64::MAX;
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let post_system_state = vec![AccountSnapshot {
            id: 0,
            balance: 100 * NANOS_PER_DOLLAR as i64,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: 95 * NANOS_PER_DOLLAR as i64,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 5 * NANOS_PER_DOLLAR as i64,
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: 5 * NANOS_PER_DOLLAR as i64,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state,
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_settlement_sell_yes() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(5);
        let order = outcome_sell(&markets, 2, m0, 0, 500_000_000, qty);
        let fill = Fill::new(2, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_revenue = notional_nanos(500_000_000, qty) as i64;
        let mint_id = u64::MAX;
        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![(m0, 0, q(10) as i64)],
            events_digest: [0u8; 32],
        }];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance + expected_revenue,
                total_deposited: 0,
                positions: vec![(m0, 0, q(5) as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: expected_revenue,
                total_deposited: 0,
                positions: vec![(m0, 0, -(q(5) as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_settlement_balance_mismatch() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        // Wrong balance in post-state
        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance, // Should be initial_balance - cost
            total_deposited: 0,
            positions: vec![(m0, 0, qty as i64)],
            events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

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
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];
        let post_state = pre_state.clone();

        let witness = BlockWitness {
            header: empty_header(),
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

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
        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 1_000_000_000; // $1
        let expected_balance = initial_balance - notional_nanos(500_000_000, qty) as i64; // -$4

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: expected_balance,
            total_deposited: 0,
            positions: vec![(m0, 0, qty as i64)],
            events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

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
        let qty = q(5);
        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let expected_revenue = notional_nanos(500_000_000, qty) as i64;

        let pre_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![], // no YES position
            events_digest: [0u8; 32],
        }];

        let post_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance + expected_revenue,
            total_deposited: 0,
            positions: vec![(m0, 0, -(qty as i64))],
            events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

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
        // Account 0 buys 10 YES at $0.50 → MINT shorts 10 YES, receives $5.
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = notional_nanos(500_000_000, qty) as i64; // $5
        let mint_revenue = fill_cost; // MINT receives yes_price * diff

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let mint_id = u64::MAX;
        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
        ];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: mint_revenue,
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: fill_cost,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(result.valid, "Violations: {:?}", result.violations);
    }

    #[test]
    fn test_mint_derivation_does_not_require_mint_in_post_system_state() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("M0");

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = notional_nanos(500_000_000, qty) as i64;
        let mint_id = u64::MAX;

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let post_system_state = vec![AccountSnapshot {
            id: 0,
            balance: initial_balance,
            total_deposited: 0,
            positions: vec![],
            events_digest: [0u8; 32],
        }];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: fill_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: fill_cost,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state,
            post_state,
            state_sidecar: Default::default(),

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

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = notional_nanos(500_000_000, qty) as i64;

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(m0, vec![500_000_000, 500_000_000]);

        let mint_id = u64::MAX;
        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
        ];

        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 999, // WRONG — should be fill_cost
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices,
            total_welfare: 0,
            minting_cost: fill_cost,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

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

        let qty = q(10);
        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, qty);
        let fill = Fill::new(1, qty, 500_000_000);

        let initial_balance = 100 * NANOS_PER_DOLLAR as i64;
        let fill_cost = notional_nanos(500_000_000, qty) as i64;
        let mint_id = u64::MAX;

        let pre_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            },
        ];

        // No clearing prices — verifier will flag MintingWithoutClearingPrice
        let post_state = vec![
            AccountSnapshot {
                id: 0,
                balance: initial_balance - fill_cost,
                total_deposited: 0,
                positions: vec![(m0, 0, qty as i64)],
                events_digest: [0u8; 32],
            },
            AccountSnapshot {
                id: mint_id,
                balance: 0,
                total_deposited: 0,
                positions: vec![(m0, 0, -(qty as i64))],
                events_digest: [0u8; 32],
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
            system_events: vec![],
            fills: vec![fill],
            clearing_prices: HashMap::new(), // empty!
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: Default::default(),

            resolved_markets: vec![],
        };

        let result = verify_settlement(&witness);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::MintingWithoutClearingPrice));
    }

    proptest! {
        #[test]
        fn prop_derive_post_state_is_identity_without_fills_or_minting(
            balances in prop::collection::vec(-1_000i64..=1_000, 0..6)
        ) {
            let post_system_state: Vec<AccountSnapshot> = balances
                .iter()
                .enumerate()
                .map(|(id, balance)| AccountSnapshot {
                    id: id as u64,
                    balance: *balance,
                    total_deposited: 0,
                    positions: vec![],
                    events_digest: [0u8; 32],
                })
                .collect();

            let derived = derive_post_state(&post_system_state, &[], &[], &HashMap::new());
            prop_assert_eq!(derived.accounts, derived_from_snapshots(&post_system_state));
            prop_assert!(derived.violations.is_empty());
        }

        #[test]
        fn prop_zero_fill_is_a_no_op(
            balance in 0i64..=10_000,
            limit_price in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64), Just(700_000_000u64)],
            max_fill in 1u64..=10,
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");

            let order = outcome_buy(&markets, 1, m0, 0, limit_price, max_fill);
            let witness_order = WitnessOrder {
                order: order.clone(),
                account_id: 0,
                is_mm: false,
            };
            let mut fill = Fill::new(order.id, 0, limit_price);
            fill.account_id = 0;

            let post_system_state = vec![AccountSnapshot {
                id: 0,
                balance,
                total_deposited: 0,
                positions: vec![],
                events_digest: [0u8; 32],
            }];

            let derived = derive_post_state(
                &post_system_state,
                &[witness_order],
                &[fill],
                &HashMap::new(),
            );
            prop_assert_eq!(derived.accounts, derived_from_snapshots(&post_system_state));
            prop_assert!(derived.violations.is_empty());
        }

        #[test]
        fn prop_fill_order_is_irrelevant_for_distinct_accounts_and_markets(
            balance_a in 1_000_000_000i64..=10_000_000_000,
            balance_b in 1_000_000_000i64..=10_000_000_000,
            qty_a in 1u64..=5,
            qty_b in 1u64..=5,
            price_a in prop_oneof![Just(100_000_000u64), Just(300_000_000u64), Just(500_000_000u64)],
            price_b in prop_oneof![Just(200_000_000u64), Just(400_000_000u64), Just(600_000_000u64)],
        ) {
            let mut markets = MarketSet::new();
            let m0 = markets.add_binary("M0");
            let m1 = markets.add_binary("M1");

            let order_a = outcome_buy(&markets, 1, m0, 0, price_a, qty_a);
            let order_b = outcome_buy(&markets, 2, m1, 0, price_b, qty_b);
            let orders = vec![
                WitnessOrder { order: order_a.clone(), account_id: 0, is_mm: false },
                WitnessOrder { order: order_b.clone(), account_id: 1, is_mm: false },
            ];

            let post_system_state = vec![
                AccountSnapshot {
                    id: 0,
                    balance: balance_a,
                    total_deposited: 0,
                    positions: vec![],
                    events_digest: [0u8; 32],
                },
                AccountSnapshot {
                    id: 1,
                    balance: balance_b,
                    total_deposited: 0,
                    positions: vec![],
                    events_digest: [0u8; 32],
                },
            ];

            let mut fill_a = Fill::new(order_a.id, qty_a, price_a);
            fill_a.account_id = 0;
            let mut fill_b = Fill::new(order_b.id, qty_b, price_b);
            fill_b.account_id = 1;
            let mut clearing_prices = HashMap::new();
            clearing_prices.insert(m0, vec![price_a, NANOS_PER_DOLLAR - price_a]);
            clearing_prices.insert(m1, vec![price_b, NANOS_PER_DOLLAR - price_b]);

            let derived_ab = derive_post_state(
                &post_system_state,
                &orders,
                &[fill_a.clone(), fill_b.clone()],
                &clearing_prices,
            );
            let derived_ba = derive_post_state(
                &post_system_state,
                &orders,
                &[fill_b, fill_a],
                &clearing_prices,
            );

            prop_assert_eq!(derived_ab.accounts, derived_ba.accounts);
            prop_assert!(derived_ab.violations.is_empty());
            prop_assert!(derived_ba.violations.is_empty());
        }
    }
}
