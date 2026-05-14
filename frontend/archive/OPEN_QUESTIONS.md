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

6. **Activity page · per-market imbalance (buys vs sells).** [NOT NOW]
   `FillResponse` has no `side: "BUY" | "SELL"` field, and `Block` doesn't expose pending orders either. Imbalance can't be computed from fills alone.
   Suggested: add `side` to `FillResponse` (cheap — `matching-engine/src/order.rs` already knows the side; just needs to round-trip through `convert.rs`). With per-fill side, imbalance = `(buys_volume − sells_volume) / total_volume` per market per batch.
   For now: mock imbalance as a small random ± offset from neutral.

7. **Specific-market page · traders/orders in the open (in-flight) batch.**
   The page wants live "traders placed so far in this batch", indicative price, indicative volume, and imbalance for the batch that's currently open. Today the only window into pre-clearing state is `/v1/orders/pending`, which is `SYBIL_DEV_MODE`-only (`crates/sybil-api/src/routes/orders.rs:229`) and exposes raw pending orders (no `side`, no `market_id` on fills downstream). Nothing exposes a mid-batch indicative clearing price/volume.
   Suggested: a prod-safe `/v1/markets/{id}/open-batch` returning `{ unique_placers: u32, placed_volume_nanos, order_count, indicative_yes_price_nanos?, indicative_volume_nanos?, imbalance_bps? }`. Or more generally a `/v1/batches/current` summary endpoint that aggregates this per market.
   For now: mock all four open-batch fields and tag them with `<MockValue>`.

8. **Specific-market page · placed orders + placed volume per market per batch.**
   `BlockResponse.order_count` is a single u32 with no per-account or per-market breakdown, and there's no notional sum for placed-but-unfilled orders anywhere on the wire. Recent-batches windows (1/5/10/100) want "unique traders who placed" and "volume placed" per market — neither is derivable from `BlockResponse` today.
   Suggested: extend `BlockResponse` with `placed_by_market: HashMap<MarketId, { count: u32, unique_placers: u32, placed_volume_nanos: u128 }>` as an off-block field (same pattern as #4's `welfare_by_market` suggestion — keep it off the block hash). Unblocks both fields for real.
   For now: mock per-market placed counts/volume proportional to per-block totals.

9. **Specific-market page · "batches this market has existed" stat.** [NOT NOW]
   `MarketResponse` exposes `created_at_ms` (epoch ms) but no `created_at_height`. With only the timestamp, the count of batches since creation is an **approximation** at the 2s FBA cadence: `floor((latestBlock.timestamp_ms − market.created_at_ms) / 2000)`. Real cadence can drift; the number is exact only if blocks land on a perfect 2s grid.
   Suggested: add `created_at_height: u64` to `MarketResponse`. Then the count is `latestBlock.height − market.created_at_height + 1` — exact, no clock arithmetic.
   For now: use the timestamp approximation, label it as approximate in the UI when `created_at_ms` is present; show "—" when null.

10. **Portfolio · avg entry price per position (cost basis).**
    `PositionValueResponse` exposes current mark + value but no entry / cost basis. Frontend reconstructs via `avgEntryPriceNanos()` in `src/lib/account/positions.ts` — sums fills whose `position_deltas[outcome].delta > 0`, qty-weighted by `fill_price_nanos`. Wrong on position flips (sell-all then re-buy reuses old basis).
    Suggested: backend tracks `cost_basis_nanos` per `(account_id, market_id, outcome)`, debits on buys / averages on sells (or FIFO), and adds `avg_entry_price_nanos: u64` to `PositionValueResponse`.
    For now: render avg entry inside `<MockValue>` everywhere it appears (positions list, hero).

11. **Portfolio · realized vs unrealized PnL split.**
    `PortfolioResponse.pnl_nanos` is one total — `portfolio_value − total_deposited`. The portfolio hero wants both halves. Frontend approximation in `src/lib/account/use-pnl-split.ts`: `unrealized = Σ (position.value − qty × avg_entry)`; `realized = pnl − unrealized`. Both depend on #10 so both are off when avg_entry is off.
    Suggested: once #10 ships, split server-side into `unrealized_pnl_nanos` + `realized_pnl_nanos` on `PortfolioResponse` so frontend doesn't have to re-walk fills on every render.
    For now: hero cells wrap both halves in `<MockValue>`.

12. **Portfolio · equity-curve time series.** [NOT NOW]
    No per-account portfolio-value history is exposed. Computing it client-side requires replaying every fill + deposit + clearing-price change since first deposit — unbounded and only partially available (`/blocks` is open-ended, `/fills` is paginated). The frontend ring buffer covers ~3 min at 2s cadence — useless for a 7d/30d/ALL chart.
    Suggested: a `/v1/accounts/{id}/equity?from=ts&to=ts&buckets=N` endpoint that aggregates portfolio_value at bucket boundaries. Sequencer maintains a per-account marked-to-batch series on every block (cheap — it already computes portfolio value for the response).
    For now: deterministic mock curve from `(accountId, range)` seed in `src/lib/account/use-equity-curve.ts`, anchored to the real endpoints (start = `total_deposited`, end = `portfolio_value`). Chart frame wears a `<MockValue>` pill.

13. **Portfolio · first-deposit timestamp.**
    No `first_deposit_ms` (or `first_deposit_height`) on `PortfolioResponse`. The hero copy "since first deposit" on the ALL range needs it. Walking system events for our `account_id` back to genesis isn't bounded from the frontend.
    Suggested: add `first_deposit_ms: u64` (and / or `first_deposit_height: u64`) to `PortfolioResponse`. Trivial — sequencer already touches this field on the first `Deposit` event for the account.
    For now: hero shows static range labels ("past 24 hours" / "since first deposit") without an actual anchor date.

14. **Portfolio · trade count.**
    Hero shows "N trades". Frontend pulls `/v1/accounts/{id}/fills?limit=200` and reports `fills.length`, displaying "200+" when capped. Beyond the cap the number is wrong.
    Suggested: include `total_fill_count: u64` in `PortfolioResponse` so we get an exact count without pulling the whole history.
    For now: cap-aware `{N}+` display; not flagged as MockValue since the typical user count will be well under 200.

15. **Portfolio · `OrderCancelled` system event.**
    `SystemEventResponse` variants are `create_account / deposit / l1_deposit / withdrawal_created / market_resolved` — no `order_cancelled`. Cancelled orders just disappear from `/v1/accounts/{id}/orders` between two blocks. The Activity tab needs CANCELLED rows alongside FILLED.
    Suggested: emit `OrderCancelled { account_id, order_id, market_id, side, remaining_quantity }` as a `SystemEventResponse` variant. Lets us show cancels for orders that get cancelled from another tab / browser too, and matches the existing chain-of-truth (everything else lives in blocks).
    For now: cancels issued *from this browser* are recorded in `localStorage[sybil:auth:cancelled_orders]` by `cancelSignedOrder` and emitted as CANCELLED rows by `use-cancelled-orders.ts`. Cancels from elsewhere are invisible.

16. **Portfolio · partial-fill progress on open orders.**
    `PendingOrderResponse` exposes `remaining_quantity` but not `original_quantity`. Design wants `filled / size` with a progress bar. Frontend could stash original `max_fill` in localStorage at submit time (we own the signed-order path) but that misses orders submitted from another browser, and partial fills don't update an authoritative "original" value either.
    Suggested: add `original_quantity: u64` (or `filled_quantity: u64`) to `PendingOrderResponse`.
    For now: the Open orders row shows `remaining_quantity` only, wrapped in `<MockValue>` to flag the missing progress information.
