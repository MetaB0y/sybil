//! JIT Provider trait.
//!
//! Interface for JIT liquidity providers. This is the boundary where
//! external providers plug in.
//!
//! Bootstrap: SimpleJitProvider (internal, greedy).
//! Future: WasmJitProvider (external, sandboxed in TEE).

use super::input::JitInput;
use super::types::JitSubmission;

/// Interface for JIT liquidity providers.
///
/// Providers receive [`JitInput`] (anonymized orderbook + base solution)
/// and submit [`JitSubmission`] (their JIT orders).
///
/// # Design Principles
///
/// 1. **Information boundary**: Provider sees ONLY JitInput, nothing else.
/// 2. **Stateless**: Each call to `provide` is independent.
/// 3. **Pluggable**: Internal and external providers use same interface.
///
/// # Example
///
/// ```ignore
/// struct MyProvider { id: ProviderId }
///
/// impl JitProvider for MyProvider {
///     fn provide(&self, input: &JitInput) -> JitSubmission {
///         let mut submission = JitSubmission::new(input.batch_id, self.id);
///
///         // Look for opportunities
///         for (market_id, unfilled) in &input.base_solution.unfilled_demand {
///             if unfilled.buy_qty > 0 {
///                 // Provide sell liquidity
///                 submission.add_order(JitOrder::sell(
///                     *market_id,
///                     input.clearing_price(*market_id).unwrap_or(0),
///                     unfilled.buy_qty,
///                 ));
///             }
///         }
///
///         submission
///     }
/// }
/// ```
pub trait JitProvider: Send + Sync {
    /// Generate JIT orders given the published input.
    ///
    /// Provider sees ONLY JitInput, nothing else. This is the information
    /// boundary between the system and external providers.
    fn provide(&self, input: &JitInput) -> JitSubmission;

    /// Provider name (for logging/debugging).
    fn name(&self) -> &str {
        "JitProvider"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::types::{BatchId, ProviderId};

    struct DummyProvider {
        id: ProviderId,
    }

    impl JitProvider for DummyProvider {
        fn provide(&self, input: &JitInput) -> JitSubmission {
            JitSubmission::new(input.batch_id, self.id)
        }

        fn name(&self) -> &str {
            "DummyProvider"
        }
    }

    #[test]
    fn test_provider_trait() {
        let provider = DummyProvider {
            id: ProviderId::new(1),
        };

        // Create a minimal JitInput for testing
        let input = JitInput {
            batch_id: BatchId::new(1),
            orderbook: Default::default(),
            base_solution: Default::default(),
            markets: vec![],
        };

        let submission = provider.provide(&input);
        assert_eq!(submission.batch_id, BatchId::new(1));
        assert_eq!(submission.provider_id, ProviderId::new(1));
        assert!(submission.orders.is_empty());
    }
}
