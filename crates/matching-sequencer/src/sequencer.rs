use std::collections::HashMap;

use matching_engine::{
    Fill, LiquidityPool, MarketGroup, MarketId, MarketSet, MmConstraint, Nanos, Order, Problem,
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

/// Manages accounts, assigns order IDs, validates, solves, and settles batches.
pub struct BatchSequencer {
    pub accounts: AccountStore,
    order_account_map: HashMap<u64, AccountId>,
    next_order_id: u64,
}

impl BatchSequencer {
    pub fn new(accounts: AccountStore) -> Self {
        Self {
            accounts,
            order_account_map: HashMap::new(),
            next_order_id: 1,
        }
    }

    /// Run a single batch: validate → solve → settle.
    pub fn run_batch(
        &mut self,
        submissions: Vec<OrderSubmission>,
        markets: &MarketSet,
        market_groups: &[MarketGroup],
    ) -> BatchResult {
        let mut all_orders: Vec<Order> = Vec::new();
        let mut all_mm_constraints: Vec<MmConstraint> = Vec::new();
        let mut rejections: Vec<Rejection> = Vec::new();

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
                    accepted_orders.push(order);
                } else {
                    // Validate: check balance for buys, position for sells
                    match validate_order(&order, account) {
                        Ok(()) => {
                            self.order_account_map.insert(order_id, account_id);
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
        // The solver handles buyer-seller matching internally.
        let mut problem = Problem::new("batch");
        problem.markets = markets.clone();
        problem.liquidity = LiquidityPool::new();
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

/// Validate an order against account state.
fn validate_order(
    order: &Order,
    account: &crate::account::Account,
) -> Result<(), RejectionReason> {
    let num_states = order.num_states as usize;

    // Check if this is a buy (positive payoffs somewhere) or sell (negative payoffs)
    let has_positive = order.payoffs[..num_states].iter().any(|&p| p > 0);
    let has_negative = order.payoffs[..num_states].iter().any(|&p| p < 0);

    if has_positive && !has_negative {
        // Pure buy: check balance covers worst-case cost
        let max_cost = order.limit_price as i64 * order.max_fill as i64;
        if max_cost > account.balance {
            return Err(RejectionReason::InsufficientBalance {
                required: max_cost,
                available: account.balance,
            });
        }
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

    Ok(())
}
