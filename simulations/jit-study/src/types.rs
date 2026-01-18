use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    #[allow(dead_code)]
    pub fn opposite(&self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: u32,
    pub side: Side,
    pub quantity: Decimal,
    pub limit_price: Decimal,
    #[allow(dead_code)]
    pub is_jit: bool,
}

impl Order {
    pub fn buy(id: u32, qty: impl Into<Decimal>, price: impl Into<Decimal>) -> Self {
        Order {
            id,
            side: Side::Buy,
            quantity: qty.into(),
            limit_price: price.into(),
            is_jit: false,
        }
    }

    pub fn sell(id: u32, qty: impl Into<Decimal>, price: impl Into<Decimal>) -> Self {
        Order {
            id,
            side: Side::Sell,
            quantity: qty.into(),
            limit_price: price.into(),
            is_jit: false,
        }
    }

    pub fn jit(side: Side, qty: impl Into<Decimal>, price: impl Into<Decimal>) -> Self {
        Order {
            id: 9999,
            side,
            quantity: qty.into(),
            limit_price: price.into(),
            is_jit: true,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Fill {
    pub order_id: u32,
    pub quantity: Decimal,
    pub price: Decimal,
}

#[derive(Debug, Clone)]
pub struct Solution {
    pub clearing_price: Decimal,
    pub fills: Vec<Fill>,
    pub total_volume: Decimal,
    pub welfare: Decimal,
}

impl Solution {
    pub fn empty() -> Self {
        Solution {
            clearing_price: dec!(0),
            fills: vec![],
            total_volume: dec!(0),
            welfare: dec!(0),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct JitOpportunity {
    pub unfilled_buy: Decimal,
    pub unfilled_sell: Decimal,
    pub best_unfilled_buy_price: Option<Decimal>,
    pub best_unfilled_sell_price: Option<Decimal>,
}

pub struct Scenario {
    pub name: &'static str,
    pub orders: Vec<Order>,
    pub true_value: Decimal,
}
