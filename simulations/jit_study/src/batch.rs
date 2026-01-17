use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use crate::types::*;

/// Solve batch auction with uniform price clearing
pub fn solve_batch(orders: &[Order]) -> Solution {
    solve_batch_with_jit(orders, &None)
}

pub fn solve_batch_with_jit(orders: &[Order], jit: &Option<Order>) -> Solution {
    let mut all_orders: Vec<Order> = orders.to_vec();
    if let Some(j) = jit {
        all_orders.push(j.clone());
    }

    // Find clearing price that maximizes volume
    // Simple approach: try all limit prices as candidate clearing prices
    let mut candidate_prices: Vec<Decimal> = all_orders
        .iter()
        .map(|o| o.limit_price)
        .collect();
    candidate_prices.sort();
    candidate_prices.dedup();

    if candidate_prices.is_empty() {
        return Solution::empty();
    }

    // Check if any crossing is possible
    let max_bid = all_orders
        .iter()
        .filter(|o| o.side == Side::Buy)
        .map(|o| o.limit_price)
        .max();
    let min_ask = all_orders
        .iter()
        .filter(|o| o.side == Side::Sell)
        .map(|o| o.limit_price)
        .min();

    // No crossing possible
    if let (Some(mb), Some(ma)) = (max_bid, min_ask) {
        if mb < ma {
            return Solution::empty();
        }
    }

    // Find all prices that achieve maximum volume
    let mut best_volume = dec!(0);
    let mut valid_prices: Vec<Decimal> = vec![];

    for &price in &candidate_prices {
        let solution = clear_at_price(&all_orders, price);
        if solution.total_volume > best_volume {
            best_volume = solution.total_volume;
            valid_prices.clear();
            valid_prices.push(price);
        } else if solution.total_volume == best_volume && best_volume > dec!(0) {
            valid_prices.push(price);
        }
    }

    if valid_prices.is_empty() || best_volume == dec!(0) {
        return Solution::empty();
    }

    // Find the range of valid clearing prices
    let min_valid = *valid_prices.first().unwrap();
    let max_valid = *valid_prices.last().unwrap();

    // Use midpoint of valid range (fair to both sides)
    let clearing_price = (min_valid + max_valid) / dec!(2);

    clear_at_price(&all_orders, clearing_price)
}

fn clear_at_price(orders: &[Order], price: Decimal) -> Solution {
    // Buy orders willing to trade: limit_price >= clearing_price
    let buy_demand: Decimal = orders
        .iter()
        .filter(|o| o.side == Side::Buy && o.limit_price >= price)
        .map(|o| o.quantity)
        .sum();

    // Sell orders willing to trade: limit_price <= clearing_price
    let sell_supply: Decimal = orders
        .iter()
        .filter(|o| o.side == Side::Sell && o.limit_price <= price)
        .map(|o| o.quantity)
        .sum();

    // Volume is min of demand and supply
    let volume = buy_demand.min(sell_supply);

    if volume == dec!(0) {
        return Solution {
            clearing_price: price,
            fills: vec![],
            total_volume: dec!(0),
            welfare: dec!(0),
        };
    }

    // Pro-rata fill for each side
    let mut fills = vec![];
    let mut welfare = dec!(0);

    // Fill buys (pro-rata if needed)
    let buy_orders: Vec<&Order> = orders
        .iter()
        .filter(|o| o.side == Side::Buy && o.limit_price >= price)
        .collect();

    for order in &buy_orders {
        let fill_qty = if buy_demand > volume {
            order.quantity * volume / buy_demand
        } else {
            order.quantity
        };

        if fill_qty > dec!(0) {
            // Buyer surplus = (limit_price - clearing_price) * qty
            welfare += (order.limit_price - price) * fill_qty;
            fills.push(Fill {
                order_id: order.id,
                quantity: fill_qty,
                price,
            });
        }
    }

    // Fill sells (pro-rata if needed)
    let sell_orders: Vec<&Order> = orders
        .iter()
        .filter(|o| o.side == Side::Sell && o.limit_price <= price)
        .collect();

    for order in &sell_orders {
        let fill_qty = if sell_supply > volume {
            order.quantity * volume / sell_supply
        } else {
            order.quantity
        };

        if fill_qty > dec!(0) {
            // Seller surplus = (clearing_price - limit_price) * qty
            welfare += (price - order.limit_price) * fill_qty;
            fills.push(Fill {
                order_id: order.id,
                quantity: fill_qty,
                price,
            });
        }
    }

    Solution {
        clearing_price: price,
        fills,
        total_volume: volume,
        welfare,
    }
}
