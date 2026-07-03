//! Core numeric types for the NP-hard matching system.
//!
//! Uses fixed-point arithmetic with nanodollars (1e-9 USD) to ensure
//! deterministic computation without floating-point issues.

use std::fmt;

use serde::{Deserialize, Serialize};

/// 1 unit = 1 nanodollar = 1e-9 USD
pub const NANOS_PER_DOLLAR: u64 = 1_000_000_000;

/// 1 share = 1000 quantity units. The smallest tradable quantity is 0.001
/// share, represented as `Qty = 1`.
pub const SHARE_SCALE: u64 = 1_000;

/// Maximum publicly admissible order quantity in share-units.
///
/// This is 1,000,000 full shares at `SHARE_SCALE = 1000`. At a $1 limit
/// price, one order's notional is capped at $1,000,000, which is ample for
/// current tests and devnet operation while bounding `price * quantity`.
pub const MAX_ORDER_QTY: u64 = 1_000_000 * SHARE_SCALE;

/// Amount in nanodollars (max ~18 billion dollars with u64)
pub type Nanos = u64;

/// Quantity in fixed-point share units (`SHARE_SCALE` units = 1 share).
pub type Qty = u64;

/// Convert whole shares into the internal fixed-point quantity unit.
pub const fn shares_to_qty(shares: u64) -> Qty {
    shares.saturating_mul(SHARE_SCALE)
}

/// Floor of `price_nanos * qty / SHARE_SCALE`, returned as `u64`.
pub fn notional_nanos(price: Nanos, qty: Qty) -> Nanos {
    ((price as u128 * qty as u128) / SHARE_SCALE as u128) as Nanos
}

/// Ceiling of `price_nanos * qty / SHARE_SCALE`, returned as `u64`.
///
/// Use this for reservations and budget checks, where under-reserving by even
/// one nano is worse than releasing a tiny surplus later.
pub fn notional_nanos_ceil(price: Nanos, qty: Qty) -> Nanos {
    let numerator = price as u128 * qty as u128;
    numerator.div_ceil(SHARE_SCALE as u128) as Nanos
}

/// Signed floor of `price_nanos * qty / SHARE_SCALE`.
pub fn signed_notional_nanos(price: Nanos, qty: i64) -> i64 {
    let abs = notional_nanos(price, qty.unsigned_abs()) as i64;
    if qty < 0 {
        -abs
    } else {
        abs
    }
}

/// Signed floor of `price_delta_nanos * qty / SHARE_SCALE`.
pub fn signed_price_delta_notional(price_delta: i64, qty: Qty) -> i64 {
    let abs = ((price_delta.unsigned_abs() as u128 * qty as u128) / SHARE_SCALE as u128) as i64;
    if price_delta < 0 {
        -abs
    } else {
        abs
    }
}

/// Market identifier
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct MarketId(pub u32);

impl MarketId {
    /// Sentinel value for unused market slots
    pub const NONE: Self = Self(u32::MAX);

    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }
}

impl fmt::Display for MarketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            write!(f, "NONE")
        } else {
            write!(f, "M{}", self.0)
        }
    }
}

/// Order side
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize)]
pub enum Side {
    Bid, // Buying
    Ask, // Selling
}

impl Side {
    pub fn opposite(&self) -> Side {
        match self {
            Side::Bid => Side::Ask,
            Side::Ask => Side::Bid,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Bid => write!(f, "BID"),
            Side::Ask => write!(f, "ASK"),
        }
    }
}

/// Categorical direction of an order with respect to its primary binary market.
///
/// Distinct from `Side` (which is bid/ask in the abstract solver). `OrderDirection`
/// surfaces in the on-chain `OrderCancelled` system event and the FE-facing cancel
/// stream, where users think in terms of YES/NO and buy/sell — not bid/ask.
///
/// For multi-market orders (spreads, bundles, etc.) the direction reflects the
/// primary market's dominant exposure as derived by `derive_order_direction`. It
/// is a categorical label, not an exhaustive description of the order's payoff
/// structure.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum OrderDirection {
    BuyYes,
    SellYes,
    BuyNo,
    SellNo,
}

impl OrderDirection {
    /// Stable single-byte encoding used inside witness leaves committed by
    /// `events_root`. The mapping is fixed for the lifetime of the protocol:
    /// `BuyYes=0, SellYes=1, BuyNo=2, SellNo=3`.
    pub fn to_byte(&self) -> u8 {
        match self {
            OrderDirection::BuyYes => 0,
            OrderDirection::SellYes => 1,
            OrderDirection::BuyNo => 2,
            OrderDirection::SellNo => 3,
        }
    }
}

impl fmt::Display for OrderDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderDirection::BuyYes => write!(f, "BuyYes"),
            OrderDirection::SellYes => write!(f, "SellYes"),
            OrderDirection::BuyNo => write!(f, "BuyNo"),
            OrderDirection::SellNo => write!(f, "SellNo"),
        }
    }
}

/// Helper functions for converting between Nanos and human-readable prices
pub mod conversions {
    use super::{Nanos, NANOS_PER_DOLLAR};

    /// Convert a decimal price (e.g., 0.53 for 53 cents) to nanos
    /// Price should be in [0, 1] for probability markets
    pub fn price_to_nanos(price: f64) -> Nanos {
        (price * NANOS_PER_DOLLAR as f64).round() as Nanos
    }

    /// Convert nanos back to a decimal price
    pub fn nanos_to_price(nanos: Nanos) -> f64 {
        nanos as f64 / NANOS_PER_DOLLAR as f64
    }

    /// Convert dollars to nanos
    pub fn dollars_to_nanos(dollars: f64) -> Nanos {
        (dollars * NANOS_PER_DOLLAR as f64).round() as Nanos
    }

    /// Convert nanos to dollars
    pub fn nanos_to_dollars(nanos: Nanos) -> f64 {
        nanos as f64 / NANOS_PER_DOLLAR as f64
    }

    /// Format nanos as a price string (e.g., "0.53")
    pub fn format_price(nanos: Nanos) -> String {
        format!("{:.4}", nanos_to_price(nanos))
    }

    /// Format nanos as a dollar amount string (e.g., "$1.23")
    pub fn format_dollars(nanos: Nanos) -> String {
        format!("${:.2}", nanos_to_dollars(nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::conversions::*;
    use super::*;

    #[test]
    fn test_market_id() {
        let m0 = MarketId::new(0);
        let m1 = MarketId::new(1);
        let none = MarketId::NONE;

        assert!(!m0.is_none());
        assert!(!m1.is_none());
        assert!(none.is_none());
        assert_ne!(m0, m1);
    }

    #[test]
    fn test_side() {
        assert_eq!(Side::Bid.opposite(), Side::Ask);
        assert_eq!(Side::Ask.opposite(), Side::Bid);
    }

    #[test]
    fn test_price_conversions() {
        let price = 0.53;
        let nanos = price_to_nanos(price);
        let recovered = nanos_to_price(nanos);
        assert!((price - recovered).abs() < 1e-9);
    }

    #[test]
    fn test_dollar_conversions() {
        let dollars = 100.50;
        let nanos = dollars_to_nanos(dollars);
        let recovered = nanos_to_dollars(nanos);
        assert!((dollars - recovered).abs() < 1e-9);
    }
}
