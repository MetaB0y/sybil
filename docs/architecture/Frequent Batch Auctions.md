---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-03-15
---

A Frequent Batch Auction (FBA) collects all incoming orders over a short time window — typically one second — and then matches them simultaneously at uniform clearing prices. Every participant in the batch gets the same deal. This stands in contrast to continuous limit order books, where the first order to arrive gets priority, creating an arms race for speed that benefits high-frequency traders at the expense of everyone else.

The intellectual foundation comes from Budish, Cramton, and Shim (2015), who proposed FBAs for equity markets as a solution to the HFT arms race. In prediction markets, FBAs are even more natural: the information environment changes discretely (news events, data releases), so there's no fundamental reason to match continuously. Batching also makes the clearing problem well-defined — you have a fixed set of orders and can solve for the welfare-maximizing allocation.

Sybil runs 1-second batches. Orders accumulate in a [[Mempool]], then at each tick the sequencer drains the mempool, merges in any [[Pending Orders and TTL|pending orders]] from prior batches, assembles a [[The LP Core|Problem]], runs a solver, settles fills, and seals a block. The entire [[Block Lifecycle]] completes within the batch interval. Because all orders in a batch see the same clearing price, there is no advantage to submitting a millisecond earlier — only the price you're willing to pay matters.

```mermaid
flowchart LR
    subgraph batch1["Batch N"]
        direction TB
        ACC1["Orders accumulate<br/>in mempool"]
        SOLVE1["Solve:<br/>welfare-maximizing match"]
        SETTLE1["Settle fills<br/>seal block"]
        ACC1 --> SOLVE1 --> SETTLE1
    end
    subgraph batch2["Batch N+1"]
        direction TB
        ACC2["Orders accumulate<br/>+ pending from N"]
        SOLVE2["Solve"]
        SETTLE2["Settle + seal"]
        ACC2 --> SOLVE2 --> SETTLE2
    end
    SETTLE1 -->|"1 second"| ACC2
```

## Key Properties
- All orders in a batch trade at the same [[LP Duality and Clearing Prices|uniform clearing price]]
- Eliminates speed advantages — only [[Welfare Maximization|willingness to pay]] matters
- Makes the matching problem a well-defined optimization over a finite order set
- Natural fit for prediction markets where information arrives discretely

## Where This Lives
> `crates/matching-sequencer/src/actor.rs` — 1-second timer triggers batch production via `BlockSequencer::produce_block()`

## See Also
- [[Block Lifecycle]] — the full flow from order submission to sealed block
- [[Mempool]] — how orders are collected before each batch
- [[Welfare Maximization]] — the objective function optimized each batch
