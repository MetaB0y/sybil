use std::collections::HashMap;

use matching_engine::order::{MAX_MARKETS_PER_ORDER, MAX_STATES};
use matching_engine::{
    bundle_sell, bundle_yes, outcome_buy, outcome_sell, spread, MarketId, MarketSet, Nanos, Order,
    NANOS_PER_DOLLAR,
};
use matching_sequencer::block::Block;
use matching_sequencer::error::Rejection;
use matching_sequencer::Account;

use crate::types::request::{OrderSpec, SignedOrderData};
use crate::types::response::*;

/// Convert an Account to an AccountResponse.
pub fn account_to_response(account: &Account) -> AccountResponse {
    let positions: Vec<PositionResponse> = account
        .positions
        .iter()
        .filter(|(_, &qty)| qty != 0)
        .map(|(&(market_id, outcome), &qty)| PositionResponse {
            market_id: market_id.0,
            outcome: if outcome == 0 {
                "YES".to_string()
            } else {
                "NO".to_string()
            },
            quantity: qty,
        })
        .collect();

    AccountResponse {
        account_id: account.id.0,
        balance_nanos: account.balance,
        positions,
    }
}

/// Convert a Block to a BlockResponse.
pub fn block_to_response(block: &Block) -> BlockResponse {
    let fills = block
        .fills
        .iter()
        .map(|f| FillResponse {
            order_id: f.order_id,
            fill_qty: f.fill_qty,
            fill_price_nanos: f.fill_price,
        })
        .collect();

    let clearing_prices_nanos: HashMap<String, Vec<u64>> = block
        .clearing_prices
        .iter()
        .map(|(mid, prices)| (mid.0.to_string(), prices.to_vec()))
        .collect();

    let rejections = block.rejections.iter().map(rejection_to_response).collect();

    BlockResponse {
        height: block.header.height,
        parent_hash: hex::encode(block.header.parent_hash),
        state_root: hex::encode(block.header.state_root),
        order_count: block.header.order_count,
        fill_count: block.header.fill_count,
        timestamp_ms: block.header.timestamp_ms,
        fills,
        clearing_prices_nanos,
        rejections,
        total_welfare_nanos: block.total_welfare,
        total_volume_nanos: block.total_volume,
        orders_filled: block.orders_filled,
    }
}

fn rejection_to_response(r: &Rejection) -> RejectionResponse {
    RejectionResponse {
        order_id: r.order_id,
        account_id: r.account_id.0,
        reason: format!("{:?}", r.reason),
    }
}

/// Convert market prices map to response format.
pub fn prices_to_response(prices: &HashMap<MarketId, Vec<Nanos>>) -> MarketPricesResponse {
    let mut map = HashMap::new();
    for (mid, ps) in prices {
        let yes_price_nanos = ps.first().copied().unwrap_or(NANOS_PER_DOLLAR / 2);
        let no_price_nanos = ps.get(1).copied().unwrap_or(NANOS_PER_DOLLAR / 2);
        map.insert(
            mid.0.to_string(),
            MarketPriceResponse {
                yes_price_nanos,
                no_price_nanos,
            },
        );
    }
    MarketPricesResponse { prices: map }
}

/// Convert an OrderSpec from the API into an internal Order.
pub fn order_spec_to_order(spec: &OrderSpec, markets: &MarketSet) -> Result<Order, String> {
    match spec {
        OrderSpec::BuyYes {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            Ok(outcome_buy(
                markets,
                0,
                mid,
                0,
                *limit_price_nanos,
                *quantity,
            ))
        }
        OrderSpec::BuyNo {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            Ok(outcome_buy(
                markets,
                0,
                mid,
                1,
                *limit_price_nanos,
                *quantity,
            ))
        }
        OrderSpec::SellYes {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            Ok(outcome_sell(
                markets,
                0,
                mid,
                0,
                *limit_price_nanos,
                *quantity,
            ))
        }
        OrderSpec::SellNo {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            Ok(outcome_sell(
                markets,
                0,
                mid,
                1,
                *limit_price_nanos,
                *quantity,
            ))
        }
        OrderSpec::Spread {
            market_a,
            market_b,
            limit_price_nanos,
            quantity,
        } => {
            let ma = MarketId::new(*market_a);
            let mb = MarketId::new(*market_b);
            validate_market(ma, markets)?;
            validate_market(mb, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            Ok(spread(markets, 0, ma, mb, *limit_price_nanos, *quantity))
        }
        OrderSpec::BundleYes {
            market_ids,
            limit_price_nanos,
            quantity,
        } => {
            let mids: Vec<MarketId> = market_ids.iter().map(|&id| MarketId::new(id)).collect();
            for &mid in &mids {
                validate_market(mid, markets)?;
            }
            validate_price_nanos(*limit_price_nanos)?;
            if mids.len() > MAX_MARKETS_PER_ORDER {
                return Err(format!(
                    "Bundle cannot span more than {} markets",
                    MAX_MARKETS_PER_ORDER
                ));
            }
            Ok(bundle_yes(markets, 0, &mids, *limit_price_nanos, *quantity))
        }
        OrderSpec::BundleSell {
            market_ids,
            limit_price_nanos,
            quantity,
        } => {
            let mids: Vec<MarketId> = market_ids.iter().map(|&id| MarketId::new(id)).collect();
            for &mid in &mids {
                validate_market(mid, markets)?;
            }
            validate_price_nanos(*limit_price_nanos)?;
            if mids.len() > MAX_MARKETS_PER_ORDER {
                return Err(format!(
                    "Bundle cannot span more than {} markets",
                    MAX_MARKETS_PER_ORDER
                ));
            }
            Ok(bundle_sell(
                markets,
                0,
                &mids,
                *limit_price_nanos,
                *quantity,
            ))
        }
        OrderSpec::Custom {
            market_ids,
            payoffs,
            limit_price_nanos,
            min_fill,
            max_fill,
        } => {
            let mids: Vec<MarketId> = market_ids.iter().map(|&id| MarketId::new(id)).collect();
            for &mid in &mids {
                validate_market(mid, markets)?;
            }
            validate_price_nanos(*limit_price_nanos)?;
            if mids.len() > MAX_MARKETS_PER_ORDER {
                return Err(format!(
                    "Custom order cannot span more than {} markets",
                    MAX_MARKETS_PER_ORDER
                ));
            }
            if payoffs.len() > MAX_STATES {
                return Err(format!("Payoff vector cannot exceed {} states", MAX_STATES));
            }

            let mut order = Order::new(0);
            for (i, &mid) in mids.iter().enumerate() {
                order.markets[i] = mid;
            }
            order.num_markets = mids.len() as u8;
            let num_states = 1usize << mids.len();
            order.num_states = num_states as u8;

            for (i, &p) in payoffs.iter().enumerate() {
                if i < MAX_STATES {
                    order.payoffs[i] = p;
                }
            }

            order.limit_price = *limit_price_nanos;
            order.min_fill = *min_fill;
            order.max_fill = *max_fill;

            Ok(order)
        }
    }
}

/// Convert a SignedOrderData to an internal Order.
pub fn signed_order_data_to_order(data: &SignedOrderData) -> Result<Order, String> {
    if data.market_ids.len() > MAX_MARKETS_PER_ORDER {
        return Err(format!(
            "Order cannot span more than {} markets",
            MAX_MARKETS_PER_ORDER
        ));
    }
    if data.payoffs.len() > MAX_STATES {
        return Err(format!("Payoff vector cannot exceed {} states", MAX_STATES));
    }
    validate_price_nanos(data.limit_price_nanos)?;

    let mut order = Order::new(0);
    for (i, &mid) in data.market_ids.iter().enumerate() {
        order.markets[i] = MarketId::new(mid);
    }
    order.num_markets = data.market_ids.len() as u8;
    let num_states = 1usize << data.market_ids.len();
    order.num_states = num_states as u8;

    for (i, &p) in data.payoffs.iter().enumerate() {
        if i < MAX_STATES {
            order.payoffs[i] = p;
        }
    }

    order.limit_price = data.limit_price_nanos;
    order.min_fill = data.min_fill;
    order.max_fill = data.max_fill;

    Ok(order)
}

fn validate_market(mid: MarketId, markets: &MarketSet) -> Result<(), String> {
    if markets.get(mid).is_none() {
        return Err(format!("Market {} not found", mid.0));
    }
    Ok(())
}

fn validate_price_nanos(price_nanos: u64) -> Result<(), String> {
    if price_nanos > NANOS_PER_DOLLAR {
        return Err(format!(
            "Price must be between 0 and {} nanos, got {}",
            NANOS_PER_DOLLAR, price_nanos
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::MarketSet;
    use matching_sequencer::AccountId;

    fn make_markets() -> MarketSet {
        let mut ms = MarketSet::new();
        ms.add_binary("Market A");
        ms.add_binary("Market B");
        ms
    }

    #[test]
    fn test_buy_yes_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 0,
            limit_price_nanos: 550_000_000,
            quantity: 10,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.num_markets, 1);
        assert_eq!(order.markets[0], MarketId::new(0));
        assert_eq!(order.payoffs[0], 1); // YES payoff
        assert_eq!(order.payoffs[1], 0); // NO payoff
        assert_eq!(order.max_fill, 10);
    }

    #[test]
    fn test_sell_yes_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::SellYes {
            market_id: 0,
            limit_price_nanos: 600_000_000,
            quantity: 5,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.payoffs[0], -1); // Selling YES
        assert_eq!(order.max_fill, 5);
    }

    #[test]
    fn test_spread_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::Spread {
            market_a: 0,
            market_b: 1,
            limit_price_nanos: 100_000_000,
            quantity: 10,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.num_markets, 2);
        assert_eq!(order.num_states, 4);
    }

    #[test]
    fn test_invalid_market_rejected() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 99,
            limit_price_nanos: 550_000_000,
            quantity: 10,
        };
        assert!(order_spec_to_order(&spec, &ms).is_err());
    }

    #[test]
    fn test_invalid_price_rejected() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 0,
            limit_price_nanos: 1_500_000_000, // > NANOS_PER_DOLLAR
            quantity: 10,
        };
        assert!(order_spec_to_order(&spec, &ms).is_err());
    }

    #[test]
    fn test_account_to_response() {
        let mut account = Account::new(AccountId(42), 100 * NANOS_PER_DOLLAR as i64);
        account.positions.insert((MarketId::new(0), 0), 10);

        let resp = account_to_response(&account);
        assert_eq!(resp.account_id, 42);
        assert_eq!(resp.balance_nanos, 100 * NANOS_PER_DOLLAR as i64);
        assert_eq!(resp.positions.len(), 1);
    }

    #[test]
    fn test_custom_order_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::Custom {
            market_ids: vec![0, 1],
            payoffs: vec![1, 0, 0, 0],
            limit_price_nanos: 200_000_000,
            min_fill: 0,
            max_fill: 10,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.num_markets, 2);
        assert_eq!(order.num_states, 4);
        assert_eq!(order.payoffs[0], 1);
        assert_eq!(order.payoffs[1], 0);
        assert_eq!(order.max_fill, 10);
    }
}
