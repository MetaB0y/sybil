# Sybil — frontend handoff

This folder is the **design source of truth** for the Sybil exchange web app. Drop it into your frontend repo (e.g. `design/` or `reference/`) and rebuild the four pages inside it using whatever stack you're using (Next.js + Tailwind, Vite + React, SvelteKit, etc.). Don't try to ship these HTML files as production — they're *specs*, not code.

```
handoff/
├── index.html                  ← open this first; links to all 4 pages
├── HANDOFF.md                  ← this file
├── BRAND.md                    ← brand voice, copy rules, visual foundations
├── tokens/
│   └── colors_and_type.css     ← the only token file; import once
├── assets/
│   ├── sybil-mark.png          ← logomark (raster — re-export to SVG if possible)
│   └── favicon.ico
├── pages/                      ← the 4 reference pages
│   ├── markets.html            ← / (all markets index)
│   ├── market-detail.html      ← /m/[id] (trade page)
│   ├── activity.html           ← /activity
│   └── portfolio.html          ← /portfolio
└── data/                       ← JSX modules each page loads
    ├── MarketsData.jsx
    ├── EventsDataV3.jsx
    ├── ActivityData.jsx
    ├── PortfolioData.jsx
    ├── PortfolioPieces.jsx
    ├── PortfolioVariants.jsx
    ├── tweaks-panel.jsx        ← design-tool helper; SAFE TO DELETE in prod
    ├── fed-data.jsx            ← market-detail mock data
    ├── fed-primitives.jsx
    ├── fed-fba-panel.jsx
    ├── fed-right-rail-modes.jsx
    └── fed-variations.jsx
```

To preview the pages exactly as designed, serve `handoff/` over any local server (e.g. `python -m http.server`) and open `index.html`. The pages render via Babel-in-the-browser; this is a design preview mechanism, not the production runtime.

---

## How to use this in Claude Code

When you start in Claude Code, paste this prompt:

> The `handoff/` folder is the design source of truth for our prediction-market frontend. Read `handoff/HANDOFF.md` first, then `handoff/BRAND.md`, then `handoff/tokens/colors_and_type.css`. Then for the page I'm building, read the corresponding HTML in `handoff/pages/` and its imported JSX modules in `handoff/data/`. Lift exact tokens, layout, and copy — don't rewrite from memory.

Claude Code can read the HTML / JSX directly, so just point it at the page you want to implement. Order of operations:

1. **Lift the tokens** — port `tokens/colors_and_type.css` to your framework's token format (Tailwind config, CSS modules, vanilla-extract, whatever). Token names should stay identical.
2. **Pick fonts** — Syne, Inter, JetBrains Mono via Google Fonts. The CSS already has the `@import`; either keep it or move it into your `<head>`/`_app`.
3. **Build the shell** — top nav (56px, fixed, blurred) + page container. All 4 pages share it. Extract it once.
4. **Build one page** — start with `markets.html` (simplest). Read the JSX file, lift the components 1:1, then refactor into your framework idioms.
5. **Replace the mock data layer** — every JSX file in `data/` exports its data to `window.*`. Treat each `window.SOMETHING` as a typed API contract; build a real fetch hook with the same shape.

---

## Page-by-page guide

### 1 · Markets — `pages/markets.html` → route `/`

**Purpose.** Browse all events. 3-column grid of event cards, grouped by category, with a live batch ticker at the top.

**Layout zones (top → bottom).**
1. **Global nav** (`<GlobalNav>` in markets.html) — fixed, 56 px. Logo + `testnet` pill + tabs (Markets / Activity / Portfolio / Docs / Dev zone dropdown) + batch pill + search + wallet button.
2. **Clearing ticker** (`<ClearingTicker>` from `MarketsData.jsx`) — 36 px strip showing last batch # and last 8 markets' clear prices, scrolling horizontally.
3. **Page title** — `All markets` + `// N events · uniform clearing every 60s`.
4. **Category tabs** (`<CategoryTabs>`) — pill row of categories + sort chips (Volume / Top movers / Closing soon / New) on the right.
5. **Card grid** — 3 cols × N rows, fixed 360 px card height. Two card shapes:
   - `<MultiCard>` — for multi-outcome events (e.g. "US 2028 · Democratic nominee")
   - `<BinaryCard>` — for Yes/No events
   Both share the SAME 5-row grid skeleton so they align row-for-row across the page: `[meta · title+image · featured price · 3 outcome rows · footer]`.

**Data contracts (from `data/EventsDataV3.jsx`).**
```ts
type Event = {
  id: string;
  category: 'Politics' | 'Elections' | 'Economy' | 'Tech' | 'Finance' | 'Culture' | 'Climate' | 'Mentions' | 'World' | 'Crypto' | 'Sports';
  title: string;
  resolves: string;        // "Aug 27, 2028"
  vol: string;             // "4.2M"
  vol24: string;           // "184K"
  traders: number;
  type: 'binary' | 'multi';
  outcomes: Outcome[];
};
type Outcome = {
  id: string;
  label: string;           // "Gavin Newsom" or "Yes"
  yes: number;             // probability cents 0–100
  delta24: number;         // +/- pct change last 24h
  vol24: string;           // "84K"
  vol: string;             // "1.2M"
  traders: number;
  liq: string;             // "380K"
};
```
Sparkline series live in `SERIES_V3[outcomeId]` — 48-tick `number[0..1]` arrays. The mock generator (`genSeries3`) is replaceable; in prod, fetch the last 48 batch clear-prices.

**Interaction notes.**
- Card click → navigate to `/m/[event.id]`.
- Category tab + sort chips are independent filters; both should be query params (`?cat=Tech&sort=volume`).
- Batch pill in the nav ticks down 60→0 then resets. In prod, anchor it to the actual next-batch timestamp.

---

### 2 · Market detail — `pages/market-detail.html` → route `/m/[id]`

**Purpose.** The trade page for one event. Right rail is the hero — a giant live batch clock with indicative clearing price, IEV (implied expected value), and imbalance. The left/center columns are chart + outcome list + order book + recent trades + comments.

**Layout zones.**
1. Top nav (same as markets).
2. Breadcrumb / event header (title, category, resolves, vol).
3. **Three-column body**:
   - **Left**: outcome list (large), price chart, recent trades, comments.
   - **Center**: order book (depth ladder), trade history.
   - **Right rail**: **the batch theater** — `<V2BatchTheater>`. Huge live clock, indicative clearing price, IEV bar, batch composition (buys vs sells), then the order-entry form below the fold.

**The data file** (`data/fed-data.jsx`) is keyed for a specific event ("Fed rate decision · March FOMC"). It exports:
- `FED_MARKET` — the event object (richer schema than markets list; includes `outcomes[]` with full price ladder, `vol`, `liq`, `traders`, `resolves`, etc.)
- `getSeriesByOutcome(market)` — returns per-outcome price series for the chart
- `FED_BATCH_HISTORY` — last N cleared batches for the recent-trades panel
- `FED_COMMENTS` — comment thread
- `useBatchSecs()` — hook that returns `secs` countdown (0–60)

**In production**, this is one **event/market endpoint** that returns the event + its outcomes + chart series + last-N-batches + comments, all denormalized for fast render. The `useBatchSecs` hook becomes a SSE/WebSocket subscription to the batch clock.

**The right rail (`<V2BatchTheater>`) is the most-iterated component.** Read `fed-variations.jsx` for its full implementation — it's the single biggest design payoff in the project, and the thing that visually differentiates Sybil from Polymarket / Kalshi.

---

### 3 · Activity — `pages/activity.html` → route `/activity`

**Purpose.** Everything happening on Sybil — all-time stats hero, last-24h pulse strip, table of recent batches with expandable detail.

**Layout zones.**
1. Top nav.
2. Page title + live batch countdown chip on the right.
3. **`<HeroAllTime>`** — editorial-scale matched-volume number on the left, 4-cell stat grid (traders / placed / matched / unmatched) on the right.
4. **`<PulseStrip>`** — 5-cell row for last-24h stats with delta %.
5. **`<BatchesTable>`** — scrollable table, sticky header, custom grid template:
   ```
   .bt-row {
     display: grid;
     grid-template-columns:
       24px         /* expander chevron */
       120px        /* batch # */
       130px        /* cleared timestamp */
       80px         /* markets count */
       130px        /* matched volume */
       120px        /* welfare saved */
       80px         /* traders count */
       1fr;         /* orders cell — placed / matched / unmatched */
     gap: 28px;
   }
   ```
   Click a row → inline-expanded `<BatchDetail>` with tx hash, sequencer, clearing duration, market-level rows, and right sidebar (donut + composition KV).

**Data contracts (from `data/ActivityData.jsx`).**
```ts
type Batch = {
  id: number;
  ts: Date;
  markets: number;
  matchedVolume: number;      // $K
  traders: number;
  ordersPlaced: number;
  ordersMatched: number;
  ordersUnmatched: number;
  detailSeed: number;         // mock-only; remove in prod
};
type BatchDetail = {
  txHash: string;
  blockNum: number;
  sequencer: string;          // "0x4f2c···7a91"
  clearingMs: number;         // 180-420
  marketRows: BatchMarketRow[];
};
type BatchMarketRow = {
  id: string;
  title: string;
  category: string;
  clearPrice: number;         // 0-100 cents
  delta: number;              // -4.0..+4.0
  placed: number;
  matched: number;
  matchedVol: string;         // "12.4" (in $K)
  buys: number;
  sells: number;
};
```

**API shape suggestion.**
- `GET /api/activity/overview` → `{ allTime, last24h }`
- `GET /api/activity/batches?cursor=...&limit=60` → `{ batches: Batch[], nextCursor }`
- `GET /api/activity/batches/:id` → `BatchDetail` (lazy; fetch on row expand)

---

### 4 · Portfolio — `pages/portfolio.html` → route `/portfolio`

**Purpose.** A trader's own positions, queued orders, closed trades, and activity. Includes a portfolio-value hero + equity curve + tabbed holdings.

**Three variants ship in `PortfolioVariants.jsx`** — `<VariantClassic>` (default, equity-first), `<VariantTerminal>` (denser, methodology-forward), `<VariantTwoCol>` (sticky left rail, right column holdings). **Ship `VariantClassic`** unless explicitly told otherwise — the tweaks panel was an exploration tool.

**Layout zones in `<VariantClassic>`** (the page wraps it):
1. Top nav (`<GlobalNav>` in `PortfolioPieces.jsx`).
2. Page title + `// positions · orders · history for 0xA17F···c92E` annotation.
3. **Hero**: identity strip + range picker (24h/7d/30d/all) on top; then a 2-col grid:
   - Left: eyebrow `Portfolio value` → big number → ▲ delta + pct → 2×2 KV grid (positions / cash / unrealized / realized).
   - Right: `<EquityChart>` (svg area chart with deposits marked, net-deposits baseline dashed).
4. (Optional) `<AllocationStrip>` — 1-row stacked bar of category allocation.
5. **`<HoldingsTabs>`** — tabbed table: Positions / Open orders / History / Activity. Each tab has its own grid template (column widths) — see `PortfolioVariants.jsx` for exact values. **Don't reflow these column widths**; the alignment is part of the design.

**Data contracts (from `data/PortfolioData.jsx`).**
```ts
type Trader     = { address, short, alias, joined, tier:'A'..'E', rank, pctile };
type Portfolio  = { totalValue, cash, positionsValue, netDeposits,
                    unrealizedPnL, realizedPnL, totalPnL, pnlPct,
                    pnl24h, pnl7d, pnl30d, pnlPct24h, pnlPct7d, pnlPct30d,
                    openPositions, openOrders, closedTrades, winRate, avgHoldDays,
                    bestTrade, worstTrade };
type Position   = { id, marketId, category, title, side:'YES'|'NO',
                    shares, entry, mark, cost, value, pnl, pnlPct,
                    series:number[], resolves, horizonDays };
type Order      = { id, marketId, category, title,
                    side:'YES'|'NO', action:'BUY'|'SELL',
                    shares, filled, limit, value,
                    tif:'1 batch'|'5 batches'|'GTC', tifRemaining,
                    queuedFor:number /* batch id */, queuedAgo };
type Closed     = { id, category, title, side, shares, entry, exit,
                    pnl, pnlPct, closedAgo,
                    outcome:'sold'|'resolved' };
type Fill       = { id, kind:'fill'|'cancel', batch, ago,
                    side, action, shares, price, market, amount };
type Allocation = { cat, val, pct }[];   // derived from positions
```

**API shape suggestion.**
- `GET /api/portfolio/:address/summary` → `{ trader, portfolio, equityCurve, deposits, allocation }`
- `GET /api/portfolio/:address/positions` → `Position[]`
- `GET /api/portfolio/:address/orders` → `Order[]`
- `GET /api/portfolio/:address/history?cursor=...` → `{ items: Closed[], nextCursor }`
- `GET /api/portfolio/:address/activity?cursor=...` → `{ items: Fill[], nextCursor }`

**Tweaks panel.** `data/tweaks-panel.jsx` is design-tool only (in-design knob controls). **Delete it in production** along with the `<TweaksPanel>` render call. The values it toggles (`positionsLayout`, `density`, `showEquityChart`, `showAllocation`, `batchDetail`) become hard-coded product decisions or user settings, depending on what you ship.

---

## Cross-cutting components (extract these once)

These appear on multiple pages — extract into shared modules first.

| Component | Lives in | Purpose | Notes |
|---|---|---|---|
| `GlobalNav` | `MarketsData.jsx`, `PortfolioPieces.jsx`, etc. | Top nav (56 px, sticky, blurred) | Three implementations exist (markets, activity, portfolio) — they're 95% identical. Unify. |
| `BatchPill` | `MarketsData.jsx` | Live batch countdown chip in nav | `secs` 0–60 + progress bar. Subscribe to a single batch-clock SSE source. |
| `Sparkline` / `Spark` | `MarketsData.jsx`, `PortfolioPieces.jsx` | Tiny inline area chart | Standardize on one impl. |
| `EquityChart` | `PortfolioPieces.jsx` | SVG area chart with deposits + baseline | Used only on portfolio today. |
| `CategoryDot` | `PortfolioPieces.jsx`, `ActivityData.jsx` | Colored 6 × 6 dot | Color map differs (`CAT_COLORS` vs inline) — unify the palette. |
| `SidePill` | `PortfolioPieces.jsx` | YES/NO pill | One impl, used on portfolio + market-detail. |
| `eyebrow` / `text-annotation` / `text-mono` | `tokens/colors_and_type.css` | Type utility classes | Already declared globally. Use as-is. |
| `useBatchClock` | every page | 60-sec countdown hook | One canonical impl in your real-time client. |

---

## Tokens — what to lift verbatim

`tokens/colors_and_type.css` is the **only** styling source. Everything is exposed as a CSS custom property. Token names should round-trip into whatever your framework uses (Tailwind theme keys, vanilla-extract vars, etc.).

| Token group | Vars | Use |
|---|---|---|
| Surfaces | `--bg-0`, `--bg-1`, `--surface-1..3`, `--overlay` | Backgrounds, layered cards |
| Foreground | `--fg-1` (92%), `--fg-2` (72%), `--fg-3` (52%), `--fg-4` (32%), `--fg-on-accent` | Text |
| Borders | `--border-1` (6%), `--border-2` (10%), `--border-3` (14%), `--border-focus` | Hairlines, hovers, focus |
| Brand | `--accent`, `--accent-hover`, `--accent-press`, `--accent-soft`, `--accent-faint` | Cyan |
| Market semantics | `--yes`, `--yes-hover`, `--yes-soft`, `--yes-faint`, `--no`, `--no-*` | Yes / No — trading contexts only |
| Status | `--warn`, `--warn-soft`, `--info`, `--info-soft` | Testnet pill, methodology callouts |
| Type | `--font-display` (Syne), `--font-sans` (Inter), `--font-mono` (JetBrains Mono) | + `--fs-*` / `--lh-*` / `--fw-*` / `--track-*` |
| Spacing | `--space-1..9` (4-96 px, 8-pt grid) | All paddings/gaps |
| Radii | `--radius-sm` (2), `--radius-md` (4), `--radius-lg` (8), `--radius-xl` (12), `--radius-pill` | Tight by default — pills only for status |
| Shadows | `--shadow-inset-top`, `--shadow-floating`, `--shadow-popover`, `--shadow-focus-ring` | Restrained |
| Motion | `--ease-standard`, `--ease-in-out`, `--dur-fast` (120 ms), `--dur-base` (200 ms), `--dur-slow` (320 ms) | Never > 400 ms |

**Hard rules** (also in `BRAND.md`):
- Dark theme only. No light mode in scope.
- Yes-green / no-red **only** on numbers and bars in trading contexts. Never on icons or text.
- No emoji anywhere. Use `→`, `↗`, `⚠`, `·`, `//` glyphs.
- Sentence case copy. UPPERCASE only for the wordmark, status enums, and tier grades.
- Tabular numerals on every number — already wired via `.text-mono` and `font-variant-numeric: tabular-nums`.
- No gradient buttons or backgrounds. Gradients allowed only as protection scrims (top/bottom of scroll areas, 10% black → transparent) and under chart price fills.

---

## Production migration checklist

Order this work in a real frontend:

- [ ] Port `colors_and_type.css` to your framework's tokens
- [ ] Load Syne / Inter / JetBrains Mono (Google Fonts CSS already in `tokens/`)
- [ ] Build `GlobalNav` + `BatchPill` as shared components
- [ ] Wire batch-clock subscription (SSE or WebSocket) → push to all `useBatchClock` consumers
- [ ] Page: `/` (markets) — use `EventsDataV3.jsx` schema as the events list contract
- [ ] Page: `/m/[id]` (market-detail) — bind `FED_MARKET` schema to a real event endpoint; the right-rail `<V2BatchTheater>` is the priority component, ship it first
- [ ] Page: `/activity` — paginate batches; lazy-load detail on row expand
- [ ] Page: `/portfolio` — bind to wallet; ship `VariantClassic` only (drop the tweaks)
- [ ] Remove all `tweaks-panel.jsx` references and the `TWEAK_DEFAULTS` block in `portfolio.html`
- [ ] Replace mock generators (`genSeries*`, `genBatches`, `genEquityCurve`, `genSeries3`) with real data fetches
- [ ] Re-export `assets/sybil-mark.png` as SVG and rebuild a wordmark-only logo in Syne (see `BRAND.md` open items)

---

## Open questions for the engineering team

These are things the design system can't decide for you:

1. **Batch cadence.** Pages assume `60s`. Production cadence may be `250 ms / 1 s / 60 s` depending on market depth. The clock + ticker work at any cadence, but copy in several places hardcodes "every 60 s" — search for `60s` and `60 s` and replace.
2. **Address format.** The placeholder address `0xA17F···c92E` uses `···` (three-dot ellipsis). If your wallet UX uses `…` or `...`, normalize once at format time.
3. **Welfare metric.** Activity shows a "welfare saved" column (`matchedVolume * 0.118`). This is a placeholder formula — replace with whatever your protocol actually surfaces.
4. **TIF semantics.** Orders carry `tif: '1 batch' | 'N batches' | 'GTC'`. Confirm that's the order-engine vocabulary before shipping.
5. **Icon set.** The pages use no third-party icon font — all icons are inline SVG. `BRAND.md` suggests Lucide if you ever need more; flag if the team has picked a different set.
6. **Wordmark.** Only a raster `sybil-mark.png` is in the kit. A vector SVG (mark + wordmark) should be added for sharp rendering at all sizes.

---

## What this handoff is NOT

- **Not a component library.** Don't try to import these JSX files as React components in production — they're written for Babel-in-browser preview, not bundled builds.
- **Not the API contract.** The `data/*.jsx` files describe data *shapes*, not endpoints. Your backend team owns the actual schema; use these as a strong starting suggestion.
- **Not a complete app.** Login, settings, docs, admin, error states, empty states, mobile breakpoints — all out of scope. Design these as you go and add them back to this kit.

Questions about anything in here? Ask the designer (this project) before guessing.
