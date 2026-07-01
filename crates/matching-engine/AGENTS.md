# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-engine** crate is the foundational core of Sybil. It defines all fundamental types, data structures, and the problem representation. It contains **zero solver logic** — only the domain model that solvers use.

## Architecture Notes

Before modifying this crate, read these vault notes (`docs/architecture/`):
- [[Payoff Vectors]] — the central abstraction for all order types
- [[Binary Markets and Market Groups]] — market structure and mutually exclusive sets
- [[Nanos and Integer Arithmetic]] — fixed-point arithmetic invariants
- [[Order Types]] — user-facing order specs converted to payoff vectors
- [[Minting]] — how group minting creates/destroys complete sets

## Key Design Decisions

- **All markets are binary** (YES/NO). Multi-outcome events are modeled as groups of binary markets at the solver layer via `MarketGroup`.
- **Payoff vectors**: Orders are represented as payoff vectors over atomic world states, enabling unified handling of simple orders, bundles, spreads, and conditionals.
- **Fixed-point arithmetic**: Uses `Nanos` (u64, 1e-9 USD) instead of f64 to guarantee deterministic computation.
- **Max 5 markets, 32 states per order**: Pragmatic constraints keep arrays small for stack allocation.

## Core Types

| Type | Purpose |
|------|---------|
| `Nanos` (u64) | Monetary unit = 1 nanodollar. Max ~$18B with u64. |
| `Qty` (u64) | Fixed-point share-units (`SHARE_SCALE = 1000`) |
| `MarketId` | Market identifier with sentinel `MarketId::NONE` |
| `Order` | Unified payoff vector representation with limit price and quantity constraints |
| `Fill` | Result of matching: order_id, fill_qty, fill_price |
| `Problem` | Complete problem instance: markets, orders, MM constraints, market groups |
| `MmConstraint` | Market maker budget constraint across orders |
| `MarketGroup` | Mutually exclusive markets (exactly one resolves YES) |

## Order Representation

Orders use payoff vectors over 2^N atomic states (N = number of markets):
- `payoffs: [i8; MAX_STATES]` — positive = long, negative = short, zero = no exposure
- State indexing uses mixed-radix encoding: `index = o0 + 2*o1 + 4*o2 + ...`

Example: Spread "Buy A YES, Sell B YES" → payoffs `[0, -1, +1, 0]` over 4 states.

## Order Builders

`order_builder.rs` provides factory functions:
- `simple_yes_buy`, `simple_no_buy` — single market limit orders
- `spread`, `spread_sell` — two-market spread trades
- `bundle_yes`, `bundle_sell` — multi-market bundles
- `butterfly` — three-market volatility play
- `conditional_buy` — price-threshold activation

## MM Capital Calculation

`MmSide` determines capital usage at clearing prices:
- `BuyYes`/`SellNo`: capital = `price × qty / SHARE_SCALE`
- `SellYes`/`BuyNo`: capital = `(NANOS - price) × qty / SHARE_SCALE`

## Module Map

| Module | Contents |
|--------|----------|
| `types.rs` | Nanos, Qty, MarketId, Side, conversions |
| `order.rs` | Order, Fill, PriceCondition |
| `order_builder.rs` | OrderBuilder + convenience factories |
| `book.rs` | LiquidityBook, JointLiquidityBook, LiquidityPool |
| `state.rs` | State indexing, StateSpace |
| `mm_constraint.rs` | MmConstraint, MmSide, validation |
| `problem.rs` | Problem, MarketGroup, ProblemSummary |
