# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-sequencer** crate is an agent-based, multi-batch sequential simulation engine. It orchestrates block production, manages account state, validates orders, and settles fills. Provides both synchronous (simulation) and async actor-based (API) interfaces.

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
| `OrderSubmission` | Request to include orders in next batch |
| `SimulationRunner` | Multi-batch orchestration with agents |

## Block Production Flow

```rust
BlockSequencer::produce_block(submissions, timestamp_ms) → (Block, PipelineResult, BlockWitness)
```

1. Re-validate pending orders (TTL check)
2. Validate new submissions (balance reservation)
3. Merge into `Problem`
4. `Pipeline::solve(&problem)`
5. `settle_fill()` for each fill
6. Persist unfilled non-MM orders
7. Compute state root, build Block

## Settlement Logic

**Simple (single binary market):**
- Buy: `balance -= price * qty; position(market, outcome) += qty`
- Sell: `balance += price * qty; position(market, outcome) -= qty`

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

## Pending Orders

- Unfilled non-MM orders persist across batches
- Expire after `order_ttl` batches (default 3)
- Re-validated on reinclusion

## Mempool

- Segregates single-market vs multi-market/MM orders
- Drain limits: per-market (100), bundles (50), total (10k)
- FIFO within pools

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
| `mempool.rs` | Order buffering |
| `agent/*.rs` | Informed, Noise, MM agents |
| `validation.rs` | Order validation rules |

## Testing

```bash
cargo test -p matching-sequencer
```
