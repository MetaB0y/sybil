pub(crate) fn append_order(value: &mut Vec<u8>, order: &matching_engine::Order) {
    value.extend_from_slice(&order.id.to_le_bytes());
    value.push(order.num_markets);
    for market in order.markets.iter().take(order.num_markets as usize) {
        value.extend_from_slice(&market.0.to_le_bytes());
    }
    value.push(order.num_states);
    for payoff in order.payoffs.iter().take(order.num_states as usize) {
        value.extend_from_slice(&payoff.to_le_bytes());
    }
    value.extend_from_slice(&order.limit_price.to_le_bytes());
    value.extend_from_slice(&order.max_fill.to_le_bytes());
    match &order.condition {
        None => value.push(0),
        Some(condition) => {
            value.push(1);
            value.extend_from_slice(&condition.market.0.to_le_bytes());
            value.extend_from_slice(&condition.threshold.to_le_bytes());
            value.push(match condition.direction {
                matching_engine::ConditionDir::Above => 0,
                matching_engine::ConditionDir::Below => 1,
            });
        }
    }
    match order.expires_at_block {
        None => value.push(0),
        Some(expires_at_block) => {
            value.push(1);
            value.extend_from_slice(&expires_at_block.to_le_bytes());
        }
    }
}
