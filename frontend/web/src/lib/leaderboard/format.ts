/**
 * Pure display helpers for the leaderboard (SYB-59). Kept separate from the
 * hook so they can be unit-tested without React.
 */

import { formatDollars } from "../format/nanos";

/** Signed dollar PnL, e.g. `+$12.34`, `-$5.00`, `$0.00`. */
export function formatSignedDollars(nanos: bigint): string {
  if (nanos === 0n) return "$0.00";
  return formatDollars(nanos, { sign: true });
}

/**
 * ROI in basis points → signed percent, e.g. `+12.3%`, `-4.0%`, `0.0%`.
 * 100 bps = 1%.
 */
export function formatRoiBps(bps: number): string {
  const pct = bps / 100;
  if (pct === 0) return "0.0%";
  const sign = pct > 0 ? "+" : "-";
  return `${sign}${Math.abs(pct).toFixed(1)}%`;
}

/** CSS var for a signed value's color: green up, red down, muted flat. */
export function signColor(value: number | bigint): string {
  if (value > 0) return "var(--yes)";
  if (value < 0) return "var(--no)";
  return "var(--fg-3)";
}
