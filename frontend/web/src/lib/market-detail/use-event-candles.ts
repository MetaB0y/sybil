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
  /** Sibling requests whose latest attempt failed. */
  failureCount: number;
  /** Failed siblings with no cached candles to render. */
  unavailableCount: number;
  /** True while a failed sibling is retrying. */
  isRetrying: boolean;
  /** Retry only failed siblings. */
  retryFailed: () => Promise<void>;
};

/**
 * Buckets fetched per outcome, keyed by resolution rather than by the caller's
 * range. Each recent cap covers its widest bounded consumer with slack. ALL
 * walks the API cursor instead of silently pretending this first page is the
 * complete chain history.
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
  /** Walk every retained page. Used only by the explicitly labelled ALL view. */
  fullHistory = false,
): EventCandles {
  const limit = LIMIT_BY_RESOLUTION[resolution] ?? 500;

  const results = useQueries({
    queries: marketIds.map((id) => ({
      queryKey: [
        "market",
        id,
        "prices",
        "candles",
        resolution,
        fullHistory ? "all" : "recent",
      ],
      queryFn: async (): Promise<PriceCandle[]> => {
        const candles: PriceCandle[] = [];
        let beforeMs: number | undefined;
        // A hard safety ceiling fails visibly instead of truncating ALL. At the
        // current 500-row page this still permits 50,000 hourly buckets.
        for (let page = 0; page < 100; page += 1) {
          const { data, error } = await api.GET(
            "/v1/markets/{id}/prices/candles",
            {
              params: {
                path: { id },
                query: {
                  resolution,
                  limit,
                  ...(beforeMs !== undefined ? { before_ms: beforeMs } : {}),
                },
              },
            },
          );
          if (error || !data) {
            throw new Error(`fetch /v1/markets/${id}/prices/candles failed`);
          }
          candles.push(...(data.candles ?? []));
          if (!fullHistory || data.next_before_ms == null) {
            return candles.sort(
              (a, b) => a.bucket_start_ms - b.bucket_start_ms,
            );
          }
          beforeMs = data.next_before_ms;
        }
        throw new Error(
          `fetch /v1/markets/${id}/prices/candles exceeded pagination safety limit`,
        );
      },
      // The open bucket keeps accruing volume as batches commit — refresh on a
      // slow tick instead of per block so N outcomes don't hammer the API.
      staleTime: fullHistory ? 5 * 60_000 : 30_000,
      refetchInterval: fullHistory ? 5 * 60_000 : 30_000,
      enabled: enabled && Number.isFinite(id) && id >= 0,
    })),
  });

  const byMarket = new Map<number, PriceCandle[]>();
  marketIds.forEach((id, i) => {
    byMarket.set(id, results[i]?.data ?? []);
  });
  const failed = results.filter((result) => result.error != null);

  return {
    byMarket,
    isPending: enabled && results.some((r) => r.isPending),
    failureCount: failed.length,
    unavailableCount: failed.filter((result) => result.data === undefined)
      .length,
    isRetrying: failed.some((result) => result.isFetching),
    retryFailed: async () => {
      await Promise.all(failed.map((result) => result.refetch()));
    },
  };
}
