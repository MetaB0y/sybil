use super::*;

impl BlockSequencer {
    #[tracing::instrument(
        skip_all,
        fields(
            height = self.height,
            orders = problem.orders.len(),
            active_markets = active_markets.len()
        )
    )]
    pub(super) fn solve_batch_phase(
        &mut self,
        problem: &Problem,
        order_account_map: &HashMap<u64, AccountId>,
        active_markets: &HashSet<MarketId>,
    ) -> SolvedBatch {
        let pipeline_result = self.solver.solve(problem);

        let markets_with_fills: HashSet<MarketId> = {
            let order_map: HashMap<u64, &Order> =
                problem.orders.iter().map(|o| (o.id, o)).collect();
            pipeline_result
                .result
                .fills
                .iter()
                .filter(|f| f.fill_qty.0 > 0)
                .filter_map(|f| order_map.get(&f.order_id))
                .flat_map(|o| o.active_markets())
                .collect()
        };

        let position_markets = CanonicalState::from_accounts(&self.accounts)
            .market_position_totals()
            .markets();
        let clearing_prices = self.analytics.merge_prices(
            &pipeline_result.price_discovery,
            &markets_with_fills,
            active_markets,
            &position_markets,
        );

        let mut fills = pipeline_result.result.fills.clone();
        for fill in &mut fills {
            if let Some(&aid) = order_account_map.get(&fill.order_id) {
                fill.account_id = aid.0;
            }
        }

        let total_welfare = pipeline_result.result.total_welfare();
        let total_volume = fills
            .iter()
            .map(|f| matching_engine::notional_nanos(f.fill_price, f.fill_qty))
            .fold(0u64, |acc, v| acc.saturating_add(v.0));
        let orders_filled = pipeline_result.result.orders_filled;

        // Per-market welfare. Reuse the same order_map already built above
        // for markets_with_fills. Multi-market orders credit every active
        // market with the full welfare contribution — platform's
        // total_welfare counts each fill once.
        let mut welfare_by_market: HashMap<MarketId, i64> = HashMap::new();
        {
            let order_map: HashMap<u64, &Order> =
                problem.orders.iter().map(|o| (o.id, o)).collect();
            for fill in &fills {
                if fill.fill_qty.0 == 0 {
                    continue;
                }
                let Some(order) = order_map.get(&fill.order_id) else {
                    continue;
                };
                let w = order.welfare_contribution(fill.fill_price, fill.fill_qty);
                for m in order.active_markets() {
                    *welfare_by_market.entry(m).or_insert(0) += w;
                }
            }
        }

        SolvedBatch {
            pipeline_result,
            fills,
            clearing_prices,
            total_welfare,
            total_volume,
            orders_filled,
            welfare_by_market,
        }
    }
}
