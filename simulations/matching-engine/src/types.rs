//! Core numeric types for the NP-hard matching system.
//!
//! Uses fixed-point arithmetic with nanodollars (1e-9 USD) to ensure
//! deterministic computation without floating-point issues.

use std::fmt;

/// 1 unit = 1 nanodollar = 1e-9 USD
pub const NANOS_PER_DOLLAR: u64 = 1_000_000_000;

/// Amount in nanodollars (max ~18 billion dollars with u64)
pub type Nanos = u64;

/// Quantity in shares/lots
pub type Qty = u64;

/// Market identifier
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
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
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
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

/// Helper functions for converting between Nanos and human-readable prices
pub mod conversions {
    use super::{Nanos, NANOS_PER_DOLLAR};

    /// Convert a decimal price (e.g., 0.53 for 53 cents) to nanos
    /// Price should be in [0, 1] for probability markets
    pub fn price_to_nanos(price: f64) -> Nanos {
        (price * NANOS_PER_DOLLAR as f64) as Nanos
    }

    /// Convert nanos back to a decimal price
    pub fn nanos_to_price(nanos: Nanos) -> f64 {
        nanos as f64 / NANOS_PER_DOLLAR as f64
    }

    /// Convert dollars to nanos
    pub fn dollars_to_nanos(dollars: f64) -> Nanos {
        (dollars * NANOS_PER_DOLLAR as f64) as Nanos
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
    use super::*;
    use super::conversions::*;

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
