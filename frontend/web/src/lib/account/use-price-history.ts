"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type PriceHistory = components["schemas"]["PriceHistoryResponse"];

/**
 * Per-market YES/NO price history for sparklines. Cached with a long
 * staleTime since these don't change every batch — re-fetch on mount but
 * not aggressively.
 */
export function usePriceHistory(marketId: number, enabled: boolean = true) {
  return useQuery({
    enabled,
    queryKey: ["market", marketId, "prices-history"],
    queryFn: async (): Promise<PriceHistory> => {
      const { data, error } = await api.GET(
        "/v1/markets/{id}/prices/history",
        { params: { path: { id: marketId } } },
      );
      if (error || !data) {
        throw new Error("fetch price history failed");
      }
      return data;
    },
    staleTime: 60_000,
    refetchOnWindowFocus: false,
  });
}
