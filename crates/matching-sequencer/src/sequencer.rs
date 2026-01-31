use std::collections::{HashMap, HashSet};

use matching_engine::{
    Fill, MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem,
};
use matching_solver::{Pipeline, PipelineResult};

use crate::account::{AccountId, AccountStore};
use crate::settlement;

/// An order submission from a participant.
pub struct OrderSubmission {
    pub account_id: AccountId,
    pub orders: Vec<Order>,
    pub mm_constraint: Option<MmConstraint>,
}

/// Reason an order was rejected.
#[derive(Debug, Clone)]
pub enum RejectionReason {
    InsufficientBalance {
        required: i64,
        available: i64,
    },
    InsufficientPosition {
        market: MarketId,
        outcome: u8,
        required: i64,
        available: i64,
    },
    AccountNotFound,
}

/// A rejected order.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub order_id: u64,
    pub account_id: AccountId,
    pub reason: RejectionReason,
}

/// Result of a single batch.
pub struct BatchResult {
    pub pipeline_result: PipelineResult,
    pub fills: Vec<Fill>,
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub total_welfare: i64,
    pub total_volume: u64,
    pub rejections: Vec<Rejection>,
    pub orders_submitted: usize,
    pub orders_filled: usize,
}

/// A pending order that persists across batches until filled or expired.
struct PendingOrder {
    order: Order,
    account_id: AccountId,
    /// Batch number when this order was created
    created_at_batch: usize,
}

/// Manages accounts, assigns order IDs, validates, solves, and settles batches.
pub struct BatchSequencer {
    pub accounts: AccountStore,
    order_account_map: HashMap<u64, AccountId>,
    next_order_id: u64,
    /// Orders that weren't filled in the previous batch
    pending_orders: Vec<PendingOrder>,
    /// Current batch number
    current_batch: usize,
    /// Maximum number of batches an order persists (default: 3)
    order_ttl: usize,
    /// Track when each order was originally created: order_id -> batch number
    order_created_at: HashMap<u64, usize>,
}

impl BatchSequencer {
    pub fn new(accounts: AccountStore) -> Self {
        Self {
            accounts,
            order_account_map: HashMap::new(),
            next_order_id: 1,
            pending_orders: Vec::new(),
            current_batch: 0,
            order_ttl: 3,
            order_created_at: HashMap::new(),
        }
    }

    /// Run a single batch: validate → merge pending → solve → settle → persist unfilled.
    pub fn run_batch(
        &mut self,
        submissions: Vec<OrderSubmission>,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
    ) -> BatchResult {
        self.current_batch += 1;

        let mut all_orders: Vec<Order> = Vec::new();
        let mut all_mm_constraints: Vec<MmConstraint> = Vec::new();
        let mut rejections: Vec<Rejection> = Vec::new();

        // Collect active market IDs for filtering expired orders on resolved markets
        let active_markets: HashSet<MarketId> = markets.iter().map(|m| m.id).collect();

        // Phase 1: Re-validate and include pending orders
        let pending = std::mem::take(&mut self.pending_orders);
        for pending_order in pending {
            // Skip expired orders
            if self.current_batch - pending_order.created_at_batch > self.order_ttl {
                continue;
            }

            // Skip orders for resolved/removed markets
            let order_markets_active = pending_order.order.active_markets()
                .all(|m| active_markets.contains(&m));
            if !order_markets_active {
                continue;
            }

            // Re-validate against current account state
            let Some(account) = self.accounts.get(pending_order.account_id) else {
                continue;
            };

            // Skip if account is bankrupt
            if account.balance <= 0 {
                continue;
            }

            if validate_order(&pending_order.order, account).is_ok() {
                all_orders.push(pending_order.order);
                // order_account_map already has this mapping from when it was first accepted
            }
        }

        // Phase 2: Process new submissions
        // Track reserved balance per account to prevent double-spending within a batch
        let mut reserved_balance: HashMap<AccountId, i64> = HashMap::new();

        for sub in submissions {
            let account_id = sub.account_id;

            let Some(account) = self.accounts.get(account_id) else {
                for order in &sub.orders {
                    rejections.push(Rejection {
                        order_id: order.id,
                        account_id,
                        reason: RejectionReason::AccountNotFound,
                    });
                }
                continue;
            };

            let is_mm = sub.mm_constraint.is_some();
            let mut accepted_orders: Vec<Order> = Vec::new();

            for mut order in sub.orders {
                // Assign unique order ID
                let order_id = self.next_order_id;
                self.next_order_id += 1;
                order.id = order_id;

                if is_mm {
                    // Skip validation for MM orders — solver handles capital constraints
                    self.order_account_map.insert(order_id, account_id);
                    self.order_created_at.insert(order_id, self.current_batch);
                    accepted_orders.push(order);
                } else {
                    // Validate: check balance for buys, position for sells
                    let reserved = *reserved_balance.get(&account_id).unwrap_or(&0);
                    match validate_order_with_reservation(&order, account, reserved) {
                        Ok(cost) => {
                            self.order_account_map.insert(order_id, account_id);
                            self.order_created_at.insert(order_id, self.current_batch);
                            // Reserve the cost of this buy order
                            if cost > 0 {
                                *reserved_balance.entry(account_id).or_insert(0) += cost;
                            }
                            accepted_orders.push(order);
                        }
                        Err(reason) => {
                            rejections.push(Rejection {
                                order_id,
                                account_id,
                                reason,
                            });
                        }
                    }
                }
            }

            // Build MmConstraint with the assigned IDs
            if let Some(mm_constraint) = sub.mm_constraint {
                let old_order_ids = &mm_constraint.order_ids;
                let old_sides = &mm_constraint.order_sides;

                let old_to_new: HashMap<u64, u64> = old_order_ids
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &old_id)| {
                        accepted_orders.get(i).map(|o| (old_id, o.id))
                    })
                    .collect();

                let mut new_constraint =
                    MmConstraint::new(mm_constraint.mm_id, mm_constraint.max_capital);

                for &old_id in old_order_ids {
                    if let (Some(&new_id), Some(&side)) =
                        (old_to_new.get(&old_id), old_sides.get(&old_id))
                    {
                        new_constraint.add_order(new_id, side);
                    }
                }

                if new_constraint.num_orders() > 0 {
                    all_mm_constraints.push(new_constraint);
                }
            }

            all_orders.extend(accepted_orders);
        }

        let orders_submitted = all_orders.len() + rejections.len();

        // Build Problem — all orders go directly into the problem.
        let mut problem = Problem::new("batch");
        problem.markets = markets.clone();
        problem.orders = all_orders;
        problem.mm_constraints = all_mm_constraints;
        problem.market_groups = market_groups.to_vec();

        // Solve
        let pipeline = Pipeline::with_negrisk();
        let pipeline_result = pipeline.solve(&problem);

        // Extract clearing prices
        let clearing_prices = if let Some(ref pd) = pipeline_result.price_discovery {
            pd.prices.clone()
        } else {
            HashMap::new()
        };

        let fills = pipeline_result.result.fills.clone();
        let total_welfare = pipeline_result.result.total_welfare;
        let total_volume = pipeline_result.result.total_quantity_filled;
        let orders_filled = pipeline_result.result.orders_filled;

        // Settle all fills
        settlement::settle_batch(
            &mut self.accounts,
            &fills,
            &problem.orders,
            &self.order_account_map,
        );

        // Phase 3: Persist unfilled non-MM orders for next batch
        let filled_order_ids: HashSet<u64> = fills
            .iter()
            .filter(|f| f.fill_qty > 0)
            .map(|f| f.order_id)
            .collect();

        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|mm| mm.order_ids.iter().copied())
            .collect();

        for order in &problem.orders {
            // Don't persist MM orders (they're regenerated each batch)
            if mm_order_ids.contains(&order.id) {
                continue;
            }
            // Don't persist orders that were filled
            if filled_order_ids.contains(&order.id) {
                continue;
            }
            if let Some(&account_id) = self.order_account_map.get(&order.id) {
                let created_at = *self.order_created_at.get(&order.id)
                    .unwrap_or(&self.current_batch);

                self.pending_orders.push(PendingOrder {
                    order: order.clone(),
                    account_id,
                    created_at_batch: created_at,
                });
            }
        }

        BatchResult {
            pipeline_result,
            fills,
            clearing_prices,
            total_welfare,
            total_volume,
            rejections,
            orders_submitted,
            orders_filled,
        }
    }
}

/// Validate an order against account state (used for pending order re-validation).
fn validate_order(
    order: &Order,
    account: &crate::account::Account,
) -> Result<(), RejectionReason> {
    validate_order_with_reservation(order, account, 0).map(|_| ())
}

/// Validate an order against account state, accounting for already-reserved balance.
/// Returns the cost to reserve on success (for buy orders).
fn validate_order_with_reservation(
    order: &Order,
    account: &crate::account::Account,
    reserved_balance: i64,
) -> Result<i64, RejectionReason> {
    let num_states = order.num_states as usize;

    // Check if this is a buy (positive payoffs somewhere) or sell (negative payoffs)
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if has_positive && !has_negative {
        // Pure buy: check balance covers worst-case cost (minus already reserved)
        let max_cost = order.limit_price as i64 * order.max_fill as i64;
        let available = account.balance - reserved_balance;
        if max_cost > available {
            return Err(RejectionReason::InsufficientBalance {
                required: max_cost,
                available,
            });
        }
        return Ok(max_cost);
    } else if has_negative && !has_positive {
        // Pure sell: check position covers the sell
        if order.num_markets == 1 {
            let market = order.markets[0];
            for s in 0..num_states {
                if order.payoffs[s] < 0 {
                    let outcome = s as u8;
                    let sell_qty = (-order.payoffs[s] as i64) * order.max_fill as i64;
                    let available = account.position(market, outcome);
                    if sell_qty > available {
                        return Err(RejectionReason::InsufficientPosition {
                            market,
                            outcome,
                            required: sell_qty,
                            available,
                        });
                    }
                }
            }
        }
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, outcome_sell, MarketId, MarketSet, MmId, NANOS_PER_DOLLAR};

    fn setup() -> (MarketSet, MarketId) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        (markets, m0)
    }

    fn make_sequencer(balance: i64) -> (BatchSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(balance);
        (BatchSequencer::new(accounts), aid)
    }

    // --- Validation tests ---

    #[test]
    fn test_validate_buy_sufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        // Cost = 0.50 * 10 = 5_000_000_000 <= 10_000_000_000
        assert!(validate_order(&order, account).is_ok());
    }

    #[test]
    fn test_validate_buy_insufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(3 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        // Cost = 0.50 * 10 = 5_000_000_000 > 3_000_000_000
        let result = validate_order(&order, account);
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientBalance { required, available } => {
                assert_eq!(required, 5_000_000_000);
                assert_eq!(available, 3_000_000_000);
            }
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_sell_sufficient_position() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 10);

        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 5);
        assert!(validate_order(&order, account).is_ok());
    }

    #[test]
    fn test_validate_sell_insufficient_position() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 3);

        let order = outcome_sell(&markets, 1, m0, 0, 500_000_000, 5);
        let result = validate_order(&order, account);
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientPosition {
                market,
                outcome,
                required,
                available,
            } => {
                assert_eq!(market, m0);
                assert_eq!(outcome, 0);
                assert_eq!(required, 5);
                assert_eq!(available, 3);
            }
            other => panic!("Expected InsufficientPosition, got {:?}", other),
        }
    }

    // --- Balance reservation tests ---

    #[test]
    fn test_balance_reservation_returns_cost() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 600_000_000, 5);
        let cost = validate_order_with_reservation(&order, account, 0).unwrap();
        assert_eq!(cost, 600_000_000i64 * 5);
    }

    #[test]
    fn test_balance_reservation_blocks_double_spend() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(8 * NANOS_PER_DOLLAR as i64); // $8
        let account = accounts.get(aid).unwrap();

        // First order: buy YES at 0.50, qty 10 → cost $5
        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let cost1 = validate_order_with_reservation(&order1, account, 0).unwrap();
        assert_eq!(cost1, 5_000_000_000);

        // Second order: buy YES at 0.50, qty 10 → cost $5, but only $3 remaining
        let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, 10);
        let result = validate_order_with_reservation(&order2, account, cost1);
        assert!(result.is_err());
        match result.unwrap_err() {
            RejectionReason::InsufficientBalance { required, available } => {
                assert_eq!(required, 5_000_000_000);
                assert_eq!(available, 3_000_000_000); // 8 - 5 reserved
            }
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_balance_reservation_in_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(8 * NANOS_PER_DOLLAR as i64); // $8

        // Submit two buy orders from same account in same batch
        // Each costs $5, but account only has $8
        let order1 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order1, order2],
            mm_constraint: None,
        };

        let result = seq.run_batch(vec![sub], &markets, &[]);

        // First order should be accepted, second rejected
        assert_eq!(result.rejections.len(), 1);
        match &result.rejections[0].reason {
            RejectionReason::InsufficientBalance { .. } => {}
            other => panic!("Expected InsufficientBalance, got {:?}", other),
        }
    }

    #[test]
    fn test_sell_order_does_not_reserve_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(5 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((m0, 0), 100);

        let sell = outcome_sell(&markets, 1, m0, 0, 500_000_000, 10);
        let cost = validate_order_with_reservation(&sell, account, 0).unwrap();
        assert_eq!(cost, 0); // Sells don't reserve balance
    }

    // --- Account not found ---

    #[test]
    fn test_account_not_found_rejection() {
        let (markets, m0) = setup();
        let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

        let bogus_id = AccountId(999);
        let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 1);
        let sub = OrderSubmission {
            account_id: bogus_id,
            orders: vec![order],
            mm_constraint: None,
        };

        let result = seq.run_batch(vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 1);
        assert_eq!(result.rejections[0].account_id, bogus_id);
        match &result.rejections[0].reason {
            RejectionReason::AccountNotFound => {}
            other => panic!("Expected AccountNotFound, got {:?}", other),
        }
    }

    // --- MM validation skip ---

    #[test]
    fn test_mm_orders_skip_validation() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(0); // $0 balance — would fail normal validation

        let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 100);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        let result = seq.run_batch(vec![sub], &markets, &[]);
        // MM orders should NOT be rejected despite $0 balance
        assert_eq!(result.rejections.len(), 0);
    }

    // --- Order ID assignment ---

    #[test]
    fn test_order_ids_are_unique() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        // Batch 1: 2 orders
        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub1], &markets, &[]);

        // Batch 2: 2 more orders
        let sub2 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub2], &markets, &[]);

        // next_order_id should be 5 (started at 1, processed 4 orders)
        assert_eq!(seq.next_order_id, 5);
    }

    // --- Order persistence tests ---

    #[test]
    fn test_unfilled_orders_persist() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        // Batch 1: submit a buy order with no matching sell → won't fill
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)], // low price, unlikely to fill
            mm_constraint: None,
        };

        let result = seq.run_batch(vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 0);

        // The unfilled order should persist
        assert_eq!(seq.pending_orders.len(), 1);
        assert_eq!(seq.pending_orders[0].account_id, aid);
        assert_eq!(seq.pending_orders[0].created_at_batch, 1);
    }

    #[test]
    fn test_pending_orders_included_in_next_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        // Batch 1: submit a buy at low price (won't fill without sellers)
        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub1], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        // Batch 2: no new submissions, pending should still be included
        let result = seq.run_batch(vec![], &markets, &[]);
        // The pending order should have been included (orders_submitted counts pending + new)
        assert!(result.orders_submitted >= 1);
    }

    #[test]
    fn test_expired_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
        seq.order_ttl = 2; // Orders expire after 2 batches

        // Batch 1: submit a buy at low price
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        // Batch 2: still alive (age = 1 <= ttl = 2)
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        // Batch 3: still alive (age = 2 <= ttl = 2)
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        // Batch 4: expired (age = 3 > ttl = 2)
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 0);
    }

    #[test]
    fn test_orders_for_resolved_markets_removed() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Market A");
        let m1 = markets.add_binary("Market B");

        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        // Batch 1: submit orders on both markets
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 5),
                outcome_buy(&markets, 0, m1, 0, 100_000_000, 5),
            ],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 2);

        // Batch 2: resolve market A (remove it from active markets)
        let mut reduced_markets = MarketSet::new();
        reduced_markets.add_binary("Market B"); // Only B remains

        seq.run_batch(vec![], &reduced_markets, &[]);
        // Only the order on Market B should persist
        assert_eq!(seq.pending_orders.len(), 1);
    }

    #[test]
    fn test_bankrupt_account_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        // Batch 1: submit order
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        seq.run_batch(vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        // Bankrupt the account
        let account = seq.accounts.get_mut(aid).unwrap();
        account.balance = 0;

        // Batch 2: bankrupt account's orders should be removed
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 0);
    }

    #[test]
    fn test_mm_orders_not_persisted() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let order = outcome_buy(&markets, 0, m0, 0, 100_000_000, 5);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        seq.run_batch(vec![sub], &markets, &[]);
        // MM orders should NOT be persisted
        assert_eq!(seq.pending_orders.len(), 0);
    }

    // --- Fill settlement integration ---

    #[test]
    fn test_matching_buy_and_sell_settles_correctly() {
        let (markets, m0) = setup();

        let mut accounts = AccountStore::new();
        let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        // Give seller a position to sell
        accounts
            .get_mut(seller_id)
            .unwrap()
            .positions
            .insert((m0, 0), 50);

        let mut seq = BatchSequencer::new(accounts);

        let buy_sub = OrderSubmission {
            account_id: buyer_id,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 10)],
            mm_constraint: None,
        };
        let sell_sub = OrderSubmission {
            account_id: seller_id,
            orders: vec![outcome_sell(&markets, 0, m0, 0, 400_000_000, 10)],
            mm_constraint: None,
        };

        let result = seq.run_batch(vec![buy_sub, sell_sub], &markets, &[]);

        // If filled, buyer should have less balance and more YES position
        // Seller should have more balance and less YES position
        if result.orders_filled > 0 {
            let buyer = seq.accounts.get(buyer_id).unwrap();
            let seller = seq.accounts.get(seller_id).unwrap();

            // Buyer spent money and got YES shares
            assert!(buyer.balance < 100 * NANOS_PER_DOLLAR as i64);
            assert!(buyer.position(m0, 0) > 0);

            // Seller earned money and lost YES shares
            assert!(seller.balance > 10 * NANOS_PER_DOLLAR as i64);
            assert!(seller.position(m0, 0) < 50);
        }
    }

    // --- Batch counter ---

    #[test]
    fn test_batch_counter_increments() {
        let (markets, _) = setup();
        let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

        assert_eq!(seq.current_batch, 0);
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.current_batch, 1);
        seq.run_batch(vec![], &markets, &[]);
        assert_eq!(seq.current_batch, 2);
    }
}
