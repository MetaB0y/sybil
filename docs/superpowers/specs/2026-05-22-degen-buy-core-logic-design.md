# Degen Buy — Core Logic Design

**Date:** 2026-05-22
**Status:** Approved (core logic). Frontend wiring is a deferred follow-up phase.
**Scope:** A single, isolated, pure frontend module that turns a degen "Bet $X" action into a marketable limit order. No backend or API changes.

---

## Goal

On a market page, "degen mode" lets a user bet a dollar amount without thinking about slippage — they accept a "degen tax." Pressing **Bet $X on YES/NO** places a limit **buy** at a price `Y` that is deliberately *worse* than the latest mark price (so it crosses and fills), good for the next 3 batches.

This spec defines only the **core logic**: the pure functions that compute `Y`, the order size, and the expiry. UI wiring is out of scope here (see "Deferred").

## Background / context

- **Mark price is already available to the frontend.** The deployed backend records a mark-price series (`clearing-if-volume>0 → book touch-midpoint → carry last mark → last clearing → 50/50`) into `price_history`, served by `GET /v1/markets/{id}/prices/history`. Each `PricePoint` carries both `yes_price_nanos` and `no_price_nanos`; `volume_nanos == 0` marks a no-trade (indicative) tick. The chart already loads this series. **The last point of the series is the latest mark price.**
- **The lightweight "current price" surfaces** (`/v1/markets`, `/v1/markets/prices`, the WS `clearing_prices_nanos` stream, and the store's `pricesByMarketId`) still carry **clearing price**, not mark.
- **Degen UI exists but is disabled.** `frontend/web/src/components/market-rail/degen-rail.tsx` renders a hard-`disabled` "Bet $X on …" button that submits nothing today.
- **The signed order path already supports everything we need:** `submitSignedOrder` (`frontend/web/src/lib/account/orders.ts`) → `POST /v1/orders/signed`, with `limit_price_nanos`, `max_fill`, `time_in_force: "GTD"`, and `expires_at_block`. One block = one batch; an unfilled remainder auto-rests across batches and is auto-expired/released at `expires_at_block` (`order_book.rs::settle`).

## Key decisions

1. **Source `Y`'s mark input from the price-history series' last point** (the chart already loads it). Zero backend change; the whole feature ships frontend-only. Acceptable freshness: between blocks the last point can lag a few seconds, but the deviation buffer absorbs it.
2. **Mark price drives `Y`, not clearing price.** Mark and clearing coincide for quiet markets today, but diverge exactly when the book moves without a trade — the staleness/manipulation case degen must respect.
3. **The logic lives in one isolated, pure module** (`frontend/web/src/lib/degen/`) so it can be re-tuned without touching the UI.

---

## Module: `frontend/web/src/lib/degen/`

All functions are pure, side-effect-free, and operate in **nanos** (`1e9 nanos = $1`), the unit used across the order path. No React, no network.

### Constants (the tunables)

```ts
export const DEGEN_PEAK_NANOS = 40_000_000n;   // $0.04 = 4¢: deviation at 50¢
export const DEGEN_EXPONENT   = 1.3;           // curve steepness toward the edges
export const DEGEN_BATCHES    = 3n;            // order lives for the next N batches
export const ONE_DOLLAR_NANOS = 1_000_000_000n;
```

Re-tuning the degen tax = editing `DEGEN_PEAK_NANOS` and `DEGEN_EXPONENT`. Re-tuning the lifetime = editing `DEGEN_BATCHES`.

### 1. `degenDeviation(priceNanos): bigint`

The degen tax in nanos — symmetric around 50¢, a power-law hump that collapses toward the edges:

```
p      = Number(priceNanos) / Number(ONE_DOLLAR_NANOS)   // 0..1 (float)
factor = (4 * p * (1 - p)) ** EXPONENT                   // dimensionless, 0..1
dev    = round(Number(DEGEN_PEAK_NANOS) * factor)        // integer nanos → bigint
```

The shaping factor `4*p*(1-p) ∈ [0,1]` peaks at `1` when `p = 0.5`, so `dev(0.5) = DEGEN_PEAK_NANOS` (= 4¢). Computed in floating point on `p ∈ [0,1]`, then converted back to integer nanos (round to nearest).

Reference values at `PEAK = 4¢`, `EXPONENT = 1.3` (symmetric: 95¢ ≈ 5¢, etc.):

| price | deviation |
|------:|----------:|
| 50¢   | 4.00¢     |
| 25¢   | 2.74¢     |
| 10¢   | 1.06¢     |
| 5¢    | 0.46¢     |
| 2¢    | 0.146¢    |
| 1¢    | 0.060¢    |
| 0.2¢  | 0.0075¢   |

### 2. `degenLimitPrice(sideMarkNanos): bigint` → **Y**

```
Y = sideMarkNanos + degenDeviation(sideMarkNanos)   // buying = pay more
Y = clamp(Y, 1n, ONE_DOLLAR_NANOS - 1n)             // never ≥ $1, never ≤ 0
```

`sideMarkNanos` is the **side's own mark field** from the last history point — `yes_price_nanos` for a YES bet, `no_price_nanos` for a NO bet. Reading the side's own field (rather than deriving NO = $1 − YES) is robust to rounding and needs no symmetry assumption. The deviation curve is symmetric, so a YES bet at 8¢ and a NO bet at 8¢ receive the same tax regardless.

### 3. `degenQuantity(budgetNanos, limitNanos): bigint` → **max_fill**

```
shares = budgetNanos / limitNanos   // integer floor: "at most $X worth"
```

`shares == 0n` (budget can't afford one share at `Y`) → the bet is invalid; the caller surfaces a minimum-bet state and does not submit.

### 4. `degenExpiry(latestHeight): bigint` → **expires_at_block**

```
expires_at_block = latestHeight + DEGEN_BATCHES
```

Submitted with `time_in_force: "GTD"`. The remainder auto-rests across batches and is auto-expired at `expires_at_block`.

---

## Mark-source read (the only external input)

A small reader (frontend, but separate from the pure module) supplies `sideMarkNanos`:

1. **Primary:** the **last point** of `GET /v1/markets/{id}/prices/history` → `yes_price_nanos` / `no_price_nanos` for the chosen side. The series value is current even when flat (no-trade ticks are coalesced, so the last point's *value* is the live mark; only its timestamp is older).
2. **Fallback** (market has no history points yet): the store's clearing price (`pricesByMarketId[marketId]`).
3. **Last resort:** 50¢ (`ONE_DOLLAR_NANOS / 2`).

## Order submission

Reuses the existing signed path unchanged:

```ts
submitSignedOrder({
  accountId, publicKeyHex,
  marketId,
  side: betSide === "YES" ? "BuyYes" : "BuyNo",
  limitPriceNanos: Y,           // degenLimitPrice(...)
  maxFill: shares,              // degenQuantity(...)
  expiresAtBlock: degenExpiry(latestHeight),   // → time_in_force "GTD"
});
```

No new backend, no new API field, no schema change.

## Edge cases

- **Y is a slippage cap, not the price paid.** Sybil clears uniform-price; a buy with limit `Y` matches at the batch clearing price (≤ `Y`). The deviation is a ceiling, often less in practice.
- **Clamp** keeps `Y` strictly inside `(0, $1)` so a near-edge buy can never exceed the $1 payout.
- **Zero/empty mark** → fallback chain above; never produce `Y = 0`.
- **`shares == 0`** → invalid bet (don't submit).
- **Rounding:** deviation rounds to nearest nano; `shares` floors. Both favor not overspending the stated budget.

## Properties

- Pure functions of `(mark, budget, height)` → fully deterministic and exhaustively unit-testable.
- The degen tax is bounded (`≤ PEAK`), symmetric, and monotonically shrinks toward both edges.

## Testing (vitest — already configured in the repo)

- `degenDeviation`: matches the reference table within tolerance at 50/25/10/5/2/1/0.2¢; symmetry `dev(p) == dev(1−p)`; peak at 50¢; monotonic decrease toward each edge; `dev` ≤ `PEAK`.
- `degenLimitPrice`: `Y > mark` for interior prices; clamp holds near $1 and near $0; never returns ≥ $1 or ≤ 0.
- `degenQuantity`: floor behavior; returns `0n` when budget < `Y`; exact for clean divisions (e.g. $10 @ 50¢ → 20).
- `degenExpiry`: `latestHeight + DEGEN_BATCHES`.

---

## Deferred (next phase — "frontend things we'll add")

Not part of this core-logic spec; planned separately:

- Wiring the module into `degen-rail.tsx` (enable the currently-disabled button).
- Reading `latestHeight` (from the store/WS) and the market's history last point into the component.
- Amount/side input UX and the payout / "max degen tax" display.
- Auth/session gating for submission (the button notes "wallet auth coming soon").
