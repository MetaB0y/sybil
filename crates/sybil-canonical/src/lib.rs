use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const MAX_MARKETS_PER_ORDER: usize = 5;
pub const MAX_STATES: usize = 32;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct MarketId(pub u32);

impl MarketId {
    pub const NONE: Self = Self(u32::MAX);
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum ConditionDir {
    Above,
    Below,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PriceCondition {
    pub market: MarketId,
    pub threshold: u64,
    pub direction: ConditionDir,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Order {
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],
    pub num_markets: u8,
    pub payoffs: [i8; MAX_STATES],
    pub num_states: u8,
    pub limit_price: u64,
    pub max_fill: u64,
    pub condition: Option<PriceCondition>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct CancelRequest {
    account_id: u64,
    order_id: u64,
}

pub fn canonical_order_bytes(order: &Order) -> Vec<u8> {
    borsh::to_vec(order).expect("canonical order serialization should not fail")
}

pub fn canonical_cancel_bytes(account_id: u64, order_id: u64) -> Vec<u8> {
    borsh::to_vec(&CancelRequest {
        account_id,
        order_id,
    })
    .expect("canonical cancel serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_with(
        markets: &[u32],
        payoffs: &[i8],
        limit_price: u64,
        max_fill: u64,
        condition: Option<PriceCondition>,
    ) -> Order {
        let mut order = Order {
            markets: [MarketId::NONE; MAX_MARKETS_PER_ORDER],
            num_markets: markets.len() as u8,
            payoffs: [0; MAX_STATES],
            num_states: (1usize << markets.len()) as u8,
            limit_price,
            max_fill,
            condition,
        };

        for (idx, market) in markets.iter().copied().enumerate() {
            order.markets[idx] = MarketId(market);
        }

        for (idx, payoff) in payoffs.iter().copied().enumerate() {
            order.payoffs[idx] = payoff;
        }

        order
    }

    #[test]
    fn buy_yes_snapshot() {
        let order = order_with(&[7], &[1, 0], 550_000_000, 10, None);
        insta::assert_snapshot!("buy_yes", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn sell_yes_snapshot() {
        let order = order_with(&[7], &[-1, 0], 425_000_000, 3, None);
        insta::assert_snapshot!("sell_yes", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn spread_snapshot() {
        let order = order_with(&[3, 9], &[0, -1, 1, 0], 125_000_000, 5, None);
        insta::assert_snapshot!("spread", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn bundle_snapshot() {
        let order = order_with(&[1, 2, 4], &[0, 0, 0, 0, 0, 0, 0, 1], 300_000_000, 2, None);
        insta::assert_snapshot!("bundle", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn conditional_snapshot() {
        let order = order_with(
            &[5],
            &[1, 0],
            610_000_000,
            9,
            Some(PriceCondition {
                market: MarketId(11),
                threshold: 490_000_000,
                direction: ConditionDir::Above,
            }),
        );
        insta::assert_snapshot!("conditional", hex::encode(canonical_order_bytes(&order)));
    }
}
