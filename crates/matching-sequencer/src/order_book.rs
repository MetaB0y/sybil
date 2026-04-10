//! Order book: resting orders with tracked balance/position reservations.
//!
//! The order book is the single source of truth for "what capital is committed."
//! It owns all accepted-but-unfilled orders and their balance/position reservations.
//! MM orders bypass the book entirely (flash liquidity, one-shot per block).
//!
//! Lifecycle:
//! 1. `accept()` — validate, reserve capital, store
//! 2. `expire()` — remove TTL-expired orders, release reservations
//! 3. `revalidate()` — after state changes (fills, withdrawals), release invalid orders
//! 4. `orders_for_batch()` — yield orders for the solver
//! 5. `settle()` — remove filled orders, create remainders, adjust reservations

use std::collections::{HashMap, HashSet};

use matching_engine::{Fill, MarketId, Order};

use crate::account::{AccountId, AccountStore};
use crate::error::RejectionReason;
use crate::validation::{sell_reservations, validate_order_with_reservation, PositionKey};

/// A resting order in the book.
#[derive(Clone, Debug)]
struct RestingOrder {
    order: Order,
    account_id: AccountId,
    /// Block height when this order was first accepted.
    created_at: u64,
    /// Balance reserved by this order (buy cost). 0 for sells.
    reserved_balance: i64,
    /// Position reservations for this order (sell quantities).
    reserved_positions: Vec<(PositionKey, i64)>,
}

/// The order book: resting orders + aggregate reservations.
pub struct OrderBook {
    orders: Vec<RestingOrder>,
    /// Per-account reserved balance (sum of buy costs for resting orders).
    balance_reservations: HashMap<AccountId, i64>,
    /// Per-account reserved positions (sum of sell quantities).
    position_reservations: HashMap<(AccountId, PositionKey), i64>,
    /// Maximum TTL in blocks.
    ttl: u64,
}

/// An accepted order returned from `accept()`, for witness tracking.
pub struct Accepted {
    pub order: Order,
    pub account_id: AccountId,
}

impl OrderBook {
    pub fn new(ttl: u64) -> Self {
        Self {
            orders: Vec::new(),
            balance_reservations: HashMap::new(),
            position_reservations: HashMap::new(),
            ttl,
        }
    }

    /// Current reserved balance for an account.
    pub fn reserved_balance(&self, account_id: AccountId) -> i64 {
        self.balance_reservations
            .get(&account_id)
            .copied()
            .unwrap_or(0)
    }

    /// Per-account position reservations view (for validation).
    fn acct_position_reservations(&self, account_id: AccountId) -> HashMap<PositionKey, i64> {
        self.position_reservations
            .iter()
            .filter(|((aid, _), _)| *aid == account_id)
            .map(|((_, key), &qty)| (*key, qty))
            .collect()
    }

    /// Accept a new order into the book. Validates against account state + existing
    /// reservations. Returns Ok(Accepted) on success, Err(reason) on rejection.
    pub fn accept(
        &mut self,
        order: Order,
        account_id: AccountId,
        account: &crate::account::Account,
        current_height: u64,
    ) -> Result<Accepted, RejectionReason> {
        let reserved = self.reserved_balance(account_id);
        let acct_positions = self.acct_position_reservations(account_id);

        let cost =
            validate_order_with_reservation(&order, account, reserved, &acct_positions)?;

        // Reserve balance
        if cost > 0 {
            *self.balance_reservations.entry(account_id).or_insert(0) += cost;
        }

        // Reserve positions
        let pos_reservations = sell_reservations(&order);
        for &(key, qty) in &pos_reservations {
            *self
                .position_reservations
                .entry((account_id, key))
                .or_insert(0) += qty;
        }

        self.orders.push(RestingOrder {
            order: order.clone(),
            account_id,
            created_at: current_height,
            reserved_balance: cost,
            reserved_positions: pos_reservations,
        });

        Ok(Accepted {
            order,
            account_id,
        })
    }

    /// Remove expired orders and release their reservations.
    pub fn expire(&mut self, current_height: u64) {
        self.orders.retain(|ro| {
            if current_height - ro.created_at > self.ttl {
                // Release reservations
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    ro,
                );
                false
            } else {
                true
            }
        });
    }

    /// Re-validate all resting orders against current account state.
    /// Removes orders that are no longer valid (account deleted, insufficient balance
    /// after fills/withdrawals, insufficient position after sells).
    ///
    /// Also removes orders for markets that are no longer active.
    pub fn revalidate(
        &mut self,
        accounts: &AccountStore,
        active_markets: &HashSet<MarketId>,
    ) {
        // We must re-validate carefully: removing one order releases its reservations,
        // which may make subsequent orders valid again. But for simplicity and safety,
        // we validate conservatively: remove anything that's invalid given current
        // reservations. This may over-reject (an order might become valid after a
        // prior order's reservation is released), but that's safe — the trader can
        // resubmit.
        let mut to_remove = Vec::new();

        for (i, ro) in self.orders.iter().enumerate() {
            // Market still active?
            let markets_active = ro.order.active_markets().all(|m| active_markets.contains(&m));
            if !markets_active {
                to_remove.push(i);
                continue;
            }

            // Account still exists and solvent?
            let Some(account) = accounts.get(ro.account_id) else {
                to_remove.push(i);
                continue;
            };
            if account.balance <= 0 {
                to_remove.push(i);
                continue;
            }

            // Check if order is still valid with current reservations
            // (subtract THIS order's reservation first, then re-validate)
            let reserved_without_self = self
                .balance_reservations
                .get(&ro.account_id)
                .copied()
                .unwrap_or(0)
                - ro.reserved_balance;
            let mut positions_without_self = self.acct_position_reservations(ro.account_id);
            for &(key, qty) in &ro.reserved_positions {
                if let Some(v) = positions_without_self.get_mut(&key) {
                    *v -= qty;
                }
            }
            if validate_order_with_reservation(
                &ro.order,
                account,
                reserved_without_self,
                &positions_without_self,
            )
            .is_err()
            {
                to_remove.push(i);
            }
        }

        // Remove in reverse order to preserve indices
        for &i in to_remove.iter().rev() {
            let ro = self.orders.remove(i);
            Self::release_reservations(
                &mut self.balance_reservations,
                &mut self.position_reservations,
                &ro,
            );
        }
    }

    /// Orders available for the current batch.
    pub fn resting_orders(&self) -> impl Iterator<Item = (&Order, AccountId)> {
        self.orders.iter().map(|ro| (&ro.order, ro.account_id))
    }

    /// Orders with full metadata (for API exposure).
    pub fn resting_orders_full(&self) -> impl Iterator<Item = (&Order, AccountId, u64)> {
        self.orders
            .iter()
            .map(|ro| (&ro.order, ro.account_id, ro.created_at))
    }

    /// TTL value.
    pub fn ttl(&self) -> u64 {
        self.ttl
    }

    /// Set TTL (for testing).
    #[cfg(test)]
    pub fn set_ttl(&mut self, ttl: u64) {
        self.ttl = ttl;
    }

    /// Number of resting orders.
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// After solving: remove filled orders, adjust partially-filled orders,
    /// release reservations for filled portions.
    ///
    /// `mm_order_ids` are excluded (MM orders never enter the book).
    pub fn settle(&mut self, fills: &[Fill], mm_order_ids: &HashSet<u64>) {
        // Build fill-qty map
        let mut filled_qty: HashMap<u64, u64> = HashMap::new();
        for f in fills {
            if f.fill_qty > 0 {
                *filled_qty.entry(f.order_id).or_insert(0) += f.fill_qty;
            }
        }

        let mut new_orders = Vec::new();

        for ro in self.orders.drain(..) {
            if mm_order_ids.contains(&ro.order.id) {
                // Should never happen (MM orders don't enter book), but defensive
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                continue;
            }

            let filled = filled_qty.get(&ro.order.id).copied().unwrap_or(0);

            if filled >= ro.order.max_fill {
                // Fully filled — release all reservations
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                continue;
            }

            if filled > 0 {
                // Partially filled — reduce order and reservations proportionally
                let remaining = ro.order.max_fill - filled;
                let ratio = remaining as f64 / ro.order.max_fill as f64;

                let old_cost = ro.reserved_balance;
                let new_cost = (old_cost as f64 * ratio).ceil() as i64;
                let released_balance = old_cost - new_cost;

                if released_balance > 0 {
                    if let Some(v) = self.balance_reservations.get_mut(&ro.account_id) {
                        *v -= released_balance;
                    }
                }

                // Release proportional position reservations
                let new_pos_reservations: Vec<(PositionKey, i64)> = ro
                    .reserved_positions
                    .iter()
                    .map(|&(key, qty)| {
                        let new_qty = (qty as f64 * ratio).ceil() as i64;
                        let released = qty - new_qty;
                        if released > 0 {
                            if let Some(v) = self.position_reservations.get_mut(&(ro.account_id, key)) {
                                *v -= released;
                            }
                        }
                        (key, new_qty)
                    })
                    .collect();

                let mut remainder = ro.order.clone();
                remainder.max_fill = remaining;

                new_orders.push(RestingOrder {
                    order: remainder,
                    account_id: ro.account_id,
                    created_at: ro.created_at,
                    reserved_balance: new_cost,
                    reserved_positions: new_pos_reservations,
                });
            } else {
                // Unfilled — keep as-is
                new_orders.push(ro);
            }
        }

        self.orders = new_orders;
    }

    /// Release the reservations held by a resting order.
    fn release_reservations(
        balance_reservations: &mut HashMap<AccountId, i64>,
        position_reservations: &mut HashMap<(AccountId, PositionKey), i64>,
        ro: &RestingOrder,
    ) {
        if ro.reserved_balance > 0 {
            if let Some(v) = balance_reservations.get_mut(&ro.account_id) {
                *v -= ro.reserved_balance;
                if *v <= 0 {
                    balance_reservations.remove(&ro.account_id);
                }
            }
        }
        for &(key, qty) in &ro.reserved_positions {
            if let Some(v) = position_reservations.get_mut(&(ro.account_id, key)) {
                *v -= qty;
                if *v <= 0 {
                    position_reservations.remove(&(ro.account_id, key));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, MarketId, MarketSet, NANOS_PER_DOLLAR};

    fn setup() -> (AccountStore, MarketSet, MarketId) {
        let accounts = AccountStore::new();
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        (accounts, markets, m0)
    }

    fn buy_yes(markets: &MarketSet, id: u64, market: MarketId, price: u64, qty: u64) -> Order {
        outcome_buy(markets, id, market, 0, price, qty)
    }

    #[test]
    fn accept_tracks_balance_reservation() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        let account = accounts.get(aid).unwrap();
        book.accept(order, aid, account, 1).unwrap();

        // Should have reserved 5 * 0.5 = 2.5 dollars
        let reserved = book.reserved_balance(aid);
        assert_eq!(reserved, (NANOS_PER_DOLLAR / 2 * 5) as i64);
    }

    #[test]
    fn accept_rejects_over_committed() {
        let (mut accounts, markets, m0) = setup();
        let balance = 3 * NANOS_PER_DOLLAR as i64;
        let aid = accounts.create_account(balance);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        // First order: costs $2
        let o1 = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 4);
        book.accept(o1, aid, account, 1).unwrap();

        // Second order: costs $2 — would exceed $3 balance
        let o2 = buy_yes(&markets, 2, m0, NANOS_PER_DOLLAR / 2, 4);
        let result = book.accept(o2, aid, account, 1);
        assert!(result.is_err());
    }

    #[test]
    fn expire_releases_reservations() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        book.accept(order, aid, account, 1).unwrap();
        assert!(book.reserved_balance(aid) > 0);

        // Expire at height 5 (TTL=3, created_at=1, 5-1=4 > 3)
        book.expire(5);
        assert_eq!(book.reserved_balance(aid), 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn settle_fully_filled_releases_reservations() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        let accepted = book.accept(order, aid, account, 1).unwrap();
        let order_id = accepted.order.id;
        assert!(book.reserved_balance(aid) > 0);

        // Fully fill
        let fills = vec![Fill {
            order_id,
            fill_qty: 5,
            fill_price: NANOS_PER_DOLLAR / 2,
            account_id: 0,
        }];
        book.settle(&fills, &HashSet::new());

        assert_eq!(book.reserved_balance(aid), 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn settle_partial_fill_adjusts_reservation() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 10);
        let accepted = book.accept(order, aid, account, 1).unwrap();
        let order_id = accepted.order.id;

        let original_reserved = book.reserved_balance(aid);

        // Partially fill 4 of 10
        let fills = vec![Fill {
            order_id,
            fill_qty: 4,
            fill_price: NANOS_PER_DOLLAR / 2,
            account_id: 0,
        }];
        book.settle(&fills, &HashSet::new());

        // Remaining: 6 of 10 = 60%
        assert_eq!(book.len(), 1);
        let new_reserved = book.reserved_balance(aid);
        assert!(new_reserved < original_reserved);
        assert!(new_reserved > 0);

        // Check remaining order has max_fill = 6
        let (remaining_order, _) = book.resting_orders().next().unwrap();
        assert_eq!(remaining_order.max_fill, 6);
    }
}
