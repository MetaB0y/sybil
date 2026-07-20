//! Book-derived "indicative" pricing for batches that do not cross.
//!
//! When a batch matches volume, the uniform clearing price is the market's
//! price. When nothing crosses, this module derives a touch midpoint from the
//! participating *single-market* orders — the analogue of an order-book mid.
//! Multi-market (bundle/spread) orders are excluded: their `limit_price` is a
//! bundle total, not attributable to one market (same rule the liquidity
//! tracker applies).
//!
//! Everything here is serving-layer only. It never feeds blocks, the witness,
//! settlement, or realized PnL.

use std::collections::HashMap;

use crate::OrderDirection;
use crate::order::derive_order_direction;
use crate::types::{MarketId, NANOS_PER_DOLLAR, Nanos};

/// Per-market YES touch midpoint over the supplied single-market orders.
///
/// Returns an entry only for markets with a two-sided, non-crossed book
/// (`best_bid < best_ask`). One-sided, empty, or crossed books are omitted —
/// the caller carries over the last mark for those.
pub fn book_midprices<'a>(
    orders: impl IntoIterator<Item = &'a crate::Order>,
) -> HashMap<MarketId, Nanos> {
    let mut best_bid: HashMap<MarketId, Nanos> = HashMap::new();
    let mut best_ask: HashMap<MarketId, Nanos> = HashMap::new();

    for order in orders {
        if order.num_markets != 1 {
            continue;
        }
        let market = order.markets[0];
        if market.is_none() {
            continue;
        }
        let (is_bid, price) = match derive_order_direction(order, market) {
            OrderDirection::BuyYes => (true, order.limit_price),
            OrderDirection::SellNo => (
                true,
                Nanos(NANOS_PER_DOLLAR.saturating_sub(order.limit_price.0)),
            ),
            OrderDirection::SellYes => (false, order.limit_price),
            OrderDirection::BuyNo => (
                false,
                Nanos(NANOS_PER_DOLLAR.saturating_sub(order.limit_price.0)),
            ),
        };
        if is_bid {
            best_bid
                .entry(market)
                .and_modify(|b| {
                    if price > *b {
                        *b = price;
                    }
                })
                .or_insert(price);
        } else {
            best_ask
                .entry(market)
                .and_modify(|a| {
                    if price < *a {
                        *a = price;
                    }
                })
                .or_insert(price);
        }
    }

    let mut mids = HashMap::new();
    for (&market, &bid) in &best_bid {
        if let Some(&ask) = best_ask.get(&market)
            && bid < ask
        {
            mids.insert(market, bid + Nanos((ask.0 - bid.0) / 2));
        }
    }
    mids
}

/// Resolve a market's `[yes, no]` mark via the ladder:
/// clearing-if-filled → midpoint → previous mark → last clearing → 50/50.
pub fn mark_yes_no(
    had_fill: bool,
    clearing: Option<&[Nanos]>,
    midpoint: Option<Nanos>,
    last_mark: Option<&[Nanos]>,
) -> Vec<Nanos> {
    let half = Nanos(NANOS_PER_DOLLAR / 2);
    if had_fill && let Some(c) = clearing {
        return c.to_vec();
    }
    if let Some(mid) = midpoint {
        return vec![mid, Nanos(NANOS_PER_DOLLAR.saturating_sub(mid.0))];
    }
    if let Some(prev) = last_mark {
        return prev.to_vec();
    }
    if let Some(c) = clearing {
        return c.to_vec();
    }
    vec![half, half]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MarketSet;
    use crate::order_builder::{outcome_buy, outcome_sell, spread};

    fn markets() -> (MarketSet, MarketId, MarketId) {
        let mut m = MarketSet::new();
        let m0 = m.add_binary("mid_m0");
        let m1 = m.add_binary("mid_m1");
        (m, m0, m1)
    }

    // BuyYes @ 40c (bid) + SellYes @ 60c (ask) → mid 50c.
    #[test]
    fn two_sided_no_cross_yields_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 400_000_000, 5);
        let ask = outcome_sell(&ms, 2, m0, 0, 600_000_000, 5);
        let orders = [bid, ask];
        let mids = book_midprices(orders.iter());
        assert_eq!(mids.get(&m0).copied(), Some(Nanos(500_000_000)));
    }

    // BuyNo @ 30c is a YES ask at 70c; SellNo @ 80c is a YES bid at 20c.
    // bid 20c, ask 70c → mid 45c.
    #[test]
    fn no_side_orders_map_into_yes_book() {
        let (ms, m0, _) = markets();
        let yes_ask_via_no = outcome_buy(&ms, 1, m0, 1, 300_000_000, 5); // BuyNo @30c -> ask 70c
        let yes_bid_via_no = outcome_sell(&ms, 2, m0, 1, 800_000_000, 5); // SellNo @80c -> bid 20c
        let orders = [yes_ask_via_no, yes_bid_via_no];
        let mids = book_midprices(orders.iter());
        assert_eq!(mids.get(&m0).copied(), Some(Nanos(450_000_000)));
    }

    // Only bids, no asks → no midpoint.
    #[test]
    fn one_sided_book_has_no_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 400_000_000, 5);
        let orders = [bid];
        let mids = book_midprices(orders.iter());
        assert!(!mids.contains_key(&m0));
    }

    // Multi-market spread orders are ignored entirely.
    #[test]
    fn multi_market_orders_excluded() {
        let (ms, m0, m1) = markets();
        let sp = spread(&ms, 1, m0, m1, 500_000_000, 5);
        let orders = [sp];
        let mids = book_midprices(orders.iter());
        assert!(mids.is_empty());
    }

    // Crossed book (bid >= ask) yields no midpoint (a real batch would match).
    #[test]
    fn crossed_book_has_no_midpoint() {
        let (ms, m0, _) = markets();
        let bid = outcome_buy(&ms, 1, m0, 0, 700_000_000, 5);
        let ask = outcome_sell(&ms, 2, m0, 0, 300_000_000, 5);
        let orders = [bid, ask];
        let mids = book_midprices(orders.iter());
        assert!(!mids.contains_key(&m0));
    }

    #[test]
    fn mark_ladder_prefers_clearing_when_filled() {
        let clearing = vec![Nanos(620_000_000), Nanos(380_000_000)];
        let got = mark_yes_no(
            true,
            Some(clearing.as_slice()),
            Some(Nanos(500_000_000)),
            None,
        );
        assert_eq!(got, clearing);
    }

    #[test]
    fn mark_ladder_uses_midpoint_when_not_filled() {
        let clearing = vec![Nanos(620_000_000), Nanos(380_000_000)];
        let got = mark_yes_no(
            false,
            Some(clearing.as_slice()),
            Some(Nanos(500_000_000)),
            None,
        );
        assert_eq!(got, vec![Nanos(500_000_000), Nanos(500_000_000)]);
    }

    #[test]
    fn mark_ladder_carries_over_when_no_midpoint() {
        let last_mark = vec![Nanos(510_000_000), Nanos(490_000_000)];
        let got = mark_yes_no(false, None, None, Some(last_mark.as_slice()));
        assert_eq!(got, last_mark);
    }

    #[test]
    fn mark_ladder_defaults_to_half() {
        let got = mark_yes_no(false, None, None, None);
        assert_eq!(
            got,
            vec![Nanos(NANOS_PER_DOLLAR / 2), Nanos(NANOS_PER_DOLLAR / 2)]
        );
    }

    #[test]
    fn mark_ladder_falls_through_when_filled_without_clearing() {
        // had_fill=true but no clearing supplied → falls through to midpoint.
        let got = mark_yes_no(true, None, Some(Nanos(500_000_000)), None);
        assert_eq!(got, vec![Nanos(500_000_000), Nanos(500_000_000)]);
    }
}
