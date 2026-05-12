# Open Questions

Running list of frontend-related questions to discuss. Keep entries short.

1. **Card `liq` metric — store pre-clearing book state in backend?**
   Idea: avg depth on both sides within ±5¢ of mid over last 5 batches.
   Today: backend persists clearing prices + fills per block, but no resting orderbook snapshot per batch. Live `/orderbook` is dev-mode only and snapshot-only.
   Question: can we add per-block resting depth (price levels + sizes) to the backend so the frontend can compute this?

2. **Card `traders` metric — expose unique trader count?**
   Data exists: every fill carries `account_id`, so distinct-count per market is derivable.
   Missing: no `trader_count` field on `MarketResponse`, no `/v1/markets/{id}/fills` endpoint, no aggregate.
   Question: add a maintained `trader_count` (HashSet of account_ids per market, updated on fills) to `MarketResponse`?

3. **Activity page · all-time + 24h rollups (volume, traders, orders).**
   Data exists per block (`BlockResponse.{total_volume_nanos, fills[].account_id, order_count, orders_filled, rejections}`), but no maintained counter / rollup endpoint. Scanning every block since genesis from the frontend is unbounded, and at the **2s FBA batch cadence** even a 24h window is **~43,200 blocks** — not just unbounded, *daily-unbounded*. The store ring buffer (cap 80) only ever covers ~2–3 minutes of history at this cadence, so client-side 24h math is structurally impossible.
   Suggested: a `/v1/activity/overview` endpoint that returns pre-aggregated rollups for `{all_time, last_24h, prior_24h}` — matched volume, unique traders (HashSet of account_ids), placed/matched/unmatched. Sequencer maintains the counters on every block; the endpoint is a cheap read.
   For now: mock the all-time + 24h numbers and tag with `<MockValue>`; the prototype's "recent activity" panel shows the honest tiny window we actually have ("last 2m 34s · 79 blocks") so we don't pretend.

4. **Activity page · per-market welfare contribution inside a batch.**
   `Block.total_welfare: i64` is computed by the solver as one aggregate number across all markets (`crates/matching-sequencer/src/sequencer.rs:1468`). Per-market breakdown is not stored.
   Suggested: change to `welfare_by_market: HashMap<MarketId, i64>` (or add a side-car field on `Block` — careful about block hashing; an off-block field on `BlockResponse` is the safer wire-level change). Derived total can stay.
   For now: mock per-market welfare by allocating `total_welfare` proportional to per-market matched volume.

5. **Activity page · placed / matched orders per market per batch.**
   `Block.fills[]` exists with `order_id` and price, but no `market_id` on the wire (`FillResponse` in `frontend/web/src/lib/api/schema.d.ts:842`). Today the frontend cannot tell which market a fill belongs to without looking up the order, which is also not on `BlockResponse`.
   Suggested: denormalize `market_id` onto `FillResponse` so the frontend can group fills by market. Once that's in, placed/matched per market per batch is `(rejections + fills group counts)`.
   For now: mock per-market counts proportional to per-market matched volume.

6. **Activity page · per-market imbalance (buys vs sells).**
   `FillResponse` has no `side: "BUY" | "SELL"` field, and `Block` doesn't expose pending orders either. Imbalance can't be computed from fills alone.
   Suggested: add `side` to `FillResponse` (cheap — `matching-engine/src/order.rs` already knows the side; just needs to round-trip through `convert.rs`). With per-fill side, imbalance = `(buys_volume − sells_volume) / total_volume` per market per batch.
   For now: mock imbalance as a small random ± offset from neutral.
