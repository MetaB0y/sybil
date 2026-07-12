"use client";

/**
 * Real per-account equity series.
 *
 * Backed by `GET /v1/accounts/{id}/equity?range=` — the backend samples each
 * account's portfolio value at block finalize (on every fill) plus a periodic
 * 60s sweep, into durable retained history. Each point carries the real
 * timestamp, portfolio value, and net-deposits baseline.
 *
 * Caveat: sampling starts at an account's first fill; a deposited-but-never-traded
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
  /** The range selected in the controls. */
  range: EquityRange;
  /** The range the visible points belong to while a range swap is in flight. */
  drawnRange: EquityRange;
  points: EquityPoint[]; // real samples, oldest-first (+ a live tip at "now")
  baseline: number; // net deposits (dollars) — the dashed floor
  startEquity: number;
  endEquity: number;
  deltaAbs: number; // endEquity − startEquity over the range
  deltaPct: number; // delta / startEquity
  isLoading: boolean;
  isFetching: boolean;
  error: Error | null;
  refetch: () => Promise<unknown>;
  isEmpty: boolean; // fewer than 2 points → nothing to draw
  /** Requested range starts before the server's retained boundary. */
  historyTruncated: boolean;
  /** Previous points remain visible, softened, while the next range loads. */
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
      return { range, points: data.points, historyTruncated: data.history_truncated };
    },
    placeholderData: (previous) => previous,
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
      isFetching: q.isFetching,
      error: q.error,
      refetch: q.refetch,
      isEmpty: points.length < 2,
      historyTruncated: q.data?.historyTruncated ?? false,
      isSwapping: q.isPlaceholderData,
    };
  }, [
    q.data,
    q.isFetching,
    q.isPending,
    q.isPlaceholderData,
    q.error,
    q.refetch,
    range,
    currentValueDollars,
    baselineDepositsDollars,
    nowMs,
  ]);
}
