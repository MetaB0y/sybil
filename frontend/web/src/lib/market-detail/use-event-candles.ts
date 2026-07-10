"use client";

/**
 * Volume candles for every market in an event group, fetched in parallel.
 *
 * `/v1/markets/{id}/prices/candles` is per-market, so a multi-outcome event
 * needs one request per sibling — same fan-out as `useEventPriceHistory`.
 * Candles only exist for buckets where the market actually cleared, so
 * consumers must treat missing buckets as zero matched volume, not gaps.
 */

import { useQueries } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type PriceCandle = components["schemas"]["PriceCandleResponse"];

export type EventCandles = {
  /** marketId → its candles, oldest-first (empty array if none yet). */
  byMarket: Map<number, PriceCandle[]>;
  /** True while any enabled sibling's first fetch is still in flight. */
  isPending: boolean;
};

/**
 * Buckets fetched per outcome, keyed by resolution rather than by the caller's
 * range. Two ranges on the same resolution (24H and ALL both ride 1h) then
 * share one query key, so switching between them is a cache hit rather than a
 * refetch. Each cap covers its widest consumer with slack: 1m→2h, 5m→10h,
 * 1h→20d.
 */
const LIMIT_BY_RESOLUTION: Record<string, number> = {
  "1m": 120,
  "5m": 120,
  "1h": 500,
};

export function useEventCandles(
  marketIds: number[],
  /** Candle resolution — "1m" | "5m" | "1h". */
  resolution: string,
  /** False parks every query (e.g. before the event group has resolved). */
  enabled: boolean,
): EventCandles {
  const limit = LIMIT_BY_RESOLUTION[resolution] ?? 500;

  const results = useQueries({
    queries: marketIds.map((id) => ({
      queryKey: ["market", id, "prices", "candles", resolution],
      queryFn: async (): Promise<PriceCandle[]> => {
        const { data, error } = await api.GET(
          "/v1/markets/{id}/prices/candles",
          { params: { path: { id }, query: { resolution, limit } } },
        );
        if (error || !data) {
          throw new Error(`fetch /v1/markets/${id}/prices/candles failed`);
        }
        // Newest-first on the wire (cursor pagination); normalize oldest-first.
        return [...(data.candles ?? [])].sort(
          (a, b) => a.bucket_start_ms - b.bucket_start_ms,
        );
      },
      // The open bucket keeps accruing volume as batches commit — refresh on a
      // slow tick instead of per block so N outcomes don't hammer the API.
      staleTime: 30_000,
      refetchInterval: 30_000,
      enabled: enabled && Number.isFinite(id) && id >= 0,
    })),
  });

  const byMarket = new Map<number, PriceCandle[]>();
  marketIds.forEach((id, i) => {
    byMarket.set(id, results[i]?.data ?? []);
  });

  return {
    byMarket,
    isPending: enabled && results.some((r) => r.isPending),
  };
}
