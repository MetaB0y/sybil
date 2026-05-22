# Degen Buy — UI Wiring + Fill Animation Design

**Date:** 2026-05-22
**Status:** Approved (design). Follow-up to the core-logic module.
**Scope:** Activate the degen rail's "Bet $X" button and add an inline submit → fill-progress → result animation. Frontend-only — no backend/API/schema changes.

Builds on the core-logic module `frontend/web/src/lib/degen/` (see `2026-05-22-degen-buy-core-logic-design.md`).

---

## Goal

Make the degen rail's currently-disabled CTA place a real degen order and show the user what happens to it: a live countdown over the order's lifetime, fill progress as partial fills land, and a final result (filled / partial / no fill). One degen bet per rail at a time.

## Key decisions

1. **FE-only, no backend.** Submit returns only `{accepted}` (no order id). We correlate the placed order client-side and track it via existing endpoints/streams. No deploy required.
2. **One active degen bet per rail.** While a bet is in flight the rail is taken over by the progress view, so a second bet can't be started until the first resolves and the user taps "Bet again". This also removes any ambiguity about which resting order is "ours".
3. **Inline rail takeover** for the animation (not a modal): the form area morphs into the progress card, then the result card, then back to the form.
4. **Reuse existing auth + submit.** Session via `useAccountSession()`; submission via `submitSignedOrder` (the signed path the pro `BuyBox` uses).

## Data sources (all already present; no backend change)

- **Submit:** `submitSignedOrder({ accountId, publicKeyHex, marketId, side, limitPriceNanos, maxFill, expiresAtBlock })` → `{ accepted }` (`frontend/web/src/lib/account/orders.ts`).
- **Tracking — the account events feed:** `GET /v1/accounts/{id}/events` → `HistoryEventResponse[]`. Each row carries `type` (`placed` | `partial_fill` | `filled` | `expired` | …), `block_height`, `market_id`, `order_id`, `side` (`"BUY"`/`"SELL"`), `outcome` (`"YES"`/`"NO"`), `qty`, `price_nanos`. **The fill events themselves carry `market_id` + `outcome`, so a degen order is fully trackable even when it fills instantly and never rests.** The endpoint exists; the FE just needs a small fetch hook (the existing `useAccountHistory` is a mock for the Portfolio tab and is left untouched).
- **Height + cadence:** `selectLatestHeight` from the store (WS-fed) + `BLOCK_INTERVAL_MS` (10s) for the countdown. Block granularity matches the events feed (fills only occur at batch boundaries = each block), so per-block polling of the feed is the right cadence.

Why the events feed, not the WS `fills[]` or `useAccountOrders`: the WS `FillResponse` has **no `market_id`** (only `order_id`/`fill_qty`/`fill_price_nanos`/`account_id`), so a fill can't be attributed to our order before we know its id; and `useAccountOrders` only lists *resting* orders, so an instant full fill — the typical degen outcome for a marketable limit — would never appear there. The events feed avoids both gaps.

## Order correlation (FE-only)

At submit time we know: `marketId`, `outcome` (`"YES"`/`"NO"` from the bet side), `targetQty` (= `maxFill`), and `submitHeight` (latest height at click). We compute `expiresAtBlock = submitHeight + DEGEN_BATCHES`.

**Bind the concrete `order_id`** from the first events-feed row that matches `market_id === marketId`, `outcome === ourOutcome`, `side === "BUY"`, and `block_height >= submitHeight`, considering rows of type `placed`, `partial_fill`, or `filled`. Because only one degen bet is active per rail, this is unambiguous. Binding off fill rows (not just `placed`) means an instant full fill still binds.

Once bound, all progress is keyed off that `order_id`.

## Submit state machine (hosted in `degen-rail.tsx`)

```
idle ──(click, no session)──> openConnectModal(); stay idle
idle ──(click, below-minimum)──> stay idle (inline "raise your bet")
idle ──(click, valid)──> signing
signing ──(submitSignedOrder ok)──> tracking
signing ──(error/!accepted)──> idle (+ error message)
tracking ──(resolved)──> result
result ──("Bet again")──> idle
```

- **idle:** form (outcome picker / yes-no / amount) + CTA. CTA label: no session → `Connect to bet`; valid → `Bet $X on YES`; below-minimum → disabled + inline hint.
- **signing:** CTA shows `Signing…`, inputs locked.
- **tracking / result:** form replaced by the progress card (below). Inputs hidden; no new submit possible (enforces one-at-a-time).

## Resolution logic — a pure reducer (testable)

Two pure functions live in the degen module (`frontend/web/src/lib/degen/track.ts`), both unit-tested:

**`findDegenOrderId(events, criteria) -> number | null`** — the binding rule above. `criteria = { marketId, outcome, submitHeight }`; scans `placed`/`partial_fill`/`filled` rows with `side === "BUY"` matching market+outcome and `block_height >= submitHeight`, returns the earliest matching `order_id` (or `null`).

**`resolveDegenBet(snapshot) -> DegenBetState`** — the phase reducer.

Inputs (`snapshot`):
- `targetQty: bigint` (requested `maxFill`)
- `currentHeight: bigint`
- `expiresAtBlock: bigint`
- `bound: boolean` (has `order_id` been identified)
- `events: { type: string; qty: bigint; priceNanos: bigint }[]` (the bound order's `partial_fill`/`filled`/`expired` rows; empty if not yet bound)

Output:
```ts
type DegenPhase = "tracking" | "filled" | "partial" | "none";
interface DegenBetState {
  phase: DegenPhase;
  filledQty: bigint;            // Σ qty of partial_fill + filled rows
  targetQty: bigint;
  avgPriceNanos: bigint | null; // Σ(qty*price)/Σqty over filled rows, else null
}
```

Rules (terminal events win; height is a backstop so a missed terminal row can't hang the spinner):
- `filledQty = Σ qty` over rows of type `partial_fill` or `filled`.
- a `filled` row present **OR** `filledQty >= targetQty` → **filled** (can resolve before expiry).
- else an `expired` row present → `filledQty > 0` ? **partial** : **none**.
- else `currentHeight >= expiresAtBlock + 1` (one-block grace backstop) → `filledQty > 0` ? **partial** : **none**. (Covers a correlation miss / silent reject too: unbound + past expiry → **none**.)
- else → **tracking**.
- `avgPriceNanos`: volume-weighted over filled rows; `null` when none (display falls back to the limit cap `≤Y¢`).

Both functions are fully deterministic → unit-tested across: binding (match, no-match, before-submit-height, instant-fill row, earliest-wins); and resolution (tracking, early full fill, partial-then-expire, zero-then-expire/none, missed-terminal height backstop, unbound-past-expiry → none, avg-price weighting).

## Tracking hook (thin wiring)

A small REST hook `useAccountEvents(accountId)` (`frontend/web/src/lib/account/use-account-events.ts`) fetches `GET /v1/accounts/{id}/events` and invalidates per block — mirroring `useAccountOrders` exactly. Returns the raw `HistoryEventResponse[]`.

`useDegenBetTracker(active)` in `frontend/web/src/lib/degen/use-degen-bet-tracker.ts`:
- Input `active: { accountId, marketId, outcome: DegenSide, targetQty, submitHeight, expiresAtBlock } | null`.
- Reads `useAccountEvents(accountId)` + `selectLatestHeight`.
- Calls `findDegenOrderId` to bind `order_id` (held in a ref once found, so it survives the order leaving any list); filters the bound order's `partial_fill`/`filled`/`expired` rows; calls `resolveDegenBet`.
- Returns `{ phase, filledQty, targetQty, avgPriceNanos, secondsLeft, timeProgress01 }`, where `secondsLeft`/`timeProgress01` come from a RAF countdown anchored at `submit` over the window `(expiresAtBlock - submitHeight) * BLOCK_INTERVAL_MS` (≈30s), throttled ~100ms like `use-batch-countdown.ts`.

## Progress card component (presentational)

`frontend/web/src/components/market-rail/degen-progress.tsx` — pure props in, no data fetching:
- **tracking:** header `FILLING…` + `⏱ {secondsLeft}s`; a time progress bar (`timeProgress01`); a fill line `{filledQty} / {targetQty} sh @ ≤{Y}¢`, with the filled fraction visually emphasized.
- **filled:** ✅ `FILLED {qty} sh @ {avg}¢` + `Bet again`.
- **partial:** ◐ `PARTIAL — {filled} of {target} filled, rest expired` + `Bet again`. (Unfilled budget is automatically freed by the engine at expiry.)
- **none:** ✕ `NO FILL — nobody took the other side` + `Bet again`.
- Styling matches existing rail conventions: side color (`--yes`/`--no`), `--accent` for the time bar, mono for readouts; bar animates via the RAF `progress01` + short CSS transition, mirroring `next-batch-banner.tsx`.

## Edge cases

- **No session:** CTA `Connect to bet` → `openConnectModal(true)`; never submits.
- **below-minimum** (`buildDegenOrder` returns `{ ok:false }`): CTA disabled, inline "raise your bet"; never submits.
- **Submit rejected / network error:** return to `idle`, show the error; no tracking entered.
- **Instant full fill (typical):** a marketable degen limit usually fully fills in its first batch and never rests. The `filled` event carries `market_id` + `outcome`, so it binds and resolves to **filled** — no reliance on the order ever appearing in a resting-orders list.
- **Order never binds within the window** (correlation miss, e.g. order rejected silently): when `currentHeight >= expiresAtBlock + 1` with no bound order and no fill rows → resolve as **none** (don't hang on the spinner).
- **Navigation away mid-bet:** rail unmounts; tracking state is local and discarded (acceptable — engine still settles the order independently).
- **Mark source for Y:** the rail passes the **selected side's** last-batch mark (history last point `yes_price_nanos` for YES, `no_price_nanos` for NO) through `resolveMarkNanos` → `buildDegenOrder`.

## Testing

- **`findDegenOrderId` (vitest):** binds on a matching `placed` row; binds on a `filled` row when no `placed` (instant fill); ignores wrong market/outcome/side; ignores rows before `submitHeight`; returns the earliest match; returns `null` when nothing matches.
- **`resolveDegenBet` (vitest):** tracking; early full fill (`filledQty >= target`); explicit `filled` row; partial-then-`expired`; zero-then-`expired` → none; missed-terminal height backstop (`currentHeight >= expiresAtBlock + 1`); unbound-past-expiry → none; avg-price volume weighting.
- The hook, `useAccountEvents`, and the presentational component are thin wiring; covered by the two reducers' tests + manual/dev verification. (No backend, so `pnpm exec vitest run` + the dev server suffice.)

## Out of scope

- Any backend change (returning order ids, new endpoints).
- Cross-rail/global bet locking (one-at-a-time is per rail).
- Persisting bet state across navigation/refresh.
