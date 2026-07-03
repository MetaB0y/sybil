use std::collections::HashMap;

use matching_engine::{outcome_buy, outcome_sell, MarketId, MarketSet, Nanos, NANOS_PER_DOLLAR};

use crate::agent::{Agent, AgentSubmission, MarketView};
use matching_sequencer::account::{Account, AccountId};

pub struct InformedTrader {
    name: String,
    account_id: AccountId,
    markets: MarketSet,
    /// True probability of YES for each market
    beliefs: HashMap<MarketId, f64>,
    /// Minimum edge (in probability) required to trade
    min_edge: f64,
    /// Max quantity per order
    max_qty: u64,
    /// Max position per market (prevents over-concentration)
    max_position: i64,
}

impl InformedTrader {
    pub fn new(
        name: String,
        account_id: AccountId,
        markets: MarketSet,
        beliefs: HashMap<MarketId, f64>,
        min_edge: f64,
        max_qty: u64,
        max_position: i64,
    ) -> Self {
        Self {
            name,
            account_id,
            markets,
            beliefs,
            min_edge,
            max_qty,
            max_position,
        }
    }
}

impl Agent for InformedTrader {
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
            let Some(&true_prob) = self.beliefs.get(&market_id) else {
                continue;
            };

            // Don't trade if we can't afford it
            if account.balance <= 0 {
                break;
            }

            // Get market price (default 0.50)
            let default_prices = vec![NANOS_PER_DOLLAR / 2, NANOS_PER_DOLLAR / 2];
            let last_prices = view.last_prices.get(&market_id).unwrap_or(&default_prices);
            let yes_market_price = last_prices[0] as f64 / NANOS_PER_DOLLAR as f64;
            let no_market_price = last_prices[1] as f64 / NANOS_PER_DOLLAR as f64;

            // Check edge for YES buy (market underprices YES)
            let yes_edge = true_prob - yes_market_price;
            if yes_edge > self.min_edge {
                let current_pos = account.position(market_id, 0);
                if current_pos < self.max_position {
                    // Buy YES at a price between market and true value
                    let limit_price =
                        ((yes_market_price + true_prob) / 2.0 * NANOS_PER_DOLLAR as f64) as Nanos;
                    let limit_price =
                        limit_price.clamp(NANOS_PER_DOLLAR / 100, NANOS_PER_DOLLAR * 99 / 100);

                    let remaining_capacity = (self.max_position - current_pos) as u64;
                    let qty = self.max_qty.min(remaining_capacity);

                    if qty > 0 {
                        let order =
                            outcome_buy(&self.markets, temp_id, market_id, 0, limit_price, qty);
                        orders.push(order);
                        temp_id += 1;
                    }
                }
            }

            // Check edge for YES sell (market overprices YES)
            let yes_sell_edge = yes_market_price - true_prob;
            if yes_sell_edge > self.min_edge {
                let current_pos = account.position(market_id, 0);
                if current_pos > 0 {
                    // Sell YES at a price between true value and market
                    let limit_price =
                        ((true_prob + yes_market_price) / 2.0 * NANOS_PER_DOLLAR as f64) as Nanos;
                    let limit_price =
                        limit_price.clamp(NANOS_PER_DOLLAR / 100, NANOS_PER_DOLLAR * 99 / 100);

                    let qty = self.max_qty.min(current_pos as u64);

                    if qty > 0 {
                        let order =
                            outcome_sell(&self.markets, temp_id, market_id, 0, limit_price, qty);
                        orders.push(order);
                        temp_id += 1;
                    }
                }
            }

            // Check edge for NO buy (market underprices NO)
            let no_edge = (1.0 - true_prob) - no_market_price;
            if no_edge > self.min_edge {
                let current_pos = account.position(market_id, 1);
                if current_pos < self.max_position {
                    let limit_price = ((no_market_price + (1.0 - true_prob)) / 2.0
                        * NANOS_PER_DOLLAR as f64) as Nanos;
                    let limit_price =
                        limit_price.clamp(NANOS_PER_DOLLAR / 100, NANOS_PER_DOLLAR * 99 / 100);

                    let remaining_capacity = (self.max_position - current_pos) as u64;
                    let qty = self.max_qty.min(remaining_capacity);

                    if qty > 0 {
                        let order =
                            outcome_buy(&self.markets, temp_id, market_id, 1, limit_price, qty);
                        orders.push(order);
                        temp_id += 1;
                    }
                }
            }

            // Check edge for NO sell (market overprices NO)
            let no_sell_edge = no_market_price - (1.0 - true_prob);
            if no_sell_edge > self.min_edge {
                let current_pos = account.position(market_id, 1);
                if current_pos > 0 {
                    let limit_price = (((1.0 - true_prob) + no_market_price) / 2.0
                        * NANOS_PER_DOLLAR as f64) as Nanos;
                    let limit_price =
                        limit_price.clamp(NANOS_PER_DOLLAR / 100, NANOS_PER_DOLLAR * 99 / 100);

                    let qty = self.max_qty.min(current_pos as u64);

                    if qty > 0 {
                        let order =
                            outcome_sell(&self.markets, temp_id, market_id, 1, limit_price, qty);
                        orders.push(order);
                        temp_id += 1;
                    }
                }
            }
        }

        AgentSubmission::with_orders(orders)
    }
}
