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

/** Shares affordable for `budgetNanos` at limit `limitNanos` (integer floor). */
export function degenQuantity(budgetNanos: bigint, limitNanos: bigint): bigint {
  if (budgetNanos <= 0n || limitNanos <= 0n) return 0n;
  return budgetNanos / limitNanos;
}

/** Last eligible block height: the next `DEGEN_BATCHES` batches. */
export function degenExpiry(latestHeight: bigint): bigint {
  return latestHeight + DEGEN_BATCHES;
}
