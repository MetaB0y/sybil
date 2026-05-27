# Degen Buy — Core Logic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure, isolated frontend module (`frontend/web/src/lib/degen/`) that turns a degen "Bet $X on YES/NO" action into a marketable limit-buy order spec (limit price, share count, expiry) — no backend or API changes.

**Architecture:** Six pure functions over `bigint` nanos: a power-law degen-tax `degenDeviation`, a `degenLimitPrice` (mark + tax, clamped inside `(0, $1)`), a `degenQuantity` (`$X → shares`, floor), a `degenExpiry` (`height + N batches`), a `resolveMarkNanos` fallback chooser, and a `buildDegenOrder` composer returning a result ready for the existing `submitSignedOrder`. All tunables live in `constants.ts`. UI wiring is explicitly out of scope (deferred phase).

**Tech Stack:** TypeScript, vitest (env `node`), Next.js app at `frontend/web`. Test runner: `pnpm exec vitest run <file>` from `frontend/web`. Path alias `@` → `frontend/web/src`.

**Spec:** `docs/superpowers/specs/2026-05-22-degen-buy-core-logic-design.md`

---

## Conventions for every task

- All work happens under `frontend/web/`. Run commands from that directory.
- Nanos are `bigint` throughout (`1_000_000_000n = $1`). The module never parses wire JSON — callers pass already-parsed bigints.
- Tests are colocated and named `*.test.ts`; the vitest include glob already covers `src/**/*.test.ts`.
- This is frontend work on the `r/dev` branch (per `frontend/CLAUDE.md`: "Don't put backend work on r/dev" — this is FE, so r/dev is correct). New files are not imported by any running UI, so the dev server is unaffected.

## File structure

- Create: `frontend/web/src/lib/degen/constants.ts` — tunables only (Task 1).
- Create/extend: `frontend/web/src/lib/degen/degen.ts` — the six functions + types (Tasks 1–5).
- Create: `frontend/web/src/lib/degen/index.ts` — barrel re-export (Task 5).
- Tests (one file per task, all in `frontend/web/src/lib/degen/`):
  - `degen-deviation.test.ts` (Task 1)
  - `degen-limit-price.test.ts` (Task 2)
  - `degen-quantity-expiry.test.ts` (Task 3)
  - `degen-mark.test.ts` (Task 4)
  - `build-degen-order.test.ts` (Task 5)

---

### Task 1: Tunables + `degenDeviation` (the curve)

**Files:**
- Create: `frontend/web/src/lib/degen/constants.ts`
- Create: `frontend/web/src/lib/degen/degen.ts`
- Test: `frontend/web/src/lib/degen/degen-deviation.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/degen-deviation.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { DEGEN_PEAK_NANOS, ONE_DOLLAR_NANOS } from "./constants";
import { degenDeviation } from "./degen";

const cents = (c: number): bigint => BigInt(Math.round(c * 1e7)); // 1¢ = 1e7 nanos

describe("degenDeviation", () => {
  it("peaks at exactly DEGEN_PEAK_NANOS at 50¢", () => {
    expect(degenDeviation(ONE_DOLLAR_NANOS / 2n)).toBe(DEGEN_PEAK_NANOS);
  });

  it("matches the reference table within 0.02¢", () => {
    const tol = cents(0.02);
    const within = (priceCents: number, expectedCents: number) => {
      const got = degenDeviation(cents(priceCents));
      const diff = got > cents(expectedCents) ? got - cents(expectedCents) : cents(expectedCents) - got;
      expect(diff <= tol).toBe(true);
    };
    within(50, 4.0);
    within(25, 2.74);
    within(10, 1.06);
    within(5, 0.46);
    within(2, 0.15);
    within(1, 0.067);
  });

  it("is symmetric around 50¢", () => {
    for (const c of [1, 5, 10, 25, 40]) {
      expect(degenDeviation(cents(c))).toBe(degenDeviation(ONE_DOLLAR_NANOS - cents(c)));
    }
  });

  it("decreases monotonically from the center toward the edge", () => {
    const seq = [50, 25, 10, 5, 1].map((c) => degenDeviation(cents(c)));
    for (let i = 1; i < seq.length; i++) {
      expect(seq[i] < seq[i - 1]).toBe(true);
    }
  });

  it("never exceeds the peak and is zero at the boundaries", () => {
    for (const c of [1, 5, 25, 50, 75, 99]) {
      expect(degenDeviation(cents(c)) <= DEGEN_PEAK_NANOS).toBe(true);
    }
    expect(degenDeviation(0n)).toBe(0n);
    expect(degenDeviation(ONE_DOLLAR_NANOS)).toBe(0n);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-deviation.test.ts`
Expected: FAIL — cannot resolve `./constants` / `./degen` (modules do not exist yet).

- [ ] **Step 3: Create the constants file**

Create `frontend/web/src/lib/degen/constants.ts`:

```ts
/**
 * Degen-buy tunables. Re-tuning the degen tax or order lifetime happens here
 * and nowhere else.
 *
 * See docs/superpowers/specs/2026-05-22-degen-buy-core-logic-design.md
 */

/** 1 USD in nanos — the unit used across the order path. */
export const ONE_DOLLAR_NANOS = 1_000_000_000n;

/** Deviation at 50¢: the peak of the degen-tax hump. $0.04 = 4¢. */
export const DEGEN_PEAK_NANOS = 40_000_000n;

/** Curve steepness: higher = the tax collapses faster toward the 0/$1 edges. */
export const DEGEN_EXPONENT = 1.3;

/** Order stays eligible for the next N batches (1 block = 1 batch). */
export const DEGEN_BATCHES = 3n;
```

- [ ] **Step 4: Implement `degenDeviation`**

Create `frontend/web/src/lib/degen/degen.ts`:

```ts
import { DEGEN_EXPONENT, DEGEN_PEAK_NANOS, ONE_DOLLAR_NANOS } from "./constants";

/**
 * The degen tax in nanos: a symmetric power-law hump that peaks at 50¢ and
 * collapses toward both edges. `dev(0.5) === DEGEN_PEAK_NANOS`; `dev` is 0 at
 * and outside the [0, $1] boundary.
 */
export function degenDeviation(priceNanos: bigint): bigint {
  const p = Number(priceNanos) / Number(ONE_DOLLAR_NANOS);
  if (p <= 0 || p >= 1) return 0n;
  const factor = (4 * p * (1 - p)) ** DEGEN_EXPONENT; // dimensionless, 0..1
  return BigInt(Math.round(Number(DEGEN_PEAK_NANOS) * factor));
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-deviation.test.ts`
Expected: PASS (all 5 tests in the `degenDeviation` suite green).

- [ ] **Step 6: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/constants.ts frontend/web/src/lib/degen/degen.ts frontend/web/src/lib/degen/degen-deviation.test.ts
git commit -m "feat(degen): tunables + power-law degen-tax deviation curve"
```

---

### Task 2: `degenLimitPrice` (mark + tax, clamped)

**Files:**
- Modify: `frontend/web/src/lib/degen/degen.ts`
- Test: `frontend/web/src/lib/degen/degen-limit-price.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/degen-limit-price.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ONE_DOLLAR_NANOS } from "./constants";
import { degenDeviation, degenLimitPrice } from "./degen";

const cents = (c: number): bigint => BigInt(Math.round(c * 1e7));

describe("degenLimitPrice", () => {
  it("adds the tax to the mark for an interior price", () => {
    const mark = ONE_DOLLAR_NANOS / 2n; // 50¢
    expect(degenLimitPrice(mark)).toBe(mark + degenDeviation(mark));
  });

  it("is strictly worse (higher) than the mark for interior prices", () => {
    for (const c of [8, 25, 50, 75, 92]) {
      const mark = cents(c);
      expect(degenLimitPrice(mark) > mark).toBe(true);
    }
  });

  it("clamps to the lower bound at price 0", () => {
    expect(degenLimitPrice(0n)).toBe(1n);
  });

  it("never reaches or exceeds $1, and stays positive, across the range", () => {
    for (const c of [0.1, 1, 5, 50, 95, 99, 99.99]) {
      const y = degenLimitPrice(cents(c));
      expect(y > 0n).toBe(true);
      expect(y < ONE_DOLLAR_NANOS).toBe(true);
    }
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-limit-price.test.ts`
Expected: FAIL — `degenLimitPrice is not a function` (export does not exist yet).

- [ ] **Step 3: Implement `degenLimitPrice`**

Append to `frontend/web/src/lib/degen/degen.ts`:

```ts
/**
 * The degen limit price `Y` for a buy: the side's mark made worse (higher) by
 * the degen tax, clamped strictly inside `(0, $1)` so a near-edge buy can never
 * exceed the $1 payout.
 */
export function degenLimitPrice(sideMarkNanos: bigint): bigint {
  const raw = sideMarkNanos + degenDeviation(sideMarkNanos);
  const max = ONE_DOLLAR_NANOS - 1n;
  if (raw < 1n) return 1n;
  if (raw > max) return max;
  return raw;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-limit-price.test.ts`
Expected: PASS (all 4 tests green).

- [ ] **Step 5: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/degen.ts frontend/web/src/lib/degen/degen-limit-price.test.ts
git commit -m "feat(degen): limit price = mark + tax, clamped inside (0, \$1)"
```

---

### Task 3: `degenQuantity` + `degenExpiry`

**Files:**
- Modify: `frontend/web/src/lib/degen/degen.ts`
- Test: `frontend/web/src/lib/degen/degen-quantity-expiry.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/degen-quantity-expiry.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { degenExpiry, degenQuantity } from "./degen";

const usd = (d: number): bigint => BigInt(Math.round(d * 1e9)); // $1 = 1e9 nanos
const cents = (c: number): bigint => BigInt(Math.round(c * 1e7));

describe("degenQuantity", () => {
  it("returns budget / limit as a floored share count", () => {
    expect(degenQuantity(usd(10), cents(50))).toBe(20n); // $10 / 50¢ = 20
  });

  it("floors fractional shares (does not overspend)", () => {
    expect(degenQuantity(usd(10), cents(30))).toBe(33n); // 33.33 -> 33
  });

  it("returns 0 when the budget cannot afford one share", () => {
    expect(degenQuantity(usd(0.1), cents(50))).toBe(0n);
  });

  it("guards against non-positive inputs", () => {
    expect(degenQuantity(0n, cents(50))).toBe(0n);
    expect(degenQuantity(usd(10), 0n)).toBe(0n);
  });
});

describe("degenExpiry", () => {
  it("is the latest height plus DEGEN_BATCHES", () => {
    expect(degenExpiry(100n)).toBe(103n); // DEGEN_BATCHES = 3
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-quantity-expiry.test.ts`
Expected: FAIL — `degenQuantity is not a function` / `degenExpiry is not a function`.

- [ ] **Step 3: Implement both functions**

Append to `frontend/web/src/lib/degen/degen.ts`:

```ts
import { DEGEN_BATCHES } from "./constants";
```

Add that import alongside the existing `./constants` import at the top (merge into one import line: `import { DEGEN_BATCHES, DEGEN_EXPONENT, DEGEN_PEAK_NANOS, ONE_DOLLAR_NANOS } from "./constants";`). Then append the functions:

```ts
/** Shares affordable for `budgetNanos` at limit `limitNanos` (integer floor). */
export function degenQuantity(budgetNanos: bigint, limitNanos: bigint): bigint {
  if (budgetNanos <= 0n || limitNanos <= 0n) return 0n;
  return budgetNanos / limitNanos;
}

/** Last eligible block height: the next `DEGEN_BATCHES` batches. */
export function degenExpiry(latestHeight: bigint): bigint {
  return latestHeight + DEGEN_BATCHES;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-quantity-expiry.test.ts`
Expected: PASS (all 5 tests green).

- [ ] **Step 5: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/degen.ts frontend/web/src/lib/degen/degen-quantity-expiry.test.ts
git commit -m "feat(degen): \$X->shares quantity (floor) and N-batch expiry"
```

---

### Task 4: `resolveMarkNanos` (fallback chooser)

**Files:**
- Modify: `frontend/web/src/lib/degen/degen.ts`
- Test: `frontend/web/src/lib/degen/degen-mark.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/degen-mark.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ONE_DOLLAR_NANOS } from "./constants";
import { resolveMarkNanos } from "./degen";

const FIFTY_CENTS = ONE_DOLLAR_NANOS / 2n;

describe("resolveMarkNanos", () => {
  it("prefers the history mark when present and positive", () => {
    expect(resolveMarkNanos(80_000_000n, 90_000_000n)).toBe(80_000_000n);
  });

  it("falls back to clearing when history is null", () => {
    expect(resolveMarkNanos(null, 90_000_000n)).toBe(90_000_000n);
  });

  it("falls back to clearing when history is zero", () => {
    expect(resolveMarkNanos(0n, 90_000_000n)).toBe(90_000_000n);
  });

  it("falls back to 50¢ when both are missing", () => {
    expect(resolveMarkNanos(null, null)).toBe(FIFTY_CENTS);
  });

  it("falls back to 50¢ when both are zero", () => {
    expect(resolveMarkNanos(0n, 0n)).toBe(FIFTY_CENTS);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-mark.test.ts`
Expected: FAIL — `resolveMarkNanos is not a function`.

- [ ] **Step 3: Implement `resolveMarkNanos`**

Append to `frontend/web/src/lib/degen/degen.ts`:

```ts
/**
 * Pick the mark to price against, in priority order: the (already-extracted)
 * history last-point mark, else the clearing price, else 50¢. `null` means the
 * source is unavailable; non-positive values are treated as unavailable.
 */
export function resolveMarkNanos(
  historyMarkNanos: bigint | null,
  clearingNanos: bigint | null,
): bigint {
  if (historyMarkNanos !== null && historyMarkNanos > 0n) return historyMarkNanos;
  if (clearingNanos !== null && clearingNanos > 0n) return clearingNanos;
  return ONE_DOLLAR_NANOS / 2n;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/degen-mark.test.ts`
Expected: PASS (all 5 tests green).

- [ ] **Step 5: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/degen.ts frontend/web/src/lib/degen/degen-mark.test.ts
git commit -m "feat(degen): mark-source fallback chooser (history -> clearing -> 50c)"
```

---

### Task 5: `buildDegenOrder` (composer) + barrel export

**Files:**
- Modify: `frontend/web/src/lib/degen/degen.ts`
- Create: `frontend/web/src/lib/degen/index.ts`
- Test: `frontend/web/src/lib/degen/build-degen-order.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/build-degen-order.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ONE_DOLLAR_NANOS } from "./constants";
import { buildDegenOrder, degenLimitPrice } from "./degen";

const usd = (d: number): bigint => BigInt(Math.round(d * 1e9));

describe("buildDegenOrder", () => {
  it("composes a YES bet into a BuyYes order spec", () => {
    const mark = ONE_DOLLAR_NANOS / 2n; // 50¢
    const res = buildDegenOrder({
      side: "YES",
      betUsdNanos: usd(10),
      markNanos: mark,
      latestHeight: 1000n,
    });
    expect(res.ok).toBe(true);
    if (!res.ok) return;
    const limit = degenLimitPrice(mark); // 5.4e8
    expect(res.order.side).toBe("BuyYes");
    expect(res.order.limitPriceNanos).toBe(limit);
    expect(res.order.maxFill).toBe(usd(10) / limit); // 18n
    expect(res.order.expiresAtBlock).toBe(1003n);
  });

  it("maps a NO bet to BuyNo", () => {
    const res = buildDegenOrder({
      side: "NO",
      betUsdNanos: usd(10),
      markNanos: ONE_DOLLAR_NANOS / 2n,
      latestHeight: 1n,
    });
    expect(res.ok).toBe(true);
    if (!res.ok) return;
    expect(res.order.side).toBe("BuyNo");
  });

  it("reports below-minimum when the budget can't afford one share", () => {
    const res = buildDegenOrder({
      side: "YES",
      betUsdNanos: usd(0.01),
      markNanos: ONE_DOLLAR_NANOS / 2n,
      latestHeight: 1n,
    });
    expect(res.ok).toBe(false);
    if (res.ok) return;
    expect(res.reason).toBe("below-minimum");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/build-degen-order.test.ts`
Expected: FAIL — `buildDegenOrder is not a function`.

- [ ] **Step 3: Implement `buildDegenOrder` and its types**

Add a type-only import for `OrderSide` at the top of `frontend/web/src/lib/degen/degen.ts` (type-only so the `"use client"` runtime of `orders.ts` is not pulled into this node-environment module):

```ts
import type { OrderSide } from "@/lib/account/orders";
```

Then append:

```ts
/** The side a degen bet backs. Maps to a buy on the order path. */
export type DegenSide = "YES" | "NO";

/** An order spec ready to spread into `submitSignedOrder` (caller adds account/market). */
export interface DegenOrder {
  side: OrderSide;
  limitPriceNanos: bigint;
  maxFill: bigint;
  expiresAtBlock: bigint;
}

export type DegenOrderResult =
  | { ok: true; order: DegenOrder }
  | { ok: false; reason: "below-minimum" };

/**
 * Compose the degen math into an order spec. `markNanos` is the already-resolved
 * mark for the chosen side (see `resolveMarkNanos`). Returns `below-minimum`
 * when the budget can't afford a single share at the degen limit price.
 */
export function buildDegenOrder(params: {
  side: DegenSide;
  betUsdNanos: bigint;
  markNanos: bigint;
  latestHeight: bigint;
}): DegenOrderResult {
  const limitPriceNanos = degenLimitPrice(params.markNanos);
  const maxFill = degenQuantity(params.betUsdNanos, limitPriceNanos);
  if (maxFill <= 0n) return { ok: false, reason: "below-minimum" };
  return {
    ok: true,
    order: {
      side: params.side === "YES" ? "BuyYes" : "BuyNo",
      limitPriceNanos,
      maxFill,
      expiresAtBlock: degenExpiry(params.latestHeight),
    },
  };
}
```

- [ ] **Step 4: Create the barrel export**

Create `frontend/web/src/lib/degen/index.ts`:

```ts
export * from "./constants";
export * from "./degen";
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/build-degen-order.test.ts`
Expected: PASS (all 3 tests green).

- [ ] **Step 6: Run the full degen suite + typecheck**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/`
Expected: PASS — all five test files green (22 tests total).

Run: `cd frontend/web && pnpm exec tsc --noEmit`
Expected: no new type errors from `src/lib/degen/` (pre-existing errors elsewhere, if any, are unrelated).

- [ ] **Step 7: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/degen.ts frontend/web/src/lib/degen/index.ts frontend/web/src/lib/degen/build-degen-order.test.ts
git commit -m "feat(degen): buildDegenOrder composer + module barrel export"
```

---

## Definition of done

- `frontend/web/src/lib/degen/` exports `degenDeviation`, `degenLimitPrice`, `degenQuantity`, `degenExpiry`, `resolveMarkNanos`, `buildDegenOrder`, the tunable constants, and the `DegenSide` / `DegenOrder` / `DegenOrderResult` types.
- `pnpm exec vitest run src/lib/degen/` is green.
- No backend, API, schema, or UI files changed (UI wiring is the deferred next phase).

## Out of scope (deferred next phase — "frontend things we'll add")

- Enabling and wiring the currently-disabled button in `frontend/web/src/components/market-rail/degen-rail.tsx`.
- A hook that reads the market's price-history last point (via `usePriceHistory` + `parseNanos`) and `latestHeight` (from the store/WS), feeds `resolveMarkNanos` + `buildDegenOrder`, then calls `submitSignedOrder`.
- Amount/side input UX and the payout / "max degen tax" display.
- Auth/session gating for submission.
