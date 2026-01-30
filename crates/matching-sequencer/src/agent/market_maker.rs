use matching_engine::{
    outcome_buy, outcome_sell, MarketSet, MmConstraint, MmId, MmSide, Nanos, NANOS_PER_DOLLAR,
};

use crate::account::{Account, AccountId};
use crate::agent::{Agent, AgentSubmission, MarketView};

pub struct MarketMakerAgent {
    name: String,
    account_id: AccountId,
    mm_id: MmId,
    markets: MarketSet,
    /// Half-spread in nanos (e.g., 25_000_000 = 2.5 cents)
    half_spread: Nanos,
    /// Max quantity per side per market
    qty_per_side: u64,
    /// Capital budget for the MmConstraint (flash quoting leverage)
    budget: Nanos,
    /// Inventory skew factor: how much to adjust quotes based on position
    skew_factor: f64,
}

impl MarketMakerAgent {
    pub fn new(
        name: String,
        account_id: AccountId,
        mm_id: MmId,
        markets: MarketSet,
        half_spread: Nanos,
        qty_per_side: u64,
        budget: Nanos,
        skew_factor: f64,
    ) -> Self {
        Self {
            name,
            account_id,
            mm_id,
            markets,
            half_spread,
            qty_per_side,
            budget,
            skew_factor,
        }
    }
}

impl Agent for MarketMakerAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn submit_orders(&mut self, view: &MarketView, account: &Account) -> AgentSubmission {
        let mut orders = Vec::new();
        let mut constraint = MmConstraint::new(self.mm_id, self.budget);
        let mut temp_id = 0u64;

        for &(market_id, _) in &view.markets {
            // Get current mid price (default 0.50)
            let default_prices = vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2];
            let last_prices = view.last_prices.get(&market_id).unwrap_or(&default_prices);
            let yes_price = last_prices[0];

            // Inventory skew: if we're long YES, lower our bid and raise our ask
            let yes_pos = account.position(market_id, 0);
            let skew =
                (yes_pos as f64 * self.skew_factor * NANOS_PER_DOLLAR as f64 / 100.0) as i64;

            // Compute bid and ask for YES
            let mid = yes_price as i64;
            let bid_price = (mid - self.half_spread as i64 - skew)
                .clamp(NANOS_PER_DOLLAR as i64 / 100, NANOS_PER_DOLLAR as i64 * 99 / 100)
                as Nanos;
            let ask_price = (mid + self.half_spread as i64 - skew)
                .clamp(NANOS_PER_DOLLAR as i64 / 100, NANOS_PER_DOLLAR as i64 * 99 / 100)
                as Nanos;

            // Bid: buy YES at bid_price
            let bid_order =
                outcome_buy(&self.markets, temp_id, market_id, 0, bid_price, self.qty_per_side);
            constraint.add_order(temp_id, MmSide::BuyYes);
            orders.push(bid_order);
            temp_id += 1;

            // Ask: sell YES at ask_price (using outcome_sell)
            let ask_order =
                outcome_sell(&self.markets, temp_id, market_id, 0, ask_price, self.qty_per_side);
            constraint.add_order(temp_id, MmSide::SellYes);
            orders.push(ask_order);
            temp_id += 1;

            // Also quote NO side (the opposite)
            let no_price = NANOS_PER_DOLLAR - yes_price;
            let no_mid = no_price as i64;

            let no_bid_price = (no_mid - self.half_spread as i64 + skew)
                .clamp(NANOS_PER_DOLLAR as i64 / 100, NANOS_PER_DOLLAR as i64 * 99 / 100)
                as Nanos;
            let no_ask_price = (no_mid + self.half_spread as i64 + skew)
                .clamp(NANOS_PER_DOLLAR as i64 / 100, NANOS_PER_DOLLAR as i64 * 99 / 100)
                as Nanos;

            let no_bid_order = outcome_buy(
                &self.markets,
                temp_id,
                market_id,
                1,
                no_bid_price,
                self.qty_per_side,
            );
            constraint.add_order(temp_id, MmSide::BuyNo);
            orders.push(no_bid_order);
            temp_id += 1;

            let no_ask_order = outcome_sell(
                &self.markets,
                temp_id,
                market_id,
                1,
                no_ask_price,
                self.qty_per_side,
            );
            constraint.add_order(temp_id, MmSide::SellNo);
            orders.push(no_ask_order);
            temp_id += 1;
        }

        if orders.is_empty() {
            AgentSubmission::empty()
        } else {
            AgentSubmission::with_mm(orders, constraint)
        }
    }
}
