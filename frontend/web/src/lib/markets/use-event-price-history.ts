"use client";

/**
 * Price history for every market in an event group, fetched in parallel.
 *
 * `/v1/markets/{id}/prices/history` is per-market, so a multi-outcome event
 * needs one request per sibling. `useQueries` fans them out; the query key is
 * identical to `usePriceHistory` so a market already fetched there (e.g. the
 * one in the URL) is served from cache instead of re-fetched.
 */

import { useQueries } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { PricePoint } from "./use-price-history";

export type EventPriceHistory = {
  /** marketId → its price points (empty array if none yet). */
  byMarket: Map<number, PricePoint[]>;
  /** True while any sibling's first fetch is still in flight. */
  isPending: boolean;
  /** Selected lanes whose latest request failed, cached or otherwise. */
  failureCount: number;
  /** Failed lanes with no saved data to render. */
  unavailableCount: number;
  /** True while a failed lane is being retried. */
  isRetrying: boolean;
  /** Retry only the lanes whose latest request failed. */
  retryFailed: () => Promise<void>;
};

export function useEventPriceHistory(marketIds: number[]): EventPriceHistory {
  const results = useQueries({
    queries: marketIds.map((id) => ({
      queryKey: ["market", id, "prices", "history"],
      queryFn: async (): Promise<PricePoint[]> => {
        const { data, error } = await api.GET(
          "/v1/markets/{id}/prices/history",
          { params: { path: { id } } },
        );
        if (error || !data) {
          throw new Error(`fetch /v1/markets/${id}/prices/history failed`);
        }
        return data.points ?? [];
      },
      staleTime: 60_000,
      enabled: Number.isFinite(id) && id >= 0,
    })),
  });

  const byMarket = new Map<number, PricePoint[]>();
  marketIds.forEach((id, i) => {
    byMarket.set(id, results[i]?.data ?? []);
  });

  const failed = results.filter((result) => result.error != null);

  return {
    byMarket,
    isPending: results.some((r) => r.isPending),
    failureCount: failed.length,
    unavailableCount: failed.filter((result) => result.data === undefined)
      .length,
    isRetrying: failed.some((result) => result.isFetching),
    retryFailed: async () => {
      await Promise.all(failed.map((result) => result.refetch()));
    },
  };
}
