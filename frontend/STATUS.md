# Sybil Frontend — Current Status

> Always-current snapshot. Read this first if you're picking up cold.
> Historical plans-of-record live in [`archive/`](./archive).

## TL;DR

- **Branch:** `r/dev` · ahead of origin/main (not pushed)
- **Stack:** Next.js 16.2.4 + React 19 + Tailwind v4 + TypeScript strict
- **Live demo:** `pnpm dev` → http://localhost:3000 · backend at `https://172-104-31-54.nip.io`
- **Built pages:** `/`, `/m/[id]`, `/m-dev/[id]`, `/activity`, `/portfolio`, `/smoke`
- **Backend-data plan:** A1–D1 + E1 all landed on `r/dev` (Phase A scaffold, Phase B off-block trackers, Phase C cost-basis + indicative scheduler, Phase D `OrderCancelled` SystemEvent, console Aggregates tab). Coordinated sequencer + verifier deploy for D1 is pending. See [`BACKEND_IMPLEMENTATION_PLAN.md`](./BACKEND_IMPLEMENTATION_PLAN.md) for the landed-commits table.

## What's built

### Real-time core
- `src/lib/ws/client.ts` — singleton `BlockStream` (versioned envelope; reconnect with `?from_block=lastSeenHeight+1`; expo backoff)
- `src/lib/store/index.ts` — Zustand store. `recentBlocks` ring cap **80** (bumped from 20 for `/activity`). All `*_nanos` parsed to `bigint` at the boundary via `parseNanos`.
- `src/lib/ws/realtime-provider.tsx` — hydration handshake (parallel `/v1/blocks/latest` + `/v1/markets/prices`, seed, then WS connect)

### Pages

| Route | State | Notes |
|---|---|---|
| `/` | ✅ done | Markets index — BinaryCard / MultiCard, category tabs, search/sort, mock metrics underlined |
| `/m/[id]` | ✅ done | Market detail — batch theater, price chart, market rail (degen-rail / batch-hero / next-batch-banner / last-batches-disclosure) |
| `/m-dev/[id]` | 🛠 prototype | Pro/dev view of market detail; numeric panels exposing every mock with hints |
| `/activity` | ✅ done | Hero all-time + 24h pulse strip + batches table + expanded batch detail w/ outcome donut. Lifted from `/activity-dev` (deleted). |
| `/portfolio` | 🟢 mostly done | Hero + positions list + open orders + activity tab + equity chart. PnL split + cost basis + first-deposit + lifetime fill count + partial-fill progress are now real (C1 + B8); cancellations land once D1 is deployed; equity curve still mocked (see `BACKEND_DATA_PLAN.md`). |
| `/smoke` | utility | Wire-things-up debug page |
| `/dev/*` | ✅ done | Dev Zone — restyled port of the sybil-api console (Overview, Markets, Blocks, Aggregates, MM & Accounts, Bot Decisions). Manual snapshot port; re-sync by hand when the console (`crates/sybil-api/static/index.html`) changes meaningfully. Trade tab excluded. |

### Cross-cutting
- `src/lib/format/nanos.ts` — `parseNanos`, `formatDollars`, `formatProbability`, `formatInt`, `formatCompactDollars`, `formatDate`, `formatCentsDelta`. All money math goes through bigint here.
- `src/components/mock-value.tsx` — dotted-underline / pill / tint variants. Every mocked render is wrapped so the user sees at a glance which numbers are placeholders.
- `src/lib/categorize.ts` — frontend display-priority over `MarketResponse.categories` (backend returns all matching buckets; FE picks one to show).
- `src/styles/sybil-tokens.css` — synced from `handoff/tokens/colors_and_type.css` via `pnpm tokens:sync`.

## Backend-data backlog

The data-plan catalog of FE surfaces lives in **[`BACKEND_DATA_PLAN.md`](./BACKEND_DATA_PLAN.md)**; the corresponding 15-step backend implementation plan is **[`BACKEND_IMPLEMENTATION_PLAN.md`](./BACKEND_IMPLEMENTATION_PLAN.md)**. Status:

- **Done (A–E1):** traders, 24h + lifetime volume, price-24h-ago delta, liquidity (last-10 ±band), per-block per-market sidecar (placers / volume / placed / matched / unmatched / welfare), partial-fill `original_quantity`, first-deposit / lifetime-fill-count, cost basis (WAC) with realized + unrealized PnL split, indicative open-batch (C2 shadow-solve), on-chain `OrderCancelled` SystemEvent (D1), Sybil console "Aggregates" tab (E1).
- **Still deferred ("Not now" in the data plan):** per-event imbalance, `created_at_height` (FE approximates from timestamp at the 2s cadence), per-account equity curve.
- **Deploy:** the D1 sequencer + verifier ship is coordinated and pending a follow-up session. Until that ships, the on-chain cancel feed is empty in prod even though the wire variant is live.

End-to-end smoke for every Phase A–D wire field lives in [`scripts/smoke-test.sh`](../scripts/smoke-test.sh).

## Phase 2 status

Polymarket mirror metadata (`event_id`, `event_title`, `event_image_url`, `event_icon_url`, `event_end_date_ms`, market-level image/icon/end-date, `categories`) is **shipped** — fields live on `MarketResponse`, populated by `sybil-polymarket` from `gamma-api.polymarket.com`. Archived plan: [`archive/PHASE_2_PLAN.md`](./archive/PHASE_2_PLAN.md).

## Local-only commits (ahead of origin)

`git push origin r/dev` to publish. CI runs via `.github/workflows/frontend.yml`. Use `git log origin/main..r/dev --oneline | wc -l` for the up-to-date count; the most recent landed work is in `BACKEND_IMPLEMENTATION_PLAN.md`'s landed-commits table (A1 → E1).

## Active design tradeoffs

1. **2s batch cadence** is the source of truth — Framer Motion springs avoided on block-clock animations (linear easing keyed to `block.height`).
2. **u64 / `*_nanos` workaround** — `scripts/patch-bigints.mjs` rewrites the generated OpenAPI schema (`number` → `string` for `*_nanos`). Frontend uses `parseNanos()` and `bigint` exclusively for money. See [`KNOWN_ISSUES.md`](./KNOWN_ISSUES.md) #1.
3. **Off-block storage for mirror metadata** (Phase 2) — `event_id`, `categories`, images, end_date live in `MarketRefData`, not block-hashed `MarketMetadata`. Clean backfill, no hash drift on Polymarket re-tags; verifier can't prove "this market was Sports at block N".
4. **Mock-marker discipline** — every value backed by mock data is wrapped in `<MockValue>`. New `NOT NOW —` prefix flags items deferred per the backend plan's "Not now" section.

## Deferred (not blocking dev work)

- **Real backend domain** — `172-104-31-54.nip.io` is IP-pinned; acceptable while dev-only.
- **Account/wallet** — order entry buttons are placeholders until wallet flow lands.
- **Per-event imbalance / created_at_height / equity curve** — see `BACKEND_DATA_PLAN.md` "Not now" section.

## Context you may need

- **[`BACKEND_DATA_PLAN.md`](./BACKEND_DATA_PLAN.md)** — backend changes catalogued surface-by-surface
- **[`KNOWN_ISSUES.md`](./KNOWN_ISSUES.md)** — active workarounds
- **[`CLAUDE.md`](./CLAUDE.md)** — gitignored session notes (deploy story, prod box, Polymarket findings, branching rules)
- **[`handoff/HANDOFF.md`](./handoff/HANDOFF.md)** — design source-of-truth
- **[`archive/`](./archive)** — completed plans (Phase 2, Activity, Scaffolding) and the original Open Questions doc
- **`docs/architecture/WebSocket Block Stream.md`** — wire format for the live block stream
- **Live demo health check:** `https://172-104-31-54.nip.io/v1/health` → `{"status":"ok","height":...}` (if not, demo VM is down)
