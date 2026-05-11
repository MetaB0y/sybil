"use client";

import { useQuery } from "@tanstack/react-query";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type PricePoint = components["schemas"]["PricePointResponse"];

export function usePriceHistory(marketId: number) {
  return useQuery({
    queryKey: ["market", marketId, "prices", "history"],
    queryFn: async (): Promise<PricePoint[]> => {
      const { data, error } = await api.GET(
        "/v1/markets/{id}/prices/history",
        { params: { path: { id: marketId } } }
      );
      if (error || !data) {
        throw new Error(`fetch /v1/markets/${marketId}/prices/history failed`);
      }
      return data.points ?? [];
    },
    staleTime: 60_000,
    enabled: Number.isFinite(marketId) && marketId >= 0,
  });
}
