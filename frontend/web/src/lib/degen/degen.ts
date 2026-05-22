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
