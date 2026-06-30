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
  /**
   * Highest order id that already existed for this market *before* this bet was
   * submitted (see `priorMaxOrderId`). Order ids are monotonic within a market,
   * so the new bet's order is always strictly greater — matching only ids above
   * this floor isolates this bet from the account's earlier orders on the same
   * market+side (otherwise a fresh bet re-binds a prior, already-resolved order
   * whose `filled`/`placed` row sits at a height ≥ this submit height). Null on
   * the first-ever bet (nothing to exclude).
   */
  minOrderIdExclusive?: number | null;
}

/**
 * Highest order id already present for `marketId` across the events + pending
 * feeds — the floor that isolates a fresh degen bet from this account's earlier
 * orders on the same market. Snapshot at submit and carried in `DegenActive`.
 * Returns null when the feeds hold nothing for this market (first bet).
 */
export function priorMaxOrderId(
  marketId: number,
  events: { market_id?: number | null; order_id?: number | null }[],
  pending: { market_id: number; order_id: number }[],
): number | null {
  let max: number | null = null;
  for (const e of events) {
    if (e.market_id !== marketId || e.order_id == null) continue;
    if (max === null || e.order_id > max) max = e.order_id;
  }
  for (const o of pending) {
    if (o.market_id !== marketId) continue;
    if (max === null || o.order_id > max) max = o.order_id;
  }
  return max;
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
    if (c.minOrderIdExclusive != null && e.orderId <= c.minOrderIdExclusive) {
      continue;
    }
    if (e.type !== "placed" && e.type !== "partial_fill" && e.type !== "filled") {
      continue;
    }
    if (best === null || e.blockHeight < best.height) {
      best = { height: e.blockHeight, orderId: e.orderId };
    }
  }
  return best?.orderId ?? null;
}

/** Minimal subset of `PendingOrderResponse` needed to bind a degen bet's id. */
export interface DegenPendingOrder {
  order_id: number;
  market_id: number;
  side: string; // "BuyYes" | "BuyNo" | "SellYes" | "SellNo" | …
  created_at_block: number;
}

/**
 * Bind our degen bet's order id from the *pending-orders* feed
 * (`/v1/accounts/{id}/orders`). The backend assigns the id at submit and exposes
 * the resting order here within ~1s — during the open batch, before the `placed`
 * event commits at the next clear — so Cancel can unlock immediately instead of
 * waiting a full batch. Matches our market + buy side, ignores any order created
 * before this bet, and takes the newest (ids are monotonic) to isolate this bet
 * from an earlier resting order on the same side.
 */
export function findDegenPendingOrderId(
  pending: DegenPendingOrder[],
  c: DegenCriteria,
): number | null {
  const wantSide = c.outcome === "YES" ? "BuyYes" : "BuyNo";
  let bestId: number | null = null;
  for (const o of pending) {
    if (o.market_id !== c.marketId) continue;
    if (o.side !== wantSide) continue;
    if (o.created_at_block < c.submitHeight) continue;
    if (c.minOrderIdExclusive != null && o.order_id <= c.minOrderIdExclusive) {
      continue;
    }
    if (bestId === null || o.order_id > bestId) bestId = o.order_id;
  }
  return bestId;
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
