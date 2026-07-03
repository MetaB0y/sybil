use std::collections::HashMap;

use matching_engine::{signed_notional_nanos, MarketId, Nanos, NANOS_PER_DOLLAR};

use matching_sequencer::account::{AccountId, AccountStore};

/// Per-batch metrics.
#[derive(Clone, Debug)]
pub struct BatchMetrics {
    pub batch: usize,
    pub total_welfare: i64,
    pub total_volume: u64,
    pub orders_submitted: usize,
    pub orders_filled: usize,
    pub rejections: usize,
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
}

impl BatchMetrics {
    pub fn fill_rate(&self) -> f64 {
        if self.orders_submitted == 0 {
            0.0
        } else {
            self.orders_filled as f64 / self.orders_submitted as f64
        }
    }
}

/// Per-agent profit/loss tracking.
#[derive(Clone, Debug)]
pub struct AgentPnL {
    pub name: String,
    pub account_id: AccountId,
    pub initial_balance: i64,
    pub final_balance: i64,
    /// Unrealized PnL from open positions (valued at last clearing prices)
    pub position_value: i64,
    /// Total PnL = (final_balance - initial_balance) + position_value
    pub total_pnl: i64,
}

/// Compute per-agent PnL.
pub fn compute_agent_pnl(
    agents: &[(String, AccountId, i64)], // (name, account_id, initial_balance)
    accounts: &AccountStore,
    last_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Vec<AgentPnL> {
    agents
        .iter()
        .map(|(name, account_id, initial_balance)| {
            let account = accounts.get(*account_id).unwrap();
            let final_balance = account.balance;

            // Value open positions at last clearing prices
            let mut position_value: i64 = 0;
            for (&(market, outcome), &qty) in &account.positions {
                if qty == 0 {
                    continue;
                }
                let default_prices = vec![Nanos(NANOS_PER_DOLLAR / 2); 2];
                let prices = last_prices.get(&market).unwrap_or(&default_prices);
                let price = prices
                    .get(outcome as usize)
                    .copied()
                    .unwrap_or(Nanos(NANOS_PER_DOLLAR / 2));
                position_value += signed_notional_nanos(price, qty);
            }

            let total_pnl = (final_balance - initial_balance) + position_value;

            AgentPnL {
                name: name.clone(),
                account_id: *account_id,
                initial_balance: *initial_balance,
                final_balance,
                position_value,
                total_pnl,
            }
        })
        .collect()
}

/// Compute price error vs true probabilities.
pub fn price_convergence(
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    true_probs: &HashMap<MarketId, f64>,
) -> f64 {
    if true_probs.is_empty() {
        return 0.0;
    }

    let mut total_error = 0.0;
    let mut count = 0;

    for (&market_id, &true_p) in true_probs {
        if let Some(prices) = clearing_prices.get(&market_id) {
            if let Some(&yes_price) = prices.first() {
                let market_p = yes_price.0 as f64 / NANOS_PER_DOLLAR as f64;
                total_error += (market_p - true_p).abs();
                count += 1;
            }
        }
    }

    if count > 0 {
        total_error / count as f64
    } else {
        0.0
    }
}

/// Compute resolved PnL after market resolution.
pub fn compute_resolved_pnl(
    agents: &[(String, AccountId, i64)],
    accounts: &AccountStore,
) -> Vec<AgentPnL> {
    agents
        .iter()
        .map(|(name, account_id, initial_balance)| {
            let account = accounts.get(*account_id).unwrap();
            let final_balance = account.balance;

            // After resolution, all positions should be cleared
            let position_value = 0i64;
            let total_pnl = final_balance - initial_balance;

            AgentPnL {
                name: name.clone(),
                account_id: *account_id,
                initial_balance: *initial_balance,
                final_balance,
                position_value,
                total_pnl,
            }
        })
        .collect()
}
