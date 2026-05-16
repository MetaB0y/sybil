# Dev Zone — design spec

**Date:** 2026-05-16
**Status:** Approved (design); implementation plan pending
**Surface:** `frontend/web` (Next.js 16 / React 19 / Tailwind v4)

## Goal

Make everything currently visible in the Sybil **console** (`crates/sybil-api/static/index.html`,
served at the root of `https://172-104-31-54.nip.io`) available inside the customer-facing
frontend, under a new **Dev Zone** section in the header.

The console is a single ~1,700-line Alpine.js HTML file. It has tabs: Overview, Markets,
Blocks, Aggregates, MM & Accounts, Bot Decisions, Trade. The Dev Zone reproduces all of
these **except Trade**.

Both the console and the frontend already consume the same public REST API and the same
`/v1/blocks/stream` WebSocket — the console just renders that data differently.

## Decisions (locked)

| Decision | Choice |
|---|---|
| Build approach | **Port HTML, restyle** — copy the console's markup/logic, convert Alpine to React, restyle with frontend design tokens. |
| Routing | **One Next.js route per subsection** under `app/dev/`. Deep-linkable, browser back/forward works, lazy-loaded per tab. |
| Data layer | **Reuse the existing singleton block stream + Zustand store** for live block data; all dev-specific fetching and UI stay isolated. One WebSocket connection. |
| Visibility | **Always visible to everyone** — Dev Zone sits in the header next to Markets / Activity / Portfolio. |
| Trade tab | **Out of scope.** |

## Subsections

The Dev Zone has six subsections, matching the console's tabs (console internal id in parens):

1. Overview (`overview`)
2. Markets (`markets`)
3. Blocks (`blocks`)
4. Aggregates (`aggregates`)
5. MM & Accounts (`participants`)
6. Bot Decisions (`bots`)

## Architecture

### File layout — isolation

All Dev Zone code lives under `*/dev/` paths. Only **two** existing files are modified:
`src/components/global-nav.tsx` (one new import) and a read-only consumer of the existing
store/WS hook.

```
frontend/web/src/
  app/dev/
    layout.tsx            # Dev Zone shell — sub-nav + status line
    overview/page.tsx
    markets/page.tsx
    blocks/page.tsx
    aggregates/page.tsx
    accounts/page.tsx     # "MM & Accounts"
    bots/page.tsx         # "Bot Decisions"
  components/dev/
    dev-zone-nav.tsx      # expandable header dropdown
    primitives/           # ported console chrome: Panel, StatGrid, DataTable, Pill
    overview/ markets/ blocks/ aggregates/ accounts/ bots/   # per-tab components
  lib/dev/
    fetchers.ts           # dev-only API calls
    format.ts             # ported money() / moneySigned() helpers
```

### Header entry

Add a "Dev Zone" entry to the header alongside Markets / Activity / Portfolio. It renders
as an **expandable dropdown** using Radix `DropdownMenu` (already a dependency) listing the
six subsections. Active state when `pathname.startsWith("/dev")`. The change to
`global-nav.tsx` is limited to importing and rendering one new `<DevZoneNav>` component.

### Dev Zone shell — `app/dev/layout.tsx`

A persistent **sub-nav row** under the global header showing the six subsections as tabs,
so once inside the Dev Zone the user can switch quickly without reopening the dropdown —
this mirrors the console's `nav` row. The shell also carries a status line (block height,
live/stale dot) ported from the console topbar.

### Porting strategy

For each console tab:

- Lift the existing markup into a React component.
- Convert Alpine directives (`x-show`, `x-text`, `x-for`, `@click`, `x-model`) to JSX +
  React state.
- Replace console CSS classes (`.panel`, `.grid.stats`, `.grid.two`, `.table-wrap`, etc.)
  with restyled equivalents driven by `src/styles/sybil-tokens.css`. The console's color
  variables (`--bg`, `--surface`, `--green`, `--red`, `--blue`, `--yellow`, `--cyan`,
  `--muted`, `--dim`) map onto the frontend's token names.
- The result keeps the console's information layout but adopts the frontend's color scheme,
  type, spacing, and component feel.

A small shared set of ported **primitives** (`Panel`, `StatGrid`, `DataTable`, `Pill`) under
`components/dev/primitives/` keeps all six subsections visually consistent and on-brand.

### Data layer — `lib/dev/`

- **Live block data** — used by Blocks, Overview, and the per-block rows in Aggregates.
  Read from the existing Zustand store / singleton block stream via a hook. **No second
  WebSocket connection.**
- **Request/response data** — markets summary, account portfolios, bot decisions, activity
  overview, open-batch indicative snapshot. Fetched via `lib/dev/fetchers.ts`, wrapped in
  React Query (already a dependency) for caching and interval refresh at the console's
  current poll cadence.
- **Formatting** — the console's `money()` / `moneySigned()` and related helpers are ported
  into `lib/dev/format.ts`, kept separate from `lib/format/nanos.ts` to honor the isolation
  requirement, even though they overlap functionally.

Endpoints in use (from the console): `/v1/health`, `/v1/markets/summary`,
`/v1/markets/groups`, `/v1/orders/pending`, `/v1/blocks/latest`, `/v1/blocks/{height}`,
`/v1/blocks/stream` (WS), `/v1/accounts/{id}/portfolio`, `/v1/accounts/{id}/fills`,
`/v1/bots/decisions`, `/v1/activity/overview`, `/v1/markets/{id}/open-batch`.

## Error / empty states

- **Bot Decisions** depends on the arena decision database being mounted. The console
  exposes `botDbAvailable` / `botDbError`; the Dev Zone reproduces a graceful unavailable
  state rather than erroring.
- **MM & Accounts / Aggregates portfolio panel** depends on an account being loaded — show
  an empty prompt, matching the console.
- Per-tab fetch failures surface inline (panel-level), not as a full-page error.

## Out of scope

- The console's **Trade** tab and any order submission / signing flow.
- Any backend or API change — the Dev Zone consumes the existing API unchanged.

## Risks

- **Drift.** The console is the backend owner's surface and will evolve. The "Port HTML,
  restyle" copy is a manual snapshot and will diverge over time. This is recorded in
  `frontend/STATUS.md` as a known, deliberate trade-off: the Dev Zone is re-synced by hand
  when the console changes meaningfully.
- **Live-data coupling.** Reusing the shared store couples the Dev Zone to the store's
  shape. Mitigated by reading through a dedicated hook so a store refactor has one
  touchpoint.

## Testing

- Unit tests for `lib/dev/format.ts` helpers (money formatting parity with the console).
- Component tests for empty / unavailable states (Bot Decisions DB missing, no account
  loaded).
- Manual parity pass: each Dev Zone subsection against the live console tab.
