use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use crate::types::*;

/// Analyze what JIT opportunity exists
pub fn analyze_jit_opportunity(solution: &Solution, orders: &[Order]) -> JitOpportunity {
    let price = solution.clearing_price;

    // Unfilled buy demand: buyers willing at clearing price but didn't fully fill
    let mut unfilled_buy = dec!(0);
    let mut best_unfilled_buy_price: Option<Decimal> = None;

    for order in orders.iter().filter(|o| o.side == Side::Buy && o.limit_price >= price) {
        let filled = solution.fills
            .iter()
            .find(|f| f.order_id == order.id)
            .map(|f| f.quantity)
            .unwrap_or(dec!(0));

        let unfilled = order.quantity - filled;
        if unfilled > dec!(0) {
            unfilled_buy += unfilled;
            best_unfilled_buy_price = Some(
                best_unfilled_buy_price
                    .map(|p| p.max(order.limit_price))
                    .unwrap_or(order.limit_price)
            );
        }
    }

    // Unfilled sell supply: sellers willing at clearing price but didn't fully fill
    let mut unfilled_sell = dec!(0);
    let mut best_unfilled_sell_price: Option<Decimal> = None;

    for order in orders.iter().filter(|o| o.side == Side::Sell && o.limit_price <= price) {
        let filled = solution.fills
            .iter()
            .find(|f| f.order_id == order.id)
            .map(|f| f.quantity)
            .unwrap_or(dec!(0));

        let unfilled = order.quantity - filled;
        if unfilled > dec!(0) {
            unfilled_sell += unfilled;
            best_unfilled_sell_price = Some(
                best_unfilled_sell_price
                    .map(|p| p.min(order.limit_price))
                    .unwrap_or(order.limit_price)
            );
        }
    }

    // Note: orders with limit < clearing for buys (or > for sells)
    // are NOT unfilled at current price, they simply weren't willing to trade

    JitOpportunity {
        unfilled_buy,
        unfilled_sell,
        best_unfilled_buy_price,
        best_unfilled_sell_price,
    }
}

/// Backrun strategy: only fill unfilled demand, no displacement
pub fn backrun_strategy(opp: &JitOpportunity, base: &Solution) -> Option<Order> {
    // No clearing happened, can't backrun
    if base.total_volume == dec!(0) {
        return None;
    }

    // If there's unfilled buy demand, JIT can sell to them
    if opp.unfilled_buy > dec!(0) {
        // Price at or below clearing to ensure fill
        let price = base.clearing_price;
        return Some(Order::jit(Side::Sell, opp.unfilled_buy, price));
    }

    // If there's unfilled sell supply, JIT can buy from them
    if opp.unfilled_sell > dec!(0) {
        let price = base.clearing_price;
        return Some(Order::jit(Side::Buy, opp.unfilled_sell, price));
    }

    None
}

/// Aggressive strategy: try to capture more by displacing passive orders
pub fn aggressive_strategy(
    opp: &JitOpportunity,
    base: &Solution,
    orders: &[Order]
) -> Option<Order> {
    // No clearing happened, can't be aggressive
    if base.total_volume == dec!(0) {
        return None;
    }

    // Find worst-priced passive order on each side
    let price = base.clearing_price;

    // If there are passive sellers at clearing price, try to undercut them
    let passive_sells: Vec<&Order> = orders
        .iter()
        .filter(|o| o.side == Side::Sell && o.limit_price <= price)
        .collect();

    if !passive_sells.is_empty() {
        // Total sell volume we could capture
        let passive_sell_volume: Decimal = passive_sells.iter().map(|o| o.quantity).sum();

        // Offer at same price, larger quantity to displace
        if passive_sell_volume > dec!(0) {
            // Try to capture half the passive volume plus unfilled
            let target = passive_sell_volume / dec!(2) + opp.unfilled_buy;
            if target > opp.unfilled_buy {
                return Some(Order::jit(Side::Sell, target, price));
            }
        }
    }

    // Same for buy side
    let passive_buys: Vec<&Order> = orders
        .iter()
        .filter(|o| o.side == Side::Buy && o.limit_price >= price)
        .collect();

    if !passive_buys.is_empty() {
        let passive_buy_volume: Decimal = passive_buys.iter().map(|o| o.quantity).sum();

        if passive_buy_volume > dec!(0) {
            let target = passive_buy_volume / dec!(2) + opp.unfilled_sell;
            if target > opp.unfilled_sell {
                return Some(Order::jit(Side::Buy, target, price));
            }
        }
    }

    None
}

/// Calculate MM profit from JIT order
#[allow(dead_code)]
pub fn calculate_jit_profit(
    jit: &Order,
    solution: &Solution,
    true_value: Option<Decimal>
) -> Decimal {
    let fill = solution.fills
        .iter()
        .find(|f| f.order_id == jit.id);

    match fill {
        Some(f) => {
            let exec_price = f.price;
            let true_val = true_value.unwrap_or(exec_price);

            match jit.side {
                // Sold at exec_price, true value is true_val
                // Profit if sold above true value
                Side::Sell => (exec_price - true_val) * f.quantity,
                // Bought at exec_price, true value is true_val
                // Profit if bought below true value
                Side::Buy => (true_val - exec_price) * f.quantity,
            }
        }
        None => dec!(0),
    }
}
