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

use std::collections::{BTreeMap, HashMap, HashSet};

use matching_engine::{Fill, MarketId, Order};
use serde::{Deserialize, Serialize};

use crate::account::{AccountId, AccountStore};
use crate::error::RejectionReason;
use crate::validation::{sell_reservations, validate_order_with_reservation, PositionKey};

fn default_resting_expires_at_block() -> u64 {
    u64::MAX
}

/// A resting order in the book.
///
/// The struct is public (and serde-serializable) so it can cross the persistence
/// boundary, but fields are crate-private: external code receives `RestingOrder`
/// values as opaque records and must go through `OrderBook` methods to mutate
/// the book. Invariants (`reserved_balance >= 0`, all reservation qtys `>= 0`,
/// aggregates equal to the sum of per-order reservations) are only enforced by
/// the construction paths in this module.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestingOrder {
    pub(crate) order: Order,
    pub(crate) account_id: AccountId,
    /// Block height when this order was first accepted.
    pub(crate) created_at: u64,
    /// Last block height where this order may participate.
    #[serde(default = "default_resting_expires_at_block")]
    pub(crate) expires_at_block: u64,
    /// Balance reserved by this order (buy cost). 0 for sells.
    pub(crate) reserved_balance: i64,
    /// Position reservations for this order (sell quantities).
    pub(crate) reserved_positions: Vec<(PositionKey, i64)>,
    /// Whether any positive fill has ever applied to this order.
    /// Set by `settle` when a fill > 0 is observed; preserved across
    /// partial-fill remainders. Consumed by OrderStatsTracker (B6).
    #[serde(default)]
    pub(crate) has_been_matched: bool,
    /// Original `max_fill` at admit time. Set once by `accept`, never
    /// mutated. Consumed by `PendingOrderResponse.original_quantity` (B8).
    #[serde(default)]
    pub(crate) original_max_fill: u64,
}

/// The order book: resting orders + aggregate reservations.
#[derive(Clone)]
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
///
/// `resting_order` is a clone of the `RestingOrder` that was pushed into
/// the book — useful when the caller needs to durably log the admission
/// (e.g. the admit-log WAL) without re-deriving the reservation fields.
pub struct Accepted {
    pub order: Order,
    pub account_id: AccountId,
    pub resting_order: RestingOrder,
}

#[derive(Debug)]
pub(crate) enum CancelError {
    NotFound,
    WrongOwner,
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

    /// Snapshot all resting orders for persistence. Reservation aggregates are
    /// derivable from the per-order reservations, so only the order list is stored.
    pub fn snapshot(&self) -> Vec<RestingOrder> {
        self.orders.clone()
    }

    /// Canonical resting-order leaves for the state-root sidecar.
    pub fn state_root_orders(&self) -> Vec<sybil_verifier::RestingOrderSnapshot> {
        resting_order_snapshots(&self.orders)
    }

    /// Canonical aggregate reservation leaves for the state-root sidecar.
    pub fn state_root_reservations(&self) -> Vec<sybil_verifier::AccountReservationSnapshot> {
        reservation_snapshots_from_aggregates(
            &self.balance_reservations,
            &self.position_reservations,
        )
    }

    /// Rebuild an order book from a persisted snapshot. Reservation aggregates
    /// are reconstructed by summing per-order reservations. Orders whose
    /// reservations violate invariants (negative balance, negative qty) are
    /// dropped with a warning rather than trusted blindly.
    pub fn restore(orders: Vec<RestingOrder>, ttl: u64) -> Self {
        let mut balance_reservations: HashMap<AccountId, i64> = HashMap::new();
        let mut position_reservations: HashMap<(AccountId, PositionKey), i64> = HashMap::new();
        let mut valid_orders = Vec::with_capacity(orders.len());
        for ro in orders {
            if ro.reserved_balance < 0 || ro.reserved_positions.iter().any(|(_, qty)| *qty < 0) {
                tracing::warn!(
                    order_id = ro.order.id,
                    account_id = ?ro.account_id,
                    reserved_balance = ro.reserved_balance,
                    "dropping resting order with invalid reservation during restore"
                );
                continue;
            }
            if ro.reserved_balance > 0 {
                *balance_reservations.entry(ro.account_id).or_insert(0) += ro.reserved_balance;
            }
            for &(key, qty) in &ro.reserved_positions {
                *position_reservations
                    .entry((ro.account_id, key))
                    .or_insert(0) += qty;
            }
            valid_orders.push(ro);
        }
        Self {
            orders: valid_orders,
            balance_reservations,
            position_reservations,
            ttl,
        }
    }

    /// Reinsert a pre-validated `RestingOrder` (replay path).
    ///
    /// Used by the admit-log recovery: the order was already validated and
    /// reserved at original admit time, so we trust the payload and just
    /// append it + update the reservation aggregates. Orders with negative
    /// reservations are dropped with a warning, matching `restore`'s behavior.
    pub fn reinsert_for_replay(&mut self, resting: RestingOrder) {
        if resting.reserved_balance < 0
            || resting.reserved_positions.iter().any(|(_, qty)| *qty < 0)
        {
            tracing::warn!(
                order_id = resting.order.id,
                account_id = ?resting.account_id,
                reserved_balance = resting.reserved_balance,
                "dropping replayed admit with invalid reservation"
            );
            return;
        }
        if resting.reserved_balance > 0 {
            *self
                .balance_reservations
                .entry(resting.account_id)
                .or_insert(0) += resting.reserved_balance;
        }
        for &(key, qty) in &resting.reserved_positions {
            *self
                .position_reservations
                .entry((resting.account_id, key))
                .or_insert(0) += qty;
        }
        self.orders.push(resting);
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

        let cost = validate_order_with_reservation(&order, account, reserved, &acct_positions)?;

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

        let resting = RestingOrder {
            order: order.clone(),
            account_id,
            created_at: current_height,
            expires_at_block: order.effective_expires_at_block(current_height, self.ttl),
            reserved_balance: cost,
            reserved_positions: pos_reservations,
            has_been_matched: false,
            original_max_fill: order.max_fill,
        };
        self.orders.push(resting.clone());

        Ok(Accepted {
            order,
            account_id,
            resting_order: resting,
        })
    }

    /// Remove expired orders and release their reservations.
    /// Returns the orders that were removed (empty when nothing expired).
    pub fn expire(&mut self, current_height: u64) -> Vec<RestingOrder> {
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(self.orders.len());
        for ro in self.orders.drain(..) {
            if current_height > ro.expires_at_block {
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                removed.push(ro);
            } else {
                kept.push(ro);
            }
        }
        self.orders = kept;
        removed
    }

    /// Re-validate all resting orders against current account state.
    /// Removes orders that are no longer valid (account deleted, insufficient balance
    /// after fills/withdrawals, insufficient position after sells).
    ///
    /// Also removes orders for markets that are no longer active.
    ///
    /// Returns the orders that were removed (empty when nothing changed).
    pub fn revalidate(
        &mut self,
        accounts: &AccountStore,
        active_markets: &HashSet<MarketId>,
    ) -> Vec<RestingOrder> {
        // We must re-validate carefully: removing one order releases its reservations,
        // which may make subsequent orders valid again. But for simplicity and safety,
        // we validate conservatively: remove anything that's invalid given current
        // reservations. This may over-reject (an order might become valid after a
        // prior order's reservation is released), but that's safe — the trader can
        // resubmit.
        let mut to_remove = Vec::new();

        for (i, ro) in self.orders.iter().enumerate() {
            // Market still active?
            let markets_active = ro
                .order
                .active_markets()
                .all(|m| active_markets.contains(&m));
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
        let mut removed = Vec::with_capacity(to_remove.len());
        for &i in to_remove.iter().rev() {
            let ro = self.orders.remove(i);
            Self::release_reservations(
                &mut self.balance_reservations,
                &mut self.position_reservations,
                &ro,
            );
            removed.push(ro);
        }
        removed
    }

    /// Orders available for the current batch.
    pub fn resting_orders(&self) -> impl Iterator<Item = (&Order, AccountId)> {
        self.orders.iter().map(|ro| (&ro.order, ro.account_id))
    }

    /// Orders with full metadata (for API exposure).
    pub fn resting_orders_full(&self) -> impl Iterator<Item = (&Order, AccountId, u64, u64)> {
        self.orders
            .iter()
            .map(|ro| (&ro.order, ro.account_id, ro.created_at, ro.expires_at_block))
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

    /// Whether there are no resting orders.
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Number of resting orders owned by one account.
    pub fn orders_for_account(&self, account_id: AccountId) -> usize {
        self.orders
            .iter()
            .filter(|ro| ro.account_id == account_id)
            .count()
    }

    /// Remove a resting order by ID and release its reservations.
    /// Returns the cancelled `RestingOrder` on success. Consumed by D1
    /// (OrderCancelled SystemEvent); unused but bound in B5.
    pub(crate) fn cancel(
        &mut self,
        account_id: AccountId,
        order_id: u64,
    ) -> Result<RestingOrder, CancelError> {
        let Some(index) = self.orders.iter().position(|ro| ro.order.id == order_id) else {
            return Err(CancelError::NotFound);
        };

        if self.orders[index].account_id != account_id {
            return Err(CancelError::WrongOwner);
        }

        let ro = self.orders.remove(index);
        Self::release_reservations(
            &mut self.balance_reservations,
            &mut self.position_reservations,
            &ro,
        );
        Ok(ro)
    }

    /// After solving: remove filled orders, adjust partially-filled orders,
    /// release reservations for filled portions.
    ///
    /// `mm_order_ids` are excluded (MM orders never enter the book).
    /// Returns the orders that were removed from the book (fully filled,
    /// expired, or MM-bypass defensive path). Partially filled orders are
    /// re-inserted as remainders and are NOT included in the return value.
    /// `has_been_matched` is set to true on any order (or remainder) that
    /// received a positive fill in this batch.
    pub fn settle(
        &mut self,
        fills: &[Fill],
        mm_order_ids: &HashSet<u64>,
        current_height: u64,
    ) -> Vec<RestingOrder> {
        // Build fill-qty map
        let mut filled_qty: HashMap<u64, u64> = HashMap::new();
        for f in fills {
            if f.fill_qty > 0 {
                *filled_qty.entry(f.order_id).or_insert(0) += f.fill_qty;
            }
        }

        let mut new_orders = Vec::new();
        let mut removed = Vec::new();

        for mut ro in self.orders.drain(..) {
            if mm_order_ids.contains(&ro.order.id) {
                // Should never happen (MM orders don't enter book), but defensive
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                removed.push(ro);
                continue;
            }

            let filled = filled_qty.get(&ro.order.id).copied().unwrap_or(0);
            if filled > 0 {
                ro.has_been_matched = true;
            }

            if filled >= ro.order.max_fill {
                // Fully filled — release all reservations
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                removed.push(ro);
                continue;
            }

            if current_height >= ro.expires_at_block {
                Self::release_reservations(
                    &mut self.balance_reservations,
                    &mut self.position_reservations,
                    &ro,
                );
                removed.push(ro);
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
                            if let Some(v) =
                                self.position_reservations.get_mut(&(ro.account_id, key))
                            {
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
                    expires_at_block: ro.expires_at_block,
                    reserved_balance: new_cost,
                    reserved_positions: new_pos_reservations,
                    has_been_matched: true,
                    original_max_fill: ro.original_max_fill,
                });
            } else {
                // Unfilled — keep as-is
                new_orders.push(ro);
            }
        }

        self.orders = new_orders;
        removed
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

pub(crate) fn resting_order_snapshots(
    orders: &[RestingOrder],
) -> Vec<sybil_verifier::RestingOrderSnapshot> {
    let mut snapshots: Vec<_> = orders
        .iter()
        .map(|resting| {
            let mut reserved_positions: Vec<_> = resting
                .reserved_positions
                .iter()
                .map(|&((market, outcome), qty)| (market, outcome, qty))
                .collect();
            reserved_positions.sort_by_key(|&(market, outcome, _)| (market.0, outcome));
            sybil_verifier::RestingOrderSnapshot {
                order: resting.order.clone(),
                account_id: resting.account_id.0,
                created_at: resting.created_at,
                expires_at_block: resting.expires_at_block,
                reserved_balance: resting.reserved_balance,
                reserved_positions,
            }
        })
        .collect();
    snapshots.sort_by_key(|snapshot| snapshot.order.id);
    snapshots
}

pub(crate) fn reservation_snapshots_from_resting_orders(
    orders: &[RestingOrder],
) -> Vec<sybil_verifier::AccountReservationSnapshot> {
    let mut balance_reservations = HashMap::new();
    let mut position_reservations = HashMap::new();
    for resting in orders {
        if resting.reserved_balance > 0 {
            *balance_reservations.entry(resting.account_id).or_insert(0) +=
                resting.reserved_balance;
        }
        for &(key, qty) in &resting.reserved_positions {
            if qty != 0 {
                *position_reservations
                    .entry((resting.account_id, key))
                    .or_insert(0) += qty;
            }
        }
    }
    reservation_snapshots_from_aggregates(&balance_reservations, &position_reservations)
}

fn reservation_snapshots_from_aggregates(
    balance_reservations: &HashMap<AccountId, i64>,
    position_reservations: &HashMap<(AccountId, PositionKey), i64>,
) -> Vec<sybil_verifier::AccountReservationSnapshot> {
    let mut by_account: BTreeMap<u64, sybil_verifier::AccountReservationSnapshot> = BTreeMap::new();

    for (&account_id, &reserved_balance) in balance_reservations {
        if reserved_balance == 0 {
            continue;
        }
        by_account
            .entry(account_id.0)
            .or_insert_with(|| sybil_verifier::AccountReservationSnapshot {
                account_id: account_id.0,
                reserved_balance: 0,
                reserved_positions: Vec::new(),
            })
            .reserved_balance += reserved_balance;
    }

    for (&(account_id, (market, outcome)), &qty) in position_reservations {
        if qty == 0 {
            continue;
        }
        by_account
            .entry(account_id.0)
            .or_insert_with(|| sybil_verifier::AccountReservationSnapshot {
                account_id: account_id.0,
                reserved_balance: 0,
                reserved_positions: Vec::new(),
            })
            .reserved_positions
            .push((market, outcome, qty));
    }

    let mut snapshots: Vec<_> = by_account.into_values().collect();
    for snapshot in &mut snapshots {
        snapshot
            .reserved_positions
            .sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    }
    snapshots
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
        book.settle(&fills, &HashSet::new(), 1);

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
        book.settle(&fills, &HashSet::new(), 1);

        // Remaining: 6 of 10 = 60%
        assert_eq!(book.len(), 1);
        let new_reserved = book.reserved_balance(aid);
        assert!(new_reserved < original_reserved);
        assert!(new_reserved > 0);

        // Check remaining order has max_fill = 6
        let (remaining_order, _) = book.resting_orders().next().unwrap();
        assert_eq!(remaining_order.max_fill, 6);
    }

    #[test]
    fn settle_ioc_unfilled_order_does_not_rest() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        let mut order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        order.expires_at_block = Some(1);
        book.accept(order, aid, account, 0).unwrap();

        book.settle(&[], &HashSet::new(), 1);

        assert_eq!(book.reserved_balance(aid), 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn gtd_expiry_uses_explicit_block_when_stricter_than_ttl() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        let account = accounts.get(aid).unwrap();

        let mut order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        order.expires_at_block = Some(2);
        book.accept(order, aid, account, 1).unwrap();

        book.expire(2);
        assert_eq!(book.len(), 1);

        book.expire(3);
        assert_eq!(book.len(), 0);
        assert_eq!(book.reserved_balance(aid), 0);
    }

    #[test]
    fn restore_drops_orders_with_negative_balance_reservation() {
        let bad = RestingOrder {
            order: Order::new(42),
            account_id: AccountId(1),
            created_at: 0,
            expires_at_block: 10,
            reserved_balance: -100,
            reserved_positions: vec![],
            has_been_matched: false,
            original_max_fill: 0,
        };
        let book = OrderBook::restore(vec![bad], 10);
        assert_eq!(book.len(), 0);
        assert_eq!(book.reserved_balance(AccountId(1)), 0);
    }

    #[test]
    fn restore_rebuilds_aggregates_from_valid_orders() {
        let ro = RestingOrder {
            order: Order::new(1),
            account_id: AccountId(7),
            created_at: 0,
            expires_at_block: 10,
            reserved_balance: 500,
            reserved_positions: vec![],
            has_been_matched: false,
            original_max_fill: 0,
        };
        let book = OrderBook::restore(vec![ro], 10);
        assert_eq!(book.len(), 1);
        assert_eq!(book.reserved_balance(AccountId(7)), 500);
    }

    #[test]
    fn cancel_releases_reservations() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(3);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        let accepted = book.accept(order, aid, account, 1).unwrap();
        assert!(book.reserved_balance(aid) > 0);

        book.cancel(aid, accepted.order.id).unwrap();

        assert_eq!(book.reserved_balance(aid), 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn expire_returns_removed_orders() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        let account = accounts.get(aid).unwrap();

        for id in 1..=3 {
            let mut order = buy_yes(&markets, id, m0, NANOS_PER_DOLLAR / 2, 2);
            order.expires_at_block = Some(1);
            book.accept(order, aid, account, 0).unwrap();
        }

        let removed = book.expire(2);
        assert_eq!(removed.len(), 3);
        assert_eq!(book.len(), 0);
        assert!(removed.iter().all(|ro| !ro.has_been_matched));
        // original_max_fill survives the removal
        assert!(removed.iter().all(|ro| ro.original_max_fill == 2));
    }

    #[test]
    fn settle_marks_matched() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 5);
        let accepted = book.accept(order, aid, account, 1).unwrap();
        let order_id = accepted.order.id;

        let fills = vec![Fill {
            order_id,
            fill_qty: 5,
            fill_price: NANOS_PER_DOLLAR / 2,
            account_id: 0,
        }];
        let removed = book.settle(&fills, &HashSet::new(), 1);
        assert_eq!(removed.len(), 1);
        assert!(removed[0].has_been_matched);
        assert_eq!(removed[0].original_max_fill, 5);
    }

    #[test]
    fn cancel_returns_order() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 7, m0, NANOS_PER_DOLLAR / 2, 5);
        let accepted = book.accept(order, aid, account, 1).unwrap();

        let ro = book.cancel(aid, accepted.order.id).unwrap();
        assert_eq!(ro.order.id, accepted.order.id);
        assert_eq!(ro.original_max_fill, 5);
        assert!(!ro.has_been_matched);
    }

    #[test]
    fn resting_order_serde_default() {
        // Old (pre-B5) layout missing both new fields. Round-trip via rmp_serde
        // — the same encoder the redb snapshot uses — and confirm the new
        // fields fall back to their #[serde(default)] values.
        #[derive(serde::Serialize)]
        struct OldRestingOrder {
            order: Order,
            account_id: AccountId,
            created_at: u64,
            expires_at_block: u64,
            reserved_balance: i64,
            reserved_positions: Vec<(PositionKey, i64)>,
        }
        let old = OldRestingOrder {
            order: Order::new(1),
            account_id: AccountId(1),
            created_at: 0,
            expires_at_block: 10,
            reserved_balance: 0,
            reserved_positions: vec![],
        };
        let bytes = rmp_serde::to_vec(&old).unwrap();
        let ro: RestingOrder = rmp_serde::from_slice(&bytes).unwrap();
        assert!(!ro.has_been_matched);
        assert_eq!(ro.original_max_fill, 0);
    }

    #[test]
    fn settle_partial_fill_remainder_marks_matched() {
        let (mut accounts, markets, m0) = setup();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        let account = accounts.get(aid).unwrap();

        let order = buy_yes(&markets, 1, m0, NANOS_PER_DOLLAR / 2, 10);
        let accepted = book.accept(order, aid, account, 1).unwrap();
        let order_id = accepted.order.id;

        let fills = vec![Fill {
            order_id,
            fill_qty: 3,
            fill_price: NANOS_PER_DOLLAR / 2,
            account_id: 0,
        }];
        let removed = book.settle(&fills, &HashSet::new(), 1);
        // Partial fill: nothing removed, but the remaining order in the book
        // now carries has_been_matched=true and original_max_fill=10.
        assert!(removed.is_empty());
        assert_eq!(book.len(), 1);
        let remainder = book.orders.first().unwrap();
        assert!(remainder.has_been_matched);
        assert_eq!(remainder.original_max_fill, 10);
    }
}
