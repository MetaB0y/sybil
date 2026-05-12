# Sybil Frontend — Current Status

> Always-current snapshot. Read this first if you're picking up cold.
> Plan-of-record (decisions + rationale) lives in `SCAFFOLDING.md`.

## TL;DR

- **Branch:** `r/dev` · **Commits ahead of origin:** 14 (not pushed; nothing on GitHub yet)
- **Stack:** Next.js 16.2.4 + React 19 + Tailwind v4 + TypeScript strict
- **Live demo:** `pnpm dev` → http://localhost:3000 · backend at https://172-104-31-54.nip.io
- **Built pages:** `/` (all markets), `/m/[id]` (market detail), `/smoke` (debug)
- **Dev server may be running** in a background task started by Claude. If `curl http://localhost:3000` 200s, it's up.

## What's built

### Real-time architecture (Milestones A → B → C — all done)
- **`src/lib/ws/client.ts`** — singleton `BlockStream` class. Versioned envelope per `docs/architecture/WebSocket Block Stream.md`. Reconnects with `?from_block=lastSeenHeight+1`. Drops state + retries fresh on "block not found". Exponential backoff (1s → 30s). visibility-change listener.
- **`src/lib/store/index.ts`** — Zustand store. Slices: `connection`, `hydration`, `latestBlock`, `recentBlocks`, `pricesByMarketId`. All `*_nanos` parsed to **bigint** at the boundary via `parseNanos`.
- **`src/lib/ws/realtime-provider.tsx`** — mounted in `app/providers.tsx`. Hydration handshake: parallel-fetch `/v1/blocks/latest` + `/v1/markets/prices`, seed store, `stream.seedLastSeenHeight(H₀)`, then `stream.connect()`. Eliminates flicker on first load.

### Markets index `/` (C.1 → C.5 — all done)
- `src/components/global-nav.tsx` — fixed 56px, wordmark + TESTNET pill + route tabs + batch pill + disabled connect button
- `src/components/batch-pill.tsx` — height + 2s linear progress bar keyed to `block.height` (not wall-clock)
- `src/components/clearing-ticker.tsx` — sticky 36px strip under nav showing markets that cleared this block (sorted by absolute distance from 50%)
- `src/components/binary-card.tsx` — 5-row card skeleton (meta · title · featured YES · YES/NO bars · footer)
- `src/components/multi-card.tsx` — collapses event groups (>5 children) into one card with top-4 outcomes + "+N more"
- `src/components/markets-filter-bar.tsx` — search input (filters titles), Volume/Name/Outcomes sort chips
- `src/lib/markets/use-markets.ts` — TanStack Query for `/v1/markets` + `/v1/markets/groups`, assembled into `{ byId, groups, ungrouped, total }`

### Market detail `/m/[id]` (D.1 → D.3 — all done)
- `src/app/m/[id]/page.tsx` — uses `use(params)` for Next 16 async params. 2-col layout (main + 360px right rail).
- `src/components/batch-theater.tsx` — right-rail showpiece. Huge live YES probability, 2s progress bar, connection state pulse, "cleared this block" KV grid, disabled order buttons. Sticky on scroll.
- `src/components/price-chart.tsx` — TradingView Lightweight Charts v5 area series. History seeded from `/v1/markets/{id}/prices/history`, live ticks via `useStore.subscribe` outside React's render path.
- `src/components/pending-orders-feed.tsx` — orders queued for the next batch in this market. Invalidates per block.
- `src/lib/markets/use-market.ts`, `use-price-history.ts`, `use-pending-orders.ts` — colocated TanStack Query hooks

### Cross-cutting
- `src/lib/format/nanos.ts` — `parseNanos`, `formatDollars`, `formatProbability`, `formatInt`, `formatCompactDollars`, `formatDate`. **All `*_nanos` math uses bigint via these helpers.**
- `src/styles/sybil-tokens.css` — synced from `handoff/tokens/colors_and_type.css` via `pnpm tokens:sync`. Handoff folder is untouched (design source of truth).
- `src/app/globals.css` — selective Tailwind v4 `@theme` (colors, fonts, radii). Shadows / motion / blurs use raw `var(--…)`.
- `src/app/{error,loading,not-found}.tsx` — Next 15+ boundary files in Sybil voice.

## Pages still to build

Pre-existing 4-page plan from `frontend/handoff/HANDOFF.md`:

| Route | Status | Notes |
|---|---|---|
| `/` | ✅ done | Markets index — feature complete |
| `/m/[id]` | 🟢 mostly done | Missing: recent fills feed (D.4 — needs per-account aggregation since block FillResponse lacks `market_id`), real order entry (needs wallet) |
| `/activity` | ❌ not started | Handoff: hero all-time stats + 24h pulse + scrollable batches table with expandable rows. Data: `/v1/blocks/*` |
| `/portfolio` | ❌ not started | Needs an account model first. `/v1/accounts/{id}/portfolio` exists. |

## Open commits (local-only, on `r/dev`)

```
b2c76e8 frontend: pending-orders feed for next batch (D.3)
611c7c4 frontend: price chart on market detail (D.2)
3d66517 frontend: market detail page shell + batch theater (D.1)
6ab9261 frontend: clearing ticker (C.5)
f5b1dad frontend: search + sort filter row (C.4b)
65dbadb frontend: MultiCard for multi-outcome events (C.4a)
3096ea4 frontend: BinaryCard + 3-col grid (C.3)
c0008dd frontend: markets list grouped by event (C.2)
20d0a08 frontend: global nav + batch pill (C.1)
8983894 frontend: REST hydration + height handshake (Milestone C)
c709063 frontend: Zustand store + RealtimeProvider (Milestone B)
e69c74b frontend: WS client + reconnect state machine (Milestone A)
720bc86 ci: add Frontend CI workflow
6068a52 frontend: scaffold Next.js web app
```

`git push origin r/dev` to publish. CI will run via `.github/workflows/frontend.yml`.

## Decisions still active (from SCAFFOLDING.md)

1. **2s batch cadence** is the source of truth — frontend adapts. Copy that says "every 60s" replaced; Framer Motion springs avoided on block-clock animations (linear easing keyed to `block.height`).
2. **shadcn kept** — copies into the repo as plain `.tsx` files, restyle aggressively per component. We haven't pulled any shadcn components yet (no buttons / dialogs / etc. needed so far). Falls back to raw Radix if a specific component fights us.
3. **u64 / `*_nanos` workaround** — `scripts/patch-bigints.mjs` rewrites generated schema; see `KNOWN_ISSUES.md`. Backend `utoipa` fix tracked separately. Frontend code uses `parseNanos()` and `bigint` exclusively for money.

## Active design tradeoffs (revisit later)

Phase 2 (Polymarket mirror metadata enrichment — event/market images, end dates, category-derived-from-tags) is planned in `PHASE_2_PLAN.md` (in this folder). **Paused, not started** — blocked on prod SSH access and on the API container recovering from OOM. Five design choices are knowingly "good for now, not sure long-term". Full rationale + revisit triggers in `KNOWN_ISSUES.md` #2. Headline calls:

1. **Off-block storage** — image/category/tags/end_date live in `MarketRefData` (mutable, off the block hash), not `MarketMetadata` (block-committed). Cleaner backfill, no hash drift on Polymarket re-tags, but a verifier can't prove "this market was Sports at block N".
2. **`end_date` is display-only** — Polymarket's `endDate` is the *expected* resolution date, not a trading cutoff. We don't route it through the matching engine's `expiry_timestamp_ms`. The resolution actor remains the only thing that closes mirrored markets.
3. **Backfill is one-shot, not recurring** — `sybil-polymarket --backfill-metadata` is run manually; Polymarket re-categorizations don't auto-propagate.
4. **Tag→category map is hardcoded in code** — new tags fall to "Other" until we extend the table.
5. **`MarketRefData` persists to JSON on disk** — mirrors the `MappingStore` pattern; write-amplifies on every metadata POST (rare today).

## Deferred (not blocking dev work)

- **Real backend domain** — `172-104-31-54.nip.io` is IP-pinned. Acceptable while dev-only. Revisit before any public preview.
- **Vector logo** — handoff ships raster PNG only. Currently using the wordmark in Syne; no mark image needed yet.
- **Account/wallet** — order entry buttons are disabled placeholders. `account_id` ghost-identity guard to be wired when login lands.
- **24h delta, trader count, liq, sparkline-per-card** — backend doesn't expose these. Cards have placeholder slots structured to accept them later.
- **Category tabs on markets page** — backend `category` field is always null. Search + sort fill in for now.
- **Recent fills feed on market detail** — block-level `FillResponse` doesn't carry `market_id` (only `AccountFillResponse` does). Workaround: maintain a derived index in the store (subscribe per-account?), or wait for backend to enrich.

## Suggested next steps

Pick one:

**A) `/activity` page** — most product surface area added per commit. Hero all-time stats + last-24h pulse strip + scrollable batches table. Data: poll `/v1/blocks/latest` for ongoing + `/v1/blocks/{height}` for backfill. Per the handoff, this is page #3 of 4.

**B) Recent fills feed on market detail (D.4)** — polish. Pulls fills from store's `recentBlocks`, filters/correlates by order_id. Probably needs an order_id → market_id index seeded from a `/v1/orders/{id}` lookup or rolling window.

**C) `/portfolio` page** — blocked on account model. Could prototype with a hardcoded `account_id` from localStorage so we can see real positions for, e.g., account 11 (a bot trader).

**D) Push commits + verify CI** — `git push origin r/dev` and confirm `.github/workflows/frontend.yml` runs green. Smart to do before too much more accumulates.

**Recommendation:** D first (1 command, instant confidence in CI), then A (most ship-worthy progress). B and C are polish/blocked respectively.

## Conversation context you may need

- **`SCAFFOLDING.md`** — original plan + decisions log
- **`KNOWN_ISSUES.md`** — the u64 nanos workaround and pending backend ticket
- **`handoff/HANDOFF.md`** — design source-of-truth for the 4 pages
- **`docs/architecture/WebSocket Block Stream.md`** — wire format for the live stream (Rust side)
- **Live demo:** `https://172-104-31-54.nip.io/v1/health` should return `{"status":"ok","height":...}`. If it doesn't, the demo VM is down — the frontend will look the same but the data will be stale or hydration will error.
