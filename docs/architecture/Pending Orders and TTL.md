---
tags: [infrastructure]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-03-15
---

Not every order fills in its first batch. When a trader's order isn't matched — perhaps the clearing price moved away from their limit, or there wasn't enough counterparty liquidity — the order doesn't disappear. Instead, it becomes a pending order that persists across future batches, automatically re-included in each subsequent [[Block Lifecycle|batch]] until it fills, expires, or is cancelled.

Each pending order has a configurable Time-To-Live (TTL) measured in batches. A fresh order enters with TTL 3, and each batch it goes unmatched decrements the counter by one. At TTL 0, the order expires and is removed. This prevents the system from accumulating unbounded stale orders while giving reasonable orders multiple chances to fill — a market that's temporarily illiquid might see liquidity arrive in the next few seconds. The sequencer re-validates pending orders at the start of each batch: checking that the account still has sufficient balance (for buys) or positions (for sells), since state may have changed due to intervening fills.

MM (market maker) quotes are explicitly excluded from the pending mechanism. MM quotes are one-shot: they are consumed by the current batch and never carried over. This design matches how professional market makers operate — they want to re-evaluate and re-quote every batch based on current conditions, not have stale quotes persist. Regular trader orders persist because retail users submit and expect fills over a reasonable time horizon.

## Key Properties
- Unfilled non-MM orders automatically persist across batches
- Configurable TTL in batches (decremented each batch, expired at 0)
- Re-validated each batch (balance/position checks against current state)
- MM quotes are one-shot — never become pending
- Prevents stale order accumulation while giving orders time to fill

## Where This Lives
> `crates/matching-sequencer/src/sequencer.rs` — pending order management in `BlockSequencer`

## See Also
- [[Block Lifecycle]] — pending orders are re-validated in step 1
- [[Mempool]] — where new orders arrive before becoming pending
- [[Frequent Batch Auctions]] — the batching context for order persistence
