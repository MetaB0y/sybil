//! Just-In-Time (JIT) Liquidity Module.
//!
//! JIT liquidity is the "informed FBA" - a second-stage auction where
//! providers see the base clearing price before committing capital.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                           BATCH LIFECYCLE                                │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  1. Orders Accumulate (Uninformed FBA - free)                           │
//! │           ↓                                                             │
//! │  2. Batch Seals                                                         │
//! │           ↓                                                             │
//! │  3. Base Solution Computed (matching solvers)                           │
//! │           ↓                                                             │
//! │  4. JitInput Published (anonymized orderbook + base solution)           │
//! │           ↓                                                             │
//! │  5. JIT Window Opens (Informed FBA - taxed)                             │
//! │           ↓                                                             │
//! │  6. JIT Submissions Validated (JitValidator - trust boundary)           │
//! │           ↓                                                             │
//! │  7. Orders Selected (price-priority matching)                           │
//! │           ↓                                                             │
//! │  8. Tax/Rebates Calculated                                              │
//! │           ↓                                                             │
//! │  9. Final Solution (UCP preserved)                                      │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Concepts
//!
//! - **Backrun**: Fills unfilled demand - no displacement, no tax.
//!   Pure value add: provides liquidity where none existed.
//!
//! - **Displacement**: Replaces passive orders - taxed, rebates to displaced.
//!   JIT provider is taking fills that would have gone to passive LPs.
//!
//! - **UCP Preserved**: All participants (passive + JIT) get the same clearing
//!   price. The tax compensates for information asymmetry, not price differences.
//!
//! # Module Structure
//!
//! - `types`: Core types (JitType, JitOrder, JitSubmission, etc.)
//! - `input`: JitInput and AnonymizedOrderbook (what providers see)
//! - `provider`: JitProvider trait (pluggable providers)
//! - `validator`: JitValidator trait (trust boundary)
//! - `tax`: Pluggable tax calculators
//! - `simple`: Bootstrap SimpleJitProvider implementation
//! - `coordinator`: JitCoordinator orchestrates the flow
//!
//! # Example Usage
//!
//! ```ignore
//! use matching_solver::jit::{JitCoordinator, JitConfig, SimpleJitProvider, ProviderId};
//!
//! // Create coordinator with a simple provider
//! let mut coordinator = JitCoordinator::new()
//!     .with_provider(Box::new(SimpleJitProvider::new(ProviderId::new(1))));
//!
//! // Run JIT phase after base matching
//! let jit_result = coordinator.run_jit_phase(batch_id, &problem, &base_result);
//!
//! // JIT fills can be integrated into the final solution
//! println!("JIT welfare improvement: {}", jit_result.welfare_improvement);
//! println!("Tax collected: {}", jit_result.total_tax);
//! ```

pub mod coordinator;
pub mod input;
pub mod provider;
pub mod simple;
pub mod tax;
pub mod types;
pub mod validator;

#[cfg(test)]
mod tests;

// Re-exports for convenience
pub use coordinator::{JitConfig, JitCoordinator};
pub use input::{AnonymizedOrderbook, BaseSolutionSummary, JitInput, MarketDepth, MarketInfo};
pub use provider::JitProvider;
pub use simple::{AggressiveJitProvider, SimpleJitProvider};
pub use tax::{
    BatchTaxSummary, DynamicTaxCalculator, FlatRateTaxCalculator, JitTaxCalculator, TaxResult,
    WelfareTaxCalculator, ZeroTaxCalculator,
};
pub use types::{
    BatchId, DisplacementRecord, JitFill, JitOrder, JitPhaseResult, JitRejection, JitStats,
    JitSubmission, JitType, ProviderId, Rebate, UnfilledDemand, ValidatedJit, ValidatedJitOrder,
};
pub use validator::{DefaultValidator, JitValidator};
