//! Core JIT types.
//!
//! JIT (Just-In-Time) liquidity is the "informed FBA" - a second-stage auction
//! where providers see the base clearing price before committing capital.

use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, Qty, Side};

/// Unique identifier for a JIT provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId(pub u64);

impl ProviderId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Unique identifier for a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct BatchId(pub u64);

impl BatchId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Classification of JIT liquidity type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitType {
    /// Fills unfilled demand - no displacement, no tax.
    /// Pure value add: provides liquidity where none existed.
    Backrun,
    /// Replaces passive orders - taxed, rebates to displaced.
    /// JIT provider is taking fills that would have gone to passive LPs.
    Displacement,
}

/// A single JIT order submitted by a provider.
#[derive(Clone, Debug)]
pub struct JitOrder {
    pub market_id: MarketId,
    pub side: Side,
    pub price: Nanos,
    pub quantity: Qty,
}

impl JitOrder {
    pub fn new(market_id: MarketId, side: Side, price: Nanos, quantity: Qty) -> Self {
        Self {
            market_id,
            side,
            price,
            quantity,
        }
    }

    /// Create a sell order (providing liquidity to buyers).
    pub fn sell(market_id: MarketId, price: Nanos, quantity: Qty) -> Self {
        Self::new(market_id, Side::Ask, price, quantity)
    }

    /// Create a buy order (providing liquidity to sellers).
    pub fn buy(market_id: MarketId, price: Nanos, quantity: Qty) -> Self {
        Self::new(market_id, Side::Bid, price, quantity)
    }

    /// Notional value of this order (price * quantity).
    pub fn notional(&self) -> u128 {
        self.price as u128 * self.quantity as u128
    }
}

/// A JIT provider's submission for a batch.
#[derive(Clone, Debug)]
pub struct JitSubmission {
    pub batch_id: BatchId,
    pub provider_id: ProviderId,
    pub orders: Vec<JitOrder>,
}

impl JitSubmission {
    pub fn new(batch_id: BatchId, provider_id: ProviderId) -> Self {
        Self {
            batch_id,
            provider_id,
            orders: Vec::new(),
        }
    }

    pub fn with_orders(batch_id: BatchId, provider_id: ProviderId, orders: Vec<JitOrder>) -> Self {
        Self {
            batch_id,
            provider_id,
            orders,
        }
    }

    pub fn add_order(&mut self, order: JitOrder) {
        self.orders.push(order);
    }

    /// Total number of orders in this submission.
    pub fn num_orders(&self) -> usize {
        self.orders.len()
    }

    /// Total notional value across all orders.
    pub fn total_notional(&self) -> u128 {
        self.orders.iter().map(|o| o.notional()).sum()
    }
}

/// A validated JIT order with classification and metrics.
#[derive(Clone, Debug)]
pub struct ValidatedJitOrder {
    pub order: JitOrder,
    pub jit_type: JitType,
    /// Volume displaced from passive LPs (0 for backrun).
    pub displaced_volume: Qty,
    /// Welfare improvement from this order.
    pub welfare_improvement: i64,
}

/// Result of validating a JIT submission.
#[derive(Clone, Debug)]
pub struct ValidatedJit {
    pub provider_id: ProviderId,
    pub backrun_orders: Vec<ValidatedJitOrder>,
    pub displacement_orders: Vec<ValidatedJitOrder>,
}

impl ValidatedJit {
    pub fn new(provider_id: ProviderId) -> Self {
        Self {
            provider_id,
            backrun_orders: Vec::new(),
            displacement_orders: Vec::new(),
        }
    }

    /// Total welfare improvement from all orders.
    pub fn total_welfare_improvement(&self) -> i64 {
        self.backrun_orders
            .iter()
            .chain(self.displacement_orders.iter())
            .map(|o| o.welfare_improvement)
            .sum()
    }

    /// Total displaced volume.
    pub fn total_displaced_volume(&self) -> Qty {
        self.displacement_orders
            .iter()
            .map(|o| o.displaced_volume)
            .sum()
    }

    /// All orders (backrun + displacement).
    pub fn all_orders(&self) -> impl Iterator<Item = &ValidatedJitOrder> {
        self.backrun_orders
            .iter()
            .chain(self.displacement_orders.iter())
    }
}

/// Reason for rejecting a JIT submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JitRejection {
    /// Batch ID doesn't match current batch.
    InvalidBatchId,
    /// Market doesn't exist or isn't valid for JIT.
    InvalidMarket(MarketId),
    /// Price is invalid (zero, exceeds bounds, etc.).
    InvalidPrice { market: MarketId, reason: String },
    /// Order doesn't improve welfare.
    NoWelfareImprovement,
    /// Order exceeds available demand/supply.
    ExceedsAvailableVolume,
    /// Provider not authorized (for future seat auction).
    UnauthorizedProvider,
}

/// Summary of unfilled demand in a market.
#[derive(Clone, Debug, Default)]
pub struct UnfilledDemand {
    /// Unfilled buy demand (wants to buy but no sellers).
    pub buy_qty: Qty,
    /// Best price buyers are willing to pay.
    pub buy_price: Nanos,
    /// Unfilled sell demand (wants to sell but no buyers).
    pub sell_qty: Qty,
    /// Best price sellers are willing to accept.
    pub sell_price: Nanos,
}

impl UnfilledDemand {
    pub fn has_buy_demand(&self) -> bool {
        self.buy_qty > 0
    }

    pub fn has_sell_demand(&self) -> bool {
        self.sell_qty > 0
    }

    pub fn is_empty(&self) -> bool {
        self.buy_qty == 0 && self.sell_qty == 0
    }
}

/// Result of the JIT phase.
#[derive(Clone, Debug)]
pub struct JitPhaseResult {
    /// All validated JIT orders that will be included.
    pub jit_fills: Vec<JitFill>,
    /// Total welfare improvement from JIT.
    pub welfare_improvement: i64,
    /// Total tax collected.
    pub total_tax: Nanos,
    /// Rebates to distribute to displaced LPs.
    pub rebates: Vec<Rebate>,
    /// Statistics about the JIT phase.
    pub stats: JitStats,
}

impl JitPhaseResult {
    pub fn empty() -> Self {
        Self {
            jit_fills: Vec::new(),
            welfare_improvement: 0,
            total_tax: 0,
            rebates: Vec::new(),
            stats: JitStats::default(),
        }
    }
}

/// A fill from JIT liquidity.
#[derive(Clone, Debug)]
pub struct JitFill {
    pub provider_id: ProviderId,
    pub market_id: MarketId,
    pub side: Side,
    pub fill_qty: Qty,
    pub fill_price: Nanos,
    pub jit_type: JitType,
    pub tax_paid: Nanos,
}

/// A rebate to a displaced passive LP.
#[derive(Clone, Debug)]
pub struct Rebate {
    /// Original order ID that was displaced.
    pub displaced_order_id: u64,
    /// Amount to rebate.
    pub amount: Nanos,
    /// Record of what was displaced.
    pub record: DisplacementRecord,
}

/// Record of a displacement event.
#[derive(Clone, Debug)]
pub struct DisplacementRecord {
    /// How much the order would have filled without JIT.
    pub original_fill: Qty,
    /// How much the order fills with JIT.
    pub new_fill: Qty,
    /// Welfare loss to the displaced user.
    pub welfare_loss: i64,
}

/// Statistics from the JIT phase.
#[derive(Clone, Debug, Default)]
pub struct JitStats {
    /// Number of providers that submitted.
    pub providers_submitted: usize,
    /// Total orders submitted.
    pub orders_submitted: usize,
    /// Orders accepted as backrun.
    pub backrun_orders_accepted: usize,
    /// Orders accepted as displacement.
    pub displacement_orders_accepted: usize,
    /// Orders rejected.
    pub orders_rejected: usize,
    /// Total backrun volume.
    pub backrun_volume: Qty,
    /// Total displacement volume.
    pub displacement_volume: Qty,
    /// Rejection reasons (for debugging).
    pub rejection_reasons: HashMap<String, usize>,
}

impl JitStats {
    pub fn total_accepted(&self) -> usize {
        self.backrun_orders_accepted + self.displacement_orders_accepted
    }

    pub fn total_volume(&self) -> Qty {
        self.backrun_volume + self.displacement_volume
    }

    pub fn record_rejection(&mut self, reason: &JitRejection) {
        let key = format!("{:?}", reason);
        *self.rejection_reasons.entry(key).or_insert(0) += 1;
        self.orders_rejected += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_order_creation() {
        let order = JitOrder::sell(MarketId::new(0), 500_000_000, 100);
        assert_eq!(order.side, Side::Ask);
        assert_eq!(order.quantity, 100);
        assert_eq!(order.notional(), 50_000_000_000);
    }

    #[test]
    fn test_jit_submission() {
        let mut submission = JitSubmission::new(BatchId::new(1), ProviderId::new(1));
        submission.add_order(JitOrder::sell(MarketId::new(0), 500_000_000, 100));
        submission.add_order(JitOrder::buy(MarketId::new(0), 400_000_000, 50));

        assert_eq!(submission.num_orders(), 2);
    }

    #[test]
    fn test_validated_jit() {
        let mut validated = ValidatedJit::new(ProviderId::new(1));
        validated.backrun_orders.push(ValidatedJitOrder {
            order: JitOrder::sell(MarketId::new(0), 500_000_000, 100),
            jit_type: JitType::Backrun,
            displaced_volume: 0,
            welfare_improvement: 1000,
        });
        validated.displacement_orders.push(ValidatedJitOrder {
            order: JitOrder::sell(MarketId::new(0), 500_000_000, 50),
            jit_type: JitType::Displacement,
            displaced_volume: 50,
            welfare_improvement: 500,
        });

        assert_eq!(validated.total_welfare_improvement(), 1500);
        assert_eq!(validated.total_displaced_volume(), 50);
    }
}
