use rand::RngExt;

use matching_engine::{outcome_buy, outcome_sell, MarketSet, Nanos, NANOS_PER_DOLLAR};

use crate::agent::{Agent, AgentSubmission, MarketView};
use matching_sequencer::account::{Account, AccountId};

pub struct NoiseTrader {
    name: String,
    account_id: AccountId,
    markets: MarketSet,
    /// Probability of placing an order on any given market each batch
    activity_rate: f64,
    /// Max quantity per order
    max_qty: u64,
    /// Random noise range around the last price (in nanos)
    price_noise: Nanos,
    rng: Box<dyn rand::Rng + Send>,
}

impl NoiseTrader {
    pub fn new(
        name: String,
        account_id: AccountId,
        markets: MarketSet,
        activity_rate: f64,
        max_qty: u64,
        price_noise: Nanos,
        rng: Box<dyn rand::Rng + Send>,
    ) -> Self {
        Self {
            name,
            account_id,
            markets,
            activity_rate,
            max_qty,
            price_noise,
            rng,
        }
    }
}

impl Agent for NoiseTrader {
    fn name(&self) -> &str {
        &self.name
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn submit_orders(&mut self, view: &MarketView, account: &Account) -> AgentSubmission {
        let mut orders = Vec::new();
        let mut temp_id = 0u64;

        for &(market_id, _) in &view.markets {
            if self.rng.random::<f64>() > self.activity_rate {
                continue;
            }

            // Don't place orders if we can't afford them
            if account.balance <= 0 {
                break;
            }

            // Random outcome: 0 = YES, 1 = NO
            let outcome: u8 = self.rng.random_range(0..2);

            // 30% chance to sell if we have a position
            let yes_pos = account.position(market_id, 0);
            let no_pos = account.position(market_id, 1);
            let sell_chance: f64 = self.rng.random();
            let has_position = (outcome == 0 && yes_pos > 0) || (outcome == 1 && no_pos > 0);

            let is_sell = sell_chance < 0.3 && has_position;

            // Use public beliefs as base price when available, otherwise last price
            let base_price = if let Some(ref beliefs) = view.public_beliefs {
                if let Some(&belief) = beliefs.get(&market_id) {
                    // Convert belief probability to nanos price for the chosen outcome
                    let p = if outcome == 0 { belief } else { 1.0 - belief };
                    (p * NANOS_PER_DOLLAR as f64) as Nanos
                } else {
                    let default_prices = vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2];
                    let last_prices = view.last_prices.get(&market_id).unwrap_or(&default_prices);
                    last_prices[outcome as usize]
                }
            } else {
                let default_prices = vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2];
                let last_prices = view.last_prices.get(&market_id).unwrap_or(&default_prices);
                last_prices[outcome as usize]
            };

            // Add noise
            let noise =
                self.rng.random_range(0..=self.price_noise * 2) as i64 - self.price_noise as i64;
            let price = (base_price as i64 + noise).clamp(
                NANOS_PER_DOLLAR as i64 / 100,      // min 1 cent
                NANOS_PER_DOLLAR as i64 * 99 / 100, // max 99 cents
            ) as Nanos;

            if is_sell {
                // Sell existing position
                let available = if outcome == 0 { yes_pos } else { no_pos };
                let qty = self
                    .rng
                    .random_range(1..=self.max_qty)
                    .min(available as u64);
                if qty > 0 {
                    let order =
                        outcome_sell(&self.markets, temp_id, market_id, outcome, price, qty);
                    orders.push(order);
                    temp_id += 1;
                }
            } else {
                // Buy
                let qty = self.rng.random_range(1..=self.max_qty);
                let order = outcome_buy(&self.markets, temp_id, market_id, outcome, price, qty);
                orders.push(order);
                temp_id += 1;
            }
        }

        AgentSubmission::with_orders(orders)
    }
}
