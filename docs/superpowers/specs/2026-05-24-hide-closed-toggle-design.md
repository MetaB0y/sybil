# "Hide closed" toggle on the markets index

**Date:** 2026-05-24
**Status:** Approved (design)
**Area:** `frontend/web` — markets index page

## Problem

The markets index (`app/page.tsx`) silently hides closed markets:

- A **standalone binary** that is closed is skipped entirely (`page.tsx`).
- A **multi-outcome event** is hidden only when *every* outcome is closed
  (`eventVisibleOnIndex`). A partially-closed event already shows, with its
  closed outcome rows greyed inside `MultiCard`.

Users have no way to see closed events/markets, and no signal that a board has
resolved. We want an explicit **"Hide closed"** toggle (ON by default — current
behavior) that, when turned OFF, surfaces the hidden closed events/binaries as
**greyed-out** cards so the user understands they are closed.

## Definitions

- **Closed market:** `market.closed === true` (`isClosed` in `use-markets.ts`).
- **Closed (binary) card:** the single market is closed.
- **Closed (multi) card:** *every* outcome in the event is closed
  (`!eventVisibleOnIndex(markets)`). A partially-closed event is **not** a
  closed card — it stays an open card with greyed rows, exactly as today, and is
  unaffected by the toggle.

The toggle governs exactly the set currently hidden: fully-closed events and
closed standalone binaries.

## Behavior

### State & URL

- New URL param `?closed=show`, read in `useFilterParams` (`page.tsx`).
- Absent (default) ⇒ **hide closed** (toggle ON). `=show` ⇒ show closed (toggle
  OFF). Matches the existing drop-default-from-URL convention (`?q`/`?sort`/
  `?category`); the default value is never written to the URL.
- The param is added to `filterKey`, so flipping the toggle resets pagination to
  page 1 (same mechanism as query/sort/category changes).

### Toggle control

- A chip in `MarketsFilterBar`, placed to the right of the sort chips.
- Label: **"Hide closed"**. Rendered in the active/filled style when ON
  (default), matching the sort-chip styling already in that bar.
- New props: `hideClosed: boolean` and `onHideClosedChange: (v: boolean) => void`.

### Filtering & ordering (`page.tsx`)

Chosen approach: build all cards once, filter/sort downstream (keeps `items`
stable; header tally and pagination follow automatically).

1. `items` builds **every** card, each tagged `closed: boolean`. The current
   `continue` skips for closed binaries and fully-closed events are removed.
2. `filtered`: when `!showClosed`, drop closed cards
   (`out = out.filter((it) => !it.closed)`).
3. Every sort comparator gets a top-level tiebreaker applied first:
   `if (a.closed !== b.closed) return a.closed ? 1 : -1;`
   Open cards keep the active sort; closed cards sink below — consistent across
   `volume` / `new` / `traders`. Pagination then pushes closed cards to later
   pages naturally.
4. The header tally (`shownMarkets` / `shownEvents`) already derives from
   `filtered`, so the counts reflect the toggle with no extra work.

### Card visual treatment

Mirror the existing closed-row idiom in `MultiCard` (`opacity: 0.5` + a small
uppercase mono "closed" tag in `--fg-4`), lifted to card level:

- **`BinaryCard`** — new closed state: whole card dimmed (`opacity: 0.5`) plus a
  small uppercase "Closed" badge by the title. Remains a clickable link to the
  detail page (detail already renders read-only/closed state).
- **`MultiCard`** — when *all* outcomes are closed (`allClosed`), apply the same
  card-level dim + "Closed" badge by the title. (Individual rows already grey.)
  Partially-closed cards are unchanged.
- Each card self-derives its closed state from its own market(s) — no behavior
  prop drilling from the page.

## Testing

- Extract the index card filter+sort out of the component into a small pure
  helper (e.g. `selectIndexCards(items, { showClosed, sort })` returning the
  ordered, filtered list), so it is unit-testable in isolation.
- Cases:
  - Closed cards hidden by default.
  - Closed cards shown when `showClosed` is set.
  - Closed cards sunk to the bottom across each sort mode (`volume`, `new`,
    `traders`).
  - Partially-closed multi event stays visible regardless of the toggle.
- Light render assertion for the card closed states (dim + "Closed" badge) if it
  fits the existing test setup.

## Files touched

- `frontend/web/src/app/page.tsx` — URL param, toggle wiring, `closed` tag on
  `CardItem`, extracted filter/sort helper.
- `frontend/web/src/components/markets-filter-bar.tsx` — "Hide closed" chip +
  props.
- `frontend/web/src/components/binary-card.tsx` — closed visual state.
- `frontend/web/src/components/multi-card.tsx` — card-level closed state when all
  outcomes closed.
- Test file for the extracted filter/sort helper.

## Non-goals

- No backend changes (the `closed` flag already ships on `MarketResponse`).
- No change to partially-closed multi-event behavior.
- No change to the clearing ticker (it already excludes closed markets via
  `openById`).
