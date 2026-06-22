import type { DegenSide } from "./degen";

/** A degen-relevant row from the account events feed, normalized to bigint. */
export interface DegenEvent {
  type: string; // "placed" | "partial_fill" | "filled" | "expired" | "cancelled" | ...
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

export type DegenPhase = "tracking" | "filled" | "partial" | "none" | "cancelled";

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
  /** The bound order's partial_fill/filled/expired/cancelled rows (empty if unbound). */
  events: DegenEvent[];
  /**
   * True when this bet's order was cancelled out-of-band — e.g. the user hit
   * Cancel in the open-orders table or in the progress card. The backend
   * doesn't emit an `OrderCancelled` event ([[use-cancelled-orders]]), so this
   * is sourced from the local cancel store rather than `events`. A `cancelled`
   * row in `events` (should the backend ever emit one) is honoured too.
   */
  cancelled?: boolean;
}

/**
 * Resolve the bet's phase. Terminal states (filled/cancelled/expired) win; the
 * height backstop (`>= expiresAtBlock + 1`) covers a missed terminal row or a
 * correlation miss so the spinner can never hang. A cancel that lands after some
 * fills reads as `partial` (the filled portion stands) — the same way an expiry
 * after partial fills does.
 */
export function resolveDegenBet(s: DegenSnapshot): DegenBetState {
  let filledQty = 0n;
  let weighted = 0n;
  let hasFilled = false;
  let hasExpired = false;
  let hasCancelled = s.cancelled === true;
  for (const e of s.events) {
    if (e.type === "partial_fill" || e.type === "filled") {
      filledQty += e.qty;
      weighted += e.qty * e.priceNanos;
      if (e.type === "filled") hasFilled = true;
    } else if (e.type === "expired") {
      hasExpired = true;
    } else if (e.type === "cancelled") {
      hasCancelled = true;
    }
  }
  const avgPriceNanos = filledQty > 0n ? weighted / filledQty : null;
  const base = { filledQty, targetQty: s.targetQty, avgPriceNanos };

  if (hasFilled || filledQty >= s.targetQty) return { phase: "filled", ...base };
  if (hasCancelled) {
    return { phase: filledQty > 0n ? "partial" : "cancelled", ...base };
  }
  if (hasExpired) {
    return { phase: filledQty > 0n ? "partial" : "none", ...base };
  }
  if (s.currentHeight >= s.expiresAtBlock + 1) {
    return { phase: filledQty > 0n ? "partial" : "none", ...base };
  }
  return { phase: "tracking", ...base };
}
