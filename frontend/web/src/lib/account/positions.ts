"use client";

/**
 * Frontend helpers for cost basis. Post-C1, the backend returns
 * `avg_entry_price_nanos` on each `PositionValueResponse` — those values
 * come straight from the off-block `CostBasisTracker` and reflect proper
 * WAC math (including position flips and short-side resolution). When
 * the field is zero (positions opened before C1 ramped, or any missing
 * row) we fall back to a qty-weighted average over visible fills.
 */

import type { components } from "@/lib/api/schema";
import { parseNanos } from "@/lib/format/nanos";

type Fill = components["schemas"]["AccountFillResponse"];
type Position = components["schemas"]["PositionValueResponse"];

/**
 * Average entry price for a given (market, outcome). Prefers the
 * backend `avg_entry_price_nanos`; falls back to fill-based approximation
 * when that's zero. Returns nanos (0..1e9 for binary outcomes) or `null`.
 */
export function avgEntryPriceNanos(
  fills: Fill[],
  marketId: number,
  outcome: string,
  position?: Position,
): bigint | null {
  if (position) {
    const backend = parseNanos(position.avg_entry_price_nanos ?? 0);
    if (backend > 0n) {
      return backend;
    }
  }

  let totalQty = 0n;
  let totalCost = 0n;
  for (const fill of fills) {
    const delta = fill.position_deltas.find(
      (d) => d.market_id === marketId && d.outcome === outcome,
    );
    if (!delta || delta.delta <= 0) continue;
    const qty = BigInt(delta.delta);
    const priceNanos = parseNanos(fill.fill_price_nanos);
    totalQty += qty;
    totalCost += qty * priceNanos;
  }
  if (totalQty === 0n) return null;
  return totalCost / totalQty;
}
