# Portfolio — Unified History Feed

Date: 2026-05-21
Status: design approved; FE built against a mock, backend pending.

## Goal

Replace the Portfolio page's two tabs (History = client-derived *closed
positions*, Activity = fills + localStorage cancels) with **one** History tab:
a complete, chronological **event log** of the account's actions and the events
relevant to it. The closed-positions table is dropped; realized P&L surfaces
inline on `filled` / `resolved` rows.

## Event taxonomy

Row-per-transition (each lifecycle step is its own entry):

| type | category | recorded today? |
|---|---|---|
| `created` | funding | ✅ system event |
| `placed` | trades | ❌ **new event needed** |
| `partial_fill` | trades | ✅ fills (label needed) |
| `filled` | trades | ✅ fills (label needed) |
| `cancelled` | trades | ✅ `OrderCancelled` (D1) |
| `expired` | trades | ❌ **new event needed** |
| `deposit` | funding | ✅ system event |
| `withdrawal` | funding | ✅ system event |
| `resolved` | settlement | ✅ `MarketResolved` |

(`rejected` is explicitly out of scope.)

## Normalized model (the FE↔BE contract)

```
HistoryEvent {
  id              // stable key: "<block>.<seq>"
  type            // one of the 9 above
  timestamp_ms, block_height
  market_id?, order_id?           // order_id ties a lifecycle together
  side?: BUY|SELL, outcome?: YES|NO
  qty?            // placed=original, fill=fill qty, cancel/expire=remaining
  price_nanos?    // placed=limit, fills=fill price
  amount_nanos?   // SIGNED cash impact (nanos-dollars); +in / -out
  payout_outcome? // resolved only
}
```

## Row anatomy

`time · type-badge · description (→ market link) · price · amount · #block`

- Fills colored green/red by side; deposit/resolved positive; withdrawal
  negative; cancelled/expired/placed muted (reserve/release, not realized P&L).
- `price`: limit (placed) or fill price (fills), else —.
- `amount`: bold signed for deposit/fill/withdrawal/resolved; muted for
  placed (reserved) / cancelled / expired / created.

## Structure

- Reverse-chronological, **grouped by day** (Today / Yesterday / MMM D dividers).
- **Filter chips:** All · Trades · Funding · Settlement (maps via category).
- Pagination: newest-first, cursor `before=<block>.<seq>`; "load more" / infinite
  scroll. Head re-fetches on block invalidation.

## Backend — what / why / how

**What.** A per-account **off-block event log** + read endpoint
`GET /v1/accounts/{id}/events?limit&before&category` returning
`HistoryEventResponse[]` newest-first, paginated.

**Why.** Today only `/fills` is per-account queryable; cancels/deposits/
withdrawals/resolutions live only in the block stream (the browser keeps ~80
blocks), and placed/expired aren't recorded at all. A durable per-account log is
the only way to show full history.

**How.**
1. Add an `AccountEventLog` sidecar (analogous to `FillRecorder` /
   `CostBasisTracker`) — **off-block**, so it never touches the block digest /
   `events_root` and avoids the "block-hashed, don't mutate" invariant and
   digest versioning that on-block events carry. These rows are informational,
   not consensus-critical.
2. Append to it at moments the sequencer already hits: order admitted
   (`placed`), fill applied (`partial_fill` / `filled` — labeled by whether the
   fill drove remaining to 0), order cancelled, order expired (resting order
   dropped at its expiry block), deposit, withdrawal, market resolved (per
   affected account, with that account's payout), account created.
3. Two genuinely new append points: **`placed`** (on admission) and
   **`expired`** (on expiry-block drop). The rest reuse existing hooks.
4. Bounded retention ring (like the other sidecars). **Caveat:** in-memory →
   resets on restart unless persisted; "full history since creation" only holds
   once persistence runs in prod (same caveat as `first_deposit_ms`).
5. Expose `HistoryEventResponse` (tagged enum, mirroring `SystemEventResponse`)
   in the OpenAPI; FE regenerates types and normalizes to `HistoryEvent`.

Also (separate, smaller, already on the list): add `created_at_ms` to
`PendingOrderResponse` for the Open-orders Created column.

## FE plan

- `lib/account/use-account-history.ts` — `HistoryEvent` types + category map +
  `useAccountHistory(accountId, marketIds)`. **Interim:** deterministic mock
  stream seeded by `accountId` (renders all 9 types). **Swap point:** replace the
  hook body with the `/events` fetch; `HistoryEvent` is the contract.
- `components/portfolio/history-feed.tsx` — day grouping, filter chips, per-type
  row rendering. Whole feed wears a `MockValue` banner until wired.
- `portfolio-tabs.tsx` — drop the `activity` tab; History becomes the feed.
- `app/portfolio/page.tsx` — render `HistoryFeed` for the History tab; remove
  Activity branch, `useClosedPositions`, `useTrackedCancels` display.
- Retire: `history-list.tsx`, `activity-list.tsx`, `use-closed-positions.ts`
  (`use-cancelled-orders.ts` stays — `orders.ts` still records cancels).
