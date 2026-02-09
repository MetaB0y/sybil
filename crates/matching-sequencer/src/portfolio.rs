use std::collections::HashMap;

use matching_engine::{MarketId, Nanos};

use crate::account::{Account, AccountId};

/// A single position valued at current market prices.
pub struct PositionValue {
    pub market_id: MarketId,
    pub outcome: u8,
    pub quantity: i64,
    pub current_price_nanos: Nanos,
    /// quantity * current_price (signed)
    pub value_nanos: i64,
}

/// Portfolio summary with valued positions and PnL.
pub struct PortfolioSummary {
    pub account_id: AccountId,
    pub balance_nanos: i64,
    pub total_deposited_nanos: i64,
    pub positions: Vec<PositionValue>,
    pub total_position_value_nanos: i64,
    /// balance + position value
    pub portfolio_value_nanos: i64,
    /// portfolio_value - total_deposited
    pub pnl_nanos: i64,
}

/// Compute a portfolio summary for an account given current market prices.
pub fn compute_portfolio(
    account: &Account,
    last_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> PortfolioSummary {
    let mut positions = Vec::new();
    let mut total_position_value: i64 = 0;

    for (&(market_id, outcome), &quantity) in &account.positions {
        if quantity == 0 {
            continue;
        }

        let price = last_prices
            .get(&market_id)
            .and_then(|p| p.get(outcome as usize).copied())
            .unwrap_or(matching_engine::NANOS_PER_DOLLAR / 2);

        let value = quantity as i128 * price as i128;
        let value_nanos = value as i64;
        total_position_value += value_nanos;

        positions.push(PositionValue {
            market_id,
            outcome,
            quantity,
            current_price_nanos: price,
            value_nanos,
        });
    }

    // Sort for deterministic output
    positions.sort_by_key(|p| (p.market_id.0, p.outcome));

    let portfolio_value = account.balance + total_position_value;
    let pnl = portfolio_value - account.total_deposited;

    PortfolioSummary {
        account_id: account.id,
        balance_nanos: account.balance,
        total_deposited_nanos: account.total_deposited,
        positions,
        total_position_value_nanos: total_position_value,
        portfolio_value_nanos: portfolio_value,
        pnl_nanos: pnl,
    }
}
