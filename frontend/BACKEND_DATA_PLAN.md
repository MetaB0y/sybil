# Backend data plan — replacing frontend mocks with real data

Catalogue of mocked frontend elements, the data point each needs, and a brief sketch of the backend change. Filled in as we walk the UI.

Deeper technical detail (wire format, on-block vs off-block, migration order) comes in a follow-up iteration.

---

## Entry shape

```
### <frontend element>

- **Where:** `<file:line>` — what renders
- **Mock today:** what is shown / how it's faked
- **Needs:** the missing field(s)
- **Backend sketch:** one paragraph
```

---

## Entries

### Unique trader counts (six surfaces, one tracker)

A "trader for `(market, time-window)`" = an `AccountId` that successfully placed (admitted) at least one order touching that market in that window. Cancels still count as placed; rejections do not. `AccountId::MINT` and MM-flagged accounts are excluded.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Total traders per market | `binary-card.tsx:421`, `multi-card.tsx:502` | `<MockValue hint="trader count">` |
| b | Total traders per event | (planned — derive from a) | not rendered yet |
| c | Total traders in a sealed batch | `last-batches-disclosure.tsx:106` (`uniqueTradersPlaced`) | `<MockValue hint="placed-trader counts not on the wire (OPEN_QUESTIONS #8)">` |
| d | Traders with active orders in the open batch (per market) | `next-batch-banner.tsx:116`, `batch-hero.tsx:114` (`tradersInBatch`) | `<MockValue hint="traders joined this batch — OPEN_QUESTIONS #7">` |
| e | Total unique traders since platform start | Activity page hero | mocked, tagged with `<MockValue>` (OPEN_QUESTIONS #3) |
| f | Unique traders in the last 24h | Activity page hero | mocked, tagged with `<MockValue>` (OPEN_QUESTIONS #3) |

**Needs (raw data already exists — `account_id` is on `RestingOrder`, `OrderSubmission`, `WitnessOrder`, `Rejection`, `Fill`):**

- a — per-market all-time count
- b — per-event all-time count (union over the event's markets)
- c — per-block "unique placers" snapshot, plus per-market breakdown
- d — open-batch unique placers per market (computed on demand from the order book + pending bundles)
- e — global all-time count
- f — 24h rolling count

**Backend sketch.** Add one new tracker `crates/matching-sequencer/src/trader_tracker.rs`, mirroring `price_tracker.rs`. State:

- `per_market: HashMap<MarketId, HashSet<AccountId>>` → powers (a), and (b) via union on demand
- `platform: HashSet<AccountId>` → powers (e)
- `hourly_buckets: VecDeque<(hour_start_ms, HashSet<AccountId>)>` (cap 25) → powers (f) at ±1h resolution

Updated at the two admission sites — `try_admit_direct` (`sequencer.rs:1058`) and the success branches of the admission loop in `produce_block_in_place` (`sequencer.rs:1772`) — both already see `(AccountId, order.active_markets(), timestamp_ms)`. (c) is captured during block production from `witness_orders` and emitted once on the block. (d) is computed on demand by iterating `order_book.market_orderbook(m)` plus `pending_bundles` filtered to `m`; no persistent state needed. Snapshot/restore alongside `PriceTracker` via `SequencerSnapshot` / `RestoredState`.

**Wire changes (all additive, default-zero so old clients keep working):**

- `MarketResponse.trader_count: u32` + `MarketSummaryResponse.trader_count: u32`
- `BlockResponse.unique_placers: u32` + `BlockResponse.placers_by_market: HashMap<String, u32>` (off-block field, alongside `clearing_prices_nanos`)
- New endpoint `GET /v1/markets/{id}/open-batch` → `{ unique_placers: u32, ... }` (will absorb the rest of OPEN_QUESTIONS #7's payload)
- New endpoint `GET /v1/events/{event_id}/traders` → `{ trader_count: u32 }` (API layer reads `market_ref_data.event_id`, gathers markets, asks the sequencer to union; cache hot events ~30s)
- New endpoint `GET /v1/activity/overview` → `{ all_time: { unique_traders, … }, last_24h: { unique_traders, … } }` (will absorb the rest of OPEN_QUESTIONS #3's payload)

**Off-block invariant.** Nothing enters `state_root` / `events_root`. No verifier, witness, or zk impact. Same property `market_volumes` enjoys today.

**Edges / costs:**

- Memory bounded by participants, not activity: per-market sets ≈ markets × placers-per-market; platform set ≈ total placers; hourly buckets ≈ 24 × active-placers-per-hour. Tens of MB at Polymarket scale.
- Restart: tracker must be snapshotted/restored or "all-time" becomes "since last restart". `sybil-api` runs with `SYBIL_DATA_DIR=""` today (in-memory) — confirm persistence story with MetaB0y before labelling (e) as all-time in the UI.
- Never serialize a `HashSet<AccountId>` to the wire — counts only (privacy + payload size).
- Per-event count requires union; summing per-market counts over-counts (one trader can trade two markets in the same event).
- Open-batch count (d) for a market with active bundles: include the bundle's account_ids — the user who submitted one second before block production has, mentally, "placed an order this batch".
- 24h at ±1h resolution is plenty for a dashboard. Drop to 5-min buckets (288 of them) if exact-now-minus-24h is ever needed.

**OPEN_QUESTIONS refs:** #2 (per-market traders), #3 (activity overview rollups), #7 (open-batch unique placers), #8 (per-market placers per batch).

### Volume (six surfaces, extends `PriceTracker`)

"Volume" = cashflow-based: a fill of qty `Q` at price `P` contributes `P × Q` nanos. Already computed across the codebase exactly this way (`price_tracker.rs:100`, `sequencer.rs:1469`). Per-market all-time + per-block totals exist today; everything below is the missing windows + per-block per-market breakdown.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | 24h volume per market | card metric + market detail | `<MockValue>` (no 24h field on wire) |
| b | 24h volume per event | card metric (event-grouped) | derived from (a); falls back to mock |
| c | Volume in last X batches (X = 1/5/10/100) | `last-batches-disclosure.tsx` | partly real for X ≤ FE ring-buffer cap; mock past it |
| d | Total volume since platform start | Activity hero | `<MockValue>` (OPEN_QUESTIONS #3) |
| e | Volume in the last 24h | Activity hero | `<MockValue>` (OPEN_QUESTIONS #3) |
| f | Per-batch volume + per-market split in a batch | Activity batch detail | block-total real; per-market split mocked (OPEN_QUESTIONS #4/#5) |

**Already-existing data:** `Block.total_volume`, `BlockResponse.total_volume_nanos`, `MarketResponse.volume_nanos` (per-market cumulative). `record_block` already computes a transient per-market split per block — currently used only to update cumulative and then thrown away.

**Needs:** rolling 24h windows (per-market + platform), platform all-time sum, per-block per-market split on the wire.

**Backend sketch.** Extend `crates/matching-sequencer/src/price_tracker.rs` (no new file — volume is scalar and lives next to the existing `market_volumes`):

- `platform_volume: u64` — running total
- `hourly_per_market: VecDeque<(hour_start_ms, HashMap<MarketId, u64>)>` (cap 25) — powers (a) and (b)
- `hourly_platform: VecDeque<(hour_start_ms, u64)>` (cap 25) — powers (e)
- Methods: `market_volume_24h(m, now_ms)`, `platform_volume_24h(now_ms)`, `platform_volume()`

Update path: in `record_block`, route the already-computed `per_market_volume: HashMap<MarketId, u64>` into the current hourly bucket (insert / roll on `hour_start_ms` change), bump `platform_volume`, bump `hourly_platform`. Persist alongside `market_volumes` in `SequencerSnapshot` / `RestoredState`.

For (f), surface the per-block split: add `volume_by_market: HashMap<MarketId, u64>` to `Block` (off-block, alongside `clearing_prices`) and to `BlockResponse.volume_by_market: HashMap<String, u64>`.

**Wire changes (additive, default-zero):**

- `MarketResponse.volume_24h_nanos: u64`
- `MarketSummaryResponse.volume_24h_nanos: u64`
- `BlockResponse.volume_by_market: HashMap<String, u64>` (off-block field)
- `GET /v1/activity/overview` (shared with traders entry) → `{ all_time: { total_volume_nanos, … }, last_24h: { total_volume_nanos, … } }`
- No per-event endpoint — FE sums `volume_24h_nanos` across the event's markets. (Volume is additive; no double-count issue like with traders.)

**Last-X-batches strategy.** For X ≤ FE ring-buffer cap (~80 today), FE sums `BlockResponse.total_volume_nanos` client-side. For X = 100+, either bump the FE ring buffer (cheap, recommended) or add `GET /v1/blocks/range?from=&to=&summary=true` later if other multi-block stats need it. No per-X rolling window on the backend.

**Off-block invariant.** Nothing enters `state_root` / `events_root`. No verifier, witness, or zk impact.

**Edges / costs:**

- **Multi-market attribution over-counts platform total.** Today, a fill on an order spanning markets A and B credits `P × Q` to *each* market (`price_tracker.rs:101`). Sum of per-market volume > actual cash exchanged when spreads/baskets are present. This is the existing convention — preserve and document; do not change in this pass. Means: per-market 24h sums (for events) are consistent with per-market cumulative, but platform totals from summing per-market over-count. Use the platform counter, not the sum.
- **MINT account fills** (arb / minting) are counted in volume because the code keys on `MarketId`, not `(account, market)`. Preserves backwards compatibility with the current `volume_nanos` semantics. Worth noting in the API doc.
- **Memory** is trivial: 24 × active-markets-per-hour × 16 bytes. Couple MB at most.
- **Restart**: same persistence story as traders. Without snapshot/restore, 24h refills over time from empty.
- **Resolution**: ±1h on 24h. Drop to finer buckets only if a hero metric demands second-level "now − 24h" precision.

**OPEN_QUESTIONS refs:** #3 (activity overview rollups), #4 (per-market welfare/volume per batch), #5 (per-market matched per batch).

### Liquidity (two surfaces, new tracker)

A deliberate, narrow disclosure on top of FBA's blind orderbook: one aggregate scalar per market per batch — total $ value of resting single-market orders whose `limit_price` is within ±band of midprice — averaged over the last 10 batches. No order-level, account, or side info is exposed. The user accepts the residual leak from a public time-series of this scalar combined with the public clearing-price trajectory; document it in API docs.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Liquidity per market (avg of last 10 batches, ±band) | card metric | `<MockValue hint="liq metric — no resting-depth aggregate on wire (OPEN_QUESTIONS #1)">` |
| b | Liquidity per event | derived from (a) | mocked; falls back from (a) |

**Definition / hooks:**

- Midprice for a binary market = `clearing_prices[m].first()` (YES price ≈ implied probability). Markets without a clearing price (never traded) → no metric, return 0 / `None`. Do not fabricate `0.5`.
- Liquidity value = `Σ (order.limit_price × order.max_fill)` over resting orders where `order.num_markets == 1` and `limit_price ∈ [mid − band, mid + band]`.
- Multi-market orders (spreads / baskets) are **excluded** — their `limit_price` is the bundle's total, attributing it to one market is meaningless. They still live in the book and still match; they just don't count in the depth disclosure.
- Hook: end of `produce_block_in_place`, right after `self.order_book.settle(...)` (`sequencer.rs:2008`). One pass over `order_book.resting_orders()`. O(book size).

**Easily-updateable band.** Config-knob, not per-request:

- `SequencerConfig.liquidity_band_nanos: u64`, default `50_000_000` (= $0.05 at 1e9 nanos/dollar). Change → redeploy → new band takes effect next block.
- Ship the band on the wire alongside the average so the FE labels it honestly when it changes ("liq within ±$0.05").
- A per-request band would require shipping a small histogram per market on `/v1/markets` (~21 buckets × 8 B × N markets). Skip until needed.

**Backend sketch.** New `crates/matching-sequencer/src/liquidity_tracker.rs`:

- `last_n_per_market: HashMap<MarketId, VecDeque<u64>>` (cap 10 per market)
- `band_nanos: u64` (the configured band, snapshotted)
- Methods: `record_block(&OrderBook, &clearing_prices, band_nanos)`, `avg_last_n(m, n)`, `current(m)`

Snapshot/restore alongside `PriceTracker`. Without persistence the ring buffer warms up over 10 blocks on restart — acceptable for a smoothed metric.

**Per-event:** FE sums per-market `liquidity_avg10_nanos` across the event's markets. Additive scalar; no backend per-event endpoint.

**Wire (additive, default-zero):**

- `MarketResponse.liquidity_avg10_nanos: u64`
- `MarketResponse.liquidity_band_nanos: u64`
- `MarketSummaryResponse.liquidity_avg10_nanos: u64`

**Off-block invariant.** Nothing enters `state_root` / `events_root`. No verifier / witness / zk impact.

**Edges / costs:**

- Thin markets (≤2 orders in band): the aggregate can approximate individual orders — privacy weakens but no individual identifiability. Accept; do not threshold-suppress (creates jumpy "—" in the UI).
- Memory: 10 × N markets × 8 B. Negligible.
- Cost: one O(book size) pass per block. Microseconds.
- Cancellations naturally drop out: the post-settle book snapshot excludes cancelled orders.

**OPEN_QUESTIONS refs:** #1 (card `liq` metric).

### Orders — placed / matched / unmatched (five surfaces, new tracker + `RestingOrder` field)

User's definitions (taken as given):

- **placed** = order live during a batch's settlement (admitted, not rejected, not cancelled before that batch). Counted *per batch* — an order resting for 5 batches counts as 5 placeds across its lifetime.
- **matched** = order received ≥1 fill of qty > 0 at any point. Counted *once per order lifetime*.
- **unmatched** = order exited the book without ever being matched (TTL expiry or revalidate-eviction). Cancellations are **not** unmatched (they're their own category — OPEN_QUESTIONS #15).

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | All-time placed / matched / unmatched (platform) | Activity hero | mocked (OPEN_QUESTIONS #3) |
| b | Last 24h placed / matched / unmatched (platform) | Activity hero | mocked (OPEN_QUESTIONS #3) |
| c | Per-batch placed / matched / unmatched | Activity batch detail | placed + matched real (`order_count`, `orders_filled`); unmatched mocked; per-market split mocked (OPEN_QUESTIONS #5/#8) |
| d | Per-market all-time + 24h placed / matched / unmatched | market detail page | mocked |
| e | Total placed in last X batches for a specific market | `last-batches-disclosure.tsx` | FE sums real per-block placed counts up to ring-buffer cap; mock past it |

**What already exists:** `BlockResponse.order_count` (placed-this-batch) and `BlockResponse.orders_filled` (matched-this-batch) are emitted today. Everything else is missing.

**Lifetime per-order state.** Today `RestingOrder.order.max_fill` is the *remaining* qty (decremented on partial fills), so "have you ever been matched" is unrecoverable. Cheapest fix: add `has_been_matched: bool` to `RestingOrder` (default false; set true in `OrderBook.settle` when this order's `filled > 0`). Persists via the existing `SequencerSnapshot.resting_orders` path. Costs 1 byte per resting order.

**Exit categorization.** Three book methods remove orders — each needs to feed the tracker. Refactor each to return the removed orders (current return type is `()`); the sequencer feeds counts from the two call sites in `produce_block_in_place`.

| Exit | `order_book.rs` | Categorize as |
|---|---|---|
| `expire(height)` (TTL) | `:240` | unmatched if `!has_been_matched`, else already-counted matched |
| `revalidate(...)` (market closed / account insolvent) | `:261` | same logic; "evicted, never matched" → unmatched (flag in API docs) |
| `settle` filled branch (`filled >= max_fill`) | `:420` | matched (mark `has_been_matched=true` if not yet, one matched-count increment) |
| `settle` expired-this-batch branch | `:430` | matched if `filled > 0` this batch OR `has_been_matched`, else unmatched |
| `cancel` | `:370` | neither (cancellation is its own category, OPEN_QUESTIONS #15) |

**Backend sketch.** New `crates/matching-sequencer/src/order_stats.rs`:

```
struct OrderStats { placed: u64, matched: u64, unmatched: u64 }

struct OrderStatsTracker {
    per_market: HashMap<MarketId, OrderStats>,     // all-time
    platform: OrderStats,                          // all-time
    hourly_platform: VecDeque<(hour_start_ms, OrderStats)>,  // cap 25, for 24h
    // per-market hourly buckets: optional second cut if 24h-per-market is hot;
    // cost is ~24 × active_markets × 24 B. Skip in first pass.
}
```

Updated at the lifecycle hooks above + at admission time for placed counts. Snapshot/restore alongside `PriceTracker` / `LiquidityTracker`. **Per-market attribution: same convention as volume — for a multi-market order, each active market gets +1 placed/matched/unmatched.** Platform counter is independent so it does not over-count.

**Per-block emission on `BlockResponse` (additive, default-zero):**

- `BlockResponse.unmatched_count: u32` (placed + matched already exist as `order_count` + `orders_filled`)
- `BlockResponse.placed_by_market: HashMap<String, u32>` (off-block, alongside `clearing_prices_nanos`)
- `BlockResponse.matched_by_market: HashMap<String, u32>`
- `BlockResponse.unmatched_by_market: HashMap<String, u32>`

**Per-market wire on `MarketResponse` (additive, default-zero):**

- `orders_placed_total / orders_matched_total / orders_unmatched_total: u64`
- `orders_placed_24h / orders_matched_24h / orders_unmatched_24h: u64` (requires per-market hourly buckets; ship platform 24h first, per-market 24h on demand)

**Activity overview** (shared endpoint with traders + volume): add `all_time.orders.{placed, matched, unmatched}` and `last_24h.orders.{placed, matched, unmatched}`.

**Last-X-batches per market:** FE sums `BlockResponse.placed_by_market[m]` over visible last-X blocks (same strategy as volume's "last X batches"). Backend range endpoint only if X regularly exceeds the FE ring-buffer cap.

**Off-block invariant.** Nothing enters `state_root` / `events_root`. No verifier / witness / zk impact.

**Edges / costs:**

- **MM orders.** Always placed=1 in their batch, then matched=1 or unmatched=1 the same batch (flash liquidity, never rest). Include them — they're real orders. Different rule than the trader-counts entry, where MMs were excluded (they're liquidity providers, not traders).
- **Revalidate-evictions** (market resolved, account insolvent) bucket as unmatched — they never matched. Strictly the user said "expired"; flag in API docs that we lump evictions in.
- **Cancellations** are excluded from both matched and unmatched per the user's definition. If a "cancelled count" is wanted later, that needs the missing `OrderCancelled` event (OPEN_QUESTIONS #15).
- **Persistence**: counters + buckets + the new `has_been_matched` flag all need snapshot/restore or "all-time" drifts on restart. Confirm with MetaB0y before labelling totals as all-time in the UI (sybil-api runs `SYBIL_DATA_DIR=""` today).
- **Refactor scope.** Changing the return shape of `expire / revalidate / settle` is the only non-trivial surgery; both current call sites are in `produce_block_in_place`.

**OPEN_QUESTIONS refs:** #3 (activity overview rollups), #5 (per-market matched per batch), #8 (per-market placed per batch), #15 (cancellations — separate concern).

### Indicative price + indicative volume (current open batch)

**Frontend surface (pro trading section, market detail page):**

| # | Surface | Today |
|---|---|---|
| a | Indicative YES / NO price for the current open batch | mocked (OPEN_QUESTIONS #7) |
| b | Indicative volume for the current open batch | mocked (OPEN_QUESTIONS #7) |

**What "indicative" means here.** FBA clearing price is the output of the solver — not midprice-of-best-bid-ask. "If the batch settled right now" therefore = run the solver on a snapshot of the current state without committing. Indicative volume = sum of `fill_price × fill_qty` from that speculative solve.

**Backend sketch.** `Solver::solve(&self, problem: &Problem) -> PipelineResult` (`crates/matching-solver/src/solver.rs:14`) is already a pure read-only function — the same call we make in `solve_batch_phase` (`sequencer.rs:1436`). A speculative solve is just:

1. Build a `Problem` from `self.order_book.resting_orders()` (Tier 1, MVP) — skips pending bundles, skips flash MM liquidity. Accurate for retail flow where most submissions admit directly into the book via `try_admit_direct` (`sequencer.rs:1058`).
2. Call `solver.solve(&problem)`.
3. Extract `pipeline_result.price_discovery.prices` (per market) and `Σ fill_price × fill_qty` (per market) into an `IndicativeSnapshot { yes_price, no_price, volume, computed_at_ms }` keyed by `MarketId`.

Tier 2 later: replicate the admission loop in `produce_block_in_place` (`sequencer.rs:1772`) so pending bundles also count. More code, more accurate when bundles are present.

**Where to run it.** Scheduled tick in the actor layer, every ~500 ms between real blocks. Cache the result in a sequencer-side `indicative_cache: HashMap<MarketId, IndicativeSnapshot>`. API reads from cache; never solves on demand. (Per-request solving is rejected — N concurrent users on a hot market would trigger N solves.)

Hard guards: skip the tick if (1) a tick is already running, (2) block production is in flight. Tiny `AtomicBool` busy flag.

**Lock strategy.** LP solves on typical book sizes (hundreds of orders, tens of markets) finish in single-digit ms. Start with **acquire-the-sequencer-lock-and-solve briefly**; if it nudges block-tick timing, switch to clone-state-then-solve-outside-the-lock.

**Wire.** Extend the `GET /v1/markets/{id}/open-batch` endpoint proposed in the traders entry. New fields:

- `indicative_yes_price_nanos: Option<u64>` — null when speculative solve is infeasible or book is empty
- `indicative_no_price_nanos: Option<u64>`
- `indicative_volume_nanos: u64`
- `indicative_computed_at_ms: u64` — so FE can show "computed N ms ago" / detect stale

**Fallback semantics:**

- Empty resting book / solver infeasible → both prices `None`, volume `0`.
- Book has orders but no matchable cross at any price → prices fall back to `PriceTracker.last_clearing_prices` (the last committed clearing for the market), volume = 0. Matches the UX: "no fills would happen if this batch settled now, but price stays where it last cleared."

**Off-block invariant.** Pure read-only over chain state. Cache is derived, not chain state — no persistence needed; refreshes on first tick after restart.

**Edges / costs:**

- **MM liquidity excluded.** MM is flash, one-shot per real block — we don't know in advance what budget will commit. The indicative shows depth as if no MM activates; realized price can therefore diverge. Document.
- **Pending bundles excluded (Tier 1).** Same caveat — non-trivial pending-bundle flow makes the indicative stale relative to the next real block.
- **Privacy.** Indicative price ≈ what the next block's `clearing_prices_nanos` will be, just shifted forward by 500 ms − 2 s. Marginal additional leak since clearing prices are already public post-settlement. Indicative volume is more sensitive: jumps between ticks reveal individual admission magnitudes. Intrinsic to disclosure; flag in API docs. Optional mitigations (round price to 1¢, floor volume to $10, suppress when book has < N orders) — not enabled by default.
- **Stale read window.** Worst case ~500 ms behind reality. `indicative_computed_at_ms` lets FE surface staleness.
- **Restart.** Cache resets, refreshes on first tick.

**OPEN_QUESTIONS refs:** #7 (open-batch indicative price/volume).

### Activity page · per-market per-batch breakdown

The activity page wants three per-market metrics for each batch: **matched volume**, **welfare**, and **placed / matched orders** (and unmatched, by extension).

**What's already covered by prior entries:**

- Per-market matched volume per batch → **volume entry** (surface f): `BlockResponse.volume_by_market: HashMap<String, u64>`. Already plumbed from `record_block`'s existing per-market split.
- Per-market placed / matched / unmatched orders per batch → **orders entry** (surface c): `BlockResponse.placed_by_market`, `matched_by_market`, `unmatched_by_market: HashMap<String, u32>`.

The new piece here is per-market welfare.

**Per-market welfare per batch.** Today `BlockResponse.total_welfare_nanos: i64` is one platform-wide number — the sum of `Order.welfare_contribution(fill_price, fill_qty)` across all fills (`crates/matching-solver/src/lp_solver.rs:745`). No per-market breakdown is stored.

**Hook.** Inside `solve_batch_phase` (`sequencer.rs:1430`), right next to the existing `total_volume` computation (lines 1469–1472), accumulate a parallel per-market welfare map using the already-present `order_map` and `Order::welfare_contribution`:

```
let mut welfare_by_market: HashMap<MarketId, i64> = HashMap::new();
for fill in &fills {
    if fill.fill_qty == 0 { continue; }
    let Some(order) = order_map.get(&fill.order_id) else { continue; };
    let w = order.welfare_contribution(fill.fill_price, fill.fill_qty);
    for m in order.active_markets() {
        *welfare_by_market.entry(m).or_insert(0) += w;
    }
}
```

Plumb onto `Block.welfare_by_market: HashMap<MarketId, i64>` and `BlockResponse.welfare_by_market: HashMap<String, i64>` (signed; small negative contributions can appear from solver rounding).

**Multi-market attribution.** Same convention as volume / orders / liquidity-platform: a fill on a multi-market order credits welfare to each active market. Platform `total_welfare_nanos` stays the solver's authoritative number; the per-market split is an additive sidecar with the same caveat (sum-of-per-market ≥ platform total when spreads are present). Document.

**Hashing.** `Block.total_welfare` is already in the witness (`sequencer.rs:1617`). The per-market split is **off-block** — not added to the witness, not in `state_root` / `events_root`. No verifier / witness / zk impact.

**Cost.** One extra HashMap accumulation per block. Microseconds. No solver work added.

**Wire (additive, default-empty):**

- `BlockResponse.welfare_by_market: HashMap<String, i64>` (off-block, alongside `clearing_prices_nanos`)

**Edges:**

- Mint-counterparty fills (arb minting) contribute welfare to their order's markets — preserves the existing solver convention; no special handling.
- Rare negative per-market values from solver rounding: signed accumulator handles natively; FE should render signed.

**OPEN_QUESTIONS refs:** #4 (per-market welfare per batch), #5 (per-market matched per batch — already covered in orders entry), #8 (per-market placed per batch — already covered in orders entry).

### Portfolio — PnL split, history, activity (three surfaces, one new tracker + one new system-event variant)

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Realized vs unrealized PnL split (hero) | portfolio hero | `<MockValue>` (FE approximates via wrong-on-flips `avgEntryPriceNanos()`; OPEN_QUESTIONS #10 / #11) |
| b | Position enter / exit history with prices | portfolio history tab | derivable from fills, but capped at 200 + no cost-basis context |
| c | Activity feed including cancellations | portfolio activity tab | cancellations only visible if cancelled from this browser (localStorage); cancels-from-elsewhere are invisible (OPEN_QUESTIONS #15) |

#### (a) Realized / unrealized PnL split + cost basis

**Cost-basis model: weighted-average cost (WAC).** Per `(account, market, outcome)` track `cost_basis_nanos`; per account track `realized_pnl_nanos`. Update on every fill in `crates/matching-sequencer/src/settlement.rs` (`settle_batch`), walking the `position_deltas` already produced by `compute_fill_settlement` (`fill_recorder.rs:76`):

- **Opening / scaling same-sign:** `new_basis = (old_basis × old_qty + fill_price × Δqty) / (old_qty + Δqty)`. No realized.
- **Reducing toward zero:** realize `(fill_price − basis) × Δqty` (longs; flip sign for shorts). Basis unchanged.
- **Closing to exactly zero:** realize the same; **reset basis to 0** (fixes the position-flip bug in OPEN_QUESTIONS #10).
- **Flipping through zero:** split the fill — realize against the prior position, start fresh basis with the remainder at `fill_price`.
- **Market resolution:** `MarketResolved` settles positions at the payout price. Treat as a forced close at that price → realize `(payout − basis) × qty` for every affected position, zero out.

**Off-block.** Cost basis is **derivable from chain history** (fills + resolutions), so it lives as an off-block sidecar without weakening the verifier — same property `market_volumes` enjoys. Do **not** add it to `Account` / `state_root`; that would require a chain-state migration.

**Backend sketch.** New `crates/matching-sequencer/src/cost_basis_tracker.rs`:

```
struct CostBasisTracker {
    basis: HashMap<(AccountId, MarketId, u8), i64>,  // nanos per share
    realized: HashMap<AccountId, i64>,                // running total
}
```

Methods: `apply_fill(account, deltas, fill_price)`, `apply_resolution(market, payout_nanos, affected_accounts)`, `cost_basis(account, market, outcome)`, `realized_pnl(account)`. Snapshot/restore alongside `PriceTracker` / `FillRecorder`.

`AccountId::MINT` is excluded (synthetic arb positions aren't real PnL).

**Computing unrealized.** Derived at `compute_portfolio` time (`portfolio.rs:31`): `Σ over open positions of (current_price − basis) × qty` for longs; flipped for shorts. Reads `last_clearing_prices` + the new tracker.

**Wire (additive, default-zero):**

- `PortfolioResponse.unrealized_pnl_nanos: i64`
- `PortfolioResponse.realized_pnl_nanos: i64`
- `PositionValueResponse.avg_entry_price_nanos: u64` (the cost basis as a positive price; sign already in `quantity`)
- Keep `PortfolioResponse.pnl_nanos` unchanged (identical to `unrealized + realized`).

#### (b) Position enter / exit history

**Today.** `FillRecorder` is in-memory, capped at 5000 fills/account; API replies capped at 200 (OPEN_QUESTIONS #14). Beyond the cap, history is lost. FE-side lifecycle derivation works in principle but uses the wrong-on-flips `avgEntryPriceNanos()`.

**Approach: keep fills as the source, give them durability + cost-basis context.**

- **Persist fills unboundedly.** Move `FillRecorder` from a 5000-cap in-memory ring to a paginated query against a redb table (or similar durable store). Lift the 200-cap on `GET /v1/accounts/{id}/fills`. The cost-basis tracker (a) gives the FE the right basis at each point, so position lifecycle can be derived correctly client-side.
- **Optional convenience** — attach to each `AccountFillResponse`: `cost_basis_after_nanos: Option<u64>`, `realized_pnl_delta_nanos: Option<i64>`. Lets the FE render entry→exit transitions per fill without re-deriving.

**Deferral note.** Going from in-memory ring to durable history touches the redb sidecar and the deploy story (`SYBIL_DATA_DIR=""` today per frontend AGENTS.md). Could be staged after (a) ships — until then, history is bounded by the ring cap. Same caveat as elsewhere: confirm persistence with MetaB0y before labelling fields as all-time.

**Alternative (deferred): backend-derived position lifecycle events.** A separate `PositionEvent { kind: Open | Close | Flip, qty, price, basis_at_event, realized_delta, … }` stream pushed per state transition (zero ↔ non-zero, sign flip). Smaller than the fill stream, easier to render. Layer on later if (b)-via-fills proves expensive.

#### (c) Activity feed including cancellations

**Add `OrderCancelled` as a `SystemEvent` variant.** In `crates/matching-sequencer/src/system_event.rs`:

```
SystemEvent::OrderCancelled {
    account_id: AccountId,
    order_id: u64,
    market_ids: Vec<MarketId>,
    side: String,           // from existing classify_order_side
    remaining_quantity: u64,
}
```

Hook: `OrderBook.cancel` (`order_book.rs:370`) is the only cancellation path (signed cancel → sequencer → book). On successful cancel, stage the event in `pending_system_events`. It flows into the next block's `system_events` via the existing pipeline.

**Hashing impact — on-chain additive change.** `SystemEvent` variants are encoded into `account.events_digest` (state root) and `events_root` (block header), and live in `BlockWitness.system_events`. A **new variant is forward-additive** under `serde`'s tagged-enum encoding: existing variants encode identically, historical digests stay valid. New blocks include the new variant; nodes on old code can't verify them. Coordinated upgrade — straightforward with Sybil's single sequencer today but distinct from the off-block sidecars elsewhere in this plan.

**Required ancillary changes:** add `crate::digest::encode_order_cancelled_event(…)`, extend `convert_system_event(…)` (`sequencer.rs:355`), and add a corresponding variant to `sybil_verifier::SystemEventWitness`. All mechanical.

**Wire on `SystemEventResponse`:**

```jsonc
{
  "type": "order_cancelled",
  "account_id": 42,
  "order_id": 12345,
  "market_ids": [7],
  "side": "BuyYes",
  "remaining_quantity": 100
}
```

**Edges:**

- Multi-market cancels → `market_ids` is a list. UI groups by primary market or shows all.
- Partial-fill-then-cancel → `remaining_quantity` is the released remainder. FE computes the filled portion as `original − remaining` once OPEN_QUESTIONS #16's `original_quantity` ships.
- No admin-initiated cancel today; if added later, emit the same event with appropriate fields.

#### See also — adjacent portfolio items

- **#13 first-deposit timestamp** — covered as its own entry below.
- **#14 exact trade count** — covered as its own entry below.
- **#16 partial-fill progress** — covered as its own entry below.
- **#12 equity-curve time series** — **NOT NOW.** Bigger effort: per-account portfolio-value bucketed during `produce_block`, persisted, exposed via `GET /v1/accounts/{id}/equity?from&to&buckets`. Memory cost ≈ 24 B × buckets × accounts; only viable with persistence. Deferred until the persistence story is sorted out.

**OPEN_QUESTIONS refs:** #10 (cost basis), #11 (realized/unrealized PnL split), #12 (equity curve — NOT NOW), #15 (`OrderCancelled` event).

### Price change (24h) — close two mock sites + fix silent buffer-cap bug

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Primary card 24h Δ (BinaryCard, MultiCard primary outcome) | `binary-card.tsx:253`, `multi-card.tsx:333` | real via `useCardHistory` — but **silently wrong on busy markets** (buffer too short) |
| b | MultiCard sibling-row 24h Δ | `multi-card.tsx:438` | `<MockValue hint="24h delta">` |
| c | OutcomeLegend per-outcome 24h Δ | `outcome-legend.tsx:84` | `<MockValue hint="24h delta — no backend rollup (OPEN_QUESTIONS #3)">` |

**Why (b) and (c) went mock.** Each sibling would need its own `/v1/markets/{id}/prices/history?from_ms=now-24h` round-trip. A 5-outcome MultiCard = 5 fetches per visible card; a 10-outcome legend = 10 per page load. `useCardHistory` itself notes: *"Each market is its own round-trip until the backend exposes a batched endpoint."*

**Silent bug on (a).** `PriceTracker.price_history` is capped at 2000 points per market and only appends on blocks with fills (`price_tracker.rs:107-128`). For a market with fills every block (2 s cadence): 2000 points ≈ **67 minutes**, not 24 hours. FE filters `from_ms = now - 24h` after load, so `points[0]` is "oldest-in-buffer" which on busy markets is ~30 min ago. The "24h delta" silently becomes a much-shorter delta.

**Needs:** server-computed "price 24h ago" per market, so the delta is one subtraction client-side (no history fetch).

**Backend sketch.** Extend `crates/matching-sequencer/src/price_tracker.rs` with an hourly bucket of clearing prices, mirroring the volume / trader hourly-bucket pattern:

```
hourly_clearing_prices: HashMap<MarketId, VecDeque<(u64 hour_start_ms, Vec<Nanos>)>>  // cap 25
```

Update path in `record_block`: on every block, take the merged clearing price (already in hand) and slot it into the current hourly bucket; roll buckets on hour boundary. Lookup: `price_n_hours_ago(m, n) = bucket where hour_start_ms ≈ now_ms - n*3_600_000`.

**Memory.** 25 buckets × (16 B + 2 × 8 B) × N markets ≈ ~3 MB at 5K markets. Trivial.

**Wire (additive, default-None):**

- `MarketResponse.yes_price_24h_ago_nanos: Option<u64>`
- `MarketResponse.no_price_24h_ago_nanos: Option<u64>`
- `MarketSummaryResponse.yes_price_24h_ago_nanos: Option<u64>` (+ no_)

FE drops `mockDelta` at (b) and (c); the primary card path stops depending on `useCardHistory` for the delta (history endpoint is still used for the sparkline). Compute = `current − snapshot`. No batched history endpoint needed.

**Off-block invariant.** Snapshot lives in the same off-block sidecar as `market_volumes`. No verifier / witness / zk impact.

**Edges:**

- Markets younger than 24h → no 24h-ago bucket → return `None`; FE renders "—".
- Markets with no clearing price yet → `None`; same handling.
- Hour-level resolution (±1h) is fine for a "24h" card metric.
- Persistence: same story as the other trackers — snapshot/restore or warmup-from-empty on restart. Without persistence, deltas read `None` for the first 24h after redeploy. Document.

**OPEN_QUESTIONS refs:** none direct; touches the price-history / 24h-delta surface flagged informally in `use-card-history.ts:30` and `outcome-legend.tsx:84`.

### First-deposit timestamp (one wire field)

- **Surface:** portfolio hero "since first deposit" copy on the ALL range; today shows static range labels with no anchor date.
- **Backend:** add `first_deposit_ms: u64` (and optionally `first_deposit_height: u64`) to `PortfolioResponse`.
- **Hook:** in `fund_account` / `ingest_l1_deposit`, write the timestamp if not yet set for the account.
- **Where to store:** off-block sidecar `HashMap<AccountId, u64>` snapshot/restored alongside `market_volumes` — adding the field to `Account` would touch `state_root` and require a coordinated chain change.
- **Off-block.** No verifier / witness / zk impact.

**OPEN_QUESTIONS refs:** #13.

### Exact trade count (one wire field)

- **Surface:** portfolio hero "N trades"; today capped because FE pulls `/v1/accounts/{id}/fills?limit=200` and reports `fills.length`, showing "200+" when capped.
- **Backend:** add `total_fill_count: u64` to `PortfolioResponse`. Maintain a per-account counter incremented in `FillRecorder.record_fills` (or piggyback on the `CostBasisTracker` if it ships first).
- **Off-block.** No chain impact.

**OPEN_QUESTIONS refs:** #14.

### Partial-fill progress — `original_quantity` (one wire field)

- **Surface:** portfolio open-orders row wants `filled / size` with a progress bar; today `PendingOrderResponse.remaining_quantity` is the only quantity on the wire.
- **Backend:** add `original_max_fill: u64` to `RestingOrder` (populated in `OrderBook.accept` at admission, never mutated after). Persist in the existing snapshot. Surface as `original_quantity: u64` on `PendingOrderResponse`. FE computes `filled = original − remaining`.
- **Dovetails** with the `has_been_matched: bool` field the orders entry already proposes — same struct, same persistence hook.
- **Off-block.** No chain impact.

**OPEN_QUESTIONS refs:** #16.

---

## Not now

Items surveyed but deferred — corresponding `MockValue` hints in the frontend are tagged `NOT NOW —`:

- **OPEN_QUESTIONS #6** — per-market imbalance (requires `side: String` on `FillResponse`; touches `convert.rs` round-trip from the engine). Mock sites: `batch-detail.tsx:350`, `batch-hero.tsx:157`, `batch-hero.tsx:184` (bar colors), `m-dev/[id]/page.tsx:216`.
- **OPEN_QUESTIONS #9** — `created_at_height: u64` on `MarketResponse` for the exact "batches this market has existed" count (FE currently approximates from `created_at_ms`). Mock sites: `m/[id]/page.tsx:252`, `m-dev/[id]/page.tsx:164`.
- **OPEN_QUESTIONS #12** — per-account equity curve. Bigger effort; depends on the persistence story. Mock sites: `equity-chart.tsx:215`, `portfolio-hero.tsx:79`.

---

## Log

- `2026-05-14` — scaffolding created.
- `2026-05-14` — entry added: unique trader counts (six surfaces, one tracker).
- `2026-05-14` — entry added: volume metrics (six surfaces, extends `PriceTracker`).
- `2026-05-14` — entry added: liquidity (two surfaces, new tracker + config band).
- `2026-05-14` — entry added: orders placed / matched / unmatched (five surfaces, new tracker + `has_been_matched` field on `RestingOrder`).
- `2026-05-14` — entry added: indicative price + indicative volume (open batch, scheduled speculative solve + cache).
- `2026-05-14` — entry added: activity page per-market per-batch breakdown (only new piece is per-market welfare; volume + orders covered by prior entries).
- `2026-05-14` — entry added: portfolio — PnL split / history / activity (new `CostBasisTracker` + `OrderCancelled` system event; cross-refs OPEN_QUESTIONS #10–#16).
- `2026-05-14` — entry added: price change 24h — server-computed 24h-ago snapshot per market, closes two mock sites + fixes silent buffer-cap bug on the working delta.
- `2026-05-14` — entries added: first-deposit timestamp (#13), exact trade count (#14), partial-fill `original_quantity` (#16) — all trivial single-field additions.
- `2026-05-14` — "Not now" section added: imbalance (#6), `created_at_height` (#9), equity curve (#12). Frontend `MockValue` hints for these tagged `NOT NOW —`.
