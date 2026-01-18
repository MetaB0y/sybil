//! Simple JIT Provider - bootstrap implementation.
//!
//! A simple provider that fills obvious imbalances (backrun opportunities).
//! This is just an EXAMPLE - external providers will be more sophisticated.

use matching_engine::Side;

use super::input::JitInput;
use super::provider::JitProvider;
use super::types::{JitOrder, JitSubmission, ProviderId};

/// Simple JIT provider that fills obvious imbalances.
///
/// This is a bootstrap implementation demonstrating the JitProvider interface.
/// It identifies unfilled demand and provides liquidity at the clearing price.
///
/// External providers (future WASM-based) will be more sophisticated,
/// potentially using ML models, market analysis, etc.
pub struct SimpleJitProvider {
    provider_id: ProviderId,
    /// Price improvement offered (in nanos). Lower = more aggressive.
    price_improvement: u64,
    /// Maximum volume to provide per market.
    max_volume_per_market: u64,
}

impl SimpleJitProvider {
    pub fn new(provider_id: ProviderId) -> Self {
        Self {
            provider_id,
            price_improvement: 1_000_000, // 0.001 price improvement
            max_volume_per_market: u64::MAX,
        }
    }

    /// Create with custom parameters.
    pub fn with_params(
        provider_id: ProviderId,
        price_improvement: u64,
        max_volume_per_market: u64,
    ) -> Self {
        Self {
            provider_id,
            price_improvement,
            max_volume_per_market,
        }
    }

    /// Set price improvement.
    pub fn with_price_improvement(mut self, improvement: u64) -> Self {
        self.price_improvement = improvement;
        self
    }

    /// Set maximum volume per market.
    pub fn with_max_volume(mut self, max_volume: u64) -> Self {
        self.max_volume_per_market = max_volume;
        self
    }
}

impl JitProvider for SimpleJitProvider {
    fn provide(&self, input: &JitInput) -> JitSubmission {
        let mut submission = JitSubmission::new(input.batch_id, self.provider_id);

        // Look for unfilled demand (backrun opportunities)
        for (market_id, unfilled) in &input.base_solution.unfilled_demand {
            // Get clearing price for this market
            let clearing_price = input
                .clearing_price(*market_id)
                .unwrap_or(unfilled.buy_price.max(unfilled.sell_price));

            // Fill unfilled buy demand by providing sell liquidity
            if unfilled.buy_qty > 0 {
                let qty = unfilled.buy_qty.min(self.max_volume_per_market);
                // Offer slightly better than clearing (lower price for sells)
                let price = clearing_price.saturating_sub(self.price_improvement);

                if price > 0 && qty > 0 {
                    submission.add_order(JitOrder::new(*market_id, Side::Ask, price, qty));
                }
            }

            // Fill unfilled sell demand by providing buy liquidity
            if unfilled.sell_qty > 0 {
                let qty = unfilled.sell_qty.min(self.max_volume_per_market);
                // Offer slightly better than clearing (higher price for buys)
                let price = clearing_price.saturating_add(self.price_improvement);

                if qty > 0 {
                    submission.add_order(JitOrder::new(*market_id, Side::Bid, price, qty));
                }
            }
        }

        submission
    }

    fn name(&self) -> &str {
        "SimpleJitProvider"
    }
}

/// Aggressive JIT provider that also does displacement.
///
/// This provider not only fills unfilled demand but also competes
/// with passive LPs by offering better prices.
pub struct AggressiveJitProvider {
    provider_id: ProviderId,
    /// Price improvement for backrun.
    backrun_improvement: u64,
    /// Price improvement for displacement (must be better to justify).
    displacement_improvement: u64,
    /// Fraction of total volume willing to displace.
    displacement_fraction: f64,
}

impl AggressiveJitProvider {
    pub fn new(provider_id: ProviderId) -> Self {
        Self {
            provider_id,
            backrun_improvement: 1_000_000,    // 0.001
            displacement_improvement: 5_000_000, // 0.005 (must be better to pay tax)
            displacement_fraction: 0.20,        // Willing to displace up to 20%
        }
    }

    pub fn with_params(
        provider_id: ProviderId,
        backrun_improvement: u64,
        displacement_improvement: u64,
        displacement_fraction: f64,
    ) -> Self {
        Self {
            provider_id,
            backrun_improvement,
            displacement_improvement,
            displacement_fraction,
        }
    }
}

impl JitProvider for AggressiveJitProvider {
    fn provide(&self, input: &JitInput) -> JitSubmission {
        let mut submission = JitSubmission::new(input.batch_id, self.provider_id);

        for (market_id, unfilled) in &input.base_solution.unfilled_demand {
            let clearing_price = input
                .clearing_price(*market_id)
                .unwrap_or(unfilled.buy_price.max(unfilled.sell_price));

            // Backrun: fill unfilled demand
            if unfilled.buy_qty > 0 {
                let price = clearing_price.saturating_sub(self.backrun_improvement);
                if price > 0 {
                    submission.add_order(JitOrder::new(*market_id, Side::Ask, price, unfilled.buy_qty));
                }
            }

            if unfilled.sell_qty > 0 {
                let price = clearing_price.saturating_add(self.backrun_improvement);
                submission.add_order(JitOrder::new(*market_id, Side::Bid, price, unfilled.sell_qty));
            }
        }

        // Displacement: compete with passive LPs
        // Look at orderbook depth and offer better prices
        for (market_id, depth) in &input.orderbook.markets {
            let clearing_price = input.clearing_price(*market_id).unwrap_or(0);
            if clearing_price == 0 {
                continue;
            }

            // Calculate how much we're willing to displace
            let total_ask_volume = depth.total_ask_qty();
            let displacement_volume = (total_ask_volume as f64 * self.displacement_fraction) as u64;

            if displacement_volume > 0 {
                // Offer better price than current asks
                let best_ask = depth.best_ask().unwrap_or(clearing_price);
                let our_price = best_ask.saturating_sub(self.displacement_improvement);

                if our_price > 0 {
                    submission.add_order(JitOrder::new(
                        *market_id,
                        Side::Ask,
                        our_price,
                        displacement_volume,
                    ));
                }
            }

            // Similar for bids
            let total_bid_volume = depth.total_bid_qty();
            let displacement_volume = (total_bid_volume as f64 * self.displacement_fraction) as u64;

            if displacement_volume > 0 {
                let best_bid = depth.best_bid().unwrap_or(clearing_price);
                let our_price = best_bid.saturating_add(self.displacement_improvement);

                submission.add_order(JitOrder::new(
                    *market_id,
                    Side::Bid,
                    our_price,
                    displacement_volume,
                ));
            }
        }

        submission
    }

    fn name(&self) -> &str {
        "AggressiveJitProvider"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::input::{AnonymizedOrderbook, BaseSolutionSummary, MarketInfo};
    use crate::jit::types::{BatchId, UnfilledDemand};
    use matching_engine::MarketId;
    use std::collections::HashMap;

    fn create_test_input() -> JitInput {
        let mut unfilled_demand = HashMap::new();
        unfilled_demand.insert(
            MarketId::new(0),
            UnfilledDemand {
                buy_qty: 100,
                buy_price: 500_000_000,
                sell_qty: 50,
                sell_price: 480_000_000,
            },
        );

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(MarketId::new(0), 500_000_000);

        JitInput {
            batch_id: BatchId::new(1),
            orderbook: AnonymizedOrderbook::default(),
            base_solution: BaseSolutionSummary {
                clearing_prices,
                total_welfare: 1000,
                fill_rate: 0.8,
                unfilled_demand,
                total_volume_filled: 500,
                orders_filled: 10,
            },
            markets: vec![MarketInfo {
                id: MarketId::new(0),
                name: "Test Market".to_string(),
                num_outcomes: 2,
            }],
        }
    }

    #[test]
    fn test_simple_provider_fills_unfilled_demand() {
        let provider = SimpleJitProvider::new(ProviderId::new(1));
        let input = create_test_input();

        let submission = provider.provide(&input);

        // Should have orders for both unfilled buy and sell demand
        assert_eq!(submission.num_orders(), 2);

        // Check sell order (fills buy demand)
        let sell_order = submission.orders.iter().find(|o| o.side == Side::Ask).unwrap();
        assert_eq!(sell_order.quantity, 100);
        assert!(sell_order.price < 500_000_000); // Better than clearing

        // Check buy order (fills sell demand)
        let buy_order = submission.orders.iter().find(|o| o.side == Side::Bid).unwrap();
        assert_eq!(buy_order.quantity, 50);
        assert!(buy_order.price > 500_000_000); // Better than clearing
    }

    #[test]
    fn test_simple_provider_respects_max_volume() {
        let provider = SimpleJitProvider::new(ProviderId::new(1)).with_max_volume(30);
        let input = create_test_input();

        let submission = provider.provide(&input);

        // All orders should be capped at max volume
        for order in &submission.orders {
            assert!(order.quantity <= 30);
        }
    }

    #[test]
    fn test_simple_provider_empty_unfilled() {
        let provider = SimpleJitProvider::new(ProviderId::new(1));

        let input = JitInput {
            batch_id: BatchId::new(1),
            orderbook: AnonymizedOrderbook::default(),
            base_solution: BaseSolutionSummary::default(),
            markets: vec![],
        };

        let submission = provider.provide(&input);
        assert!(submission.orders.is_empty());
    }

    #[test]
    fn test_aggressive_provider() {
        let provider = AggressiveJitProvider::new(ProviderId::new(2));
        let input = create_test_input();

        let submission = provider.provide(&input);

        // Should have at least backrun orders
        assert!(!submission.orders.is_empty());
    }
}
