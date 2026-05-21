# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **sybil-verifier** crate provides comprehensive block verification for ZK proof integration. It validates every aspect of a block produced by the sequencer across 4 independent layers.

## Architecture Notes

Before modifying this crate, read these vault notes (`docs/architecture/`):
- [[Four-Layer Verification]] — the 4-layer verification model
- [[Block Witness]] — witness structure for ZK proof generation
- [[ZK Integration Path]] — roadmap from verifier to SNARK circuits

## Verification Layers

### Layer 1: Match Verification (`match_verifier.rs`)
- **Per-fill checks**: order existence, quantity constraints, price limits, no duplicates
- **UCP**: single-market fills match clearing price
- **Price complementarity**: YES + NO = $1 for binary markets
- **Market group constraints**: grouped markets' YES prices sum ≤ $1
- **MM budget constraints**: fills don't exceed allocated capital
- **Welfare consistency**: computed total matches reported

### Layer 2: Settlement Verification (`settlement.rs`)
- Re-derives post-state from `pre_state + fills`
- Derives protocol MINT account adjustments from post-fill position imbalance
- Verifies balances and positions after settlement
- Uses i128 intermediates to avoid overflow

### Layer 3: Block Integrity (`block.rs`)
- **State root**: BLAKE3 hash of post-state
- **Parent hash chaining**: `header.parent_hash == hash(previous_header)`
- **Height verification**: consecutive block heights
- **Count verification**: order_count, fill_count match witness

### Layer 4: Order Validation (`orders.rs`)
- Pre-state balance checks (buy orders)
- Pre-state position checks (sell orders)
- Intra-batch double-spend detection
- Rejection validation (false rejections, incorrect reasons)

## Key Types

```rust
struct BlockWitness {
    header: WitnessBlockHeader,
    previous_header: Option<WitnessBlockHeader>,
    orders: Vec<WitnessOrder>,
    rejections: Vec<WitnessRejection>,
    system_events: Vec<SystemEventWitness>,
    fills: Vec<Fill>,
    clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    total_welfare: i64,
    minting_cost: i64,
    mm_constraints: Vec<MmConstraint>,
    market_groups: Vec<MarketGroup>,
    pre_state: Vec<AccountSnapshot>,
    post_system_state: Vec<AccountSnapshot>,
    post_state: Vec<AccountSnapshot>,
    state_sidecar: StateSidecarSnapshot,
    resolved_markets: Vec<MarketId>,
}

struct VerificationResult {
    valid: bool,
    violations: Vec<Violation>,
    stats: VerificationStats,
}
```

## Usage

```rust
use sybil_verifier::{verify_match, BlockWitness};

let result = verify_match(&witness);
if !result.valid {
    for violation in &result.violations {
        eprintln!("{:?}", violation.kind);
    }
}
```

## Strict vs Lenient Mode

| Mode | Zero Fills | Welfare Tolerance |
|------|-----------|-------------------|
| Lenient (default) | Allowed | 1000 nanos |
| Strict (ZK) | Forbidden | 0 |

## Violation Types (37 total)

**Layer 1**: OrderNotFound, QuantityExceedsMax, PriceExceedsLimit, DuplicateFill, WelfareMismatch, MmBudgetExceeded, UniformClearingPriceViolation, PriceComplementarityViolation, MarketGroupConstraintViolation, ...

**Layer 2**: SettlementBalanceMismatch, SettlementPositionMismatch, SettlementOverflow

**Layer 3**: StateRootMismatch, ParentHashMismatch, HeightNotConsecutive, OrderCountMismatch

**Layer 4**: InsufficientBalance, InsufficientPosition, FalseRejection, IncorrectRejectionReason

## vs matching-solver/verifier

| Aspect | sybil-verifier | matching-solver/verifier |
|--------|----------------|--------------------------|
| Scope | Complete block (4 layers) | Match result only |
| Input | BlockWitness | Problem + MatchingResult |
| Purpose | ZK circuit integration | Solver output correctness |

## Module Map

| Module | Purpose |
|--------|---------|
| `match_verifier.rs` | Layer 1: fill/market invariants |
| `settlement.rs` | Layer 2: state transition |
| `block.rs` | Layer 3: header integrity |
| `orders.rs` | Layer 4: pre-state/rejections |
| `types.rs` | BlockWitness, AccountSnapshot |
| `violations.rs` | ViolationKind enum |
| `arithmetic.rs` | Overflow-safe computations |
