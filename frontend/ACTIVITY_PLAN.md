# Activity page — implementation plan

Scope: build `/activity` end-to-end against the existing backend (no backend changes). Everything that needs data we don't have is mocked behind `<MockValue>`. The four items from `OPEN_QUESTIONS.md` (#3–#6) cover what's mocked and why.

Branch: `r/dev` (long-lived frontend branch). No backend touched.

## 0 · What's already in place

- API client: `openapi-fetch` typed against `src/lib/api/schema.d.ts` — `BlockResponse` exposes everything we need per block (`height`, `timestamp_ms`, `total_volume_nanos`, `total_welfare_nanos`, `fill_count`, `order_count`, `orders_filled`, `clearing_prices_nanos`, `fills`, `rejections`).
- WS client: singleton `BlockStream` (`src/lib/ws/client.ts`) with replay handshake — connect with `?from_block=H` and the server replays every block since.
- Store: `useStore` (`src/lib/store/index.ts`) already has `applyBlock` and a `recentBlocks` ring buffer. **Cap is 20**; activity needs 60.
- Realtime provider: `RealtimeProvider` (`src/lib/ws/realtime-provider.tsx`) handles REST hydration + WS subscribe on app mount — mounted from `app/providers.tsx`.
- Format helpers: `src/lib/format/nanos.ts` (`parseNanos`, `formatDollars`, `formatProbability`, `formatInt`).
- Mocked-value marker: `<MockValue hint="...">` (`src/components/mock-value.tsx`) — dotted underline + tooltip.
- Smoke pattern: `/smoke` (`app/smoke/page.tsx`) is the established "wire-things-up and look at it" page.

## 1 · Phase order (build → test on prototype → migrate to /activity)

The user asked specifically: **prototype on a temp page, then rewrite into `/activity`**. We will use `app/activity-dev/page.tsx` as the prototype route. Once verified end-to-end (visual + numbers match a hand calc), we lift the components into `app/activity/page.tsx` and delete the dev route in one commit.

| Phase | Deliverable | Verification | Backend touched? |
|---|---|---|---|
| **P1** | Data hooks + pure derivers under `src/lib/activity/` | Render numbers on `/activity-dev` raw (no styling); confirm they match a manual calc from `curl /v1/blocks/{h}` for the same heights | No |
| **P2** | Visual components, lifted from `handoff/pages/activity.html` 1:1 | `/activity-dev` styled; matches handoff screenshot zone-for-zone | No |
| **P3** | Expanded `<BatchDetail>` + donut chart | Click any row → detail expands, prev-batch delta computed correctly | No |
| **P4** | Lift to `/activity`, wire into GlobalNav tab, delete `/activity-dev` | Visual smoke from real prod data | No |

---

## 2 · Data layer (P1)

### 2.1 Store change — bump recent-blocks cap

`src/lib/store/index.ts` currently has `RECENT_BLOCKS_CAP = 20`. Activity needs 60. Two options:

- **A (chosen):** bump the global cap to **80** (60 for the table + 20 headroom for live-prepend churn). Memory cost: ~80 × (≈ 3–10 KB per block depending on fill count) ≈ 0.5–1 MB. Fine.
- **B (rejected):** add a separate `activityBlocks` slice with its own cap, dual-write from `applyBlock`. Adds complexity for no real benefit — there's only one `recentBlocks` consumer today, and the 80-cap is tiny.

### 2.2 New module — `src/lib/activity/`

```
src/lib/activity/
  use-activity-overview.ts   // hook: all-time + 24h rollups
  use-batches.ts             // hook: last-60 batches for the table
  use-batch-detail.ts        // hook: one batch's expanded detail (per-market rows)
  derive-overview.ts         // pure: blocks[] → { allTime, last24h, prior24h }
  derive-batch.ts            // pure: blocks[] + marketsById → BatchMarketRow[]
  mocks.ts                   // per-market welfare/imbalance/side splits (see open Qs #4–#6)
```

### 2.3 Backfill strategy — how the table gets to 60 rows

**Default path: WS replay, not REST.** `BlockStream` already supports `seedLastSeenHeight(H) → connect()` — the server replays every block from `H+1` to head over the same socket and emits a `replay-complete` envelope when done (`src/lib/ws/client.ts:151-163`, `:179-185`). One connection, server-side streaming, zero per-request HTTP overhead.

On `/activity` mount:

```ts
// pseudocode in use-batches.ts
const stream = getBlockStream();
const latest = latestBlock?.height ?? null;
if (latest != null && recentBlocks.length < 60) {
  // Force a re-handshake from `latest - 60` so the server replays the window.
  // Existing live subscribers are unaffected — the WS singleton handles its own
  // reconnect; we just bump its baseline.
  stream.seedLastSeenHeight(Math.max(0, latest - 60));
  stream.disconnect();   // triggers reconnect with the new from_block
  stream.connect();
}
```

The 60 replayed blocks land in the store via the existing `applyBlock` dispatcher — no new code path needed.

**Fallback: REST per-height, only when replay can't help.** Two cases:

1. Server returns `1008 "block not found"` (the replay window has been pruned). `BlockStream` already resets to a fresh subscription; the table's `useEffect` then loops over the missing heights with `GET /v1/blocks/{height}`, concurrency capped at 4, each wrapped in `.catch()` so one pruned/410 height doesn't unwind the lot — missing heights render as a gap row.
2. After `replay-complete`, `recentBlocks` still has holes (rare; means the server dropped envelopes mid-replay). Same per-height fetch path.

This keeps the happy path at **one WS reconnect, zero parallel REST calls** — a meaningful win for the 1-vCPU backend. The punch list in `OPEN_QUESTIONS.md` still tracks a paginated `/v1/blocks?limit=N` endpoint as the eventual fix; until it lands the WS approach is good enough.

### 2.4 24h window and "all-time" — concrete decisions

- **Batch cadence is ~2s on this network** (per `frontend/CLAUDE.md`: "2s FBA block cadence"), not the 60s the design doc assumes. That makes 24h = **~43,200 blocks**, and 80-cap ring buffer covers only ~2–3 minutes of history. Client-side 24h math is structurally infeasible regardless of how cleverly we backfill.
- **Last 24h + prior 24h** are mocked with `<MockValue>` until `/v1/activity/overview` lands (OPEN_QUESTIONS #3). The hero pulse strip displays the mocked numbers; the prototype's debug panel shows the honest tiny window ("last 2m 34s · 79 blocks") for sanity-check.
- **All-time** = same story. Mock with the handoff values (`$487.2M`, `18.4K traders`, …); only `totalBatches` and `liveMarkets` come from real endpoints (`/v1/blocks/latest.height` and `/v1/markets/summary` count).
- **Live "this batch is being filled" stats** = fully live today — driven by `useStore.latestBlock`. The hero's batch countdown pill is already wired.

This is the deliberate trade-off: client-side aggregation isn't just expensive (the architecture review correctly killed the 1440-RPC strawman), it's *physically impossible* at 2s cadence within a bounded buffer. The backend rollup endpoint isn't a "nice to have" — it's the only real path.

### 2.5 Per-batch row data — all real, no mocks

Per-batch row data is fully derivable from one `BlockResponse`:

```ts
type BatchRow = {
  height: number;                      // .height
  timestampMs: number;                 // .timestamp_ms
  matchedVolumeNanos: bigint;          // parseNanos(.total_volume_nanos)
  welfareNanos: bigint;                // parseNanos(.total_welfare_nanos)
  ordersPlaced: number;                // .order_count
  ordersMatched: number;               // .orders_filled
  ordersUnmatched: number;             // .order_count - .orders_filled - (.rejections?.length ?? 0)
  marketsTouched: number;              // Object.keys(.clearing_prices_nanos ?? {}).length
  uniqueTraders: number;               // new Set((.fills ?? []).map(f => f.account_id)).size
};
```

`marketsTouched` is "markets that cleared something this block" — close enough to the design's "markets" column. If we want "active markets at the time of this batch" instead, we'd need `MarketResponse.status` at time-of-block which isn't recorded. Either definition is fine; we use the cheap one and document it.

### 2.6 Per-market per-batch detail — mocked where data is missing

`BatchMarketRow` (one row inside `<BatchDetail>`):

| Field | Source | Open Q |
|---|---|---|
| Market title + category | `MarketResponse` from `/v1/markets` (already cached) | — |
| Clearing price | `BlockResponse.clearing_prices_nanos[market_id][0]` | — |
| Δ vs prev batch | `current_block.clearing_prices_nanos[mid] − previous_block.clearing_prices_nanos[mid]` — both already in the ring buffer | — |
| Per-market matched vol | **mocked** — proportional split of `total_volume_nanos` (weight = 1/N for cleared markets; refine when fills carry market_id) | #5 |
| Per-market welfare | **mocked** — proportional to per-market matched vol | #4 |
| Placed / matched count | **mocked** — proportional split of block-level counts | #5 |
| Imbalance | **mocked** — `(hash(mid + block.height) % 200 − 100) / 1000` → ±10% deterministic | #6 |

All four mock values run through one helper `mockPerMarketSplit(block, marketsTouched)` in `mocks.ts` so we replace them in one place when backend lands.

---

## 3 · Visual components (P2 + P3)

Lifted 1:1 from `handoff/pages/activity.html`. CSS tokens already live in `globals.css`.

```
src/components/activity/
  hero-all-time.tsx       // editorial-scale matched volume + 4-cell grid
  pulse-strip.tsx         // 5-cell 24h row with ±% deltas
  batches-table.tsx       // sticky-header table with expandable rows
  batch-row.tsx           // one row (collapsed)
  batch-detail.tsx        // expanded inline panel: market rows + right sidebar
  market-row.tsx          // one row inside batch-detail
  donut-outcome.tsx       // matched/unmatched donut, SVG
  composition-kv.tsx      // KV list inside the right sidebar
```

Rules:
- All inline-styled with CSS vars (matches the smoke + markets pattern).
- Tabular numerals on every number (already on `.text-mono`).
- Every mocked number wrapped in `<MockValue hint="…">`.
- No new third-party deps. Donut is plain SVG — `Sparkline.tsx` already shows the pattern.
- Grid template for `<BatchesTable>` comes verbatim from `activity.html`:
  `24px 120px 130px 80px 130px 120px 80px 1fr` with `gap: 28px`.

---

## 4 · Routing / shell (P4)

- **Prototype:** `app/activity-dev/page.tsx` — uses real hooks, real store, full visual.
- **Lift:** copy `activity-dev/page.tsx` → `activity/page.tsx`, delete `activity-dev/` in the same commit.
- **Nav:** `global-nav.tsx` already has "Activity" as a tab — currently it doesn't route anywhere. Wire `next/link` `href="/activity"`.

`app/providers.tsx` already mounts `RealtimeProvider`. No provider tree changes.

---

## 5 · Tests / verification

The frontend has **no test framework set up** (no Jest / Vitest / Playwright — confirmed via `find … -name "*.test.*"` returning only `node_modules`). Verification is manual via the prototype page, matching the convention used for `/smoke` and the markets page.

Checklist before lifting `activity-dev` → `activity`:

1. **Live WS hookup** — open `/activity-dev` against prod (`https://172-104-31-54.nip.io`). Within ~60s the top row of `<BatchesTable>` should swap to a new block; the live batch countdown pill in nav should tick.
2. **Backfill** — open with empty store (hard reload). Within 5–10s the table should fill to 60 rows.
3. **Per-batch numbers match a hand calc** — pick a height from the table, `curl /v1/blocks/{h}`, eyeball: `total_volume_nanos` → matches "matched volume" cell; `order_count − orders_filled − rejections.length` → matches "unmatched"; etc.
4. **Δ on per-market row matches** — same drill on two consecutive heights.
5. **Mocks are visibly marked** — uptime, welfare per market, etc. all show the dotted underline + tooltip.
6. **No console errors** during a 5-minute soak as new blocks roll in (specifically: bigint serialization, missing `clearing_prices_nanos` on bridge-only blocks).
7. **`pnpm lint` + `pnpm build` clean** before merge to `r/dev`.
8. **Bigint pitfalls** — every per-row arithmetic goes through `parseNanos`. Acknowledge the wire-level precision bug (`KNOWN_ISSUES.md`) and don't paper over it.
9. **`?debug=1` HUD** — gate a small fixed-bottom-right panel on `useSearchParams().get("debug") === "1"` showing: `connection.state`, `recentBlocks.length`, count of `lagged`/`block-not-found`/`reconnecting` transitions seen this session. Subscribe to `BlockStream`'s existing events (`stream.on("connection", …)`, `stream.on("lagged", …)`) — no new instrumentation needed on the server. This is the cheapest "is the page healthy?" signal and survives into prod.
10. **Soak the failure paths.**
    - Kill the WS connection in devtools → confirm the disconnected banner appears and the table keeps showing the last known data.
    - Backfill against a pruned window: temporarily seed `lastSeenHeight = 1` before connect → confirm the page reaches `failed`, then re-hydrates from REST and recovers.
    - Empty deployment: point at a fresh wipe → confirm the empty state renders, not a half-filled table.

If we want lightweight automated coverage later, the right scope is **unit-testing the pure derivers** (`derive-overview.ts`, `derive-batch.ts`, `mocks.ts`) with Vitest — they're side-effect-free and the logic is non-trivial (bigint sums, time-window filtering, deterministic mock splits). Out of scope for this PR; tracked as a follow-up.

---

## 6 · Risks / things that could bite

1. **Bridge-only blocks** — some `BlockResponse` payloads may have no `clearing_prices_nanos` and zero fills (block produced for bridge events alone). The derivers must handle "empty match" gracefully; rows should still render with zeros, not crash.
2. **`fills[]` size for popular blocks** — a busy block could carry hundreds of `FillResponse` entries (50–200 KB JSON each). At cap 80 the store can carry **30–80 MB** of block payloads steady-state once you include parse overhead — fine on a laptop, visible on a 4 GB phone. Keep derivation in the hook layer; never pass raw fills into the render tree. If memory becomes a concern, the cheap fix is to drop `fills` and `rejections` on blocks older than position 1 in the ring buffer — the table only needs them for the currently-expanded row, which we re-fetch via `GET /v1/blocks/{h}` on expand anyway.
3. **Ring buffer is the single source of truth.** Cap raised to 80 (60 displayed + 20 headroom for live churn). The `<BatchesTable>` reads `recentBlocks` directly. No React-Query-vs-store merge layer — anything not in the buffer gets fetched on-demand via `GET /v1/blocks/{h}` and **also** dispatched into the store via `applyBlock`, so there's exactly one place state lives.
4. **`@/` import alias** — confirm `tsconfig.json` paths includes `"@/*": ["src/*"]` (it does in markets code). Use throughout.
5. **Number coercion** — `parseNanos` accepts `string | number | bigint` but **cannot recover precision lost on the wire**. The OpenAPI types declare `*_nanos: string` but the live API still serializes u64 as a JSON number, so `JSON.parse` truncates anything `>2^53` (≈ $9M per block) before our code ever runs. This is an existing repo-wide bug tracked in `KNOWN_ISSUES.md` and is **out of scope for this PR**. For Activity the practical effect is: a single block with matched volume >$9M will round in the low cents — visible only in totals, not the per-batch UI. Real fix is server-side (utoipa → emit u64 as JSON string).
6. **Stale React Query cache when a block is re-fetched** — heights are immutable once committed; `staleTime: Infinity` is correct for `/v1/blocks/{h}`. Use it.
7. **WS in `failed` state on Activity mount.** `BlockStream` can be `idle` / `failed` / `reconnecting` when the user navigates to Activity (see `selectConnection`). The page must render a banner ("live updates disconnected · retrying") instead of waiting silently; the batches table still renders from `recentBlocks` if there's anything there. Don't gate the page on `connection.state === "live"`.
8. **Replay window pruned mid-session.** If the user backgrounds the tab past the server's replay window, the next reconnect closes with code 1008. `BlockStream` retries once then surfaces `failed` and calls `resetForFreshSnapshot()` — which wipes `recentBlocks`. The Activity hook must detect this transition (`connection.state === "failed"` with previously-`live` history) and trigger a fresh REST backfill from `/v1/blocks/latest`, not assume the store is still warm.
9. **`lagged` envelope leaves a permanent hole.** If the server emits `lagged` mid-session, blocks between `lastSentHeight` and what we'd see next are dropped on the floor. Mitigation: subscribe to the `lagged` event in `use-batches.ts` and trigger a per-height REST fetch for the gap `[lastSeen+1 .. envelope.last_sent_height]` before the next live block arrives.
10. **Empty-deployment case.** Just after a fresh prod wipe, the store has 0–3 blocks and `latestBlock?.height` may be `0`. The seed-and-reconnect dance in §2.3 must short-circuit when `latest < 1`. The page renders an explicit "no batches yet" state, distinct from the "loading" state.
11. **New block lands mid-backfill.** WS pushes `latest+1` into the store while the (rare) REST fallback is fetching `latest-59..latest`. The deriver must sort by height desc + dedup before slicing to 60, and the table's React `key` is the height. Easy to get wrong; covered by the unit test for the deriver (deferred — see §5).
12. **No deploy target exists yet.** The frontend has no `vercel.json`, no deploy workflow — `frontend/SCAFFOLDING.md` says Vercel is deferred. Today the page only "ships" via `pnpm dev` against prod API. That's not blocking this PR but the merge to `r/dev` does not make the page user-visible. Wiring a deploy target is a separate PR.

---

## 7 · Out of scope (explicitly)

- Backend changes of any kind. Items in `OPEN_QUESTIONS.md` track the backend asks.
- Search box in nav (`⌘K`) — visual only for now.
- "Show 10 more" inside the expanded batch detail (handoff shows it; ship the first batch of rows on expand, defer pagination).
- Mobile breakpoints.
- A real `/v1/activity/overview` endpoint. Mock the hero + pulse strip with handoff-equivalent values until that lands.
- **Fixing the wire-level u64-as-JSON-number precision bug.** Repo-wide, pre-existing, tracked in `KNOWN_ISSUES.md`. Real fix is on the backend (utoipa). Doing it in this PR conflates transport-layer with feature work.
- **Wiring a real deploy target** (Vercel, etc.). The merge to `r/dev` does not put `/activity` in front of users — it only enables `pnpm dev` against prod API. Wiring deploy is its own PR.
- **Automated test framework setup.** Verification is manual via the prototype page. The pure derivers (`derive-overview.ts`, `derive-batch.ts`, `mocks.ts`) are written to be easy to unit-test later when Vitest lands.

---

## 8 · Commit boundaries

Suggested commits (each one runs `pnpm build && pnpm lint` clean):

1. `store: bump RECENT_BLOCKS_CAP to 80 for activity page`
2. `activity: pure derivers (no UI yet)` — `derive-*.ts`, `mocks.ts`
3. `activity: hooks` — `use-activity-overview.ts`, `use-batches.ts`, `use-batch-detail.ts`
4. `activity: prototype page at /activity-dev` — raw rendering of derived data, no styling
5. `activity: visual components` — `hero-all-time`, `pulse-strip`, `batches-table`, `batch-row`
6. `activity: expanded batch detail + donut`
7. `activity: lift /activity-dev to /activity; wire nav tab; delete prototype`
8. `OPEN_QUESTIONS: tighten endpoint asks once page is real` (anything we learned while building)
