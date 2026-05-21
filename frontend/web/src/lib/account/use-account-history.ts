"use client";

/**
 * Unified portfolio history feed (event log).
 *
 * TARGET: GET /v1/accounts/{id}/events — a per-account, off-block event log
 * merging order lifecycle (placed / partial_fill / filled / cancelled /
 * expired), funding (deposit / withdrawal), settlement (resolved) and account
 * creation, newest-first and paginated. See
 * docs/superpowers/specs/2026-05-21-portfolio-history-feed-design.md.
 *
 * INTERIM: that endpoint doesn't exist yet, so this hook returns a deterministic
 * MOCK stream seeded by accountId — enough to render and review the full feed.
 * `HistoryEvent` is the FE↔BE contract; when the endpoint lands, replace the
 * body of `useAccountHistory` with the fetch and drop `mockHistory`. The whole
 * feed wears a MockValue banner until then.
 */

import { useMemo } from "react";

export type HistoryEventType =
  | "created"
  | "placed"
  | "partial_fill"
  | "filled"
  | "cancelled"
  | "expired"
  | "deposit"
  | "withdrawal"
  | "resolved";

export type HistoryCategory = "all" | "trades" | "funding" | "settlement";

export interface HistoryEvent {
  id: string;
  type: HistoryEventType;
  timestampMs: number;
  blockHeight: number;
  marketId?: number;
  orderId?: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  qty?: number;
  priceNanos?: bigint; // limit (placed) or fill price (fills)
  amountNanos?: bigint; // signed cash impact, nanos-dollars (+in / -out)
  payoutOutcome?: "YES" | "NO"; // resolved only
}

/** Which filter chip an event type falls under. */
export const CATEGORY_OF: Record<
  HistoryEventType,
  Exclude<HistoryCategory, "all">
> = {
  created: "funding",
  placed: "trades",
  partial_fill: "trades",
  filled: "trades",
  cancelled: "trades",
  expired: "trades",
  deposit: "funding",
  withdrawal: "funding",
  resolved: "settlement",
};

export interface AccountHistory {
  events: HistoryEvent[];
  isMock: boolean;
  // Pagination stubs for the future /events endpoint.
  hasMore: boolean;
  loadMore: () => void;
}

export function useAccountHistory(
  accountId: number | null,
  marketIds: number[] = [],
): AccountHistory {
  const events = useMemo(
    () => (accountId == null ? [] : mockHistory(accountId, marketIds)),
    [accountId, marketIds],
  );
  return { events, isMock: true, hasMore: false, loadMore: () => {} };
}

// ---- mock generator (delete when wired to /events) ------------------------

const CADENCE_MS = 2000;

function mockHistory(accountId: number, marketIds: number[]): HistoryEvent[] {
  const rand = seeded((accountId | 0) ^ 0x5bd1e995);
  const pickMarket = () =>
    marketIds.length
      ? marketIds[Math.floor(rand() * marketIds.length)]!
      : 1 + Math.floor(rand() * 40);

  const out: HistoryEvent[] = [];
  let block = 240_000 + Math.floor(rand() * 1000);
  let t = Date.now();
  let seq = 0;

  const push = (
    e: Omit<HistoryEvent, "id" | "timestampMs" | "blockHeight">,
  ) => {
    out.push({ ...e, id: `${block}.${seq++}`, timestampMs: t, blockHeight: block });
  };
  const step = () => {
    const dBlocks = 1 + Math.floor(rand() * 4000);
    block = Math.max(1, block - dBlocks);
    t -= dBlocks * CADENCE_MS;
  };

  push({ type: "deposit", amountNanos: dollars(250 + Math.floor(rand() * 750)) });
  step();

  for (let i = 0; i < 8; i++) {
    const mid = pickMarket();
    const orderId = 15_000_000 + Math.floor(rand() * 1_000_000);
    const side: "BUY" | "SELL" = rand() < 0.7 ? "BUY" : "SELL";
    const outcome: "YES" | "NO" = rand() < 0.55 ? "YES" : "NO";
    const placed = (1 + Math.floor(rand() * 20)) * 50;
    const limit = cents(15 + Math.floor(rand() * 70));

    push({ type: "placed", marketId: mid, orderId, side, outcome, qty: placed, priceNanos: limit });
    step();

    const roll = rand();
    if (roll < 0.45) {
      const part = Math.max(1, Math.floor(placed * (0.3 + rand() * 0.3)));
      const fp1 = cents(15 + Math.floor(rand() * 70));
      push({ type: "partial_fill", marketId: mid, orderId, side, outcome, qty: part, priceNanos: fp1, amountNanos: cashImpact(side, part, fp1) });
      step();
      const rest = placed - part;
      const fp2 = cents(15 + Math.floor(rand() * 70));
      push({ type: "filled", marketId: mid, orderId, side, outcome, qty: rest, priceNanos: fp2, amountNanos: cashImpact(side, rest, fp2) });
    } else if (roll < 0.7) {
      const fp = cents(15 + Math.floor(rand() * 70));
      push({ type: "filled", marketId: mid, orderId, side, outcome, qty: placed, priceNanos: fp, amountNanos: cashImpact(side, placed, fp) });
    } else if (roll < 0.88) {
      push({ type: "cancelled", marketId: mid, orderId, side, outcome, qty: placed });
    } else {
      push({ type: "expired", marketId: mid, orderId, side, outcome, qty: placed });
    }
    step();

    if (rand() < 0.25) {
      const mid2 = pickMarket();
      const po: "YES" | "NO" = rand() < 0.5 ? "YES" : "NO";
      push({ type: "resolved", marketId: mid2, payoutOutcome: po, amountNanos: dollars(Math.floor(rand() * 400)) });
      step();
    }
    if (rand() < 0.15) {
      push({ type: "withdrawal", amountNanos: -dollars(50 + Math.floor(rand() * 200)) });
      step();
    }
  }

  push({ type: "created" });
  return out;
}

function cashImpact(side: "BUY" | "SELL", qty: number, priceNanos: bigint): bigint {
  const gross = BigInt(qty) * priceNanos; // shares × price = nanos-dollars
  return side === "BUY" ? -gross : gross;
}
function dollars(d: number): bigint {
  return BigInt(Math.round(d)) * 1_000_000_000n;
}
function cents(c: number): bigint {
  return BigInt(Math.round(c)) * 10_000_000n; // 1¢ = 1e7 nanos
}
function seeded(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 0x100000000;
  };
}
