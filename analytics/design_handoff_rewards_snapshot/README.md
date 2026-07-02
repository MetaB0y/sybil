# Handoff: Polymarket Liquidity Rewards — Market Snapshot (Grid dashboard)

## Overview
A single-page, desktop-first analytics dashboard for a researcher taking a **live snapshot** of every rewarded market (~2,000–2,300 at a time) and exploring **where the total daily reward pool ($/day) is going**. The screen shows a headline summary, six breakdown charts (each a distribution of the *same* total across buckets), and a paginated, sortable drill-down table of individual markets. The signature interaction is **cross-filtering**: clicking any bucket in any chart filters every other chart and the table to that slice; stacking selections yields intersections like "Sports ∩ 0–5¢ spread".

## About the Design Files
The files in this bundle are **design references created in HTML** — a working prototype that demonstrates the intended look, layout, data model, and interaction behavior. They are **not production code to copy directly**. The task is to **recreate this design in the target codebase's existing environment** (React, Vue, Svelte, etc.) using its established component patterns, charting approach, state management, and styling conventions. If no frontend environment exists yet, pick the most appropriate stack for the project (a React + a lightweight chart/SVG approach works well) and implement there. The prototype uses a bespoke templating runtime (`support.js`) — **ignore that runtime**; only the markup structure, styles, data logic, and interaction semantics are relevant.

The real data source is out of scope here — the prototype generates a synthetic snapshot client-side so the interactions are fully demonstrable. See **Data Model** for how to swap in a real feed.

## Fidelity
**High-fidelity (hifi).** Final colors, typography, spacing, bar/chart treatment, table layout, and interaction behavior are all intended as shown. Recreate the UI to match, using the codebase's existing primitives where they exist (buttons, table, chips). The one exception is the underlying data, which is synthetic placeholder data in the prototype.

---

## Screens / Views

### Screen: Rewards Snapshot (single view)
- **Purpose**: Understand the composition of the daily reward pool and drill into the specific markets behind any slice.
- **Overall layout**: A single centered "app frame" card, fixed **1180px** wide, on a warm off-white page background (`#f4f3f0`, page padding `34px 24px 60px`, content centered horizontally). The frame is white, `border-radius: 12px`, `1px` border `rgba(0,0,0,.10)`, shadow `0 8px 30px rgba(0,0,0,.07)`. It contains, top to bottom:
  1. **Window chrome bar** — `10px 15px` padding, bottom border, panel bg `#fbfbfa`. Three inert `9px` dots (`rgba(0,0,0,.11)`) + title text "Liquidity Rewards — Market Snapshot" (`600 11px system-ui`, color `#8a8a8e`, margin-left `9px`).
  2. **Body** — padding `22px 24px 24px`. Contains headline, filter chips row, the 3×2 chart grid, and the table + pager.

#### Component: Headline row
- Flex row, `align-items: flex-end`, `gap: 34px`, bottom padding `16px`, bottom border `1px rgba(0,0,0,.10)`.
- **KPI 1 — Total $/day**: value `700 30px/1 system-ui`, `letter-spacing: -.025em`, `font-variant-numeric: tabular-nums` (e.g. `$54,020`), with a `/day` suffix (`600 14px`, color `#8a8a8e`). Caption below (`11px`, `#8a8a8e`): "Total daily reward pool" — or when filtered, "filtered · of $54,020 total".
- **KPI 2 — # markets**: value `700 22px system-ui` (e.g. `2,259`). Caption "Rewarded markets" — or when filtered "of 2,259 in snapshot". Shows the **filtered** count when filters are active.
- **Snapshot / refresh** (pushed right, `margin-left: auto`, text-right): timestamp line `500 11px 'IBM Plex Mono'` color `#8a8a8e`, e.g. "snapshot · 2 Jul 14:33 UTC". Below it a **Refresh snapshot** button.
  - Button: `↻ Refresh snapshot`, `600 11px system-ui`, padding `8px 13px`, `border-radius: 8px`, solid accent bg `#3b5bdb`, white text, `1px` accent border. Hover `filter: brightness(1.06)`; active `translateY(1px)`.

#### Component: Filter chips row
- Flex row, `gap: 8px`, wraps, `align-items: center`, margin `15px 0 3px`, `min-height: 24px` (reserves space when empty so layout doesn't jump).
- Shown only when ≥1 filter is active: leading label "Filtering:" (`11px`, `#8a8a8e`), then one **chip** per active filter, then a **clear all** link.
- **Chip**: inline-flex, `gap: 7px`, padding `4px 6px 4px 10px`, `500 11px 'IBM Plex Mono'`, text/border accent (`#3b5bdb` / `rgba(59,91,219,.30)`), bg `rgba(59,91,219,.10)`, `border-radius: 14px`. Enter animation `pop .18s ease` (scale .9→1, opacity 0→1). Label = optional dimension prefix + bucket (e.g. "Sports", "spread 0–5¢", "vol 10–100k", "age 1–7d", "ends >30d", "contested").
  - **× button**: `15px` circle, bg `rgba(59,91,219,.16)`, `font-size: 11px`, cursor pointer, hover bg `rgba(59,91,219,.3)`. Removes that one filter.
- **clear all**: `11px`, accent color, underline on hover. Clears all filters.
- **Summary** (pushed right, `margin-left: auto`, `11px 'IBM Plex Mono'`, color `#b8b8bc`): when unfiltered shows "2,259 rewarded markets · $54,020/day"; when filtered shows the intersection + counts, e.g. "Sports ∩ 0–5¢ · 312 of 2,259 markets".

#### Component: Chart grid (six breakdowns)
- CSS grid, `grid-template-columns: repeat(3, 1fr)`, `gap: 13px`, margin `16px 0`. Six cards in a 3×2 arrangement, in this fixed order:
  1. **Category** — buckets: Sports, Crypto, Politics, Pop-culture, Economy, Other. Subtitle "click to filter".
  2. **Current spread** — 0–5¢, 5–10¢, 10–20¢, 20–30¢, 30–50¢, >50¢. Subtitle "hover for $ + %".
  3. **24h volume** — $0, <1k, 1–10k, 10–100k, >100k. Subtitle "click to filter".
  4. **Market age** — <1d, 1–7d, 7–30d, >30d. Subtitle "click to filter".
  5. **Time to resolution** — <1d, 1–7d, 7–30d, >30d, no end. Subtitle "click to filter".
  6. **Competitiveness** — no farmers, thin, contested. Subtitle "farmers per market".
- **Card** (`.bd`): `1px` border `rgba(0,0,0,.10)`, `border-radius: 10px`, padding `14px 15px`, white bg. Header row (`.bdhd`): title `600 10.5px system-ui uppercase`, `letter-spacing: .055em`, color `#1c1c1e`, pushed apart from a right-aligned subtitle (`10px system-ui`, color `#b8b8bc`, nowrap). Header margin-bottom `12px`.
- **Bars** (`.bars`): vertical flex, `gap: 8px`. Each **bar row** (`.bar`) is a 3-column grid: `70px | 1fr | 34px`, `gap: 9px`, `align-items: center`, `font 11px/1 system-ui`, `cursor: pointer`.
  - **Label** (col 1): color `#8a8a8e`, ellipsis-truncated. On row hover → `#1c1c1e`.
  - **Track** (col 2): `height: 12px`, bg `rgba(0,0,0,.055)`, `border-radius: 3px`, `overflow: hidden`. Contains a **fill** bar: `height: 100%`, `border-radius: 3px`, accent bg `#3b5bdb`, **width = value / max-bucket-value in this chart** (so the largest bucket is full-width). On row hover → `filter: brightness(.9)`.
  - **Value** (col 3): right-aligned `500 10.5px 'IBM Plex Mono'`, color `#1c1c1e`. Shows **% of the (filtered) pool** for that chart, rounded; `<1%` for tiny non-zero; `—` for empty buckets.
- **Bar fill opacity encodes selection state** within each chart:
  - No selection in this dimension → all fills `opacity: 0.9`.
  - A bucket is selected in this dimension → selected fill `opacity: 1`, all others `opacity: 0.24` (dimmed).

#### Component: Drill-down table
- Section label (`.secttl`): `600 10.5px system-ui uppercase`, `letter-spacing: .055em`, color `#8a8a8e`, margin `2px 0 10px`. Text: "Markets — 2,259 · click a header to sort" (count reflects the filtered set).
- Table wrapper: `1px` border, `border-radius: 10px`, `overflow: hidden`. Table: full width, `border-collapse: collapse`, `font-size: 11px`.
- **Columns**: Question | Category | Spread | 24h vol (num) | Reward/day (num) | Competitiveness.
- **Header `th`**: left-aligned (right for numeric cols), `600 9.5px system-ui uppercase`, `letter-spacing: .05em`, color `#8a8a8e`, padding `9px 10px`, bottom border, bg `#fbfbfa`, `cursor: pointer`, `user-select: none`. Hover → `#1c1c1e`. **Active sort column** → accent color `#3b5bdb` with a trailing arrow `↓` (desc) or `↑` (asc).
- **Body `td`**: padding `8px 10px`, bottom border `1px rgba(0,0,0,.055)` (last row none). Row hover bg `rgba(59,91,219,.04)`. Numeric cells right-aligned `500 10.5px 'IBM Plex Mono'`. Question cell wraps, `max-width: 340px`. Others nowrap.
- **Competitiveness pill**: inline-block, `500 9.5px system-ui`, padding `2px 8px`, `border-radius: 10px`. Variants: **thin** → bg `rgba(180,83,9,.12)` text `#b45309`; **contested** → bg `rgba(59,91,219,.10)` text `#3b5bdb`; **no farmers** → bg `rgba(0,0,0,.055)` text `#8a8a8e`.

#### Component: Pagination (pager)
- Flex row, `gap: 6px`, `align-items: center`, margin-top `12px`.
- **Range readout** (`.pginfo`, pushed left via `margin-right: auto`): `11px 'IBM Plex Mono'`, color `#8a8a8e`, e.g. "1–20 of 2,259".
- **Prev / Next buttons** and **numbered page buttons** (`.pgbtn`): `600 11px system-ui`, `min-width: 30px`, `height: 28px`, padding `0 9px`, `1px` border, `border-radius: 7px`, white bg. Hover → bg `#fbfbfa`, border `#8a8a8e`. **Active page** (`.on`) → accent bg `#3b5bdb`, accent border, white text. **Disabled** (`.dis`, on Prev at page 1 / Next at last page) → `opacity: .4`, `pointer-events: none`.
- **Page number windowing**: always show page 1 and the last page, plus the current page ±1, plus pages 2–3 when near the start and last-1/last-2 when near the end. Insert a non-clickable `…` gap (`.pggap`, color `#b8b8bc`) wherever the sequence skips.
- **Page size: 20 rows per page.**

#### Component: Hover tooltip
- Fixed-position, follows the cursor, `pointer-events: none`, `z-index: 60`. Positioned at `left = cursorX + 14px`, `top = cursorY − 12px`, with `transform: translateY(-115%)` so it floats just above the pointer.
- Dark bg `#1c1c1e`, white text, padding `7px 9px`, `border-radius: 8px`, shadow `0 6px 20px rgba(0,0,0,.25)`.
- **Line 1**: `600 12px 'IBM Plex Mono'` — the exact dollar figure, e.g. `$19,782`.
- **Line 2**: `400 10.5px 'IBM Plex Mono'`, color `#c7c7cc` — `<bucket> · <pct>% of pool · <count> mkts`, e.g. "0–5¢ · 41% of pool · 894 mkts".
- Appears on `mousemove` over a bar row, hides on `mouseleave`.

---

## Interactions & Behavior

### Cross-filtering (the core interaction)
- **Click a bucket** in any chart → toggle a filter on that dimension for that bucket. Clicking the already-selected bucket clears it.
- Filters combine as an **AND / intersection** across dimensions (`{cat: 'Sports', spread: '0–5¢'}` → markets that are Sports **and** 0–5¢).
- **Each chart recomputes against all filters EXCEPT its own dimension.** This is the crossfilter convention: a chart always shows the full distribution of the currently-selected *other* filters so you can still see and re-pick within its own dimension. Concretely: for chart of dimension `D`, sum reward per bucket over markets that pass every active filter whose key ≠ `D`. The selected bucket in `D` is highlighted (opacity), the rest dimmed.
- **The headline KPIs and the table use ALL active filters** (full intersection).
- Every value change **animates**: bar fills transition `width .55s cubic-bezier(.22,.61,.36,1)` and `opacity .3s`. Do not re-mount bars on filter change — update the existing bars' width/opacity so the CSS transition runs (i.e. keep stable keys per bucket). "Transitions, not redraws."

### Filter chips
- One chip per active filter. `×` removes just that filter. "clear all" empties all filters.
- Any filter change **resets the table to page 1**.

### Sorting
- Click a column header to sort by it. Clicking the active column toggles asc/desc. Switching to a new column defaults to **desc for numeric** columns (24h vol, Reward/day) and **asc for text** columns (Question, Category), plus **desc for Spread/Competitiveness** which sort by their bucket order index.
- Default sort on load: **Reward/day, descending.**
- Sort keys: Reward/day → numeric reward; 24h vol → numeric volume; Spread → bucket index; Competitiveness → bucket index; Category & Question → string localeCompare.
- Changing sort **resets to page 1.**

### Pagination
- 20 rows/page over the full filtered+sorted set. Prev/Next and numbered jumps. Current page is clamped to valid range whenever the filtered set shrinks.

### Refresh snapshot
- Regenerates the snapshot (new synthetic data in the prototype; in production, re-fetch the live snapshot). Updates the timestamp, advances it a few minutes. **Preserves active filters and sort**, resets to page 1. Bar values animate to their new positions.

---

## State Management
State needed:
- `snapshot` — the array of market records + derived totals (`grandTotal`, `grandCount`) + `snapLabel` timestamp string.
- `filters` — an object `{ dimensionKey: bucketLabel }`, one entry per active filter (absent key = no filter on that dimension).
- `sort` — `{ key, dir }` (`dir` is `'asc' | 'desc'`).
- `page` — current 1-based table page.
- `tip` — tooltip state `{ show, x, y, l1, l2 }` for hover.

Derived each render (memoize on `snapshot` + `filters`):
- Per-chart bucket sums using the "all filters except this dimension" rule, plus per-bucket counts (for tooltips).
- Filtered totals for the KPIs (all filters).
- Filtered + sorted market list for the table, then the current page slice.

Triggers: bucket click → mutate `filters`, reset `page`; chip × / clear all → mutate `filters`, reset `page`; header click → mutate `sort`, reset `page`; pager → set `page`; refresh → replace `snapshot`, reset `page`; bar hover → set/clear `tip`.

---

## Data Model
Each **market record** has: `question` (string), `category` (one of the Category buckets), `spread` (spread bucket), `volume` (volume bucket) + `volumeNum` (raw number for sorting/display), `age` (age bucket), `resolution` (resolution bucket), `competitiveness` (comp bucket), and `reward` (number, $/day).

Bucketing dimensions and their bucket orders are fixed (see the six charts above). In production, replace the synthetic generator with the real snapshot feed and bucket each market on ingest:
- **Spread** buckets from current best bid/ask spread in cents.
- **24h volume** buckets: `$0`, `<1k`, `1–10k`, `10–100k`, `>100k`.
- **Market age** / **Time to resolution** buckets computed from timestamps relative to snapshot time; resolution has an extra "no end" bucket for markets without an end date.
- **Competitiveness**: `no farmers` / `thin` / `contested` (a measure of how many liquidity providers are competing on the market).
- All six breakdowns are distributions of the **same** `reward` total — every chart's buckets sum to the same pool (given the same filter context), just partitioned differently.

The prototype uses a seeded PRNG (mulberry32) to produce a stable ~2,060–2,280 market snapshot (~$48–55k/day total). Volume correlates loosely with reward (higher-volume buckets get a higher reward multiplier). None of this generation logic ships — it's only there to make the interactions demonstrable.

---

## Design Tokens
**Colors**
- Ink / primary text: `#1c1c1e`
- Muted text: `#8a8a8e`
- Faint text / disabled: `#b8b8bc`
- Hairline border: `rgba(0,0,0,.10)`; lighter row divider: `rgba(0,0,0,.055)`
- Track / neutral pill bg: `rgba(0,0,0,.055)`
- Panel bg (chrome/headers): `#fbfbfa`
- Page bg: `#f4f3f0`
- Card / surface bg: `#ffffff`
- **Accent (single accent color)**: `#3b5bdb`
- Accent tint (bg): `rgba(59,91,219,.10)`; accent border: `rgba(59,91,219,.30)`
- Warn (thin competitiveness): text `#b45309`, bg `rgba(180,83,9,.12)`

**Typography**
- UI sans: `system-ui, -apple-system, sans-serif`
- Mono (numbers, timestamps, tooltip, chips, pager info): `'IBM Plex Mono'` (Google Font, weights 400/500/600)
- Notable sizes: KPI value `30px/700`; secondary KPI `22px/700`; chart title `10.5px/600 uppercase`; bar row `11px`; table body `11px`; table header `9.5px/600 uppercase`; captions `10–11px`.
- `font-variant-numeric: tabular-nums` on the headline value; monospace already handles alignment elsewhere.

**Spacing / geometry**
- Frame width: `1180px`; body padding `22px 24px 24px`.
- Card radius `10px`; frame radius `12px`; button radius `7–8px`; chip radius `14px`; pill radius `10px`; track/fill radius `3px`.
- Grid gap `13px`; bar gap `8px`; chip gap `8px`.
- Bar track height `12px`; pager button height `28px`.

**Shadows**
- Frame: `0 8px 30px rgba(0,0,0,.07)`
- Tooltip: `0 6px 20px rgba(0,0,0,.25)`

**Motion**
- Bar fill: `width .55s cubic-bezier(.22,.61,.36,1), opacity .3s`
- Chip enter: `pop .18s ease` (scale .9→1, opacity 0→1)
- Button hover `filter: brightness` ~.15s; pager button bg/border ~.12s.

---

## Assets
- No raster images or custom icons. The only glyphs are Unicode characters: `↻` (refresh), `×` (chip dismiss), `↓`/`↑` (sort arrows), `‹`/`›` (pager), `…` (pager gap), `∩` (intersection, in the summary line). Use the codebase's existing icon set if preferred.
- Font: **IBM Plex Mono** via Google Fonts (or self-host). UI sans is system default.

## Files
Included in this handoff folder:
- `Rewards Snapshot - Grid.dc.html` — the **hi-fi implemented dashboard** (the design to recreate). Contains the full markup, inline styles / `<style>` block, and the data + interaction logic. Ignore the `support.js` runtime references; read it for structure, tokens, and behavior.
- `Rewards Snapshot Wireframes.dc.html` — the earlier **low-fi exploration** of six layout paradigms (grid of small multiples, big-chart + switcher, scroll story, sidebar-filter, dense terminal, bento). Context only; direction chosen was the **grid of small multiples**. Not required for implementation.

Open either file directly in a browser to view. To read the source, open the `.dc.html` files in a text editor — the markup and the `class Component` logic block describe everything above in code.
