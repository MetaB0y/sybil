use super::*;

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
    let mut failures = Vec::new();
    for (account_id, account) in accounts.iter() {
        if *account_id != AccountId::MINT && account.balance < 0 {
            failures.push(BlockInvariantFailure::NegativeBalance {
                account_id: *account_id,
                balance: account.balance,
            });
        }
    }

    let post_state = CanonicalState::from_accounts(accounts);
    let post_position_totals = post_state.market_position_totals();
    for market in markets.iter() {
        let (total_yes, total_no) = post_position_totals.totals_for(market.id);
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
        let pre_total_balance: i64 = self.accounts.iter().map(|(_, a)| a.balance).sum();
        let pre_market_totals = CanonicalState::from_accounts(&self.accounts)
            .market_position_totals()
            .minting_inputs();
        let pre_mint_adjustments =
            matching_engine::derive_minting(&pre_market_totals, clearing_prices);

        settlement::settle_batch(&mut self.accounts, fills, &problem.orders, self.height);

        let market_totals = CanonicalState::from_accounts(&self.accounts)
            .market_position_totals()
            .minting_inputs();
        let mint_adjustments = matching_engine::derive_minting(&market_totals, clearing_prices);
        let fill_balance_delta =
            matching_engine::fill_balance_delta_from_fills(problem.orders.iter(), fills);
        let minting_cost = matching_engine::minting_cost_from_incremental_adjustments(
            fill_balance_delta,
            &pre_mint_adjustments,
            &mint_adjustments,
        );
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

        let post_total_balance: i64 = self.accounts.iter().map(|(_, a)| a.balance).sum();
        let balance_delta = post_total_balance - pre_total_balance;
        let mut invariant_failures = Vec::new();
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

        invariant_failures.extend(collect_account_invariant_failures(
            &self.accounts,
            &self.markets,
        ));

        let post_state = CanonicalState::from_accounts(&self.accounts);

        FinalizedBlockState {
            post_state,
            volume_by_market,
            mark_prices,
            minting_cost,
            invariant_failures,
        }
    }
}
