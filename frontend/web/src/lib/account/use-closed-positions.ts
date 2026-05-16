"use client";

/**
 * Closed-position rows for the History tab. Backend has no concept of
 * "closed position" — only the current non-zero positions. We approximate
 * by grouping fills by `(market_id, outcome)`, computing per-pair entry
 * (qty-weighted avg of buys) and exit (qty-weighted avg of sells), and
 * emitting a row for any pair where we have both buys and sells AND no
 * open position remains for that pair.
 *
 * Always render with `<MockValue>` (OPEN_QUESTIONS #17). Edge cases like
 * sell-then-rebuy or partial closes aren't perfectly handled.
 */

import { useMemo } from "react";
import { parseNanos } from "@/lib/format/nanos";
import type { AccountFill } from "./use-account-fills";
import type { Portfolio } from "./use-portfolio";

export interface ClosedPosition {
  marketId: number;
  outcome: string;
  buyQty: number;
  sellQty: number;
  avgEntryNanos: bigint;
  avgExitNanos: bigint;
  realizedNanos: bigint;
  realizedPct: number | null;
  lastFillTimestampMs: number;
}

export function useClosedPositions(
  fills: AccountFill[],
  portfolio: Portfolio | null,
): ClosedPosition[] {
  return useMemo(() => {
    const openKeys = new Set<string>();
    if (portfolio) {
      for (const p of portfolio.positions) {
        if (p.quantity !== 0) openKeys.add(`${p.market_id}:${p.outcome}`);
      }
    }

    // bucket fills by (market_id, outcome)
    type Bucket = {
      marketId: number;
      outcome: string;
      buyQty: bigint;
      buyCost: bigint; // sum of qty × price (nanos)
      sellQty: bigint;
      sellProceeds: bigint;
      lastTimestampMs: number;
    };
    const buckets = new Map<string, Bucket>();
    for (const fill of fills) {
      const priceNanos = parseNanos(fill.fill_price_nanos);
      for (const d of fill.position_deltas) {
        const key = `${d.market_id}:${d.outcome}`;
        const b: Bucket = buckets.get(key) ?? {
          marketId: d.market_id,
          outcome: d.outcome,
          buyQty: 0n,
          buyCost: 0n,
          sellQty: 0n,
          sellProceeds: 0n,
          lastTimestampMs: 0,
        };
        const qty = BigInt(Math.abs(d.delta));
        if (d.delta > 0) {
          b.buyQty += qty;
          b.buyCost += qty * priceNanos;
        } else if (d.delta < 0) {
          b.sellQty += qty;
          b.sellProceeds += qty * priceNanos;
        }
        if (fill.timestamp_ms > b.lastTimestampMs)
          b.lastTimestampMs = fill.timestamp_ms;
        buckets.set(key, b);
      }
    }

    const out: ClosedPosition[] = [];
    for (const [key, b] of buckets) {
      // Only "closed" pairs: no current open position AND both buys+sells happened.
      if (openKeys.has(key)) continue;
      if (b.buyQty === 0n || b.sellQty === 0n) continue;

      const avgEntry = b.buyCost / b.buyQty;
      const avgExit = b.sellProceeds / b.sellQty;
      const tradedQty =
        b.sellQty < b.buyQty ? b.sellQty : b.buyQty; // realized portion
      const realized = (avgExit - avgEntry) * tradedQty;
      const costBasis = avgEntry * tradedQty;
      const realizedPct =
        costBasis === 0n
          ? null
          : Number((realized * 10000n) / costBasis) / 100;

      out.push({
        marketId: b.marketId,
        outcome: b.outcome,
        buyQty: Number(b.buyQty),
        sellQty: Number(b.sellQty),
        avgEntryNanos: avgEntry,
        avgExitNanos: avgExit,
        realizedNanos: realized,
        realizedPct,
        lastFillTimestampMs: b.lastTimestampMs,
      });
    }
    // newest closes first
    out.sort((a, b) => b.lastFillTimestampMs - a.lastFillTimestampMs);
    return out;
  }, [fills, portfolio]);
}
