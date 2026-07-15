import type { OrderSide } from "@/lib/account/orders";
import { SHARE_SCALE } from "@/lib/account/quantity";
import { DEGEN_BATCHES, DEGEN_EXPONENT, DEGEN_PEAK_NANOS, ONE_DOLLAR_NANOS } from "./constants";

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

/** Share-units affordable for `budgetNanos` at limit `limitNanos` (integer floor). */
export function degenQuantity(budgetNanos: bigint, limitNanos: bigint): bigint {
  if (budgetNanos <= 0n || limitNanos <= 0n) return 0n;
  return (budgetNanos * SHARE_SCALE) / limitNanos;
}

/** Last eligible block height: the next `DEGEN_BATCHES` batches. */
export function degenExpiry(latestHeight: bigint): bigint {
  return latestHeight + DEGEN_BATCHES;
}

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
 * when the budget can't afford the minimum 0.001 share at the degen limit price.
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
