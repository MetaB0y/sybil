//! Overflow-safe arithmetic helpers for verification.
//!
//! All price*qty computations go through matching-engine helpers so verifier
//! arithmetic matches the sequencer's fixed-point quantity semantics.

use matching_engine::{notional_nanos, signed_price_delta_notional, Nanos, Qty, SHARE_SCALE};

/// Compute `price * qty / SHARE_SCALE` as i64.
/// Returns `None` on overflow (value doesn't fit in i64).
pub fn checked_price_qty(price: Nanos, qty: Qty) -> Option<i64> {
    let result = (price.0 as i128)
        .checked_mul(qty.0 as i128)?
        .checked_div(SHARE_SCALE as i128)?;
    i64::try_from(result).ok()
}

/// Compute `price * qty / SHARE_SCALE` as i64.
pub fn price_qty(price: Nanos, qty: Qty) -> i64 {
    notional_nanos(price, qty).0 as i64
}

/// Compute welfare for a single fill.
///
/// Buyers: `(limit_price - fill_price) * fill_qty / SHARE_SCALE`
/// Sellers: `(fill_price - limit_price) * fill_qty / SHARE_SCALE`
pub fn welfare(limit_price: Nanos, fill_price: Nanos, fill_qty: Qty, is_seller: bool) -> i64 {
    if fill_qty == Qty::ZERO {
        return 0;
    }
    let surplus_per_unit = if is_seller {
        fill_price.0 as i64 - limit_price.0 as i64
    } else {
        limit_price.0 as i64 - fill_price.0 as i64
    };
    signed_price_delta_notional(surplus_per_unit, fill_qty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::shares_to_qty;

    #[test]
    fn test_price_qty_basic() {
        assert_eq!(
            price_qty(Nanos(500_000_000), shares_to_qty(100)),
            50_000_000_000
        );
    }

    #[test]
    fn test_checked_price_qty_overflow() {
        // u64::MAX * u64::MAX would overflow i64
        assert!(checked_price_qty(Nanos(u64::MAX), Qty(u64::MAX)).is_none());
    }

    #[test]
    fn test_welfare_buyer() {
        // Buyer: limit=0.60, fill=0.50, qty=100 => welfare = 0.10 * 100
        let w = welfare(
            Nanos(600_000_000),
            Nanos(500_000_000),
            shares_to_qty(100),
            false,
        );
        assert_eq!(w, 10_000_000_000);
    }

    #[test]
    fn test_welfare_seller() {
        // Seller: limit=0.40, fill=0.50, qty=100 => welfare = 0.10 * 100
        let w = welfare(
            Nanos(400_000_000),
            Nanos(500_000_000),
            shares_to_qty(100),
            true,
        );
        assert_eq!(w, 10_000_000_000);
    }

    #[test]
    fn test_welfare_zero_qty() {
        assert_eq!(
            welfare(Nanos(600_000_000), Nanos(500_000_000), Qty(0), false),
            0
        );
    }
}
