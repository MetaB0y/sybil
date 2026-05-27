# "Hide closed" Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Hide closed" toggle (ON by default) to the markets index; when OFF, surface fully-closed events and closed standalone binaries as greyed-out, still-clickable cards sunk to the bottom of the list.

**Architecture:** Build every card once in `page.tsx` (tagging each with `closed: boolean`), then filter + order through a new pure helper `selectIndexCards`. Toggle state lives in the URL (`?closed=show`) like `?q`/`?sort`/`?category`. Cards self-derive their closed state and apply the existing dim + uppercase-"closed" idiom at card level.

**Tech Stack:** Next.js 16, React, TypeScript, vitest. Run from `frontend/web`.

**Design doc:** `docs/superpowers/specs/2026-05-24-hide-closed-toggle-design.md`

**Working directory for all commands:** `frontend/web`

---

### Task 1: Pure filter/sort helper with closed handling

Extract the index card filter + sort out of `page.tsx` into a unit-testable pure
function that drops closed cards unless `showClosed`, and always sinks closed
cards below open ones regardless of sort mode.

**Files:**
- Create: `frontend/web/src/lib/markets/select-index-cards.ts`
- Test: `frontend/web/src/lib/markets/select-index-cards.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/markets/select-index-cards.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { selectIndexCards, type CardItem } from "./select-index-cards";
import type { Market } from "./use-markets";

function mk(partial: Partial<Market> & { market_id: number }): Market {
  return { name: `m${partial.market_id}`, status: "active", ...partial } as Market;
}

function binary(
  id: number,
  opts: { vol?: bigint; closed?: boolean; category?: string | null } = {},
): CardItem {
  return {
    kind: "binary",
    market: mk({ market_id: id }),
    volumeNanos: opts.vol ?? 0n,
    sortKey: `m${id}`,
    createdMs: 0,
    primaryCategory: opts.category ?? null,
    closed: opts.closed ?? false,
  };
}

const NO_TRADERS = new Map<string, number>();
const base = {
  query: "",
  sort: "volume" as const,
  category: null,
  eventTraders: NO_TRADERS,
};

function ids(out: CardItem[]): number[] {
  return out.map((it) => (it.kind === "binary" ? it.market.market_id : -1));
}

describe("selectIndexCards", () => {
  it("hides closed cards by default (showClosed=false)", () => {
    const items = [binary(1, { closed: false }), binary(2, { closed: true })];
    expect(ids(selectIndexCards(items, { ...base, showClosed: false }))).toEqual([1]);
  });

  it("shows closed cards when showClosed=true", () => {
    const items = [binary(1, { closed: false }), binary(2, { closed: true })];
    expect(selectIndexCards(items, { ...base, showClosed: true })).toHaveLength(2);
  });

  it("sinks closed cards below open ones under volume sort, even with higher volume", () => {
    const items = [
      binary(1, { vol: 10n, closed: false }),
      binary(2, { vol: 999n, closed: true }),
      binary(3, { vol: 5n, closed: false }),
    ];
    const out = selectIndexCards(items, { ...base, sort: "volume", showClosed: true });
    expect(ids(out)).toEqual([1, 3, 2]);
  });

  it("sinks closed cards below open ones under 'new' sort, even when newer", () => {
    const open1: CardItem = { ...binary(1, { closed: false }), createdMs: 100 };
    const closedNewer: CardItem = { ...binary(2, { closed: true }), createdMs: 999 };
    const open2: CardItem = { ...binary(3, { closed: false }), createdMs: 200 };
    const out = selectIndexCards([open1, closedNewer, open2], {
      ...base,
      sort: "new",
      showClosed: true,
    });
    expect(ids(out)).toEqual([3, 1, 2]);
  });

  it("filters by category and query", () => {
    const items = [
      binary(1, { category: "Politics" }),
      binary(2, { category: "Sports" }),
    ];
    expect(ids(selectIndexCards(items, { ...base, category: "Sports", showClosed: true }))).toEqual([2]);
    expect(ids(selectIndexCards([...items], { ...base, query: "m1", showClosed: true }))).toEqual([1]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/lib/markets/select-index-cards.test.ts`
Expected: FAIL — cannot resolve `./select-index-cards`.

- [ ] **Step 3: Write minimal implementation**

Create `frontend/web/src/lib/markets/select-index-cards.ts`:

```ts
/**
 * Filter + order the markets-index cards.
 *
 * Cards are built once in the page (one `CardItem` per binary market or
 * multi-outcome event, each tagged with `closed`). This helper owns the
 * query/category/closed filtering and the sort. Closed cards are dropped unless
 * `showClosed`, and always sink below open cards regardless of the active sort.
 */

import type { Market } from "./use-markets";
import type { SortKey } from "@/components/markets-filter-bar";

export type CardItem =
  | {
      kind: "multi";
      name: string;
      eventId: string;
      markets: Market[];
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
      closed: boolean;
    }
  | {
      kind: "binary";
      market: Market;
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
      closed: boolean;
    };

export type SelectOptions = {
  query: string;
  sort: SortKey;
  category: string | null;
  showClosed: boolean;
  eventTraders: Map<string, number>;
};

/** Outcomes a card represents (volume tie-break: bigger events first). */
export function sizeOf(item: CardItem): number {
  return item.kind === "multi" ? item.markets.length : 1;
}

/** Trader count for sorting: per-market for binary, event union for multi. */
export function traderCountOf(
  item: CardItem,
  eventTraders: Map<string, number>,
): number {
  if (item.kind === "binary") return item.market.trader_count ?? 0;
  return eventTraders.get(item.eventId) ?? 0;
}

function compareBySort(
  a: CardItem,
  b: CardItem,
  sort: SortKey,
  eventTraders: Map<string, number>,
): number {
  if (sort === "new") {
    return b.createdMs - a.createdMs;
  }
  if (sort === "traders") {
    const ta = traderCountOf(a, eventTraders);
    const tb = traderCountOf(b, eventTraders);
    if (ta !== tb) return tb - ta;
    if (a.volumeNanos === b.volumeNanos) return 0;
    return a.volumeNanos < b.volumeNanos ? 1 : -1;
  }
  // volume desc; tie-break by size desc.
  if (a.volumeNanos !== b.volumeNanos) {
    return a.volumeNanos < b.volumeNanos ? 1 : -1;
  }
  return sizeOf(b) - sizeOf(a);
}

export function selectIndexCards(
  items: CardItem[],
  opts: SelectOptions,
): CardItem[] {
  const q = opts.query.trim().toLowerCase();
  let out = items;
  if (q) out = out.filter((it) => it.sortKey.includes(q));
  if (opts.category) {
    out = out.filter((it) => it.primaryCategory === opts.category);
  }
  if (!opts.showClosed) {
    out = out.filter((it) => !it.closed);
  }
  out = [...out];
  out.sort((a, b) => {
    // Closed cards always sink below open ones, regardless of sort mode.
    if (a.closed !== b.closed) return a.closed ? 1 : -1;
    return compareBySort(a, b, opts.sort, opts.eventTraders);
  });
  return out;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/lib/markets/select-index-cards.test.ts`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/lib/markets/select-index-cards.ts src/lib/markets/select-index-cards.test.ts
git commit -m "feat(markets): selectIndexCards helper — filter/sort with closed sink"
```

---

### Task 2: Wire `page.tsx` to the helper, tag `closed`, add the URL toggle

Replace the inline `CardItem`/filter/sort in the page with the helper, tag every
card with `closed`, stop skipping closed cards while building, and add the
`?closed=show` URL toggle.

**Files:**
- Modify: `frontend/web/src/app/page.tsx`

- [ ] **Step 1: Swap the imports and drop the local `CardItem`**

In `frontend/web/src/app/page.tsx`, replace the import block for `use-markets`
(currently lines ~14-19) so it ALSO imports the helper, and delete the local
`CardItem` type (currently lines ~24-42).

Replace:

```tsx
import {
  useMarketsList,
  eventVisibleOnIndex,
  isClosed,
  type Market,
} from "@/lib/markets/use-markets";
```

with:

```tsx
import {
  useMarketsList,
  eventVisibleOnIndex,
  isClosed,
  type Market,
} from "@/lib/markets/use-markets";
import { selectIndexCards, type CardItem } from "@/lib/markets/select-index-cards";
```

Then delete the entire local `type CardItem = … ;` block (the union type, ~lines
24-42). `Market` is still used elsewhere in the file, keep its import.

- [ ] **Step 2: Read `showClosed` + setter from `useFilterParams`**

In `MarketsPageInner`, change the destructure (currently line ~70) from:

```tsx
  const { query, sort, setSort, category } = useFilterParams();
```

to:

```tsx
  const { query, sort, setSort, category, showClosed, setHideClosed } =
    useFilterParams();
```

- [ ] **Step 3: Tag every card with `closed`, stop skipping closed**

Replace the `items` memo body (currently lines ~72-120) with:

```tsx
  const items = useMemo(() => {
    if (!bundle) return null;
    const all: CardItem[] = [];
    for (const g of bundle.groups) {
      if (g.markets.length >= 2) {
        // Multi-outcome event. Closed only when EVERY outcome is closed; a
        // partially-closed event stays open (its closed rows render greyed).
        const first = g.markets[0]!;
        const primary = pickDisplayCategory(first.categories, first.category).primary;
        all.push({
          kind: "multi",
          name: g.name,
          eventId: g.eventId,
          markets: g.markets,
          volumeNanos: sumVolume(g.markets),
          sortKey: g.name.toLowerCase(),
          createdMs: eventNewnessMs(g.markets),
          primaryCategory: primary,
          closed: !eventVisibleOnIndex(g.markets),
        });
      } else {
        for (const m of g.markets) {
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
            createdMs: marketNewnessMs(m),
            primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
            closed: isClosed(m),
          });
        }
      }
    }
    for (const m of bundle.ungrouped) {
      all.push({
        kind: "binary",
        market: m,
        volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
        sortKey: m.name.toLowerCase(),
        createdMs: m.created_at_ms ?? 0,
        primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
        closed: isClosed(m),
      });
    }
    return all;
  }, [bundle]);
```

- [ ] **Step 4: Replace the `filtered` memo with the helper**

Replace the entire `filtered` memo (currently lines ~137-171) with:

```tsx
  const filtered = useMemo(() => {
    if (!items) return null;
    return selectIndexCards(items, {
      query,
      sort,
      category,
      showClosed,
      eventTraders: eventTradersMap,
    });
  }, [items, query, sort, category, showClosed, eventTradersMap]);
```

- [ ] **Step 5: Delete the now-unused `sizeOf` and `traderCountOf` from the page**

These moved to the helper. Delete the `function sizeOf(...)` (currently ~lines
338-340) and `function traderCountOf(...)` (currently ~lines 350-357) from
`page.tsx`. Keep `sumVolume`, `marketNewnessMs`, `eventNewnessMs` — the `items`
memo still uses them.

- [ ] **Step 6: Add `showClosed` to the page-reset filter key**

Change the `filterKey` line (currently ~line 178) from:

```tsx
  const filterKey = `${query} ${sort} ${category ?? ""}`;
```

to:

```tsx
  const filterKey = `${query} ${sort} ${category ?? ""} ${showClosed}`;
```

- [ ] **Step 7: Pass toggle props to the filter bar**

Change the `<MarketsFilterBar ... />` usage (currently ~line 250) from:

```tsx
        <MarketsFilterBar sort={sort} onSortChange={setSort} />
```

to:

```tsx
        <MarketsFilterBar
          sort={sort}
          onSortChange={setSort}
          hideClosed={!showClosed}
          onHideClosedChange={setHideClosed}
        />
```

- [ ] **Step 8: Extend `useFilterParams` with `closed`**

In `useFilterParams`, add the read (after `const category = …`, ~line 311):

```tsx
  const showClosed = searchParams.get("closed") === "show";
```

Change the `update` callback signature + body to handle `showClosed`
(the `update` arg type is currently `{ q?: string; sort?: SortKey }`):

```tsx
  const update = useCallback(
    (next: { q?: string; sort?: SortKey; showClosed?: boolean }) => {
      const params = new URLSearchParams(searchParams.toString());
      if (next.q !== undefined) {
        if (next.q) params.set("q", next.q);
        else params.delete("q");
      }
      if (next.sort !== undefined) {
        if (next.sort !== "volume") params.set("sort", next.sort);
        else params.delete("sort");
      }
      if (next.showClosed !== undefined) {
        // Default is hide-closed; only write the param when showing them.
        if (next.showClosed) params.set("closed", "show");
        else params.delete("closed");
      }
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams]
  );
```

Change the return object (currently ~lines 330-335) to expose `showClosed` and
`setHideClosed`:

```tsx
  return {
    query,
    sort,
    category,
    showClosed,
    setSort: (s: SortKey) => update({ sort: s }),
    setHideClosed: (hide: boolean) => update({ showClosed: !hide }),
  };
```

- [ ] **Step 9: Typecheck**

Run: `pnpm exec tsc --noEmit`
Expected: no errors from `page.tsx` or `select-index-cards.ts`. (Pre-existing
errors in unrelated files from concurrent work may appear — confirm none are in
the files this task touches.)

- [ ] **Step 10: Commit**

```bash
git add src/app/page.tsx
git commit -m "feat(markets): wire selectIndexCards + ?closed=show URL toggle on index"
```

---

### Task 3: "Hide closed" chip in the filter bar

**Files:**
- Modify: `frontend/web/src/components/markets-filter-bar.tsx`

- [ ] **Step 1: Add the two new props**

Change the `Props` type (currently ~lines 33-36) from:

```tsx
type Props = {
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
};
```

to:

```tsx
type Props = {
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
  hideClosed: boolean;
  onHideClosedChange: (hide: boolean) => void;
};
```

And update the function signature (currently ~line 38):

```tsx
export function MarketsFilterBar({
  sort,
  onSortChange,
  hideClosed,
  onHideClosedChange,
}: Props) {
```

- [ ] **Step 2: Render a divider + "Hide closed" chip after the sort chips**

Inside the right-hand flex cluster (the `<div>` that wraps `{SORTS.map(...)}`,
currently ~lines 52-98), add a divider and the chip immediately AFTER the
closing `})}` of the `SORTS.map` and BEFORE that `<div>`'s closing tag:

```tsx
          <span
            aria-hidden
            style={{
              width: 1,
              height: 16,
              background: "var(--border-2)",
              margin: "0 var(--space-1)",
            }}
          />
          <button
            type="button"
            onClick={() => onHideClosedChange(!hideClosed)}
            title={
              hideClosed
                ? "Closed markets hidden — click to show them greyed out"
                : "Closed markets shown — click to hide"
            }
            style={{
              height: 26,
              padding: "0 var(--space-3)",
              background: hideClosed ? "var(--surface-2)" : "transparent",
              color: hideClosed ? "var(--fg-1)" : "var(--fg-3)",
              border: `1px solid ${
                hideClosed ? "var(--border-3)" : "var(--border-2)"
              }`,
              borderRadius: "var(--radius-sm)",
              fontFamily: "var(--font-mono)",
              fontSize: "11px",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
              cursor: "pointer",
              transition: "all var(--dur-fast) var(--ease-standard)",
            }}
          >
            Hide closed
          </button>
```

- [ ] **Step 3: Typecheck**

Run: `pnpm exec tsc --noEmit`
Expected: no errors in `markets-filter-bar.tsx` or `page.tsx`.

- [ ] **Step 4: Commit**

```bash
git add src/components/markets-filter-bar.tsx
git commit -m "feat(markets): 'Hide closed' chip in the index filter bar"
```

---

### Task 4: Closed visual state on `BinaryCard`

Dim the whole card and show "closed" in the eyebrow's right slot. Card stays a
clickable link to detail.

**Files:**
- Modify: `frontend/web/src/components/binary-card.tsx`

- [ ] **Step 1: Dim the card when closed**

In `BinaryCard`, the root `<Link>`'s `style` object (currently ~lines 55-71)
ends with `cursor: "pointer",`. Add an `opacity` line right after it:

```tsx
        cursor: "pointer",
        opacity: market.closed === true ? 0.5 : 1,
```

- [ ] **Step 2: Show "closed" in the eyebrow**

In `EyebrowRow`, the right-hand `<span>` currently renders
`{endDate ?? "yes / no"}` (the last `<span>` in that component, ~line 152).
Replace that expression with:

```tsx
        {market.closed === true ? "closed" : (endDate ?? "yes / no")}
```

- [ ] **Step 3: Typecheck**

Run: `pnpm exec tsc --noEmit`
Expected: no errors in `binary-card.tsx`.

- [ ] **Step 4: Commit**

```bash
git add src/components/binary-card.tsx
git commit -m "feat(markets): greyed closed state on BinaryCard"
```

---

### Task 5: Card-level closed state on `MultiCard` (all outcomes closed)

When every outcome is closed, dim the whole card and badge it "closed". Suppress
the per-row dim/tag in that case so dimming doesn't compound (article 0.5 × row
0.5 = 0.25). Partially-closed cards keep their per-row greying unchanged.

**Files:**
- Modify: `frontend/web/src/components/multi-card.tsx`

- [ ] **Step 1: Derive `allClosed` and dim the article**

In `MultiCard`, after `const secondary = …` / `const hiddenCount = …`
(currently ~lines 61-62), add:

```tsx
  const allClosed =
    markets.length > 0 && markets.every((m) => m.closed === true);
```

In the `<article>`'s `style` object (currently ~lines 87-100) the last property
is `cursor: "pointer",`. Add an `opacity` line after it:

```tsx
        cursor: "pointer",
        opacity: allClosed ? 0.5 : 1,
```

- [ ] **Step 2: Pass `allClosed` to the eyebrow and badge it**

Change the `<EyebrowRow ... />` usage (currently ~lines 102-106) to pass
`allClosed`:

```tsx
      <EyebrowRow
        markets={markets}
        count={markets.length}
        hiddenCount={hiddenCount}
        allClosed={allClosed}
      />
```

Update `EyebrowRow`'s props (currently ~lines 135-143) to accept it:

```tsx
function EyebrowRow({
  markets,
  count,
  hiddenCount,
  allClosed,
}: {
  markets: Market[];
  count: number;
  hiddenCount: number;
  allClosed: boolean;
}) {
```

In `EyebrowRow`'s right-hand `<span>` (the one containing `{count} outcomes`,
currently ~lines 189-203), add a leading "closed ·" when `allClosed`, right
after that `<span style={{...}}>` opening tag and before `<span>{count} outcomes</span>`:

```tsx
        {allClosed && (
          <>
            <span style={{ color: "var(--fg-4)" }}>closed</span>
            <span style={{ margin: "0 4px", color: "var(--fg-4)" }}>·</span>
          </>
        )}
        <span>{count} outcomes</span>
```

- [ ] **Step 3: Thread `cardClosed` into the secondary rows**

Change the `<SecondaryList ... />` usage (currently ~lines 120-125) to pass it:

```tsx
      <SecondaryList
        markets={secondary}
        prices={prices}
        inView={inView}
        getLabel={getLabel}
        cardClosed={allClosed}
      />
```

Update `SecondaryList`'s props (currently ~lines 369-379) and forward it:

```tsx
function SecondaryList({
  markets,
  prices,
  inView,
  getLabel,
  cardClosed,
}: {
  markets: Market[];
  prices: Record<number, MarketPrice>;
  inView: boolean;
  getLabel: (m: Market) => string;
  cardClosed: boolean;
}) {
```

In its `markets.map(...)` (currently ~lines 390-399), pass `cardClosed` to each
row:

```tsx
      {markets.map((m, i) => (
        <SecondaryRow
          key={m.market_id}
          market={m}
          price={prices[m.market_id]}
          first={i === 0}
          inView={inView}
          getLabel={getLabel}
          cardClosed={cardClosed}
        />
      ))}
```

- [ ] **Step 4: Suppress per-row dim/tag when the whole card is closed**

Update `SecondaryRow`'s props (currently ~lines 404-416) to accept `cardClosed`:

```tsx
function SecondaryRow({
  market,
  price,
  first,
  inView,
  getLabel,
  cardClosed,
}: {
  market: Market;
  price: MarketPrice | undefined;
  first?: boolean;
  inView: boolean;
  getLabel: (m: Market) => string;
  cardClosed: boolean;
}) {
```

Add a derived flag right after `const label = getLabel(market);`
(currently ~line 417):

```tsx
  // Per-row greying only when this row is closed inside an OPEN card. When the
  // whole card is closed the <article> already dims at 0.5 — self-dimming here
  // would compound to 0.25.
  const rowClosed = market.closed === true && !cardClosed;
```

In the `<Link>` style for the row, change the opacity line (currently ~line 441)
from:

```tsx
        opacity: market.closed === true ? 0.5 : 1,
```

to:

```tsx
        opacity: rowClosed ? 0.5 : 1,
```

In the label `<span>` color (currently ~line 448) change:

```tsx
          color: market.closed === true ? "var(--fg-4)" : "var(--fg-2)",
```

to:

```tsx
          color: rowClosed ? "var(--fg-4)" : "var(--fg-2)",
```

And the inline per-row "closed" tag (currently ~lines 457-470) — change its
guard from `market.closed === true &&` to `rowClosed &&`:

```tsx
        {rowClosed && (
          <span
            className="text-mono"
            style={{
              marginLeft: 6,
              fontSize: "9px",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
              color: "var(--fg-4)",
            }}
          >
            closed
          </span>
        )}
```

Note: the ranked-sort closed tiebreak (lines ~48-51) stays as-is — it harmlessly
no-ops when all outcomes are closed.

- [ ] **Step 5: Typecheck**

Run: `pnpm exec tsc --noEmit`
Expected: no errors in `multi-card.tsx`.

- [ ] **Step 6: Commit**

```bash
git add src/components/multi-card.tsx
git commit -m "feat(markets): card-level greyed state on fully-closed MultiCard"
```

---

### Task 6: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Run the whole test suite**

Run: `pnpm test`
Expected: PASS, including the new `select-index-cards.test.ts`.

- [ ] **Step 2: Typecheck the whole project**

Run: `pnpm exec tsc --noEmit`
Expected: no NEW errors in any file touched by this plan (`page.tsx`,
`select-index-cards.ts`, `markets-filter-bar.tsx`, `binary-card.tsx`,
`multi-card.tsx`). Pre-existing errors in unrelated files (concurrent work) are
out of scope — note them but do not fix here.

- [ ] **Step 3: Lint the touched files**

Run: `pnpm lint`
Expected: no new lint errors in the touched files.

- [ ] **Step 4: Manual smoke in the dev server**

Run: `pnpm dev`, open the index page, and confirm:
- Default: "Hide closed" chip is active (filled); no closed cards visible; URL
  has no `closed` param.
- Click "Hide closed" off: URL gains `?closed=show`; previously-hidden
  fully-closed events and closed standalone binaries now appear, dimmed, with a
  "closed" eyebrow tag, sunk to the bottom (later pages). They still navigate to
  detail on click.
- Toggle persists across refresh (param in URL) and resets to page 1 when
  flipped.
- A partially-closed event looks identical to before (open card, individual
  closed rows greyed) in both toggle states.
- Header tally ("N markets · M events") grows when closed are shown.

- [ ] **Step 5: Final commit (only if Steps 1-4 surfaced fixes)**

```bash
git add -A
git commit -m "fix(markets): address verification findings for hide-closed toggle"
```

---

## Self-review notes

- **Spec coverage:** State/URL → Task 2 (steps 2,6,7,8). Toggle control → Task 3.
  "Closed" definition → Task 2 step 3 (`!eventVisibleOnIndex` / `isClosed`).
  Filtering+ordering → Task 1 + Task 2 step 4. Card visuals → Tasks 4 & 5.
  Testing → Task 1 + Task 6.
- **Type consistency:** `CardItem` defined once (Task 1), imported by `page.tsx`
  (Task 2). `selectIndexCards(items, { query, sort, category, showClosed,
  eventTraders })` signature matches its call site. `hideClosed` /
  `onHideClosedChange` prop names match between `page.tsx` (Task 2 step 7) and
  `markets-filter-bar.tsx` (Task 3). `setHideClosed(hide)` maps to
  `update({ showClosed: !hide })`; the chip's active state is `hideClosed`
  (= `!showClosed`).
- **No placeholders.**
```
