---
tags: [infrastructure]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-03-15
---

Settlement is the step in the [[Block Lifecycle]] where fills become real: balances are debited, positions are credited, and account state is updated. It runs after the solver returns fills and clearing prices but before the block is sealed. The system uses i128 signed intermediates for all arithmetic to prevent overflow — a price times a quantity in [[Nanos and Integer Arithmetic|nanos]] can exceed u64 range, and settlement involves both debits (negative) and credits (positive).

There are two settlement paths. The **simple path** handles single-market binary orders: buying YES debits `price * qty` from the buyer's balance and credits `qty` to their YES position; selling YES does the reverse. The **generic path** handles bundles and spreads via [[Payoff Vectors|payoff vector]] marginals. For each market the order spans, the marginal payoff determines the position change, and the cost is computed from the per-state decomposition. Both paths use the same i128 intermediate arithmetic and the same validation checks.

Market resolution is also handled through settlement. When a market resolves via the [[Oracle Lifecycle|oracle]] (see [[Market Resolution]]), YES shares pay out `yes_payout_nanos` per share and NO shares pay out `NANOS_PER_DOLLAR - yes_payout_nanos`. Fractional resolution is supported — a market can resolve 70/30 instead of binary 100/0 — which allows for nuanced outcomes. Resolution is irreversible: once settled, positions are converted to balance and the market is marked as resolved. The [[Four-Layer Verification|settlement verification layer]] independently re-derives the post-state from pre-state plus fills to confirm correctness.

## Key Properties
- i128 intermediates for overflow-safe `price * qty` calculations
- Simple path: single-market binary orders (most common)
- Generic path: bundles/spreads via [[Payoff Vectors|payoff vector]] marginals
- Market resolution: YES → `payout_nanos`, NO → `NANOS_PER_DOLLAR - payout_nanos`
- Fractional resolution supported (e.g., 70%/30%)
- Resolution is irreversible

## Where This Lives
> `crates/matching-sequencer/src/settlement.rs` — fill settlement and market resolution logic

## See Also
- [[Block Lifecycle]] — settlement is step 6 of the pipeline
- [[Nanos and Integer Arithmetic]] — why i128 intermediates are needed
- [[Four-Layer Verification]] — Layer 2 independently verifies settlement
- [[Market Resolution]] — the oracle-triggered resolution process
- [[Oracle Lifecycle]] — the state machine that triggers resolution decisions
- [[Pending Orders and TTL]] — unfilled orders persist after settlement for future batches
