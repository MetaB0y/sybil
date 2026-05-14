# Backend data plan — replacing frontend mocks with real data

Catalogue of mocked frontend elements, the data point each needs, and a brief sketch of the backend change.

Deeper technical detail (wire format, on-block vs off-block, migration order) comes in a follow-up iteration.

---

## Ground rules

The entries below share a small set of conventions. Stated once here; every entry assumes them.

**Off-block by default.** New aggregates live as off-block sidecars next to `PriceTracker.market_volumes` — they do not enter `state_root` / `events_root` / `BlockWitness`, do not change historical proofs, and have no verifier / witness / zk impact. **One exception** is the new `OrderCancelled` `SystemEvent` variant — see the "On-chain change" callout below.

**Multi-market attribution.** A fill on an order spanning markets A and B credits per-market counters (volume, orders, welfare) to *each* active market. Sum-of-per-market therefore over-counts when multi-market orders (spreads / baskets) are present. The platform total is the authoritative scalar — independently maintained, not a sum. Preserves the existing convention in `price_tracker.rs:101`.

One deliberate divergence: **liquidity excludes multi-market orders entirely** (their `limit_price` is the bundle total, not attributable to one market). Called out in the liquidity entry.

**MM and MINT inclusion** ("excluded" = the account is skipped for this metric):

| Metric | MM (flash liquidity) | `AccountId::MINT` (synthetic) |
|---|---|---|
| Unique traders | excluded — liquidity provider, not trader | excluded — system account |
| Volume | included — real cashflow | included — preserves current `market_volumes` semantics |
| Orders placed / matched / unmatched | included — real orders | n/a — MINT doesn't submit orders |
| Per-market welfare | included — solver convention | included — solver convention |
| Liquidity disclosure | excluded — never rests in book | n/a |
| Indicative price / volume | excluded — flash, one-shot per real block | n/a — synthetic minting only on real blocks |
| Cost basis / per-account PnL | n/a | excluded — synthetic positions aren't real PnL (early-return guard at `apply_fill` entry) |
| First-deposit timestamp | included if it ever deposits | excluded — system account |
| Trade count | included | excluded — system account |

**Rolling windows.** Two conventions, deliberate:
- **24h rolling** uses **hourly buckets** capped at 25 entries (`VecDeque<(hour_start_ms, T)>`). ±1h resolution; trivial memory (≤ a few MB at Sybil scale). Drop to finer buckets only if a hero metric demands exact "now − 24h" precision.
- **Last-N-batches** uses a per-batch ring buffer (e.g. liquidity's last 10). Per-batch grain matters when the metric must be smoothed across volatile micro-windows; hourly grain is fine for dashboards.

**FE-side range vs backend range.** For "last X batches per market," the FE sums per-block emissions (`BlockResponse.by_market[m].volume_nanos`, etc.) up to its ring-buffer cap (~80 today). A backend `GET /v1/blocks/range?from&to&summary=true` endpoint is only worth shipping when X regularly exceeds the cap. None of the entries below require it.

**Snapshot / restore plumbing.** Every new tracker — and every extension to `PriceTracker` — plugs into the existing `SequencerSnapshot` / `RestoredState` path, same way `market_volumes` does today. Without snapshot/restore the "all-time" fields drift on restart.

**Persistence caveat (load-bearing).** `sybil-api` currently runs with `SYBIL_DATA_DIR=""` (in-memory). Until persistence ships, every "all-time" field reads "since last restart". UI labels for these fields must be cautious — confirm the persistence story with MetaB0y before promoting them.

**Wire-additive only.** All new fields default-zero / default-None / default-empty on existing response types so older clients keep working. No breaking changes to existing fields.

---

## On-chain change — `OrderCancelled` SystemEvent

The plan's only on-chain change. Listed separately because its deploy story differs from every off-block sidecar below: it touches `account.events_digest`, `events_root`, and `BlockWitness.system_events`, and requires the verifier to know the new variant. A `serde` enum variant addition is **forward-additive** under externally-tagged encoding (the default) — historical blocks encode identically and historical digests stay valid — but **new blocks need the new variant on both sides** (sequencer + verifier). Coordinated upgrade: straightforward today (single sequencer, no third-party verifiers in production), but flag it vs the hot-reload everything else can do.

Mechanical pieces (5 sites, not 4 — initial sketch under-counted):
- New variant on `SystemEvent` (`system_event.rs`) with `account_id`, `order_id`, `market_ids: Vec<MarketId>`, `side: matching_engine::Side`, `remaining_quantity: u64`. **Use the existing `Side` enum**, not `String` — every other `SystemEventWitness` field is typed + byte-encoded; a string would be the only non-canonical one.
- Matching variant on `sybil_verifier::SystemEventWitness`.
- Matching arm in `convert_system_event` (`sequencer.rs:355`).
- Matching arm in the **per-account `events_digest` match** inside `produce_block_in_place` (`sequencer.rs:1641-1712`) — 5 arms today, sixth gets added.
- New leaf-encoding arm in `system_event_leaf_value` (`crates/sybil-verifier/src/event_schema.rs:24-92`) — tag bytes are sorted 0-4 today; `OrderCancelled` slots in as tag byte 5 (next free). Leaf-tag ordering matters for the `events_root` hash.
- New `encode_order_cancelled_event` in `digest.rs` (matching the existing `encode_*_event` family).
- Hook: the single signed-cancel path in `OrderBook.cancel` (`order_book.rs:370`) stages the event in `pending_system_events`; it flows into the next block's `system_events` via the existing pipeline.

Consumed by the portfolio activity feed (see "Portfolio — PnL split, history, activity" → (c)).

**OPEN_QUESTIONS refs:** #15.

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

A "trader for `(market, time-window)`" = an `AccountId` that successfully placed (admitted) at least one order touching that market in that window. Cancels still count as placed; rejections do not. MM and MINT excluded per the inclusion table.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Total traders per market | `binary-card.tsx:421`, `multi-card.tsx:502` | `<MockValue hint="trader count">` |
| b | Total traders per event | (planned — derive from a) | not rendered yet |
| c | Total traders in a sealed batch | `last-batches-disclosure.tsx:106` (`uniqueTradersPlaced`) | `<MockValue hint="placed-trader counts not on the wire (OPEN_QUESTIONS #8)">` |
| d | Traders with active orders in the open batch (per market) | `next-batch-banner.tsx:116`, `batch-hero.tsx:114` (`tradersInBatch`) | `<MockValue hint="traders joined this batch — OPEN_QUESTIONS #7">` |
| e | Total unique traders since platform start | Activity page hero | mocked, tagged with `<MockValue>` (OPEN_QUESTIONS #3) |
| f | Unique traders in the last 24h | Activity page hero | mocked, tagged with `<MockValue>` (OPEN_QUESTIONS #3) |

**Needs (raw data already on `account_id` of `RestingOrder` / `OrderSubmission` / `WitnessOrder` / `Rejection` / `Fill`):** per-market all-time (a); per-event count via union over the event's markets (b); per-block placers snapshot + per-market breakdown (c); open-batch placers per market on demand (d); platform all-time (e); platform 24h (f).

**Backend sketch.** New `TraderTracker` (mirroring `PriceTracker`):

- `per_market: HashMap<MarketId, HashSet<AccountId>>` → powers (a), and (b) via union on demand
- `platform: HashSet<AccountId>` → powers (e)
- `hourly_buckets: VecDeque<(hour_start_ms, HashSet<AccountId>)>` (cap 25) → powers (f)

Updated at the two admission sites — `try_admit_direct` (`sequencer.rs:1058`) and the admission loop in `produce_block_in_place` (`sequencer.rs:1772`). (c) is captured during block production from `witness_orders` and emitted once on the block. (d) is computed on demand by iterating `order_book.market_orderbook(m)` plus `pending_bundles` filtered to `m`; no persistent state.

**Edges:**
- Open-batch count (d) for a market with active bundles: include the bundle's account_ids — the user who submitted one second before block production has, mentally, "placed an order this batch".
- Per-event count requires union; summing per-market counts over-counts (one trader can trade two markets in the same event).
- Never serialize a `HashSet<AccountId>` to the wire — counts only (privacy + payload size).
- Memory bounded by participants, not activity: tens of MB at Polymarket scale.
- 24h at ±1h resolution is plenty for a dashboard. Drop to 5-min buckets (288 of them) if exact-now-minus-24h is ever needed.

**Wire:** see inventory below. (a, e, f) via `MarketResponse.trader_count` + `GET /v1/activity/overview`; (b) via `GET /v1/events/{event_id}/traders`; (c) via `BlockResponse.unique_placers` (platform scalar) + `BlockResponse.by_market[mid].placers`; (d) via `GET /v1/markets/{id}/open-batch`.

**OPEN_QUESTIONS refs:** #2, #3, #7, #8.

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

**Already-existing data:** `Block.total_volume`, `BlockResponse.total_volume_nanos`, `MarketResponse.volume_nanos` (per-market cumulative). `record_block` already computes a transient per-market split per block — today it's discarded after updating cumulative; tomorrow it's also routed into hourly buckets and surfaced on `BlockResponse.by_market`.

**Needs:** rolling 24h windows (per-market + platform), platform all-time sum, per-block per-market split on the wire.

**Backend sketch.** Extend `PriceTracker` (no new file — volume is scalar and lives next to the existing `market_volumes`):

- `platform_volume: u64` — running total
- `hourly_per_market: VecDeque<(hour_start_ms, HashMap<MarketId, u64>)>` (cap 25) → powers (a), (b)
- `hourly_platform: VecDeque<(hour_start_ms, u64)>` (cap 25) → powers (e)
- Methods: `market_volume_24h(m, now_ms)`, `platform_volume_24h(now_ms)`, `platform_volume()`

Update path: in `record_block`, route the already-computed `per_market_volume: HashMap<MarketId, u64>` into the current hourly bucket (insert / roll on `hour_start_ms` change), bump `platform_volume`, bump `hourly_platform`. For (f), surface the per-block per-market volume nested under `BlockResponse.by_market[mid].volume_nanos` (one `by_market` map per block; see Q1 decision in the inventory section).

**Edges:**
- Multi-market + MINT handling per ground rules (no restatement).
- Memory: 24 × active-markets-per-hour × 16 B. Couple MB at most.

**Wire:** see inventory. `volume_24h_nanos` on `MarketResponse` / `MarketSummaryResponse`; per-block per-market volume nested under `BlockResponse.by_market[mid].volume_nanos`; platform numbers on `GET /v1/activity/overview`.

**Last-X-batches strategy:** per ground rules, FE sums per-block emissions up to ring-buffer cap.

**OPEN_QUESTIONS refs:** #3, #4, #5.

### Price change (24h) — extends `PriceTracker`

Co-located with Volume because both extensions live on `PriceTracker`. Closes two mock sites and fixes a silent bug on the working delta.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Primary card 24h Δ (BinaryCard, MultiCard primary outcome) | `binary-card.tsx:253`, `multi-card.tsx:333` | real via `useCardHistory` — but **silently wrong on busy markets** (buffer too short) |
| b | MultiCard sibling-row 24h Δ | `multi-card.tsx:438` | `<MockValue hint="24h delta">` |
| c | OutcomeLegend per-outcome 24h Δ | `outcome-legend.tsx:84` | `<MockValue hint="24h delta — no backend rollup (OPEN_QUESTIONS #3)">` |

**Why (b) and (c) went mock.** Each sibling would need its own `/v1/markets/{id}/prices/history?from_ms=now-24h` round-trip. A 5-outcome MultiCard = 5 fetches per visible card; a 10-outcome legend = 10 per page load. `useCardHistory` notes: *"Each market is its own round-trip until the backend exposes a batched endpoint."*

**Silent bug on (a).** `PriceTracker.price_history` is capped at 2000 points per market and only appends on blocks with fills (`price_tracker.rs:107-128`). For a market with fills every block (2s cadence): 2000 points ≈ **67 minutes**, not 24 hours. FE filters `from_ms = now - 24h` after load, so `points[0]` is "oldest-in-buffer" which on busy markets is ~30 min ago. The "24h delta" silently becomes a much-shorter delta.

**Needs:** server-computed "price 24h ago" per market, so the delta is one subtraction client-side (no history fetch).

**Backend sketch.** Extend `PriceTracker` with an hourly bucket of clearing prices (ground-rules hourly-bucket idiom):

```
hourly_clearing_prices: HashMap<MarketId, VecDeque<(u64 hour_start_ms, Vec<Nanos>)>>  // cap 25
```

Update path in `record_block`: on every block, slot the merged clearing price into the current hourly bucket if no entry exists yet for that hour (**first-of-hour wins** — subsequent prices in the same hour leave the bucket untouched). Roll buckets on hour boundary. Lookup: `price_n_hours_ago(m, n) = the bucket whose hour_start_ms contains now_ms - n*3_600_000` (not approximation; exact bucket lookup).

**Memory.** 25 buckets × (16 B + 2 × 8 B) × N markets ≈ ~3 MB at 5K markets. Trivial.

**Edges:**
- Markets younger than 24h → no 24h-ago bucket → return `None`; FE renders "—".
- Markets with no clearing price yet → `None`.

FE drops `mockDelta` at (b) and (c); the primary card path stops depending on `useCardHistory` for the delta (history endpoint is still used for the sparkline). Compute = `current − snapshot`. No batched history endpoint needed.

**Wire:** see inventory. `yes_price_24h_ago_nanos` / `no_price_24h_ago_nanos` on `MarketResponse` / `MarketSummaryResponse`.

**OPEN_QUESTIONS refs:** none direct; touches the price-history / 24h-delta surface flagged informally in `use-card-history.ts:30` and `outcome-legend.tsx:84`.

### Liquidity (two surfaces, new tracker)

A deliberate, narrow disclosure on top of FBA's blind orderbook: one aggregate scalar per market per batch — total $ value of resting single-market orders whose `limit_price` is within ±band of midprice — averaged over the last 10 batches. No order-level, account, or side info is exposed. **Deliberate privacy choice** (unique among the entries here): the user accepts the residual leak from a public time-series of this scalar combined with the public clearing-price trajectory; document in API docs.

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Liquidity per market (avg of last 10 batches, ±band) | card metric | `<MockValue hint="liq metric — no resting-depth aggregate on wire (OPEN_QUESTIONS #1)">` |
| b | Liquidity per event | derived from (a) | mocked; falls back from (a) |

**Definition / hooks:**

- Midprice for a binary market = `clearing_prices[m].first()` (YES price ≈ implied probability). Markets without a clearing price (never traded) → no metric, return 0 / `None`. Do not fabricate `0.5`.
- Liquidity value = `Σ (order.limit_price × order.max_fill)` over resting orders where `order.num_markets == 1` and `limit_price ∈ [mid − band, mid + band]`.
- Multi-market orders (spreads / baskets) are **excluded** — depth ≠ counter (their `limit_price` is the bundle's total, attributing it to one market is meaningless). They still live in the book and still match; they just don't count in the depth disclosure. Diverges from the ground-rules attribution convention; intentional.
- Hook: end of `produce_block_in_place`, right after `self.order_book.settle(...)` (`sequencer.rs:2008`). One pass over `order_book.resting_orders()`. O(book size).

**Easily-updateable band.** Config knob, not per-request:

- `SequencerConfig.liquidity_band_nanos: u64`, default `50_000_000` (= $0.05 at 1e9 nanos/dollar). Change → redeploy → new band takes effect next block.
- Ship the band on the wire alongside the average so the FE labels it honestly when it changes ("liq within ±$0.05").
- A per-request band would require shipping a small histogram per market on `/v1/markets` (~21 buckets × 8 B × N markets). Skip until needed.

**Backend sketch.** New `LiquidityTracker`:

- `last_n_per_market: HashMap<MarketId, VecDeque<u64>>` (cap 10 per market) — **per-batch ring buffer, not hourly.** Rationale (deeper than "smoothed micro-window"): liquidity is a *function of current book state*, sampled per block; the other rolling aggregates are *event counters* accumulated across blocks. Different semantics → different bucketing is appropriate.
- `band_nanos_at_last_update: u64` — snapshotted alongside the ring. Required because retroactively averaging across a band change would produce a meaningless mixed-band number; readers compare this against the live `SequencerConfig.liquidity_band_nanos` to detect "average is from before the band changed."
- Methods: `record_block(&OrderBook, &clearing_prices, band_nanos)`, `avg_last_n(m, n)`, `current(m)`

Without persistence the ring buffer warms up over 10 blocks on restart — acceptable for a smoothed metric.

**Per-event:** FE sums per-market `liquidity_avg10_nanos` across the event's markets. Additive scalar; no backend per-event endpoint.

**Edges:**
- Thin markets (≤2 orders in band): the aggregate can approximate individual orders — privacy weakens but no individual identifiability. Accept; do not threshold-suppress (creates jumpy "—" in the UI).
- Memory: 10 × N markets × 8 B. Negligible.
- Cost: one O(book size) pass per block. Microseconds.
- Cancellations naturally drop out: the post-settle book snapshot excludes them.

**Wire:** see inventory. `liquidity_avg10_nanos` + `liquidity_band_nanos` on `MarketResponse` / `MarketSummaryResponse`.

**OPEN_QUESTIONS refs:** #1.

### Orders — placed / matched / unmatched (five surfaces, new tracker + `RestingOrder` field)

User's definitions (taken as given):

- **placed** = order live during a batch's settlement (admitted, not rejected, not cancelled before that batch). Counted *per batch* — an order resting for 5 batches counts as 5 placeds across its lifetime.
- **matched** = order received ≥1 fill of qty > 0 at any point. Counted *once per order lifetime*.
- **unmatched** = order exited the book without ever being matched (TTL expiry or revalidate-eviction). Cancellations are **not** unmatched — own category, flowing through the `OrderCancelled` SystemEvent (top-of-plan callout).

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | All-time placed / matched / unmatched (platform) | Activity hero | mocked (OPEN_QUESTIONS #3) |
| b | Last 24h placed / matched / unmatched (platform) | Activity hero | mocked (OPEN_QUESTIONS #3) |
| c | Per-batch placed / matched / unmatched | Activity batch detail | placed + matched real (`order_count`, `orders_filled`); unmatched mocked; per-market split mocked (OPEN_QUESTIONS #5/#8) |
| d | Per-market all-time + 24h placed / matched / unmatched | market detail page | mocked |
| e | Total placed in last X batches for a specific market | `last-batches-disclosure.tsx` | FE sums real per-block placed counts up to ring-buffer cap; mock past it |

**What already exists:** `BlockResponse.order_count` (placed-this-batch) and `BlockResponse.orders_filled` (matched-this-batch) are emitted today. Everything else is missing.

**Lifetime per-order state.** Today `RestingOrder.order.max_fill` is the *remaining* qty (decremented on partial fills), so "have you ever been matched" is unrecoverable. Cheapest fix: add `has_been_matched: bool` to `RestingOrder` (default false; set true in `OrderBook.settle` when this order's `filled > 0`). Persists via the existing `SequencerSnapshot.resting_orders` path. Costs 1 byte per resting order. **Both this and `original_max_fill: u64` (from Small additions #16) need `#[serde(default)]`** so old snapshots / admit-log payloads round-trip — same pattern `expires_at_block` already uses at `order_book.rs:42`. **Ship as one combined "RestingOrder annotations" change** (Q6 decision): same struct, same persistence hook, same snapshot-format coordination — splitting doubles the dance.

**Exit categorization.** Three book methods remove orders — each needs to feed the tracker. Refactor each to return the removed orders (current return type is `()`); the sequencer feeds counts from the two call sites in `produce_block_in_place`.

| Exit | `order_book.rs` | Categorize as |
|---|---|---|
| `expire(height)` (TTL) | `:240` | unmatched if `!has_been_matched`, else already-counted matched |
| `revalidate(...)` (market closed / account insolvent) | `:261` | same logic; "evicted, never matched" → unmatched (flag in API docs) |
| `settle` filled branch (`filled >= max_fill`) | `:420` | matched (mark `has_been_matched=true` if not yet, one matched-count increment) |
| `settle` expired-this-batch branch | `:430` | matched if `filled > 0` this batch OR `has_been_matched`, else unmatched |
| `cancel` | `:370` | neither (cancellation flows through `OrderCancelled`) |

**Backend sketch.** New `OrderStatsTracker`:

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

Updated at the lifecycle hooks above + at admission time for placed counts. **Per-market attribution per ground rules** — for a multi-market order, each active market gets +1 placed/matched/unmatched; platform counter is independent so it does not over-count.

**Edges:**
- MM and MINT handling per ground rules (no restatement).
- Revalidate-evictions bucket as unmatched — they never matched. Strictly the user said "expired"; flag in API docs.
- Cancellations excluded from both matched and unmatched. They flow through `OrderCancelled` (above).
- **Refactor scope is bigger than initially sketched.** Changing the return shape of `expire / revalidate / settle` from `() → Vec<RestingOrder>` touches **4 production sites** (`sequencer.rs:1749` expire, `:1750` revalidate, `:1873` the phantom-fill STP-undo settle, `:2008` post-solve settle) **+ 7 test sites** in `order_book.rs` (around lines 670, 694, 720, 744, 761, 764). All mechanical, but more than two.

**Wire:** see inventory. Per-market totals on `MarketResponse`; per-block per-market splits on `BlockResponse`; platform 24h on `/v1/activity/overview`.

**Last-X-batches per market:** per ground rules — FE sums per-block emissions.

**OPEN_QUESTIONS refs:** #3, #5, #8 (#15 handled by `OrderCancelled`).

### Per-market welfare per batch (one new piece; volume + orders covered elsewhere)

The activity page's per-batch detail wants three per-market metrics: **matched volume**, **welfare**, and **placed / matched / unmatched orders**. Volume + orders are already covered by their entries (`BlockResponse.by_market[mid].volume_nanos` and the `placed / matched / unmatched` fields on the same struct). The new piece is per-market welfare.

Today `BlockResponse.total_welfare_nanos: i64` is one platform-wide number — the sum of `Order.welfare_contribution(fill_price, fill_qty)` across all fills (`crates/matching-solver/src/lp_solver.rs:745`). No per-market breakdown is stored.

**Hook.** Inside `solve_batch_phase` (`sequencer.rs:1430`), right next to the existing `total_volume` computation (lines 1469-1472), accumulate a parallel per-market welfare map using the already-present `order_map` and `Order::welfare_contribution`:

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

Plumb onto `Block.welfare_by_market: HashMap<MarketId, i64>` and surface nested as `BlockResponse.by_market[mid].welfare_nanos: i64` (signed; small negative contributions can appear from solver rounding).

**Hashing.** `Block.total_welfare` is already in the witness (`sequencer.rs:1617`). The per-market split is **off-block** per ground rules. Multi-market attribution + platform-total-stays-authoritative per ground rules (no restatement).

**Cost.** One extra HashMap accumulation per block. Microseconds. No solver work added.

**Edges:**
- MINT-counterparty fills (arb minting) contribute welfare to their order's markets — preserves the existing solver convention.
- Rare negative per-market values from solver rounding: signed accumulator handles natively; FE renders signed.

**Wire:** see inventory. `by_market[mid].welfare_nanos: i64` on `BlockResponse`.

**OPEN_QUESTIONS refs:** #4 (#5 / #8 covered by Volume + Orders entries).

### Indicative price + indicative volume (current open batch)

**Frontend surface (pro trading section, market detail page):**

| # | Surface | Today |
|---|---|---|
| a | Indicative YES / NO price for the current open batch | mocked (OPEN_QUESTIONS #7) |
| b | Indicative volume for the current open batch | mocked (OPEN_QUESTIONS #7) |

**What "indicative" means here.** FBA clearing price is the output of the solver — not midprice-of-best-bid-ask. "If the batch settled right now" therefore = run the solver on a snapshot of the current state without committing. Indicative volume = sum of `fill_price × fill_qty` from that speculative solve.

**Backend sketch.** `Solver::solve(&self, problem: &Problem) -> PipelineResult` (`crates/matching-solver/src/solver.rs:14`) is already a pure read-only function — the same call we make in `solve_batch_phase` (`sequencer.rs:1436`). A speculative solve is:

1. **Tier 1, MVP.** Build a `Problem` from `self.order_book.resting_orders()` — skips pending bundles and flash MM liquidity. Accurate for retail flow where most submissions admit directly into the book via `try_admit_direct` (`sequencer.rs:1058`).
2. Call `solver.solve(&problem)`.
3. Extract `pipeline_result.price_discovery.prices` (per market) and `Σ fill_price × fill_qty` (per market) into an `IndicativeSnapshot { yes_price, no_price, volume, computed_at_ms }` keyed by `MarketId`.

**Tier 2 later.** Replicate the admission loop in `produce_block_in_place` (`sequencer.rs:1772`) so pending bundles also count. More code, more accurate when bundles are present.

**Where to run it.** From the actor's existing tick loop (`crates/matching-sequencer/src/actor.rs`, the timer task that drives block production): on each idle tick between real blocks, **clone the speculative `Problem` from `order_book.resting_orders()` snapshot**, dispatch the solve to `tokio::task::spawn_blocking`, and on completion send a `IndicativeUpdate { snapshot }` self-message that updates the cache. `Problem` is cheap to clone; `Solver::solve` is pure; no shared lock, no `AtomicBool` — the actor's mailbox naturally serializes cache updates against block-production ticks. Per-request solving is rejected (N concurrent users on a hot market would trigger N solves).

**Cache location.** `indicative_cache: HashMap<MarketId, IndicativeSnapshot>` lives on `SequencerActorState` (next to `latest_block`), not inside `BlockSequencer`. It's a derived view consumed by HTTP — the pure core stays pure. API reads via a `GetIndicative { market_id }` RPC, mirroring the existing `GetMarketPrices` pattern.

**Fallback semantics:**
- Empty resting book / solver infeasible → both prices `None`, volume `0`.
- Book has orders but no matchable cross at any price → prices fall back to `PriceTracker.last_clearing_prices` (the last committed clearing for the market), volume = 0. Matches the UX: "no fills would happen if this batch settled now, but price stays where it last cleared."

**Off-block.** Cache is derived, not chain state — no persistence needed; refreshes on first tick after restart.

**Edges:**
- **MM liquidity excluded.** Flash, one-shot per real block — we don't know in advance what budget will commit. The indicative shows depth as if no MM activates; realized price can therefore diverge. For Polymarket-quoted markets specifically, MM behavior is deterministic against a fixed reference, so the indicative will be systematically biased against the realized clearing price every block. Either include a deterministic MM snapshot in the speculative solve (Tier 3) or document the known bias on those markets.
- **Pending bundles excluded (Tier 1).** Non-trivial pending-bundle flow makes the indicative stale relative to the next real block.
- **Privacy.** Indicative price ≈ what the next block's `clearing_prices_nanos` will be, shifted forward by 500 ms − 2 s. Marginal additional leak since clearing prices are already public post-settlement. Indicative volume is more sensitive: jumps between ticks reveal individual admission magnitudes. Intrinsic to disclosure; flag in API docs. Optional mitigations (round price to 1¢, floor volume to $10, suppress when book has < N orders) — not enabled by default.
- **Stale read window.** Worst case ~500 ms behind reality. `indicative_computed_at_ms` lets FE surface staleness.

**Wire:** extends `GET /v1/markets/{id}/open-batch` — see inventory.

**OPEN_QUESTIONS refs:** #7.

### Portfolio — PnL split, history, activity (three surfaces, one new tracker)

**Frontend surfaces & today's mock:**

| # | Surface | Where | Today |
|---|---|---|---|
| a | Realized vs unrealized PnL split (hero) | portfolio hero | `<MockValue>` (FE approximates via wrong-on-flips `avgEntryPriceNanos()`; OPEN_QUESTIONS #10 / #11) |
| b | Position enter / exit history with prices | portfolio history tab | derivable from fills, but capped at 200 + no cost-basis context |
| c | Activity feed including cancellations | portfolio activity tab | cancellations only visible if cancelled from this browser (localStorage); cancels-from-elsewhere are invisible (OPEN_QUESTIONS #15) |

#### (a) Realized / unrealized PnL split + cost basis

**Cost-basis model: weighted-average cost (WAC).** Per `(account, market, outcome)` track `cost_basis_nanos`; per account track `realized_pnl_nanos`. Update rules per fill:

- **Opening / scaling same-sign:** `new_basis = (old_basis × old_qty + fill_price × Δqty) / (old_qty + Δqty)`. No realized.
- **Reducing toward zero:** realize `(fill_price − basis) × Δqty` (longs; flip sign for shorts). Basis unchanged.
- **Closing to exactly zero:** realize the same; **reset basis to 0** (fixes the position-flip bug in OPEN_QUESTIONS #10).
- **Flipping through zero:** split the fill — realize against the prior position, start fresh basis with the remainder at `fill_price`.

**Hook sites (two, deliberately separate):**

1. **On every fill** — call `CostBasisTracker.apply_fill(...)` from **inside `FillRecorder.record_fills` (`fill_recorder.rs:76`)**, not as a parallel walk in `settle_batch`. `record_fills` already iterates `position_deltas` from `compute_fill_settlement`; piggybacking shares the walk and prevents the WAC and position state from drifting apart. **MINT guard:** early-return on `fill.account_id == AccountId::MINT.0` at the top of `apply_fill`.
2. **On market resolution** — call `CostBasisTracker.apply_resolution(market, payout_nanos, affected_accounts)` from **inside the `SystemEvent::MarketResolved` arm in `convert_system_event` (`sequencer.rs:355`)**, immediately after `settle_resolution` (`settlement.rs:83`) writes payouts to `account.balance`. Resolution is NOT a fill — it bypasses the fill stream and writes balances directly — so the fill-stream hook above doesn't see it. `apply_resolution` realizes `(payout_nanos − basis) × qty` for every affected position and zeros the basis entries for the resolved market.

**Off-block.** Cost basis is **derivable from chain history** (fills + resolutions), so it lives as an off-block sidecar per ground rules — same property `market_volumes` enjoys.

**Backend sketch.** New `CostBasisTracker`:

```
struct CostBasisTracker {
    basis: HashMap<(AccountId, MarketId, u8), i64>,  // nanos per share
    realized: HashMap<AccountId, i64>,                // running total
}
```

Methods: `apply_fill(account, deltas, fill_price)`, `apply_resolution(market, payout_nanos, affected_accounts)`, `cost_basis(account, market, outcome)`, `realized_pnl(account)`.

**NegRisk minting edge.** When MM triggers self-trade arbitrage minting (`sequencer.rs:425`), MINT is the system counterparty (excluded) but the *user's* side of that fill writes a real position at a synthetic solver-set price. That synthetic price becomes the basis. UX implication: users may see "I bought at $X" where $X looks unusual; document in API docs.

**Computing unrealized.** Derived at `compute_portfolio` time (`portfolio.rs:31`): `Σ over open positions of (current_price − basis) × qty` for longs; flipped for shorts. Reads `last_clearing_prices` + the new tracker.

**Wire surface for (a):** `realized_pnl_nanos` + `unrealized_pnl_nanos` on `PortfolioResponse`; **`avg_entry_price_nanos` on `PositionValueResponse`** (= `cost_basis(account, market, outcome)` for that position; sign already lives in `quantity`). Closes OPEN_QUESTIONS #10.

#### (b) Position enter / exit history

**Today.** `FillRecorder` is in-memory, capped at 5000 fills/account; API replies capped at 200 (OPEN_QUESTIONS #14). Beyond the cap, history is lost. FE-side lifecycle derivation works in principle but uses the wrong-on-flips `avgEntryPriceNanos()`.

**Approach: keep fills as the source, give them durability + cost-basis context.**

- **Persist fills unboundedly.** Move `FillRecorder` from a 5000-cap in-memory ring to a paginated query against a redb table (or similar durable store). Lift the 200-cap on `GET /v1/accounts/{id}/fills`. The cost-basis tracker (a) gives the FE the right basis at each point, so position lifecycle can be derived correctly client-side.
- **Optional convenience** — attach to each `AccountFillResponse`: `cost_basis_after_nanos: Option<u64>`, `realized_pnl_delta_nanos: Option<i64>`. Lets the FE render entry→exit transitions per fill without re-deriving.

**Deferral note.** Going from in-memory ring to durable history touches the redb sidecar and the persistence story (ground rules). Could be staged after (a) ships — until then, history is bounded by the ring cap.

**Alternative (deferred): backend-derived position lifecycle events.** A separate `PositionEvent { kind: Open | Close | Flip, qty, price, basis_at_event, realized_delta, … }` stream pushed per state transition (zero ↔ non-zero, sign flip). Smaller than the fill stream, easier to render. Layer on later if (b)-via-fills proves expensive.

#### (c) Activity feed including cancellations

Consumes `SystemEventResponse { type: "order_cancelled", ... }` — see the top-of-plan "On-chain change" callout for the wire shape and the deploy note.

**Edges:**
- Multi-market cancels → `market_ids` is a list. UI groups by primary market or shows all.
- Partial-fill-then-cancel → `remaining_quantity` is the released remainder. FE computes the filled portion as `original − remaining` once `original_quantity` ships (see Small additions #16).
- No admin-initiated cancel today; if added later, emit the same event with appropriate fields.

#### See also

- **#12 equity-curve time series** — **NOT NOW.** Per-account portfolio-value bucketed during `produce_block`, persisted, exposed via `GET /v1/accounts/{id}/equity?from&to&buckets`. Memory cost ≈ 24 B × buckets × accounts; only viable with persistence. Deferred until the persistence story is sorted out.

**OPEN_QUESTIONS refs:** #10, #11, #12 (NOT NOW), #15 (handled by `OrderCancelled` callout).

### Small additions (#13, #14, #16 — single-field wire changes)

Three trivial single-field additions, grouped for brevity. All off-block, default-zero, no chain impact.

**#13 — first-deposit timestamp** → `PortfolioResponse.first_deposit_ms: u64`
- **Surface:** portfolio hero "since first deposit" copy on the ALL range; today shows static range labels with no anchor date.
- **Hook:** in `fund_account` (`sequencer.rs:857`) / `ingest_l1_deposit` (`sequencer.rs:929`), write the timestamp if not yet set for the account.
- **Storage:** off-block sidecar `HashMap<AccountId, u64>` snapshot/restored alongside `market_volumes`. Adding to `Account` would touch `state_root`.

**#14 — exact trade count** → `PortfolioResponse.total_fill_count: u64`
- **Surface:** portfolio hero "N trades"; today capped because FE pulls `/v1/accounts/{id}/fills?limit=200` and reports `fills.length`, showing "200+" when capped.
- **Hook:** **counter on `FillRecorder`** (decision below). Add `total_count: HashMap<AccountId, u64>` to `FillRecorder`, bump in `record_fills` (`fill_recorder.rs:59`). `FillRecorder` already touches every fill, already snapshots, already keyed by `AccountId` — no new module needed.

**#16 — partial-fill progress** → `PendingOrderResponse.original_quantity: u64`
- **Surface:** portfolio open-orders row wants `filled / size` with a progress bar; today `PendingOrderResponse.remaining_quantity` is the only quantity on the wire.
- **Hook:** add `original_max_fill: u64` to `RestingOrder` (populated in `OrderBook.accept` at admission, never mutated after). Persists in the existing snapshot.
- **Ships as part of the combined "RestingOrder annotations" change** alongside `has_been_matched: bool` from the Orders entry — same struct, same `#[serde(default)]` migration, same review cycle.

**OPEN_QUESTIONS refs:** #13, #14, #16.

---

## Not now

Items surveyed but deferred — corresponding `MockValue` hints in the frontend are tagged `NOT NOW —`:

- **OPEN_QUESTIONS #6** — per-market imbalance (requires `side: String` on `FillResponse`; touches `convert.rs` round-trip from the engine). Mock sites: `batch-detail.tsx:350`, `batch-hero.tsx:157`, `batch-hero.tsx:184` (bar colors), `m-dev/[id]/page.tsx:216`.
- **OPEN_QUESTIONS #9** — `created_at_height: u64` on `MarketResponse` for the exact "batches this market has existed" count (FE currently approximates from `created_at_ms`). Mock sites: `m/[id]/page.tsx:252`, `m-dev/[id]/page.tsx:164`.
- **OPEN_QUESTIONS #12** — per-account equity curve. Bigger effort; depends on the persistence story. Mock sites: `equity-chart.tsx:215`, `portfolio-hero.tsx:79`.

---

## Wire-change inventory

Single source of truth for the API surface change. All additive, all default-zero / default-None / default-empty, no breaking changes. Cross-referenced by the entries above.

### `MarketResponse` / `MarketSummaryResponse` (per-market fields)

All new per-market fields appear on **both** types — `MarketSummaryResponse` is the lighter list payload, `MarketResponse` is the detail payload, but the FE needs every field on cards (which use the list endpoint). Card-relevance is noted per row.

| Field | Type | Default | Source entry | Notes |
|---|---|---|---|---|
| `trader_count` | `u32` | `0` | Traders | card metric |
| `volume_24h_nanos` | `u64` | `0` | Volume | card metric |
| `liquidity_avg10_nanos` | `u64` | `0` | Liquidity | card metric |
| `liquidity_band_nanos` | `u64` | `0` | Liquidity | FE labels with this |
| `yes_price_24h_ago_nanos` | `Option<u64>` | `None` | Price 24h | card delta |
| `no_price_24h_ago_nanos` | `Option<u64>` | `None` | Price 24h | sibling-row delta |
| `orders_placed_total` | `u64` | `0` | Orders | market-detail metric |
| `orders_matched_total` | `u64` | `0` | Orders | market-detail metric |
| `orders_unmatched_total` | `u64` | `0` | Orders | market-detail metric |
| `orders_placed_24h` | `u64` | `0` | Orders | phase 2; ship platform 24h first |
| `orders_matched_24h` | `u64` | `0` | Orders | phase 2 |
| `orders_unmatched_24h` | `u64` | `0` | Orders | phase 2 |

### `BlockResponse` (per-block off-block sidecar fields)

Per Q1 decision (below), per-market scalars are **nested under one `by_market` map**, not eight parallel maps. One key per market on the wire, single FE lookup, `BlockMarketStats` derives `Default` + `Serialize` trivially with `#[serde(default)]` per inner field.

| Field | Type | Default | Source entry |
|---|---|---|---|
| `unique_placers` | `u32` | `0` | Traders (platform scalar, not per-market) |
| `by_market` | `HashMap<String, BlockMarketStats>` | `{}` | Aggregates the 6 per-market scalars below |

```
struct BlockMarketStats {
    volume_nanos: u64,        // Volume
    welfare_nanos: i64,       // Per-market welfare
    placed: u32,              // Orders
    matched: u32,             // Orders
    unmatched: u32,           // Orders
    placers: u32,             // Traders
}
```

Existing per-block scalars (`order_count`, `orders_filled`, `total_volume_nanos`, `total_welfare_nanos`) stay unchanged at the top level.

### `PortfolioResponse`

| Field | Type | Default | Source entry |
|---|---|---|---|
| `unrealized_pnl_nanos` | `i64` | `0` | Portfolio (a) |
| `realized_pnl_nanos` | `i64` | `0` | Portfolio (a) |
| `first_deposit_ms` | `u64` | `0` | Small additions #13 |
| `total_fill_count` | `u64` | `0` | Small additions #14 |

Existing `pnl_nanos` field unchanged — identical to `unrealized + realized`.

### `PositionValueResponse`

| Field | Type | Default | Source entry |
|---|---|---|---|
| `avg_entry_price_nanos` | `u64` | `0` | Portfolio (a) — the cost basis as a positive price; sign already in `quantity` |

### `PendingOrderResponse`

| Field | Type | Default | Source entry |
|---|---|---|---|
| `original_quantity` | `u64` | `0` | Small additions #16 |

### `AccountFillResponse` (optional convenience)

| Field | Type | Default | Source entry |
|---|---|---|---|
| `cost_basis_after_nanos` | `Option<u64>` | `None` | Portfolio (b) |
| `realized_pnl_delta_nanos` | `Option<i64>` | `None` | Portfolio (b) |

### `SystemEventResponse` (new variant — the only on-chain change)

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

See "On-chain change" callout at top.

### New endpoints

**`GET /v1/activity/overview`** — platform aggregates for the activity page hero. Replaces OPEN_QUESTIONS #3.

```jsonc
{
  "all_time": {
    "unique_traders": u64,
    "total_volume_nanos": u64,
    "orders": { "placed": u64, "matched": u64, "unmatched": u64 }
  },
  "last_24h": { /* same shape */ }
}
```

**`GET /v1/markets/{id}/open-batch`** — current open-batch (uncommitted) state per market.

```jsonc
{
  "unique_placers": u32,
  "indicative_yes_price_nanos": Option<u64>,
  "indicative_no_price_nanos": Option<u64>,
  "indicative_volume_nanos": u64,
  "indicative_computed_at_ms": u64
}
```

**`GET /v1/events/{event_id}/traders`** — per-event union. API layer reads `market_ref_data.event_id`, gathers the event's markets, asks the sequencer for the union. Cache hot events ~30s.

```jsonc
{ "trader_count": u32 }
```

---

## Decisions

Resolved after a code-review / lead-architect / lead-Rust-dev specialist pass on `2026-05-14`. Recorded here so the next plan iteration doesn't relitigate them.

**Q1 — per-block per-market sidecar shape.** **Nest into `BlockResponse.by_market: HashMap<String, BlockMarketStats>`.** Wins on every axis: smaller wire payload (one market_id key not six), single FE lookup per market, `BlockMarketStats` derives `Serialize`/`Default` trivially, new metrics added in 6 months are one struct field not one new HashMap. The "old client ignores a field" cost is hypothetical (FE owns both sides; types are regenerated per the project workflow).

**Q2 — tracker co-location + `HourlyBuckets<T>` helper.** **Co-locate yes, abstract no — yet.** Move the new trackers under `crates/matching-sequencer/src/aggregates/` for discoverability. Skip the shared `HourlyBuckets<T>` helper for now: the three uses have subtly different `T` (per-market `HashMap<MarketId, u64>`, platform scalar `u64`, per-market `HashMap<MarketId, Vec<Nanos>>`) and one outlier (Liquidity's per-batch ring), so a generic forces `T: Default + AddAssign + Send + Sync + Serialize` bounds that may not match every update pattern. Build all four trackers first, then extract if the shape is genuinely identical. Premature abstraction here would leak.

**Q3 — trade-count storage.** **`FillRecorder`.** Add `total_count: HashMap<AccountId, u64>` to it, bump in `record_fills`. Already touches every fill, already snapshots, already keyed by `AccountId`. Coupling it to `CostBasisTracker` would entangle an exact-count metric with a derived-value tracker (different rates of change, different concerns).

**Q4 — liquidity rolling-window asymmetry.** **Keep the asymmetry.** Tightened rationale: liquidity is a *function of current book state*, sampled per block — the other rolling aggregates are *event counters* accumulated across blocks. Different semantics → different bucketing. Already reflected in the liquidity entry above.

**Q5 — persistence as prerequisite.** **Two-part rule.**
1. **Per-tracker snapshot/restore plumbing is mandatory** — every new tracker plugs into `SequencerSnapshot` / `RestoredState`, gets a redb `TableDefinition`, write path in `save_block_inner`, read path in `load_state`, layout-version bump. **~5 sites per tracker, ~30 min each — non-trivial, not free.**
2. **UI labels for "all-time" fields gate on production persistence** being enabled (`SYBIL_DATA_DIR` set, redb persisting across restart). Until then, every such field is "since last restart". Front-end ships a single `<RestartCaveatBadge />` component used on every surface that depends on an "all-time" tracker — one badge, not 12 per-field disclaimers. When persistence flips on in prod, drop the badge. This avoids gating six unrelated FE surfaces on a multi-week persistence rollout while keeping users honestly informed.

**Q6 — combine `has_been_matched` + `original_max_fill`.** **Yes, one "RestingOrder annotations" change.** Same struct, same `#[serde(default)]` migration, same snapshot-format coordination. Splitting would double the dance. Already reflected in the Orders + Small-additions entries above.

---

## Log

- `2026-05-14` — scaffolding created.
- `2026-05-14` — entry added: unique trader counts (six surfaces, one tracker).
- `2026-05-14` — entry added: volume metrics (six surfaces, extends `PriceTracker`).
- `2026-05-14` — entry added: liquidity (two surfaces, new tracker + config band).
- `2026-05-14` — entry added: orders placed / matched / unmatched (five surfaces, new tracker + `has_been_matched` field on `RestingOrder`).
- `2026-05-14` — entry added: indicative price + indicative volume (open batch, scheduled speculative solve + cache).
- `2026-05-14` — entry added: activity page per-market per-batch breakdown (only new piece is per-market welfare; volume + orders covered by prior entries).
- `2026-05-14` — entry added: portfolio — PnL split / history / activity (new `CostBasisTracker` + `OrderCancelled` system event; cross-refs OPEN_QUESTIONS #10-#16).
- `2026-05-14` — entry added: price change 24h — server-computed 24h-ago snapshot per market, closes two mock sites + fixes silent buffer-cap bug on the working delta.
- `2026-05-14` — entries added: first-deposit timestamp (#13), exact trade count (#14), partial-fill `original_quantity` (#16) — all trivial single-field additions.
- `2026-05-14` — "Not now" section added: imbalance (#6), `created_at_height` (#9), equity curve (#12). Frontend `MockValue` hints for these tagged `NOT NOW —`.
- `2026-05-14` — plan refined holistically: lifted cross-cutting principles (off-block invariant, attribution rule, MM/MINT inclusion table, hourly-bucket idiom, persistence caveat) into a top "Ground rules" section; promoted `OrderCancelled` as the plan's only on-chain change to a top-level callout; consolidated wire fields into a single inventory at the bottom; re-shaped the activity-breakdown entry to just per-market welfare (since volume + orders are covered elsewhere); grouped #13/#14/#16 as "Small additions"; co-located Volume + Price-24h (both extend `PriceTracker`); added an "Open structural questions" section for specialist review.
- `2026-05-14` — specialist pass complete (code reviewer / lead architect / lead Rust dev). Folded findings: `OrderCancelled.side` now typed `matching_engine::Side` not `String`; on-chain change propagation expanded to 5 sites (added per-account `events_digest` arm at `sequencer.rs:1641-1712` + leaf-encoding arm in `event_schema.rs` at tag byte 5); indicative-solve scheduler relocated to actor tick + `spawn_blocking` with `Problem` clone (was: shared lock + `AtomicBool`); `indicative_cache` relocated to `SequencerActorState` (was: `BlockSequencer`); cost-basis hook clarified to live **inside** `FillRecorder.record_fills` (shares the `position_deltas` walk) plus a separate `apply_resolution` hook in the `MarketResolved` arm of `convert_system_event` (since resolution bypasses the fill stream); `OrderBook` refactor scope corrected to 4 production + 7 test sites; `RestingOrder` new fields flagged as needing `#[serde(default)]` for old-snapshot compat; MM/MINT table grew an Indicative row + MINT early-return guard noted in cost-basis hook; per-market welfare body trimmed of attribution boilerplate; liquidity `band_nanos` rationale stated (band-at-last-update, not duplicated config); price-24h bucket semantics specified as "first-of-hour wins"; persistence per-tracker cost stated as ~5 sites/~30 min each; NegRisk minting cost-basis edge and MM-quoted indicative bias both flagged; Q1-Q6 promoted to "Decisions" section with each resolution recorded (Q1 nest into `BlockMarketStats`, Q2 co-locate but no shared helper yet, Q3 FillRecorder, Q4 keep asymmetry, Q5 two-part rule (plumbing mandatory + label-gate badge), Q6 combined RestingOrder annotations). Wire inventory's `BlockResponse` rewritten: one `by_market: HashMap<String, BlockMarketStats>` replaces 6 parallel maps; `unmatched_count` scalar dropped as asymmetric afterthought.
