"use client";

/**
 * Cumulative realized PnL over time, in nanos-dollars.
 *
 * Realized PnL is BACKEND-COMPUTED, per fill/settlement, by the C1
 * `CostBasisTracker` (weighted-average-cost — WAC, not FIFO). Every
 * `filled` / `partial_fill` / `resolved` history event that closes or reduces a
 * position carries its own signed `realizedPnlNanos`; we sort those events
 * chronologically and running-sum them. The series therefore matches sybil-api's
 * realized PnL exactly — we never re-derive cost basis on the client, we only
 * accumulate the server's per-event figures over time.
 *
 * Same share-unit / nanos conventions as the rest of the frontend: values stay
 * in nanos-dollars (1 unit = 1e9 nanos) as bigint until a formatter renders them.
 */

import type { HistoryEvent } from "./use-account-history";

export interface RealizedPnlPoint {
  /** Event timestamp, ms since epoch. */
  t: number;
  /** Cumulative realized PnL up to and including this event, nanos-dollars. */
  cumNanos: bigint;
}

/**
 * Fold an account's history events into a chronological cumulative realized-PnL
 * series (oldest-first). Only events that carry a `realizedPnlNanos` contribute
 * — the backend sets that field precisely where a realization occurred (closing
 * or reducing a position, or settlement), so summing them double-counts nothing
 * and drops nothing. Returns `[]` when the account has realized nothing yet.
 */
export function cumulativeRealizedPnl(events: HistoryEvent[]): RealizedPnlPoint[] {
  const realized = events
    .filter((e) => e.realizedPnlNanos != null)
    .slice()
    // Chronological; break ties by block height then id so the running sum is
    // deterministic regardless of the fetch order (history arrives newest-first).
    .sort(
      (a, b) =>
        a.timestampMs - b.timestampMs ||
        a.blockHeight - b.blockHeight ||
        a.id.localeCompare(b.id),
    );

  const points: RealizedPnlPoint[] = [];
  let cum = 0n;
  for (const e of realized) {
    cum += e.realizedPnlNanos ?? 0n;
    points.push({ t: e.timestampMs, cumNanos: cum });
  }
  return points;
}

/** Total realized PnL (the last cumulative point, or 0n if none). */
export function totalRealizedPnl(points: RealizedPnlPoint[]): bigint {
  return points.length ? points[points.length - 1]!.cumNanos : 0n;
}
