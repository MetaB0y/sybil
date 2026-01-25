# MM Allocation Iteration Issue

## Problem Statement

There's a circular dependency between price discovery and MM allocation:

1. **PriceDiscovery** needs to know which orders will trade to compute clearing prices
2. **MmAllocation** needs clearing prices to decide which MM orders fit within budget
3. But MmAllocation filters out some MM orders due to budget constraints
4. So the prices computed with ALL MM orders are wrong for the actual set that trades

## Current Behavior

MM orders are included in PriceDiscovery:
- Prices are computed assuming ALL MM orders trade
- MmAllocation then filters some out due to budget
- Result: prices were computed for a different set of orders than actually trade

This is an approximation that works reasonably well when MM budget utilization is high (most MM orders trade anyway).

## Correct Solution: Iterative MM Allocation

The pipeline already has iteration, but it doesn't properly handle the MM price feedback:

```
for each iteration:
    1. PriceDiscovery(all remaining orders including MM) → prices
    2. MmAllocation(prices) → which MM orders are activated
    3. Merge fills for activated orders only
    4. Next iteration works on remaining orders
```

The issue: within a single iteration, prices assume all MM orders trade, but allocation filters some.

### Proposed Fix

Option A: Re-run price discovery after allocation
```
for each iteration:
    1. PriceDiscovery(non-MM orders only) → base prices
    2. MmAllocation(base prices) → activated MM orders
    3. PriceDiscovery(non-MM + activated MM) → final prices
    4. Generate fills at final prices
```

Option B: Fixed-point within iteration
```
for each iteration:
    activated_mm = all MM orders
    repeat until converged:
        1. PriceDiscovery(non-MM + activated_mm) → prices
        2. MmAllocation(prices) → new_activated_mm
        3. if new_activated_mm == activated_mm: break
        activated_mm = new_activated_mm
    4. Generate fills
```

## Why This Matters

When MM budget is tight relative to order volume:
- Many MM orders get filtered by allocation
- Prices computed with all MM orders are significantly different from prices with filtered set
- This can lead to suboptimal welfare

When MM budget is generous:
- Most MM orders are activated anyway
- The approximation is good enough
- Current approach works fine

## Related: Negrisk Arbitrage

The negrisk arbitrage phase helps here:
- If prices are inconsistent across related markets, arbitrage fills are created
- This helps stabilize prices even with MM filtering

## Status

Negrisk arbitrage is now implemented. This issue may be revisited if needed.
