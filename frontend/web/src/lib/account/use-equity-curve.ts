"use client";

/**
 * Real per-account equity series.
 *
 * Backed by `GET /v1/accounts/{id}/equity?range=` — the backend samples each
 * account's portfolio value at block finalize (on every fill) plus a periodic
 * 60s sweep, into a bounded ring (~30 days). Each point carries the real
 * timestamp, portfolio value, and net-deposits baseline.
 *
 * Caveats (both backend, until off-block aggregates are persisted):
 *   - The series resets on backend restart, so it reaches back only to the last
 *     restart, not to account creation.
 *   - Sampling starts at an account's first fill; a deposited-but-never-traded
 *     account returns an empty series. We surface that as `isEmpty`.
 */

import { useEffect, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { parseNanos } from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";

export type EquityRange = "24H" | "7D" | "30D" | "ALL";

const RANGE_QUERY: Record<EquityRange, string> = {
  "24H": "24h",
  "7D": "7d",
  "30D": "30d",
  ALL: "all",
};

export interface EquityPoint {
  t: number; // timestamp, ms
  value: number; // portfolio value, dollars
}

export interface EquityCurve {
  /** The range the user has selected. */
  range: EquityRange;
  /** The range `points` actually belong to. Differs from `range` only while a
   *  swap is in flight, when the previous range's series is still on screen. */
  drawnRange: EquityRange;
  points: EquityPoint[]; // real samples, oldest-first (+ a live tip at "now")
  baseline: number; // net deposits (dollars) — the dashed floor
  startEquity: number;
  endEquity: number;
  deltaAbs: number; // endEquity − startEquity over the range
  deltaPct: number; // delta / startEquity
  isLoading: boolean;
  isEmpty: boolean; // fewer than 2 points → nothing to draw
  /** True while a newly-picked range is still in flight and `points` are still
   *  the previous range's. The chart holds them, blurred, instead of flashing
   *  an empty box — see `EquityChart`. */
  isSwapping: boolean;
}

export function useEquityCurve(args: {
  accountId: number;
  range: EquityRange;
  currentValueDollars: number;
  baselineDepositsDollars: number;
}): EquityCurve {
  const { accountId, range, currentValueDollars, baselineDepositsDollars } =
    args;
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);

  // Refresh as blocks land so new samples (and the live tip) stay current.
  useEffect(() => {
    qc.invalidateQueries({ queryKey: ["account", accountId, "equity"] });
  }, [accountId, latest?.height, qc]);

  const q = useQuery({
    queryKey: ["account", accountId, "equity", range],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/accounts/{id}/equity", {
        params: {
          path: { id: accountId },
          query: { range: RANGE_QUERY[range] },
        },
      });
      if (error || !data) throw new Error("fetch equity series failed");
      // Tag the payload with its range: when `placeholderData` hands back the
      // previous range's result, this is what tells the chart which series is
      // actually on screen.
      return { range, points: data.points };
    },
    // Hold the previous range's series while the newly-picked one loads, so the
    // chart can crossfade between two drawn curves instead of collapsing to
    // "no equity history yet" and snapping back. Only a *key* change (a new
    // range) yields placeholder data; the per-block refetch below already has
    // data for its key and never flags as placeholder.
    placeholderData: (prev) => prev,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  const nowMs = latest?.timestamp_ms ?? 0;

  return useMemo(() => {
    const points: EquityPoint[] = (q.data?.points ?? []).map((p) => ({
      t: p.timestamp_ms,
      value: Number(parseNanos(p.portfolio_value_nanos)) / 1e9,
    }));

    // Append a live tip at the latest block time so the curve ends on the same
    // value as the hero's portfolio number (samples can lag up to ~60s between
    // sweeps). Block time keeps this pure — no `Date.now()` during render.
    if (points.length > 0 && currentValueDollars > 0 && nowMs > 0) {
      const last = points[points.length - 1]!;
      if (nowMs > last.t) points.push({ t: nowMs, value: currentValueDollars });
    }

    const startEquity = points.length ? points[0]!.value : 0;
    const endEquity = points.length ? points[points.length - 1]!.value : 0;
    const deltaAbs = endEquity - startEquity;
    const deltaPct = startEquity === 0 ? 0 : (deltaAbs / startEquity) * 100;

    return {
      range,
      drawnRange: q.data?.range ?? range,
      points,
      baseline: baselineDepositsDollars,
      startEquity,
      endEquity,
      deltaAbs,
      deltaPct,
      isLoading: q.isPending,
      isEmpty: points.length < 2,
      isSwapping: q.isPlaceholderData,
    };
  }, [
    q.data,
    q.isPending,
    q.isPlaceholderData,
    range,
    currentValueDollars,
    baselineDepositsDollars,
    nowMs,
  ]);
}
