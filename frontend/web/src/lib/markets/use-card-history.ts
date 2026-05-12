"use client";

import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";
import { parseNanos } from "@/lib/format/nanos";

export type PricePoint = components["schemas"]["PricePointResponse"];

const DAY_MS = 24 * 60 * 60 * 1000;

export type CardHistory = {
  points: PricePoint[];
  /** YES price 24h delta, in cents (absolute change). */
  delta24Cents: number | null;
  /** NO price 24h delta, in cents (absolute change). */
  noDelta24Cents: number | null;
};

/**
 * Lazy per-card price history for the markets grid: fetches the last 24h
 * for one market when `enabled` flips true (driven by IntersectionObserver).
 * Returns the points (for the sparkline) and a derived 24h delta %.
 *
 * Sibling to `usePriceHistory` (full range) — kept separate so the queryKey
 * scope is "card 24h" and the staleTime can differ. Each market is its own
 * round-trip until the backend exposes a batched endpoint.
 */
export function useCardHistory(marketId: number, enabled: boolean) {
  const query = useQuery({
    queryKey: ["market", marketId, "history", "24h"],
    queryFn: async (): Promise<PricePoint[]> => {
      const fromMs = Date.now() - DAY_MS;
      const { data, error } = await api.GET(
        "/v1/markets/{id}/prices/history",
        {
          params: {
            path: { id: marketId },
            query: { from_ms: fromMs },
          },
        }
      );
      if (error || !data) {
        throw new Error(`card history fetch failed for #${marketId}`);
      }
      return data.points ?? [];
    },
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    enabled: enabled && Number.isFinite(marketId) && marketId >= 0,
  });

  const derived = useMemo<CardHistory>(() => {
    const points = query.data ?? [];
    if (points.length < 2) {
      return { points, delta24Cents: null, noDelta24Cents: null };
    }
    const firstPt = points[0]!;
    const lastPt = points[points.length - 1]!;
    const yesFirst = parseNanos(firstPt.yes_price_nanos ?? 0);
    const yesLast = parseNanos(lastPt.yes_price_nanos ?? 0);
    const noFirst = parseNanos(firstPt.no_price_nanos ?? 0);
    const noLast = parseNanos(lastPt.no_price_nanos ?? 0);
    // nanos / 1e7 = cents. Diff is signed; safely within Number range.
    const yesDeltaCents = Number(yesLast - yesFirst) / 1e7;
    const noDeltaCents = Number(noLast - noFirst) / 1e7;
    return {
      points,
      delta24Cents: yesDeltaCents,
      noDelta24Cents: noDeltaCents,
    };
  }, [query.data]);

  return derived;
}
