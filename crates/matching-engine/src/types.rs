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

/// Amount in nanodollars (max ~18 billion dollars with `u64`).
///
/// A newtype over `u64` (SYB-196). Money on the consensus / state-root path is
/// fixed-point integer nanodollars; the newtype makes it impossible to
/// accidentally mix a money amount with a bare count or a [`Qty`], and channels
/// all scaling through the checked helpers below. `#[serde(transparent)]`
/// guarantees the serialized bytes are identical to the inner `u64`, so
/// canonical encodings and golden vectors are unaffected.
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Nanos(pub u64);

/// Quantity in fixed-point share units (`SHARE_SCALE` units = 1 share).
///
/// Newtype over `u64` (SYB-196); see [`Nanos`] for the rationale.
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Qty(pub u64);

impl Nanos {
    pub const ZERO: Self = Nanos(0);

    /// The inner `u64`. Use at boundaries that still speak in raw integers
    /// (canonical byte encodings, the floating-point solver, external APIs).
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn saturating_add(self, rhs: Nanos) -> Nanos {
        Nanos(self.0.saturating_add(rhs.0))
    }

    #[inline]
    pub const fn checked_add(self, rhs: Nanos) -> Option<Nanos> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Nanos(value)),
            None => None,
        }
    }

    #[inline]
    pub const fn saturating_sub(self, rhs: Nanos) -> Nanos {
        Nanos(self.0.saturating_sub(rhs.0))
    }

    #[inline]
    pub const fn checked_sub(self, rhs: Nanos) -> Option<Nanos> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Nanos(value)),
            None => None,
        }
    }
}

impl Qty {
    pub const ZERO: Self = Qty(0);

    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn saturating_add(self, rhs: Qty) -> Qty {
        Qty(self.0.saturating_add(rhs.0))
    }

    #[inline]
    pub const fn checked_add(self, rhs: Qty) -> Option<Qty> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Qty(value)),
            None => None,
        }
    }

    #[inline]
    pub const fn saturating_sub(self, rhs: Qty) -> Qty {
        Qty(self.0.saturating_sub(rhs.0))
    }

    #[inline]
    pub const fn checked_sub(self, rhs: Qty) -> Option<Qty> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Qty(value)),
            None => None,
        }
    }
}

impl fmt::Display for Nanos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Add for Nanos {
    type Output = Nanos;
    #[inline]
    fn add(self, rhs: Nanos) -> Nanos {
        Nanos(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Nanos {
    type Output = Nanos;
    #[inline]
    fn sub(self, rhs: Nanos) -> Nanos {
        Nanos(self.0 - rhs.0)
    }
}

impl std::ops::AddAssign for Nanos {
    #[inline]
    fn add_assign(&mut self, rhs: Nanos) {
        self.0 += rhs.0;
    }
}

impl std::ops::SubAssign for Nanos {
    #[inline]
    fn sub_assign(&mut self, rhs: Nanos) {
        self.0 -= rhs.0;
    }
}

impl std::iter::Sum for Nanos {
    fn sum<I: Iterator<Item = Nanos>>(iter: I) -> Nanos {
        Nanos(iter.map(|n| n.0).sum())
    }
}

impl std::ops::Add for Qty {
    type Output = Qty;
    #[inline]
    fn add(self, rhs: Qty) -> Qty {
        Qty(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Qty {
    type Output = Qty;
    #[inline]
    fn sub(self, rhs: Qty) -> Qty {
        Qty(self.0 - rhs.0)
    }
}

impl std::iter::Sum for Qty {
    fn sum<I: Iterator<Item = Qty>>(iter: I) -> Qty {
        Qty(iter.map(|q| q.0).sum())
    }
}

/// Convert whole shares into the internal fixed-point quantity unit.
pub const fn shares_to_qty(shares: u64) -> Qty {
    Qty(shares.saturating_mul(SHARE_SCALE))
}

/// Floor of `price_nanos * qty / SHARE_SCALE`, returned as [`Nanos`].
pub fn notional_nanos(price: Nanos, qty: Qty) -> Nanos {
    Nanos(((price.0 as u128 * qty.0 as u128) / SHARE_SCALE as u128) as u64)
}

/// Checked floor of `price_nanos * qty / SHARE_SCALE`, returned as [`Nanos`].
pub fn checked_notional_nanos(price: Nanos, qty: Qty) -> Option<Nanos> {
    let numerator = i128::from(price.0).checked_mul(i128::from(qty.0))?;
    let scaled = numerator.checked_div(i128::from(SHARE_SCALE))?;
    u64::try_from(scaled).ok().map(Nanos)
}

/// Checked floor of `price_nanos * qty / SHARE_SCALE`, returned as `i64`.
pub fn checked_notional_i64(price: Nanos, qty: Qty) -> Option<i64> {
    let nanos = checked_notional_nanos(price, qty)?;
    i64::try_from(nanos.0).ok()
}

/// Ceiling of `price_nanos * qty / SHARE_SCALE`, returned as [`Nanos`].
///
/// Use this for reservations and budget checks, where under-reserving by even
/// one nano is worse than releasing a tiny surplus later.
pub fn notional_nanos_ceil(price: Nanos, qty: Qty) -> Nanos {
    let numerator = price.0 as u128 * qty.0 as u128;
    Nanos(numerator.div_ceil(SHARE_SCALE as u128) as u64)
}

/// Checked ceiling of `price_nanos * qty / SHARE_SCALE`, returned as [`Nanos`].
pub fn checked_notional_nanos_ceil(price: Nanos, qty: Qty) -> Option<Nanos> {
    let numerator = i128::from(price.0).checked_mul(i128::from(qty.0))?;
    let rounded = numerator.checked_add(i128::from(SHARE_SCALE - 1))? / i128::from(SHARE_SCALE);
    u64::try_from(rounded).ok().map(Nanos)
}

/// Checked ceiling of `price_nanos * qty / SHARE_SCALE`, returned as `i64`.
pub fn checked_notional_ceil_i64(price: Nanos, qty: Qty) -> Option<i64> {
    let nanos = checked_notional_nanos_ceil(price, qty)?;
    i64::try_from(nanos.0).ok()
}

/// Exact integer `ceil(value * numer / denom)` computed with `i128`
/// intermediates.
///
/// This is the **consensus-canonical** way to scale a reservation down by the
/// fraction `numer / denom` (e.g. releasing part of a resting order's balance
/// or position reservation on a partial fill). The result is committed into the
/// state root via `RestingOrderSnapshot` / `AccountReservationSnapshot` leaves,
/// so every re-execution (sequencer, verifier, guest) must derive it bit for
/// bit identically.
///
/// # Why this replaced an `f64` path (SEQ-2)
///
/// The historical code computed `(value as f64 * (numer as f64 / denom as f64))
/// .ceil()`. Above `2^53` nanodollars an `f64` can no longer represent every
/// integer, so an independent re-execution could round differently and derive a
/// different root — a soundness break on the state-root path. Even *inside* the
/// "sane" range (`< 2^53`) the `f64` path is not equal to exact integer ceiling:
/// e.g. `value=2997, numer=17, denom=999` gives `ceil(51.000000000000007) = 52`
/// in `f64` but the exact integer ceiling is `51`. This function returns the
/// exact integer value; that is the intended (devnet-stage) consensus
/// definition, and [`tests::ceil_mul_ratio_pins_f64_divergence`] pins the
/// divergence case so it can never be "fixed" back to the float behaviour.
///
/// # Panics / overflow
///
/// `value`, `numer`, `denom` are widened to `u128`, so `value * numer` cannot
/// overflow for any `u64` operands. Panics if `denom == 0` (callers guarantee a
/// non-zero denominator, e.g. `max_fill > 0`).
pub fn ceil_mul_ratio(value: u64, numer: u64, denom: u64) -> u64 {
    assert!(denom != 0, "ceil_mul_ratio: denom must be non-zero");
    let numerator = value as u128 * numer as u128;
    numerator.div_ceil(denom as u128) as u64
}

/// Signed floor of `price_nanos * qty / SHARE_SCALE`.
pub fn signed_notional_nanos(price: Nanos, qty: i64) -> i64 {
    let abs = notional_nanos(price, Qty(qty.unsigned_abs())).0 as i64;
    if qty < 0 { -abs } else { abs }
}

/// Checked signed floor of `price_nanos * qty / SHARE_SCALE`.
pub fn checked_signed_notional_nanos(price: Nanos, qty: i64) -> Option<i64> {
    let abs = checked_notional_i64(price, Qty(qty.unsigned_abs()))?;
    if qty < 0 {
        abs.checked_neg()
    } else {
        Some(abs)
    }
}

/// Signed floor of `price_delta_nanos * qty / SHARE_SCALE`.
pub fn signed_price_delta_notional(price_delta: i64, qty: Qty) -> i64 {
    let abs = ((price_delta.unsigned_abs() as u128 * qty.0 as u128) / SHARE_SCALE as u128) as i64;
    if price_delta < 0 { -abs } else { abs }
}

/// Checked signed floor of `price_delta_nanos * qty / SHARE_SCALE`.
pub fn checked_signed_price_delta_notional(price_delta: i64, qty: Qty) -> Option<i64> {
    let abs = checked_notional_i64(Nanos(price_delta.unsigned_abs()), qty)?;
    if price_delta < 0 {
        abs.checked_neg()
    } else {
        Some(abs)
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
// Exempt from the f64 ban: these are display/UX conversions (human-readable
// dollar/price formatting and parsing). They never produce a value committed
// into the state root — money on the consensus path stays integer nanos.
#[allow(clippy::disallowed_types)]
pub mod conversions {
    use super::{NANOS_PER_DOLLAR, Nanos};

    /// Convert a decimal price (e.g., 0.53 for 53 cents) to nanos
    /// Price should be in [0, 1] for probability markets
    pub fn price_to_nanos(price: f64) -> Nanos {
        Nanos((price * NANOS_PER_DOLLAR as f64).round() as u64)
    }

    /// Convert nanos back to a decimal price
    pub fn nanos_to_price(nanos: Nanos) -> f64 {
        nanos.0 as f64 / NANOS_PER_DOLLAR as f64
    }

    /// Convert dollars to nanos
    pub fn dollars_to_nanos(dollars: f64) -> Nanos {
        Nanos((dollars * NANOS_PER_DOLLAR as f64).round() as u64)
    }

    /// Convert nanos to dollars
    pub fn nanos_to_dollars(nanos: Nanos) -> f64 {
        nanos.0 as f64 / NANOS_PER_DOLLAR as f64
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

    /// Pins the exact-integer semantics of [`ceil_mul_ratio`] against the
    /// historical `f64` reservation-scaling path (SEQ-2). The old code was
    /// `(value as f64 * (numer as f64 / denom as f64)).ceil() as i64`, which on
    /// this input yields `ceil(51.000000000000007) = 52`. The consensus-correct
    /// integer answer is `51`. If anyone "restores" the float behaviour this
    /// test fails.
    // Deliberately exercises the old f64 path to prove the divergence.
    #[allow(clippy::disallowed_types)]
    #[test]
    fn ceil_mul_ratio_pins_f64_divergence() {
        let value = 2997u64;
        let numer = 17u64;
        let denom = 999u64;

        // The integer path is exact: 2997*17 = 50949, 50949/999 = 51 exactly.
        assert_eq!(ceil_mul_ratio(value, numer, denom), 51);

        // Demonstrate the divergence the fix intentionally corrects.
        let f64_result = (value as f64 * (numer as f64 / denom as f64)).ceil() as u64;
        assert_eq!(f64_result, 52, "f64 path diverges (this is the SEQ-2 bug)");
        assert_ne!(ceil_mul_ratio(value, numer, denom), f64_result);
    }

    #[test]
    fn ceil_mul_ratio_basic_and_high_range() {
        // Exact division, no rounding.
        assert_eq!(ceil_mul_ratio(100, 3, 3), 100);
        // Genuine ceiling.
        assert_eq!(ceil_mul_ratio(10, 1, 3), 4); // 10/3 = 3.33 -> 4
        // remaining == max_fill -> unchanged.
        assert_eq!(ceil_mul_ratio(u64::MAX / 2, 5, 5), u64::MAX / 2);
        // Above 2^53, where f64 loses integer precision but i128 stays exact.
        let value = (1u64 << 53) + 1;
        assert_eq!(ceil_mul_ratio(value, 4, 4), value);
    }

    #[test]
    #[should_panic(expected = "denom must be non-zero")]
    fn ceil_mul_ratio_zero_denom_panics() {
        ceil_mul_ratio(1, 1, 0);
    }

    #[test]
    fn test_dollar_conversions() {
        let dollars = 100.50;
        let nanos = dollars_to_nanos(dollars);
        let recovered = nanos_to_dollars(nanos);
        assert!((dollars - recovered).abs() < 1e-9);
    }
}
