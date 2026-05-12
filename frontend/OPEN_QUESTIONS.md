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

3. **Activity page · all-time unique traders count.**
   Data exists per fill (`FillResponse.account_id` on every `BlockResponse.fills[]`), but no maintained set or rollup endpoint. Scanning every block since genesis from the frontend is unbounded.
   Suggested: keep a `unique_traders_total: u64` counter in sequencer state (HashSet of account_ids ever seen on a fill) and expose it on `/v1/health` or a new `/v1/activity/overview` rollup. Same counter, time-windowed for last-24h, would also unblock the 24h pulse strip.
   For now: mock the all-time number; for 24h, compute client-side over the last N (≈1440) blocks that the WS replay yields — accept it as an approximation until backend lands.

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
