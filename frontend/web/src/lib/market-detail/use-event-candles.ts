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
 * Buckets requested per outcome. The server clamps to its own maximum; one
 * page is plenty for every range the activity chart offers (its hourly "ALL"
 * view outlives the young chain by weeks).
 */
const CANDLE_LIMIT = 500;

export function useEventCandles(
  marketIds: number[],
  /** Candle resolution — "1m" | "5m" | "1h" (or raw seconds). */
  resolution: string,
  /** False parks every query (e.g. while the batch-granularity view is up). */
  enabled: boolean,
): EventCandles {
  const results = useQueries({
    queries: marketIds.map((id) => ({
      queryKey: ["market", id, "prices", "candles", resolution],
      queryFn: async (): Promise<PriceCandle[]> => {
        const { data, error } = await api.GET(
          "/v1/markets/{id}/prices/candles",
          { params: { path: { id }, query: { resolution, limit: CANDLE_LIMIT } } },
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
      // slow tick instead of per 2s block so N outcomes don't hammer the API.
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
