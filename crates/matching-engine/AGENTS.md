# matching-engine

Core data structures for the prediction market matching engine.

## Purpose

Defines the fundamental types used across the matching system:
- `Problem`: The input to the solver (markets, orders, liquidity, constraints)
- `Order`: Unified payoff-vector representation for all order types
- `Market`/`MarketSet`: Market definitions with outcome counts
- `LiquidityPool`/`LiquidityBook`: Market maker liquidity
- `Fill`: Order execution results
- `MmConstraint`: Market maker budget constraints

## Key Concepts

### Payoff Vector Orders
Every order is represented as a payoff vector over atomic world states:
- Simple limit: `[+1, 0]` (long YES on binary market)
- Bundle: `[+1, 0, 0, 0]` (long A YES AND B YES on 2 binary markets)

### Markets
All markets are conceptually binary (YES/NO for each outcome). A "multi-outcome market"
is a grouping where outcomes must sum to 100%.

### Liquidity
Market makers provide liquidity through `LiquidityBook` asks at various price levels.
The solver matches orders against this liquidity.

## Dependencies

None (leaf crate).

## Used By

- `matching-solver`: Uses types for solving
- `matching-scenarios`: Uses types for test generation
- `matching-sim`: Uses types for simulation
