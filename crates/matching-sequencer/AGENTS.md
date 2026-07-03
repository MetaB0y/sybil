# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-sequencer** crate is an agent-based, multi-batch sequential simulation engine. It orchestrates block production, manages account state, validates orders, and settles fills. Provides both synchronous (simulation) and async actor-based (API) interfaces.

## Architecture Notes

Before modifying this crate, read these vault notes (`docs/architecture/`):
- [[Block Lifecycle]] — batch collection, solving, settlement, sealed block
- [[Mempool]] — order buffering, segregation, and drain limits
- [[Settlement]] — fill settlement logic (simple and generic)
- [[Fractional Quantities]] — `Qty` is fixed-point share-units
- [[Pending Orders and TTL]] — cross-batch order persistence and expiry
- [[State Root and Parent Hash]] — block chaining and state commitment

## Core Architecture

```
SimulationRunner / SequencerHandle (async)
        ↓
  SequencerActor (tokio task, 1-second timer)
        ↓
  BlockSequencer (core sync logic)
        ↓
  Validation → Solving → Settlement → State Commitment
```

## Key Types

| Type | Purpose |
|------|---------|
| `BlockSequencer` | Core state engine: accounts, markets, pending orders, block production |
| `SequencerActor` | Async wrapper with message-passing, mempool, SSE broadcasts |
| `Block` / `BlockHeader` | Immutable output with fills, prices, state root, parent hash |
| `Account` / `AccountStore` | Balance + positions per account |
| `OrderBook` | Resting orders with tracked balance/position reservations |
| `OrderSubmission` | Request to include orders in next batch |
| `SimulationRunner` | Multi-batch orchestration with agents |

## Block Production Flow

```rust
BlockSequencer::produce_block(submissions, timestamp_ms) → (Block, PipelineResult, BlockWitness)
```

1. `order_book.expire()` — remove stale resting orders
2. `order_book.revalidate()` — remove orders for resolved markets, bankrupt accounts
3. Collect resting orders from book
4. Accept new non-MM submissions via `order_book.accept()` (validates + reserves)
5. Accept MM submissions (flash liquidity, bypass book, STP check only)
6. Build `Problem`, `Pipeline::solve(&problem)`
7. `settle_fill()` for each fill, derive minting from position imbalance
8. `order_book.settle()` — release filled, adjust partial, keep unfilled
9. Compute state root, build Block

## Settlement Logic

**Simple (single binary market):**
- Buy: `balance -= price * qty / SHARE_SCALE; position(market, outcome) += qty`
- Sell: `balance += price * qty / SHARE_SCALE; position(market, outcome) -= qty`

**Generic (bundles, spreads):**
- Debit balance by cost
- Credit positions based on payoff-vector marginals
- Uses mixed-radix state indexing

**Market Resolution:**
- YES shares → `yes_payout_nanos`
- NO shares → `NANOS_PER_DOLLAR - yes_payout_nanos`
- Supports fractional resolution (e.g., 70%/30%)

## Agent Framework

| Agent | Behavior |
|-------|----------|
| `InformedTrader` | Knows true probs, trades on edge > threshold |
| `NoiseTrader` | Random order flow with configurable activity |
| `MarketMakerAgent` | Quotes both sides, respects budget constraint |

Agents implement: `submit_orders(view: &MarketView, account: &Account) → AgentSubmission`

## Order Book

- `OrderBook` is the single source of truth for committed capital
- Unfilled non-MM orders become resting orders with tracked reservations
- Balance reserved at acceptance time (buys), positions reserved (sells)
- Expire after TTL blocks (default `63_072_000`, effectively GTC at the normal cadence), reservations released
- Re-validated each block (market resolution, account solvency)
- MM orders bypass the book entirely (flash liquidity, one-shot)

## Deferred Bundle Buffer

- Single-market non-MM orders can admit directly into `OrderBook`
- MM, multi-market, and multi-order submissions are buffered as pending bundles
- Defaults from `SequencerConfig`: `max_pending_bundles = 10_000`, `max_pending_bundles_per_account = 100`, `max_orders_per_submission = 64`
- Rate limits: 50 submissions/account/second with burst 200; 1,000 global submissions/second with burst 3,000

## State Commitment

- **State root**: `blake3(canonical account encoding)`
- **Parent hash**: Links blocks into chain
- Enables ZK proof integration via `BlockWitness`

## Module Map

| Module | Purpose |
|--------|---------|
| `sequencer.rs` | BlockSequencer core |
| `actor.rs` | SequencerActor async wrapper |
| `simulation.rs` | SimulationRunner multi-batch |
| `settlement.rs` | Fill and market resolution |
| `account.rs` | Account, AccountStore |
| `block.rs` | Block, BlockHeader |
| `order_book.rs` | OrderBook: resting orders + reservations |
| `agent/*.rs` | Informed, Noise, MM agents |
| `validation.rs` | Order validation rules |

## Testing

```bash
cargo test -p matching-sequencer
```
