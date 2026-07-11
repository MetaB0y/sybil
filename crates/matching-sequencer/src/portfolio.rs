use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, Qty, signed_notional_nanos, signed_price_delta_notional};

use crate::account::{Account, AccountId};
use crate::aggregates::CostBasisTracker;

/// A single position valued at current market prices.
pub struct PositionValue {
    pub market_id: MarketId,
    pub outcome: u8,
    pub quantity: i64,
    pub current_price_nanos: Nanos,
    /// quantity * current_price (signed)
    pub value_nanos: i64,
    /// Weighted-average entry price for this position (C1). `0` if there is
    /// no recorded cost-basis entry (e.g. positions opened before C1 or
    /// after a cold restart).
    pub avg_entry_price_nanos: u64,
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
    /// First-deposit timestamp in ms since epoch (B8). `0` if no
    /// deposit has been recorded for this account.
    pub first_deposit_ms: u64,
    /// All-time fill count for the account (B8). The bounded fill
    /// window can be smaller than this when trim has happened.
    pub total_fill_count: u64,
    /// Realized PnL across all closed positions (C1). Signed nanos.
    pub realized_pnl_nanos: i64,
    /// Mark-to-market PnL across currently open positions (C1).
    pub unrealized_pnl_nanos: i64,
}

/// Compute a portfolio summary for an account given current market prices
/// and the cost-basis tracker.
pub fn compute_portfolio(
    account: &Account,
    last_prices: &HashMap<MarketId, Vec<Nanos>>,
    first_deposit_ms: u64,
    total_fill_count: u64,
    cost_basis_tracker: &CostBasisTracker,
) -> PortfolioSummary {
    let mut positions = Vec::new();
    let mut total_position_value: i64 = 0;
    let mut unrealized: i128 = 0;

    for (&(market_id, outcome), &quantity) in &account.positions {
        if quantity == 0 {
            continue;
        }

        let price = last_prices
            .get(&market_id)
            .and_then(|p| p.get(outcome as usize).copied())
            .unwrap_or(matching_engine::Nanos(
                matching_engine::NANOS_PER_DOLLAR / 2,
            ));

        let value_nanos = signed_notional_nanos(price, quantity);
        total_position_value += value_nanos;

        let basis = cost_basis_tracker.cost_basis(account.id, market_id, outcome);
        unrealized +=
            signed_price_delta_notional(price.0 as i64 - basis, Qty(quantity.unsigned_abs()))
                as i128
                * quantity.signum() as i128;

        positions.push(PositionValue {
            market_id,
            outcome,
            quantity,
            current_price_nanos: price,
            value_nanos,
            avg_entry_price_nanos: basis.max(0) as u64,
        });
    }

    // Sort for deterministic output
    positions.sort_by_key(|p| (p.market_id.0, p.outcome));

    let portfolio_value = account.balance + total_position_value;
    let pnl = portfolio_value - account.total_deposited;
    let realized = cost_basis_tracker.realized_pnl(account.id);

    PortfolioSummary {
        account_id: account.id,
        balance_nanos: account.balance,
        total_deposited_nanos: account.total_deposited,
        positions,
        total_position_value_nanos: total_position_value,
        portfolio_value_nanos: portfolio_value,
        pnl_nanos: pnl,
        first_deposit_ms,
        total_fill_count,
        realized_pnl_nanos: realized,
        unrealized_pnl_nanos: unrealized as i64,
    }
}
