"use client";

/**
 * Split portfolio PnL into realized + unrealized, both straight from the
 * backend CostBasisTracker (C1):
 *
 *   realized   = portfolio.realized_pnl_nanos
 *   unrealized = portfolio.unrealized_pnl_nanos
 *   total      = realized + unrealized
 *
 * Pre-C1 trades are not retroactively backfilled — the tracker starts
 * empty on rollout, so realized may be zero for positions whose entries
 * predate C1. Users with fresh positions see correct numbers immediately.
 */

import { useMemo } from "react";
import type { Portfolio } from "./use-portfolio";
import { parseNanos } from "@/lib/format/nanos";

export interface PnlSplit {
  unrealizedNanos: bigint;
  realizedNanos: bigint;
  totalNanos: bigint;
}

export function usePnlSplit(portfolio: Portfolio | null): PnlSplit | null {
  return useMemo(() => {
    if (!portfolio) return null;
    const realized = parseNanos(portfolio.realized_pnl_nanos ?? 0);
    const unrealized = parseNanos(portfolio.unrealized_pnl_nanos ?? 0);
    return {
      realizedNanos: realized,
      unrealizedNanos: unrealized,
      totalNanos: realized + unrealized,
    };
  }, [portfolio]);
}
