//! Finite liquidity order book.
//!
//! Each outcome in each market has its own book with limited depth.
//! Orders compete for this finite liquidity - filling one order may
//! exhaust shares needed by another.

use crate::types::{MarketId, Nanos, Qty, Side};

/// A single price level in the order book.
#[derive(Clone, Debug)]
pub struct BookLevel {
    /// Price per share in nanodollars
    pub price: Nanos,
    /// Available quantity at this price level
    pub available_qty: Qty,
    /// Bid or Ask
    pub side: Side,
}

impl BookLevel {
    pub fn new(price: Nanos, available_qty: Qty, side: Side) -> Self {
        Self {
            price,
            available_qty,
            side,
        }
    }

    pub fn bid(price: Nanos, qty: Qty) -> Self {
        Self::new(price, qty, Side::Bid)
    }

    pub fn ask(price: Nanos, qty: Qty) -> Self {
        Self::new(price, qty, Side::Ask)
    }
}

/// Order book for a single outcome in a market.
#[derive(Clone, Debug)]
pub struct LiquidityBook {
    pub market: MarketId,
    /// Which outcome (0..num_outcomes)
    pub outcome_idx: u8,
    /// Price levels sorted by price (ascending for asks, descending for bids)
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
}

impl LiquidityBook {
    pub fn new(market: MarketId, outcome_idx: u8) -> Self {
        Self {
            market,
            outcome_idx,
            bids: Vec::new(),
            asks: Vec::new(),
        }
    }

    /// Add a bid level (sorted by price descending - best bid first).
    pub fn add_bid(&mut self, price: Nanos, qty: Qty) {
        let level = BookLevel::bid(price, qty);
        let pos = self.bids.iter().position(|l| l.price < price).unwrap_or(self.bids.len());
        self.bids.insert(pos, level);
    }

    /// Add an ask level (sorted by price ascending - best ask first).
    pub fn add_ask(&mut self, price: Nanos, qty: Qty) {
        let level = BookLevel::ask(price, qty);
        let pos = self.asks.iter().position(|l| l.price > price).unwrap_or(self.asks.len());
        self.asks.insert(pos, level);
    }

    /// Get all bid levels (best first).
    pub fn bids(&self) -> &[BookLevel] {
        &self.bids
    }

    /// Get all ask levels (best first).
    pub fn asks(&self) -> &[BookLevel] {
        &self.asks
    }

    /// Best bid price (highest), or None if no bids.
    pub fn best_bid(&self) -> Option<Nanos> {
        self.bids.first().map(|l| l.price)
    }

    /// Best ask price (lowest), or None if no asks.
    pub fn best_ask(&self) -> Option<Nanos> {
        self.asks.first().map(|l| l.price)
    }

    /// Midpoint price, or None if either side is empty.
    pub fn mid_price(&self) -> Option<Nanos> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2),
            _ => None,
        }
    }

    /// Total bid quantity available.
    pub fn total_bid_qty(&self) -> Qty {
        self.bids.iter().map(|l| l.available_qty).sum()
    }

    /// Total ask quantity available.
    pub fn total_ask_qty(&self) -> Qty {
        self.asks.iter().map(|l| l.available_qty).sum()
    }

    /// Get available quantity at or better than a price for buying (asks).
    /// Returns (total_qty, average_price_in_nanos).
    pub fn available_to_buy(&self, max_price: Nanos) -> (Qty, Nanos) {
        let mut total_qty = 0u64;
        let mut total_cost = 0u128;

        for level in &self.asks {
            if level.price <= max_price {
                total_qty += level.available_qty;
                total_cost += level.price as u128 * level.available_qty as u128;
            } else {
                break; // Asks are sorted ascending, so we can stop
            }
        }

        let avg_price = if total_qty > 0 {
            (total_cost / total_qty as u128) as Nanos
        } else {
            0
        };

        (total_qty, avg_price)
    }

    /// Get available quantity at or better than a price for selling (bids).
    /// Returns (total_qty, average_price_in_nanos).
    pub fn available_to_sell(&self, min_price: Nanos) -> (Qty, Nanos) {
        let mut total_qty = 0u64;
        let mut total_cost = 0u128;

        for level in &self.bids {
            if level.price >= min_price {
                total_qty += level.available_qty;
                total_cost += level.price as u128 * level.available_qty as u128;
            } else {
                break; // Bids are sorted descending, so we can stop
            }
        }

        let avg_price = if total_qty > 0 {
            (total_cost / total_qty as u128) as Nanos
        } else {
            0
        };

        (total_qty, avg_price)
    }

    /// Consume liquidity from the ask side (for a buyer).
    /// Returns the actual quantity filled and total cost.
    pub fn consume_asks(&mut self, max_qty: Qty, max_price: Nanos) -> (Qty, Nanos) {
        let mut remaining = max_qty;
        let mut total_cost = 0u128;
        let mut filled = 0u64;

        for level in &mut self.asks {
            if level.price > max_price || remaining == 0 {
                break;
            }

            let fill_qty = remaining.min(level.available_qty);
            level.available_qty -= fill_qty;
            remaining -= fill_qty;
            filled += fill_qty;
            total_cost += level.price as u128 * fill_qty as u128;
        }

        // Remove empty levels
        self.asks.retain(|l| l.available_qty > 0);

        let avg_price = if filled > 0 {
            (total_cost / filled as u128) as Nanos
        } else {
            0
        };

        (filled, avg_price)
    }

    /// Consume liquidity from the bid side (for a seller).
    /// Returns the actual quantity filled and total proceeds.
    pub fn consume_bids(&mut self, max_qty: Qty, min_price: Nanos) -> (Qty, Nanos) {
        let mut remaining = max_qty;
        let mut total_proceeds = 0u128;
        let mut filled = 0u64;

        for level in &mut self.bids {
            if level.price < min_price || remaining == 0 {
                break;
            }

            let fill_qty = remaining.min(level.available_qty);
            level.available_qty -= fill_qty;
            remaining -= fill_qty;
            filled += fill_qty;
            total_proceeds += level.price as u128 * fill_qty as u128;
        }

        // Remove empty levels
        self.bids.retain(|l| l.available_qty > 0);

        let avg_price = if filled > 0 {
            (total_proceeds / filled as u128) as Nanos
        } else {
            0
        };

        (filled, avg_price)
    }

    /// Create a snapshot (clone) of this book for simulation.
    pub fn snapshot(&self) -> Self {
        self.clone()
    }
}

/// Collection of liquidity books for all outcomes across all markets.
#[derive(Clone, Debug, Default)]
pub struct LiquidityPool {
    /// Books indexed by (market_id, outcome_idx)
    pub books: std::collections::HashMap<(MarketId, u8), LiquidityBook>,
}

impl LiquidityPool {
    pub fn new() -> Self {
        Self {
            books: std::collections::HashMap::new(),
        }
    }

    /// Get or create a book for a specific market outcome.
    pub fn book_mut(&mut self, market: MarketId, outcome_idx: u8) -> &mut LiquidityBook {
        self.books
            .entry((market, outcome_idx))
            .or_insert_with(|| LiquidityBook::new(market, outcome_idx))
    }

    /// Get a book (immutable).
    pub fn book(&self, market: MarketId, outcome_idx: u8) -> Option<&LiquidityBook> {
        self.books.get(&(market, outcome_idx))
    }

    /// Add liquidity to a specific market outcome.
    pub fn add_bid(&mut self, market: MarketId, outcome_idx: u8, price: Nanos, qty: Qty) {
        self.book_mut(market, outcome_idx).add_bid(price, qty);
    }

    pub fn add_ask(&mut self, market: MarketId, outcome_idx: u8, price: Nanos, qty: Qty) {
        self.book_mut(market, outcome_idx).add_ask(price, qty);
    }

    /// Create a snapshot of all books for simulation.
    pub fn snapshot(&self) -> Self {
        Self {
            books: self.books.clone(),
        }
    }

    /// Iterate over all books.
    pub fn iter(&self) -> impl Iterator<Item = (&(MarketId, u8), &LiquidityBook)> {
        self.books.iter()
    }

    /// Set a book directly for a specific market outcome.
    pub fn set(&mut self, market: MarketId, outcome_idx: u8, book: LiquidityBook) {
        self.books.insert((market, outcome_idx), book);
    }

    /// Get a mutable reference to a book if it exists.
    pub fn get_mut(&mut self, market: MarketId, outcome_idx: u8) -> Option<&mut LiquidityBook> {
        self.books.get_mut(&(market, outcome_idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::conversions::price_to_nanos;

    #[test]
    fn test_book_level_creation() {
        let bid = BookLevel::bid(price_to_nanos(0.52), 100);
        assert_eq!(bid.side, Side::Bid);
        assert_eq!(bid.available_qty, 100);

        let ask = BookLevel::ask(price_to_nanos(0.53), 150);
        assert_eq!(ask.side, Side::Ask);
        assert_eq!(ask.available_qty, 150);
    }

    #[test]
    fn test_book_ordering() {
        let mut book = LiquidityBook::new(MarketId::new(0), 0);

        // Add bids in random order
        book.add_bid(price_to_nanos(0.50), 100);
        book.add_bid(price_to_nanos(0.52), 100);
        book.add_bid(price_to_nanos(0.51), 100);

        // Should be sorted descending (best first)
        let bids = book.bids();
        assert_eq!(bids[0].price, price_to_nanos(0.52));
        assert_eq!(bids[1].price, price_to_nanos(0.51));
        assert_eq!(bids[2].price, price_to_nanos(0.50));

        // Add asks in random order
        book.add_ask(price_to_nanos(0.55), 100);
        book.add_ask(price_to_nanos(0.53), 100);
        book.add_ask(price_to_nanos(0.54), 100);

        // Should be sorted ascending (best first)
        let asks = book.asks();
        assert_eq!(asks[0].price, price_to_nanos(0.53));
        assert_eq!(asks[1].price, price_to_nanos(0.54));
        assert_eq!(asks[2].price, price_to_nanos(0.55));
    }

    #[test]
    fn test_consume_asks() {
        let mut book = LiquidityBook::new(MarketId::new(0), 0);
        book.add_ask(price_to_nanos(0.53), 100);
        book.add_ask(price_to_nanos(0.54), 200);

        // Try to buy 150 at max price 0.54
        let (filled, _avg_price) = book.consume_asks(150, price_to_nanos(0.54));
        assert_eq!(filled, 150);

        // 100 from first level consumed, 50 from second
        assert_eq!(book.total_ask_qty(), 150); // 200 - 50 = 150 remaining
    }

    #[test]
    fn test_liquidity_competition() {
        let mut book = LiquidityBook::new(MarketId::new(0), 0);
        book.add_ask(price_to_nanos(0.53), 150); // Only 150 available!

        // Order 1 wants 300, only gets 150
        let (filled1, _) = book.consume_asks(300, price_to_nanos(0.53));
        assert_eq!(filled1, 150);

        // Order 2 wants 200, gets nothing
        let (filled2, _) = book.consume_asks(200, price_to_nanos(0.53));
        assert_eq!(filled2, 0);

        // This demonstrates liquidity competition - both orders wanted
        // 500 total shares but only 150 existed.
    }
}
