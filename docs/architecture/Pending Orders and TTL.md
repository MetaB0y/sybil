---
tags: [infrastructure]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-04-10
---

Not every order fills in its first batch. When a trader's order isn't matched — perhaps the clearing price moved away from their limit, or there wasn't enough counterparty liquidity — the order doesn't disappear. Instead, it becomes a **resting order** in the [[Order Book]], automatically re-included in each subsequent [[Block Lifecycle|batch]] until it fills, expires, or is cancelled.

Each resting order has a configurable Time-To-Live (TTL) measured in blocks. A fresh order enters with TTL 3, and each block it goes unmatched counts against the TTL. When the order has been resting longer than the TTL, it expires and is removed. This prevents the system from accumulating unbounded stale orders while giving reasonable orders multiple chances to fill — a market that's temporarily illiquid might see liquidity arrive in the next few seconds.

MM (market maker) quotes are explicitly excluded from the order book. MM quotes are one-shot: they bypass the book entirely and are consumed by the current batch. This design matches how professional market makers operate — they want to re-evaluate and re-quote every batch based on current conditions, not have stale quotes persist. Regular trader orders persist because retail users submit and expect fills over a reasonable time horizon.

## Order Book and Reservations

The `OrderBook` component (`order_book.rs`) is the single source of truth for committed capital. When an order is accepted into the book, its worst-case cost is **reserved immediately** — balance for buys, positions for sells. This prevents over-commitment: a trader with $10 can't have two $8 resting orders simultaneously.

The reservation lifecycle:
1. **Accept**: validate order against account state + existing reservations → reserve capital
2. **Expire**: remove orders past TTL → release their reservations
3. **Revalidate**: after state changes (market resolution, bankruptcy) → remove invalid orders
4. **Settle**: after solving → fully filled orders release all reservations, partial fills adjust proportionally

This design ensures that the "available balance" (`balance - reserved`) is always accurate, regardless of how many orders are resting across how many blocks.

## Key Properties
- Unfilled non-MM orders become resting orders in the OrderBook
- Balance/position reservations tracked at acceptance time (single source of truth)
- Configurable TTL in blocks (default 3)
- Re-validated each block (market resolution, account solvency)
- MM quotes are one-shot — never enter the book
- Partial fills adjust reservations proportionally

## Where This Lives
> `crates/matching-sequencer/src/order_book.rs` — OrderBook struct, reservation tracking
> `crates/matching-sequencer/src/sequencer.rs` — BlockSequencer uses OrderBook in produce_block

## See Also
- [[Block Lifecycle]] — resting orders are expired and revalidated at block start
- [[Mempool]] — where new orders arrive before entering the order book
- [[Frequent Batch Auctions]] — the batching context for order persistence
- [[Persistence]] — Tier 2 will persist the order book across restarts
