use std::collections::BTreeMap;

use super::*;

#[derive(Default)]
struct AccountAggregates {
    total_balance: i64,
    market_position_totals: BTreeMap<MarketId, (i64, i64)>,
    negative_balances: Vec<(AccountId, i64)>,
}

impl AccountAggregates {
    fn from_accounts(accounts: &AccountStore) -> Self {
        let mut aggregates = Self::default();
        for (account_id, account) in accounts.iter() {
            aggregates.observe_account(*account_id, account);
        }
        aggregates
    }

    fn observe_account(&mut self, account_id: AccountId, account: &Account) {
        self.total_balance += account.balance;
        if account_id != AccountId::MINT && account.balance < 0 {
            self.negative_balances.push((account_id, account.balance));
        }
        for (&(market_id, outcome), &qty) in &account.positions {
            if qty == 0 {
                continue;
            }
            self.apply_position_delta(market_id, outcome, qty);
        }
    }

    fn apply_position_deltas(
        &mut self,
        position_deltas: impl IntoIterator<Item = (MarketId, u8, i64)>,
    ) {
        for (market_id, outcome, qty_delta) in position_deltas {
            self.apply_position_delta(market_id, outcome, qty_delta);
        }
    }

    fn apply_position_delta(&mut self, market_id: MarketId, outcome: u8, qty_delta: i64) {
        let entry = self
            .market_position_totals
            .entry(market_id)
            .or_insert((0, 0));
        match outcome {
            0 => entry.0 += qty_delta,
            1 => entry.1 += qty_delta,
            _ => {}
        }
    }

    fn minting_inputs(&self) -> Vec<(MarketId, i64, i64)> {
        self.market_position_totals
            .iter()
            .map(|(&market_id, &(total_yes, total_no))| (market_id, total_yes, total_no))
            .collect()
    }

    fn totals_for(&self, market_id: MarketId) -> (i64, i64) {
        self.market_position_totals
            .get(&market_id)
            .copied()
            .unwrap_or((0, 0))
    }
}

fn canonical_state_with_aggregates(accounts: &AccountStore) -> (CanonicalState, AccountAggregates) {
    let mut aggregates = AccountAggregates::default();
    let snapshots = accounts.iter().map(|(account_id, account)| {
        aggregates.observe_account(*account_id, account);
        snapshot_account(account)
    });
    (CanonicalState::from_snapshot_iter(snapshots), aggregates)
}

pub(crate) fn expected_balance_delta_from_fills(
    fills: &[Fill],
    order_map: &HashMap<u64, &Order>,
    mint_adjustments: &[matching_engine::MintAdjustment],
) -> i64 {
    let fill_delta = fills.iter().fold(0, |net_delta, fill| {
        if fill.fill_qty.0 == 0 {
            return net_delta;
        }

        let Some(order) = order_map.get(&fill.order_id) else {
            return net_delta;
        };

        let Some(delta) = matching_engine::compute_fill_settlement(order, fill) else {
            return net_delta;
        };

        net_delta + delta.balance_delta
    });
    let mint_delta: i64 = mint_adjustments.iter().map(|adj| adj.balance_delta).sum();
    fill_delta + mint_delta
}

pub(crate) fn collect_account_invariant_failures(
    accounts: &AccountStore,
    markets: &MarketSet,
) -> Vec<BlockInvariantFailure> {
    let aggregates = AccountAggregates::from_accounts(accounts);
    collect_account_invariant_failures_from_aggregates(&aggregates, markets)
}

fn collect_account_invariant_failures_from_aggregates(
    aggregates: &AccountAggregates,
    markets: &MarketSet,
) -> Vec<BlockInvariantFailure> {
    let mut failures = Vec::new();
    for &(account_id, balance) in &aggregates.negative_balances {
        failures.push(BlockInvariantFailure::NegativeBalance {
            account_id,
            balance,
        });
    }

    for market in markets.iter() {
        let (total_yes, total_no) = aggregates.totals_for(market.id);
        if total_yes != total_no {
            failures.push(BlockInvariantFailure::PositionImbalance {
                market_id: market.id,
                total_yes,
                total_no,
            });
        }
    }

    failures
}

impl BlockSequencer {
    #[tracing::instrument(
        skip_all,
        fields(height = self.height, fills = fills.len())
    )]
    pub(super) fn finalize_block_state_phase(
        &mut self,
        fills: &[Fill],
        problem: &Problem,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
        timestamp_ms: u64,
    ) -> FinalizedBlockState {
        let mut pre_aggregates = AccountAggregates::from_accounts(&self.accounts);
        let pre_total_balance = pre_aggregates.total_balance;
        let (settle_failures, fill_position_deltas) = settlement::settle_batch_with_position_deltas(
            &mut self.accounts,
            fills,
            &problem.orders,
            self.height,
        );

        pre_aggregates.apply_position_deltas(fill_position_deltas);
        let market_totals = pre_aggregates.minting_inputs();
        let mint_adjustments = matching_engine::derive_minting(&market_totals, clearing_prices);
        let fill_balance_delta =
            matching_engine::fill_balance_delta_from_fills(problem.orders.iter(), fills);
        let minting_cost =
            matching_engine::minting_cost_from_fill_balance_delta(fill_balance_delta);
        if !mint_adjustments.is_empty() {
            let mint = self
                .accounts
                .get_mut(crate::account::AccountId::MINT)
                .expect("mint account must exist");
            settlement::apply_minting(mint, &mint_adjustments, self.height);
        }

        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
        // Touch midpoints from the resting single-market book for markets that
        // did not cross this batch. Scoped so the immutable book borrow is
        // released before the &mut self.analytics call below.
        let midpoints = {
            let resting: Vec<&Order> = self.order_book.resting_orders().map(|(o, _)| o).collect();
            matching_engine::book_midprices(resting.iter().copied())
        };
        let (volume_by_market, mark_prices) = self.analytics.record_finalized_block(
            fills,
            &order_map,
            clearing_prices,
            &midpoints,
            self.height,
            timestamp_ms,
            &self.accounts,
        );

        let (post_state, post_aggregates) = canonical_state_with_aggregates(&self.accounts);
        let post_total_balance = post_aggregates.total_balance;
        let balance_delta = post_total_balance - pre_total_balance;
        let mut invariant_failures = settle_failures;
        if balance_delta != 0 {
            let expected_balance_delta =
                expected_balance_delta_from_fills(fills, &order_map, &mint_adjustments);
            if balance_delta != expected_balance_delta {
                invariant_failures.push(BlockInvariantFailure::BalanceDeltaMismatch {
                    balance_delta,
                    expected_balance_delta,
                });
            }
        }

        invariant_failures.extend(collect_account_invariant_failures_from_aggregates(
            &post_aggregates,
            &self.markets,
        ));

        FinalizedBlockState {
            post_state,
            volume_by_market,
            mark_prices,
            minting_cost,
            invariant_failures,
        }
    }
}
