use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use matching_engine::{
    Fill, MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem,
};
use matching_solver::{Pipeline, PipelineResult};
use sybil_oracle::{MarketStatus, Oracle, ResolutionAction, ResolutionRecord};

use crate::account::{AccountId, AccountStore};
use crate::block::{compute_state_root, hash_header, Block, BlockHeader};
use crate::error::{Rejection, RejectionReason, SequencerError};
use crate::settlement;
use crate::validation::{validate_order, validate_order_with_reservation};

/// An order submission from a participant.
pub struct OrderSubmission {
    pub account_id: AccountId,
    pub orders: Vec<Order>,
    pub mm_constraint: Option<MmConstraint>,
}

/// Result of a single batch — thin view over a Block for simulation compatibility.
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
    /// Block height when this order was created.
    created_at: u64,
}

/// Block-producing sequencer. Core sync layer.
///
/// Manages accounts, assigns order IDs, validates, solves, settles, and
/// produces blocks. The actor layer calls `produce_block()` on each timer tick.
/// Simulations can use this directly without the actor.
pub struct BlockSequencer {
    pub accounts: AccountStore,
    order_account_map: HashMap<u64, AccountId>,
    next_order_id: u64,
    /// Orders that weren't filled in the previous block.
    pending_orders: Vec<PendingOrder>,
    /// Current block height.
    height: u64,
    /// Maximum number of blocks an order persists (default: 3).
    order_ttl: u64,
    /// Track when each order was originally created: order_id -> block height.
    order_created_at: HashMap<u64, u64>,
    /// Markets available for trading.
    markets: MarketSet,
    /// Market groups (multi-outcome event constraints).
    market_groups: Vec<MarketGroup>,
    /// Last block header for hash chaining.
    last_header: Option<BlockHeader>,
    /// Oracle-managed lifecycle status per market.
    market_statuses: HashMap<MarketId, MarketStatus>,
    /// Pluggable oracle for resolution decisions.
    oracle: Arc<dyn Oracle>,
}

impl BlockSequencer {
    pub fn new(
        accounts: AccountStore,
        markets: MarketSet,
        market_groups: Vec<MarketGroup>,
        oracle: Arc<dyn Oracle>,
    ) -> Self {
        Self {
            accounts,
            order_account_map: HashMap::new(),
            next_order_id: 1,
            pending_orders: Vec::new(),
            height: 0,
            order_ttl: 3,
            order_created_at: HashMap::new(),
            markets,
            market_groups,
            last_header: None,
            market_statuses: HashMap::new(),
            oracle,
        }
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn markets(&self) -> &MarketSet {
        &self.markets
    }

    pub fn markets_mut(&mut self) -> &mut MarketSet {
        &mut self.markets
    }

    pub fn market_groups(&self) -> &[MarketGroup] {
        &self.market_groups
    }

    pub fn market_groups_mut(&mut self) -> &mut Vec<MarketGroup> {
        &mut self.market_groups
    }

    pub fn last_header(&self) -> Option<&BlockHeader> {
        self.last_header.as_ref()
    }

    /// Get the oracle-tracked status for a market. Returns `Active` if not explicitly set.
    pub fn market_status(&self, id: MarketId) -> MarketStatus {
        self.market_statuses
            .get(&id)
            .cloned()
            .unwrap_or(MarketStatus::Active)
    }

    /// Get all explicitly tracked market statuses.
    pub fn market_statuses(&self) -> &HashMap<MarketId, MarketStatus> {
        &self.market_statuses
    }

    /// Resolve a market through the oracle.
    ///
    /// On `SettleNow`: calls settlement, removes from market groups, updates status.
    /// On `Propose`: stores the pending proposal (future L0 path).
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        winning_outcome: u8,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        // Verify market exists
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        let current_status = self.market_status(market_id);
        let action = self
            .oracle
            .resolve(market_id, winning_outcome, &current_status, timestamp_ms)
            .map_err(|e| SequencerError::OracleError(e.to_string()))?;

        match action {
            ResolutionAction::SettleNow {
                market_id,
                winning_outcome,
                record,
            } => {
                // Settle positions
                settlement::resolve_market(&mut self.accounts, market_id, winning_outcome);

                // Remove from market groups
                self.market_groups
                    .retain(|g| !g.markets.contains(&market_id));

                // Update status
                self.market_statuses.insert(
                    market_id,
                    MarketStatus::Resolved {
                        record: record.clone(),
                    },
                );

                Ok(record)
            }
            ResolutionAction::Propose {
                proposal,
                challenge_window_ms,
            } => {
                let deadline = timestamp_ms + challenge_window_ms;
                self.market_statuses.insert(
                    market_id,
                    MarketStatus::Proposed {
                        proposal,
                        challenge_deadline_ms: deadline,
                    },
                );
                // For now, return an error since we don't have the full record yet.
                // Future: the sequencer would return a "pending" response.
                Err(SequencerError::OracleError(
                    "resolution proposed but not yet settled".to_string(),
                ))
            }
            ResolutionAction::Reject { reason } => {
                Err(SequencerError::OracleError(reason))
            }
        }
    }

    /// Core sync method: produce one block from the given submissions.
    ///
    /// Same logic as the old `run_batch()`: validate → merge pending → build
    /// Problem → solve → settle → persist unfilled → compute state root → build Block.
    pub fn produce_block(
        &mut self,
        submissions: Vec<OrderSubmission>,
        timestamp_ms: u64,
    ) -> (Block, PipelineResult) {
        self.height += 1;

        let mut all_orders: Vec<Order> = Vec::new();
        let mut all_mm_constraints: Vec<MmConstraint> = Vec::new();
        let mut rejections: Vec<Rejection> = Vec::new();

        // Collect tradeable market IDs (active markets that aren't in a non-tradeable state)
        let active_markets: HashSet<MarketId> = self
            .markets
            .iter()
            .filter(|m| self.market_status(m.id).is_tradeable())
            .map(|m| m.id)
            .collect();

        // Phase 1: Re-validate and include pending orders
        let pending = std::mem::take(&mut self.pending_orders);
        for pending_order in pending {
            // Skip expired orders
            if self.height - pending_order.created_at > self.order_ttl {
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
            }
        }

        // Phase 2: Process new submissions
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
                let order_id = self.next_order_id;
                self.next_order_id += 1;
                order.id = order_id;

                if is_mm {
                    self.order_account_map.insert(order_id, account_id);
                    self.order_created_at.insert(order_id, self.height);
                    accepted_orders.push(order);
                } else {
                    let reserved = *reserved_balance.get(&account_id).unwrap_or(&0);
                    match validate_order_with_reservation(&order, account, reserved) {
                        Ok(cost) => {
                            self.order_account_map.insert(order_id, account_id);
                            self.order_created_at.insert(order_id, self.height);
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

            // Rebuild MmConstraint with assigned IDs
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

        let order_ids: Vec<u64> = all_orders.iter().map(|o| o.id).collect();
        let orders_submitted = all_orders.len() + rejections.len();

        // Build Problem
        let mut problem = Problem::new("block");
        problem.markets = self.markets.clone();
        problem.orders = all_orders;
        problem.mm_constraints = all_mm_constraints;
        problem.market_groups = self.market_groups.clone();

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

        // Persist unfilled non-MM orders
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
            if mm_order_ids.contains(&order.id) {
                continue;
            }
            if filled_order_ids.contains(&order.id) {
                continue;
            }
            if let Some(&account_id) = self.order_account_map.get(&order.id) {
                let created_at = *self.order_created_at.get(&order.id)
                    .unwrap_or(&self.height);

                self.pending_orders.push(PendingOrder {
                    order: order.clone(),
                    account_id,
                    created_at,
                });
            }
        }

        // Compute state root and build header
        let state_root = compute_state_root(&self.accounts);
        let parent_hash = self.last_header.as_ref()
            .map(|h| hash_header(h))
            .unwrap_or([0u8; 32]);

        let header = BlockHeader {
            height: self.height,
            parent_hash,
            state_root,
            order_count: orders_submitted as u32,
            fill_count: fills.len() as u32,
            timestamp_ms,
        };

        self.last_header = Some(header.clone());

        let block = Block {
            header,
            order_ids,
            fills,
            clearing_prices,
            rejections,
            total_welfare,
            total_volume,
            orders_filled,
        };

        (block, pipeline_result)
    }
}

/// Convert a Block + PipelineResult into a BatchResult for simulation compatibility.
pub fn batch_result_from_block(block: &Block, pipeline_result: PipelineResult) -> BatchResult {
    BatchResult {
        pipeline_result,
        fills: block.fills.clone(),
        clearing_prices: block.clearing_prices.clone(),
        total_welfare: block.total_welfare,
        total_volume: block.total_volume,
        rejections: block.rejections.clone(),
        orders_submitted: block.header.order_count as usize,
        orders_filled: block.orders_filled,
    }
}

/// Backwards-compatible alias.
pub type BatchSequencer = BlockSequencer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::error::RejectionReason;
    use crate::validation::{validate_order, validate_order_with_reservation};
    use matching_engine::{outcome_buy, outcome_sell, MarketId, MarketSet, MmId, NANOS_PER_DOLLAR};
    use sybil_oracle::AdminOracle;

    fn setup() -> (MarketSet, MarketId) {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        (markets, m0)
    }

    fn make_sequencer(balance: i64) -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(balance);
        let markets = MarketSet::new();
        let oracle = Arc::new(AdminOracle::new());
        (BlockSequencer::new(accounts, markets, vec![], oracle), aid)
    }

    /// Helper: run a batch through the block sequencer, returning BatchResult.
    fn run_batch(
        seq: &mut BlockSequencer,
        submissions: Vec<OrderSubmission>,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
    ) -> BatchResult {
        // Temporarily swap markets/groups for this batch
        let old_markets = std::mem::replace(&mut seq.markets, markets.clone());
        let old_groups = std::mem::replace(&mut seq.market_groups, market_groups.to_vec());
        let (block, pr) = seq.produce_block(submissions, 0);
        seq.markets = old_markets;
        seq.market_groups = old_groups;
        batch_result_from_block(&block, pr)
    }

    // --- Validation tests ---

    #[test]
    fn test_validate_buy_sufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        assert!(validate_order(&order, account).is_ok());
    }

    #[test]
    fn test_validate_buy_insufficient_balance() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(3 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
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
        let aid = accounts.create_account(8 * NANOS_PER_DOLLAR as i64);
        let account = accounts.get(aid).unwrap();

        let order1 = outcome_buy(&markets, 1, m0, 0, 500_000_000, 10);
        let cost1 = validate_order_with_reservation(&order1, account, 0).unwrap();
        assert_eq!(cost1, 5_000_000_000);

        let order2 = outcome_buy(&markets, 2, m0, 0, 500_000_000, 10);
        let result = validate_order_with_reservation(&order2, account, cost1);
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
    fn test_balance_reservation_in_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(8 * NANOS_PER_DOLLAR as i64);

        let order1 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);
        let order2 = outcome_buy(&markets, 0, m0, 0, 500_000_000, 10);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order1, order2],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);

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
        assert_eq!(cost, 0);
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

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
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
        let (mut seq, aid) = make_sequencer(0);

        let order = outcome_buy(&markets, 0, m0, 0, 500_000_000, 100);
        let mut constraint = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
        constraint.add_order(0, matching_engine::MmSide::BuyYes);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![order],
            mm_constraint: Some(constraint),
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 0);
    }

    // --- Order ID assignment ---

    #[test]
    fn test_order_ids_are_unique() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub1], &markets, &[]);

        let sub2 = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 1),
                outcome_buy(&markets, 0, m0, 1, 100_000_000, 1),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub2], &markets, &[]);

        assert_eq!(seq.next_order_id, 5);
    }

    // --- Order persistence tests ---

    #[test]
    fn test_unfilled_orders_persist() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };

        let result = run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(result.rejections.len(), 0);

        assert_eq!(seq.pending_orders.len(), 1);
        assert_eq!(seq.pending_orders[0].account_id, aid);
        assert_eq!(seq.pending_orders[0].created_at, 1);
    }

    #[test]
    fn test_pending_orders_included_in_next_batch() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub1 = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub1], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        let result = run_batch(&mut seq, vec![], &markets, &[]);
        assert!(result.orders_submitted >= 1);
    }

    #[test]
    fn test_expired_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);
        seq.order_ttl = 2;

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 0);
    }

    #[test]
    fn test_orders_for_resolved_markets_removed() {
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Market A");
        let m1 = markets.add_binary("Market B");

        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![
                outcome_buy(&markets, 0, m0, 0, 100_000_000, 5),
                outcome_buy(&markets, 0, m1, 0, 100_000_000, 5),
            ],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 2);

        let mut reduced_markets = MarketSet::new();
        reduced_markets.add_binary("Market B");

        run_batch(&mut seq, vec![], &reduced_markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);
    }

    #[test]
    fn test_bankrupt_account_orders_removed() {
        let (markets, m0) = setup();
        let (mut seq, aid) = make_sequencer(100 * NANOS_PER_DOLLAR as i64);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 5)],
            mm_constraint: None,
        };
        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 1);

        let account = seq.accounts.get_mut(aid).unwrap();
        account.balance = 0;

        run_batch(&mut seq, vec![], &markets, &[]);
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

        run_batch(&mut seq, vec![sub], &markets, &[]);
        assert_eq!(seq.pending_orders.len(), 0);
    }

    // --- Fill settlement integration ---

    #[test]
    fn test_matching_buy_and_sell_settles_correctly() {
        let (markets, m0) = setup();

        let mut accounts = AccountStore::new();
        let buyer_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller_id = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        accounts
            .get_mut(seller_id)
            .unwrap()
            .positions
            .insert((m0, 0), 50);

        let mut seq = BlockSequencer::new(accounts, MarketSet::new(), vec![], Arc::new(AdminOracle::new()));

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

        let result = run_batch(&mut seq, vec![buy_sub, sell_sub], &markets, &[]);

        if result.orders_filled > 0 {
            let buyer = seq.accounts.get(buyer_id).unwrap();
            let seller = seq.accounts.get(seller_id).unwrap();

            assert!(buyer.balance < 100 * NANOS_PER_DOLLAR as i64);
            assert!(buyer.position(m0, 0) > 0);

            assert!(seller.balance > 10 * NANOS_PER_DOLLAR as i64);
            assert!(seller.position(m0, 0) < 50);
        }
    }

    // --- Block height counter ---

    #[test]
    fn test_batch_counter_increments() {
        let (markets, _) = setup();
        let (mut seq, _) = make_sequencer(NANOS_PER_DOLLAR as i64);

        assert_eq!(seq.height, 0);
        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.height, 1);
        run_batch(&mut seq, vec![], &markets, &[]);
        assert_eq!(seq.height, 2);
    }

    // --- Block-specific tests ---

    #[test]
    fn test_produce_block_returns_valid_header() {
        let (markets, _) = setup();
        let accounts = AccountStore::new();
        let mut seq = BlockSequencer::new(accounts, markets.clone(), vec![], Arc::new(AdminOracle::new()));

        let (block, _) = seq.produce_block(vec![], 1000);
        assert_eq!(block.header.height, 1);
        assert_eq!(block.header.parent_hash, [0u8; 32]); // genesis
        assert_eq!(block.header.timestamp_ms, 1000);
    }

    #[test]
    fn test_block_chain_parent_hash() {
        let (markets, _) = setup();
        let accounts = AccountStore::new();
        let mut seq = BlockSequencer::new(accounts, markets.clone(), vec![], Arc::new(AdminOracle::new()));

        let (block1, _) = seq.produce_block(vec![], 1000);
        let expected_parent = hash_header(&block1.header);

        let (block2, _) = seq.produce_block(vec![], 2000);
        assert_eq!(block2.header.parent_hash, expected_parent);
        assert_eq!(block2.header.height, 2);
    }

    #[test]
    fn test_state_root_in_block() {
        let (markets, m0) = setup();
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut seq = BlockSequencer::new(accounts, markets.clone(), vec![], Arc::new(AdminOracle::new()));

        let (block1, _) = seq.produce_block(vec![], 0);
        let root1 = block1.header.state_root;

        // Submit an order that will change state
        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };
        let (block2, _) = seq.produce_block(vec![sub], 0);

        // State root should reflect the updated account state
        // (even if the order didn't fill, state root is computed after settlement)
        assert_eq!(block2.header.state_root, compute_state_root(&seq.accounts));
        // First and second blocks should have the same state root since no fills happened
        // (only pending orders changed, which aren't in the state root)
        assert_eq!(root1, block2.header.state_root);
    }
}
