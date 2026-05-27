# Degen Buy — UI Wiring + Fill Animation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Activate the degen rail's "Bet $X" button to place a real degen order and show an inline submit → live fill-progress → result animation, frontend-only.

**Architecture:** Two pure, unit-tested functions (`findDegenOrderId`, `resolveDegenBet`) drive tracking off the account **events feed**; a small REST hook `useAccountEvents` + a thin `useDegenBetTracker` hook feed a presentational `DegenProgress` card; `degen-rail.tsx` becomes a small state machine (form → signing → tracking → result) reusing the existing session + signed-submit path. No backend/API/schema changes.

**Tech Stack:** TypeScript, React, vitest (env `node`; component tests use `react-dom/server` `renderToStaticMarkup`), Next.js app `frontend/web`. Run tests from `frontend/web` via `pnpm exec vitest run <file>`; typecheck via `pnpm exec tsc --noEmit`. Path alias `@` → `frontend/web/src`.

**Spec:** `docs/superpowers/specs/2026-05-22-degen-buy-ui-and-fill-animation-design.md`
**Builds on:** `frontend/web/src/lib/degen/` (core logic: `buildDegenOrder`, `resolveMarkNanos`, `DegenSide`, `DEGEN_BATCHES`, `ONE_DOLLAR_NANOS`).

---

## Conventions for every task

- All work under `frontend/web/`; run commands from there. Branch: `r/dev` (FE branch).
- Nanos are `bigint` (`parseNanos` from `@/lib/format/nanos`). Quantities/heights/ids are `number` on the wire (only `*_nanos` are string-typed).
- New files; the running dev server is unaffected until `degen-rail.tsx` is changed (Task 5).
- Do not touch the two files the other terminal has modified (`src/app/m/[id]/page.tsx`, `src/lib/markets/use-event-raw.ts`).

## File structure

- Create: `frontend/web/src/lib/degen/track.ts` — pure `findDegenOrderId` + `resolveDegenBet` + types (Task 1).
- Create: `frontend/web/src/lib/degen/track.test.ts` (Task 1).
- Modify: `frontend/web/src/lib/degen/index.ts` — re-export `./track` (Task 1).
- Create: `frontend/web/src/lib/account/use-account-events.ts` — REST events hook (Task 2).
- Create: `frontend/web/src/lib/degen/use-degen-bet-tracker.ts` — tracking hook (Task 3).
- Create: `frontend/web/src/components/market-rail/degen-progress.tsx` — progress/result card (Task 4).
- Create: `frontend/web/src/components/market-rail/degen-progress.test.tsx` (Task 4).
- Modify: `frontend/web/src/components/market-rail/degen-rail.tsx` — state machine + submit wiring (Task 5).

---

### Task 1: Pure tracking logic — `findDegenOrderId` + `resolveDegenBet`

**Files:**
- Create: `frontend/web/src/lib/degen/track.ts`
- Test: `frontend/web/src/lib/degen/track.test.ts`
- Modify: `frontend/web/src/lib/degen/index.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/lib/degen/track.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  findDegenOrderId,
  resolveDegenBet,
  type DegenEvent,
} from "./track";

function ev(p: Partial<DegenEvent>): DegenEvent {
  return {
    type: "filled",
    blockHeight: 0,
    marketId: 7,
    orderId: 1,
    side: "BUY",
    outcome: "YES",
    qty: 0n,
    priceNanos: 0n,
    ...p,
  };
}

const crit = { marketId: 7, outcome: "YES" as const, submitHeight: 100 };

describe("findDegenOrderId", () => {
  it("binds a matching placed row", () => {
    const events = [ev({ type: "placed", orderId: 42, blockHeight: 100 })];
    expect(findDegenOrderId(events, crit)).toBe(42);
  });

  it("binds a filled row when there is no placed (instant fill)", () => {
    const events = [ev({ type: "filled", orderId: 43, blockHeight: 101, qty: 5n })];
    expect(findDegenOrderId(events, crit)).toBe(43);
  });

  it("ignores wrong market, outcome, side, and pre-submit rows", () => {
    const events = [
      ev({ orderId: 1, marketId: 8 }),
      ev({ orderId: 2, outcome: "NO" }),
      ev({ orderId: 3, side: "SELL" }),
      ev({ orderId: 4, blockHeight: 99 }),
    ];
    expect(findDegenOrderId(events, crit)).toBeNull();
  });

  it("returns the earliest matching order id", () => {
    const events = [
      ev({ type: "filled", orderId: 9, blockHeight: 103 }),
      ev({ type: "placed", orderId: 8, blockHeight: 100 }),
    ];
    expect(findDegenOrderId(events, crit)).toBe(8);
  });

  it("returns null when nothing matches", () => {
    expect(findDegenOrderId([], crit)).toBeNull();
  });
});

describe("resolveDegenBet", () => {
  const base = { targetQty: 20n, currentHeight: 101, expiresAtBlock: 103 };

  it("is tracking with no events before expiry", () => {
    const s = resolveDegenBet({ ...base, events: [] });
    expect(s.phase).toBe("tracking");
    expect(s.filledQty).toBe(0n);
  });

  it("is filled when a filled row is present", () => {
    const s = resolveDegenBet({
      ...base,
      events: [ev({ type: "filled", qty: 20n, priceNanos: 530_000_000n })],
    });
    expect(s.phase).toBe("filled");
    expect(s.filledQty).toBe(20n);
    expect(s.avgPriceNanos).toBe(530_000_000n);
  });

  it("is filled when partial fills reach the target", () => {
    const s = resolveDegenBet({
      ...base,
      events: [
        ev({ type: "partial_fill", qty: 12n, priceNanos: 500_000_000n }),
        ev({ type: "partial_fill", qty: 8n, priceNanos: 600_000_000n }),
      ],
    });
    expect(s.phase).toBe("filled");
    expect(s.filledQty).toBe(20n);
    // volume-weighted: (12*5e8 + 8*6e8)/20 = 5.4e8
    expect(s.avgPriceNanos).toBe(540_000_000n);
  });

  it("is partial when an expired row follows some fills", () => {
    const s = resolveDegenBet({
      ...base,
      events: [
        ev({ type: "partial_fill", qty: 12n, priceNanos: 500_000_000n }),
        ev({ type: "expired" }),
      ],
    });
    expect(s.phase).toBe("partial");
    expect(s.filledQty).toBe(12n);
  });

  it("is none when expired with zero fills", () => {
    const s = resolveDegenBet({ ...base, events: [ev({ type: "expired" })] });
    expect(s.phase).toBe("none");
    expect(s.filledQty).toBe(0n);
    expect(s.avgPriceNanos).toBeNull();
  });

  it("falls back to height when the terminal row is missed", () => {
    const partial = resolveDegenBet({
      ...base,
      currentHeight: 104, // >= expiresAtBlock + 1
      events: [ev({ type: "partial_fill", qty: 5n, priceNanos: 5n })],
    });
    expect(partial.phase).toBe("partial");
    const none = resolveDegenBet({ ...base, currentHeight: 104, events: [] });
    expect(none.phase).toBe("none");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/track.test.ts`
Expected: FAIL — cannot resolve `./track`.

- [ ] **Step 3: Implement `track.ts`**

Create `frontend/web/src/lib/degen/track.ts`:

```ts
import type { DegenSide } from "./degen";

/** A degen-relevant row from the account events feed, normalized to bigint. */
export interface DegenEvent {
  type: string; // "placed" | "partial_fill" | "filled" | "expired" | ...
  blockHeight: number;
  marketId: number | null;
  orderId: number | null;
  side: string | null; // "BUY" | "SELL"
  outcome: string | null; // "YES" | "NO"
  qty: bigint;
  priceNanos: bigint;
}

export interface DegenCriteria {
  marketId: number;
  outcome: DegenSide;
  submitHeight: number;
}

/**
 * Bind our degen bet's order id from the events feed: the earliest
 * placed/partial_fill/filled BUY row for this market+outcome at or after the
 * submit height. Binding off fill rows (not just `placed`) means an order that
 * fills instantly and never rests is still found.
 */
export function findDegenOrderId(
  events: DegenEvent[],
  c: DegenCriteria,
): number | null {
  let best: { height: number; orderId: number } | null = null;
  for (const e of events) {
    if (e.orderId === null) continue;
    if (e.marketId !== c.marketId) continue;
    if (e.side !== "BUY") continue;
    if (e.outcome !== c.outcome) continue;
    if (e.blockHeight < c.submitHeight) continue;
    if (e.type !== "placed" && e.type !== "partial_fill" && e.type !== "filled") {
      continue;
    }
    if (best === null || e.blockHeight < best.height) {
      best = { height: e.blockHeight, orderId: e.orderId };
    }
  }
  return best?.orderId ?? null;
}

export type DegenPhase = "tracking" | "filled" | "partial" | "none";

export interface DegenBetState {
  phase: DegenPhase;
  filledQty: bigint;
  targetQty: bigint;
  avgPriceNanos: bigint | null;
}

export interface DegenSnapshot {
  targetQty: bigint;
  currentHeight: number;
  expiresAtBlock: number;
  /** The bound order's partial_fill/filled/expired rows (empty if unbound). */
  events: DegenEvent[];
}

/**
 * Resolve the bet's phase. Terminal rows (filled/expired) win; the height
 * backstop (`>= expiresAtBlock + 1`) covers a missed terminal row or a
 * correlation miss so the spinner can never hang.
 */
export function resolveDegenBet(s: DegenSnapshot): DegenBetState {
  let filledQty = 0n;
  let weighted = 0n;
  let hasFilled = false;
  let hasExpired = false;
  for (const e of s.events) {
    if (e.type === "partial_fill" || e.type === "filled") {
      filledQty += e.qty;
      weighted += e.qty * e.priceNanos;
      if (e.type === "filled") hasFilled = true;
    } else if (e.type === "expired") {
      hasExpired = true;
    }
  }
  const avgPriceNanos = filledQty > 0n ? weighted / filledQty : null;
  const base = { filledQty, targetQty: s.targetQty, avgPriceNanos };

  if (hasFilled || filledQty >= s.targetQty) return { phase: "filled", ...base };
  if (hasExpired) {
    return { phase: filledQty > 0n ? "partial" : "none", ...base };
  }
  if (s.currentHeight >= s.expiresAtBlock + 1) {
    return { phase: filledQty > 0n ? "partial" : "none", ...base };
  }
  return { phase: "tracking", ...base };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/track.test.ts`
Expected: PASS (all tests green).

- [ ] **Step 5: Re-export from the barrel**

Append to `frontend/web/src/lib/degen/index.ts` (after the existing exports):

```ts
export * from "./track";
```

- [ ] **Step 6: Verify the whole degen suite + typecheck**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/` — Expected: PASS (all degen tests, including the prior 24).
Run: `cd frontend/web && pnpm exec tsc --noEmit 2>&1 | grep "src/lib/degen" || echo CLEAN` — Expected: `CLEAN`.

- [ ] **Step 7: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/track.ts frontend/web/src/lib/degen/track.test.ts frontend/web/src/lib/degen/index.ts
git commit -m "feat(degen): pure bet-tracking logic (bind order id + resolve phase)"
```

---

### Task 2: `useAccountEvents` REST hook

**Files:**
- Create: `frontend/web/src/lib/account/use-account-events.ts`

This is thin wiring (a fetch hook) verified by typecheck; it mirrors the existing `use-account-orders.ts` exactly.

- [ ] **Step 1: Implement the hook**

Create `frontend/web/src/lib/account/use-account-events.ts`:

```ts
"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { selectLatestBlock, useStore } from "@/lib/store";

export type AccountEvent = components["schemas"]["HistoryEventResponse"];

/**
 * GET /v1/accounts/{id}/events — the per-account event log (placed /
 * partial_fill / filled / expired / …). Invalidates each block so the degen
 * tracker sees fills as batches clear. Mirrors `useAccountOrders`.
 */
export function useAccountEvents(accountId: number | null) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "events"] });
  }, [accountId, latest?.height, qc]);

  return useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "events"],
    queryFn: async (): Promise<AccountEvent[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/events", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account events failed");
      return data;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
```

- [ ] **Step 2: Typecheck**

Run: `cd frontend/web && pnpm exec tsc --noEmit 2>&1 | grep "use-account-events" || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 3: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/account/use-account-events.ts
git commit -m "feat(account): useAccountEvents — per-block events feed hook"
```

---

### Task 3: `useDegenBetTracker` hook

**Files:**
- Create: `frontend/web/src/lib/degen/use-degen-bet-tracker.ts`

Thin wiring over the Task-1 pure functions + Task-2 hook + a RAF countdown; verified by typecheck (logic is covered by Task 1's tests).

- [ ] **Step 1: Implement the hook**

Create `frontend/web/src/lib/degen/use-degen-bet-tracker.ts`:

```ts
"use client";

import { useEffect, useRef, useState } from "react";
import { useAccountEvents } from "@/lib/account/use-account-events";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";
import { parseNanos } from "@/lib/format/nanos";
import { selectLatestHeight, useStore } from "@/lib/store";
import type { DegenSide } from "./degen";
import {
  findDegenOrderId,
  resolveDegenBet,
  type DegenBetState,
  type DegenEvent,
} from "./track";

export interface DegenActive {
  accountId: number;
  marketId: number;
  outcome: DegenSide;
  targetQty: bigint;
  limitPriceNanos: bigint; // for display only
  submitHeight: number;
  expiresAtBlock: number;
}

export interface DegenTracking extends DegenBetState {
  secondsLeft: number;
  timeProgress01: number;
}

/**
 * Track an in-flight degen bet off the account events feed. Returns null when
 * inactive. `timeProgress01`/`secondsLeft` are a RAF countdown over the bet's
 * GTD window; `phase`/`filledQty`/`avgPriceNanos` come from the pure reducers.
 */
export function useDegenBetTracker(
  active: DegenActive | null,
): DegenTracking | null {
  const { data: rawEvents } = useAccountEvents(active?.accountId ?? null);
  const latestHeight = useStore(selectLatestHeight);

  const [timeProgress01, setTimeProgress01] = useState(0);
  const anchorRef = useRef<number | null>(null);
  const rafRef = useRef<number>(0);

  const submitHeight = active?.submitHeight ?? null;
  const expiresAtBlock = active?.expiresAtBlock ?? null;

  useEffect(() => {
    anchorRef.current = active ? performance.now() : null;
    setTimeProgress01(0);
  }, [active?.accountId, submitHeight, active?.marketId, active?.outcome]);

  useEffect(() => {
    if (submitHeight === null || expiresAtBlock === null) return;
    const totalMs = Math.max(
      1,
      (expiresAtBlock - submitHeight) * BLOCK_INTERVAL_MS,
    );
    let last = 0;
    const step = (t: number) => {
      if (anchorRef.current !== null && t - last >= 100) {
        last = t;
        const elapsed = performance.now() - anchorRef.current;
        setTimeProgress01(Math.min(1, elapsed / totalMs));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, [submitHeight, expiresAtBlock]);

  if (!active) return null;

  const events: DegenEvent[] = (rawEvents ?? []).map((e) => ({
    type: e.type,
    blockHeight: e.block_height,
    marketId: e.market_id ?? null,
    orderId: e.order_id ?? null,
    side: e.side ?? null,
    outcome: e.outcome ?? null,
    qty: e.qty != null ? BigInt(e.qty) : 0n,
    priceNanos: e.price_nanos != null ? parseNanos(e.price_nanos) : 0n,
  }));

  const boundId = findDegenOrderId(events, {
    marketId: active.marketId,
    outcome: active.outcome,
    submitHeight: active.submitHeight,
  });
  const ours = boundId === null ? [] : events.filter((e) => e.orderId === boundId);

  const state = resolveDegenBet({
    targetQty: active.targetQty,
    currentHeight: latestHeight ?? active.submitHeight,
    expiresAtBlock: active.expiresAtBlock,
    events: ours,
  });

  const totalMs = Math.max(
    1,
    (active.expiresAtBlock - active.submitHeight) * BLOCK_INTERVAL_MS,
  );
  const secondsLeft = Math.max(
    0,
    Math.ceil((totalMs * (1 - timeProgress01)) / 1000),
  );

  return { ...state, secondsLeft, timeProgress01 };
}
```

- [ ] **Step 2: Typecheck**

Run: `cd frontend/web && pnpm exec tsc --noEmit 2>&1 | grep "use-degen-bet-tracker" || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 3: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/lib/degen/use-degen-bet-tracker.ts
git commit -m "feat(degen): useDegenBetTracker — events-feed tracking + countdown"
```

---

### Task 4: `DegenProgress` card

**Files:**
- Create: `frontend/web/src/components/market-rail/degen-progress.tsx`
- Test: `frontend/web/src/components/market-rail/degen-progress.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `frontend/web/src/components/market-rail/degen-progress.test.tsx`:

```tsx
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DegenProgress } from "./degen-progress";

const common = {
  side: "YES" as const,
  secondsLeft: 24,
  timeProgress01: 0.4,
  filledQty: 12n,
  targetQty: 20n,
  limitPriceNanos: 540_000_000n,
  avgPriceNanos: 530_000_000n,
  onBetAgain: () => {},
};

describe("DegenProgress", () => {
  it("shows the countdown and fill meter while tracking", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="tracking" />,
    );
    expect(html).toMatch(/FILLING/i);
    expect(html).toContain("24s");
    expect(html).toContain("12");
    expect(html).toContain("20");
    expect(html).not.toMatch(/Bet again/i);
  });

  it("shows a filled result with avg price and a reset", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="filled" filledQty={20n} />,
    );
    expect(html).toMatch(/FILLED/i);
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a partial result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="partial" />,
    );
    expect(html).toMatch(/PARTIAL/i);
    expect(html).toContain("12");
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a no-fill result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="none" filledQty={0n} avgPriceNanos={null} />,
    );
    expect(html).toMatch(/NO FILL/i);
    expect(html).toMatch(/Bet again/i);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/web && pnpm exec vitest run src/components/market-rail/degen-progress.test.tsx`
Expected: FAIL — cannot resolve `./degen-progress`.

- [ ] **Step 3: Implement the component**

Create `frontend/web/src/components/market-rail/degen-progress.tsx`:

```tsx
"use client";

import type { DegenSide } from "@/lib/degen";
import type { DegenPhase } from "@/lib/degen/track";

/** Format price nanos as cents with one decimal (5.4e8 -> "54.0"). */
function cents(n: bigint): string {
  return (Number(n) / 1e7).toFixed(1);
}

export interface DegenProgressProps {
  phase: DegenPhase;
  side: DegenSide;
  secondsLeft: number;
  timeProgress01: number;
  filledQty: bigint;
  targetQty: bigint;
  limitPriceNanos: bigint;
  avgPriceNanos: bigint | null;
  onBetAgain: () => void;
}

export function DegenProgress(props: DegenProgressProps) {
  const accent = props.side === "YES" ? "var(--yes)" : "var(--no)";

  if (props.phase === "tracking") {
    return (
      <div style={cardStyle}>
        <div style={rowStyle}>
          <span style={labelStyle}>FILLING…</span>
          <span style={monoStyle}>⏱ {props.secondsLeft}s</span>
        </div>
        <div style={barTrackStyle}>
          <div
            style={{
              width: `${Math.round(props.timeProgress01 * 100)}%`,
              height: "100%",
              background: "var(--accent)",
              transition: "width 120ms linear",
            }}
          />
        </div>
        <div style={monoStyle}>
          {props.filledQty.toString()} / {props.targetQty.toString()} sh @ ≤
          {cents(props.limitPriceNanos)}¢
        </div>
      </div>
    );
  }

  const result =
    props.phase === "filled"
      ? `✅ FILLED ${props.targetQty.toString()} sh @ ${cents(
          props.avgPriceNanos ?? props.limitPriceNanos,
        )}¢`
      : props.phase === "partial"
        ? `◐ PARTIAL — ${props.filledQty.toString()} of ${props.targetQty.toString()} filled, rest expired`
        : `✕ NO FILL — nobody took the other side`;

  return (
    <div style={cardStyle}>
      <div style={{ ...rowStyle, color: accent }}>
        <span style={{ fontFamily: "var(--font-sans)", fontSize: 14, fontWeight: 700 }}>
          {result}
        </span>
      </div>
      <button type="button" onClick={props.onBetAgain} style={betAgainStyle}>
        Bet again
      </button>
    </div>
  );
}

const cardStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 10,
  padding: "16px",
  borderRadius: 6,
  border: "1px solid var(--border-2)",
  background: "var(--surface-2)",
};
const rowStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
};
const labelStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.06em",
  color: "var(--fg-3)",
};
const monoStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  color: "var(--fg-2)",
};
const barTrackStyle: React.CSSProperties = {
  height: 4,
  borderRadius: 2,
  background: "var(--border-1)",
  overflow: "hidden",
};
const betAgainStyle: React.CSSProperties = {
  padding: "12px 0",
  borderRadius: 6,
  border: "1px solid var(--border-2)",
  background: "transparent",
  color: "var(--fg-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/web && pnpm exec vitest run src/components/market-rail/degen-progress.test.tsx`
Expected: PASS (4 tests green).

- [ ] **Step 5: Typecheck**

Run: `cd frontend/web && pnpm exec tsc --noEmit 2>&1 | grep "degen-progress" || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 6: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/components/market-rail/degen-progress.tsx frontend/web/src/components/market-rail/degen-progress.test.tsx
git commit -m "feat(degen): DegenProgress card (countdown + fill meter + result)"
```

---

### Task 5: Wire `degen-rail.tsx` (state machine + submit)

**Files:**
- Modify: `frontend/web/src/components/market-rail/degen-rail.tsx` (full replacement)

Integration task; verified by typecheck + dev. Replaces the disabled placeholder button with the real flow.

- [ ] **Step 1: Replace the file**

Overwrite `frontend/web/src/components/market-rail/degen-rail.tsx` with:

```tsx
"use client";

/**
 * Degen rail — "tap & win" betting flow. Banner → outcome picker → yes/no →
 * amount → CTA. On submit the form area is replaced inline by a live
 * fill-progress card and then a result (DegenProgress). One bet at a time.
 */

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { submitSignedOrder } from "@/lib/account/orders";
import { useAccountSession, useSetConnectModalOpen } from "@/lib/account/use-account";
import { ONE_DOLLAR_NANOS, buildDegenOrder, resolveMarkNanos } from "@/lib/degen";
import {
  useDegenBetTracker,
  type DegenActive,
} from "@/lib/degen/use-degen-bet-tracker";
import { parseNanos } from "@/lib/format/nanos";
import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { usePriceHistory } from "@/lib/markets/use-price-history";
import { selectLatestHeight, useStore } from "@/lib/store";
import { DegenAmount } from "./degen-amount";
import { DegenOutcomePicker } from "./degen-outcome-picker";
import { DegenProgress } from "./degen-progress";
import { NextBatchBanner } from "./next-batch-banner";
import type { Side } from "./yes-no-toggle";
import { YesNoToggle } from "./yes-no-toggle";
import { WhyWaiting } from "./why-waiting";

export function DegenRail({ group }: { group: EventGroup }) {
  const [side, setSide] = useState<Side>("YES");
  const [amount, setAmount] = useState<string>("100");
  const [active, setActive] = useState<DegenActive | null>(null);
  const [signing, setSigning] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const session = useAccountSession();
  const openConnectModal = useSetConnectModalOpen();
  const qc = useQueryClient();
  const latestHeight = useStore(selectLatestHeight);

  const selected =
    group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
    group.outcomes[0];

  const { data: pricePoints } = usePriceHistory(selected?.marketId ?? -1);
  const tracking = useDegenBetTracker(active);

  // mark price for the selected side: last price-history point, else clearing.
  const markNanos = useMemo(() => {
    if (!selected) return ONE_DOLLAR_NANOS / 2n;
    const last = pricePoints?.[pricePoints.length - 1];
    const histYes = last ? parseNanos(last.yes_price_nanos) : null;
    const histNo = last ? parseNanos(last.no_price_nanos) : null;
    const clearYes =
      selected.yesCents == null
        ? null
        : BigInt(Math.round(selected.yesCents * 1e7));
    const clearNo = clearYes == null ? null : ONE_DOLLAR_NANOS - clearYes;
    return side === "YES"
      ? resolveMarkNanos(histYes, clearYes)
      : resolveMarkNanos(histNo, clearNo);
  }, [pricePoints, selected, side]);

  const amountNum = parseFloat(amount) || 0;
  const built = useMemo(() => {
    const betUsdNanos = BigInt(Math.round(amountNum * 1e9));
    return buildDegenOrder({
      side,
      betUsdNanos,
      markNanos,
      latestHeight: BigInt(latestHeight ?? 0),
    });
  }, [amountNum, side, markNanos, latestHeight]);

  if (!selected) return null;
  const yesCents = selected.yesCents;

  async function onBet() {
    if (!session) {
      openConnectModal(true);
      return;
    }
    if (!built.ok || latestHeight == null) return;
    setSigning(true);
    setSubmitError(null);
    try {
      const res = await submitSignedOrder({
        accountId: session.accountId,
        publicKeyHex: session.publicKeyHex,
        marketId: selected.marketId,
        side: built.order.side,
        limitPriceNanos: built.order.limitPriceNanos,
        maxFill: built.order.maxFill,
        expiresAtBlock: built.order.expiresAtBlock,
      });
      if (!res.accepted) throw new Error("order not accepted");
      setActive({
        accountId: session.accountId,
        marketId: selected.marketId,
        outcome: side,
        targetQty: built.order.maxFill,
        limitPriceNanos: built.order.limitPriceNanos,
        submitHeight: latestHeight,
        expiresAtBlock: Number(built.order.expiresAtBlock),
      });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "events"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "orders"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "portfolio"] });
      qc.invalidateQueries({ queryKey: ["orders", "pending"] });
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : "submit failed");
    } finally {
      setSigning(false);
    }
  }

  const connected = session !== null;
  const ctaLabel = !connected
    ? "Connect to bet"
    : signing
      ? "Signing…"
      : !built.ok
        ? "Raise your bet"
        : `Bet $${amountNum} on ${side}${group.isMultiOutcome ? ` · ${selected.shortLabel}` : ""}`;
  const ctaDisabled = connected && (signing || !built.ok);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <NextBatchBanner marketId={selected.marketId} />

      {active ? (
        <DegenProgress
          phase={tracking?.phase ?? "tracking"}
          side={active.outcome}
          secondsLeft={tracking?.secondsLeft ?? 0}
          timeProgress01={tracking?.timeProgress01 ?? 0}
          filledQty={tracking?.filledQty ?? 0n}
          targetQty={active.targetQty}
          limitPriceNanos={active.limitPriceNanos}
          avgPriceNanos={tracking?.avgPriceNanos ?? null}
          onBetAgain={() => setActive(null)}
        />
      ) : (
        <>
          {group.isMultiOutcome && (
            <div>
              <SectionLabel>pick outcome</SectionLabel>
              <DegenOutcomePicker
                outcomes={group.outcomes}
                currentMarketId={group.currentMarketId}
              />
            </div>
          )}

          <div>
            <SectionLabel>will it happen?</SectionLabel>
            <YesNoToggle value={side} onChange={setSide} />
          </div>

          <div>
            <SectionLabel>your bet</SectionLabel>
            <DegenAmount
              amount={amount}
              setAmount={setAmount}
              yesPriceCents={yesCents}
              side={side}
            />
          </div>

          <button
            type="button"
            onClick={onBet}
            disabled={ctaDisabled}
            style={{
              marginTop: 4,
              padding: "16px 0",
              borderRadius: 6,
              border: 0,
              cursor: ctaDisabled ? "not-allowed" : "pointer",
              background: side === "YES" ? "var(--yes)" : "var(--no)",
              color: "#0A0E12",
              fontFamily: "var(--font-sans)",
              fontSize: 15,
              fontWeight: 700,
              letterSpacing: "-0.005em",
              opacity: ctaDisabled ? 0.65 : 1,
            }}
          >
            {ctaLabel}
          </button>

          {submitError && (
            <div
              role="alert"
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                color: "var(--no)",
                textAlign: "center",
              }}
            >
              {submitError}
            </div>
          )}
        </>
      )}

      <WhyWaiting />
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
        textTransform: "uppercase",
        letterSpacing: "0.06em",
        marginBottom: 8,
      }}
    >
      {children}
    </div>
  );
}
```

- [ ] **Step 2: Typecheck (degen + rail clean)**

Run: `cd frontend/web && pnpm exec tsc --noEmit 2>&1 | grep -E "degen-rail|src/lib/degen|degen-progress|use-account-events" || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 3: Full degen test suite still green**

Run: `cd frontend/web && pnpm exec vitest run src/lib/degen/ src/components/market-rail/degen-progress.test.tsx`
Expected: PASS.

- [ ] **Step 4: Manual dev verification**

Start the dev server if not running (`cd frontend/web && pnpm dev`), open a market page's degen rail, and confirm: (a) with no session the CTA reads "Connect to bet" and opens the connect modal; (b) a tiny amount shows "Raise your bet" disabled; (c) a valid bet submits, the card morphs to the FILLING countdown, and resolves to a filled/partial/no-fill result with a working "Bet again". (No automated browser test — pure FE, manual check.)

- [ ] **Step 5: Commit**

```bash
cd /Users/r/pr/Sybil
git add frontend/web/src/components/market-rail/degen-rail.tsx
git commit -m "feat(degen): wire degen rail — submit + inline fill/result animation"
```

---

## Definition of done

- The degen rail places a real signed order via `buildDegenOrder` + `submitSignedOrder`, gated on session, with a `below-minimum` guard.
- After submit, the rail shows an inline countdown + fill meter, then a filled / partial / no-fill result, then "Bet again".
- `pnpm exec vitest run src/lib/degen/ src/components/market-rail/degen-progress.test.tsx` is green; `tsc --noEmit` clean for all new/changed degen files.
- No backend/API/schema change; the portfolio `useAccountHistory` mock is untouched.

## Out of scope

- Backend changes (order id in submit response, new endpoints).
- Global/cross-rail bet locking; persistence across navigation.
- Replacing the portfolio History tab's mock feed.
