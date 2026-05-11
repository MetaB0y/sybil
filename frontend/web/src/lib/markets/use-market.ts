"use client";

import { useQuery } from "@tanstack/react-query";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type Market = components["schemas"]["MarketResponse"];

export function useMarket(marketId: number) {
  return useQuery({
    queryKey: ["market", marketId],
    queryFn: async (): Promise<Market> => {
      const { data, error } = await api.GET("/v1/markets/{id}", {
        params: { path: { id: marketId } },
      });
      if (error || !data) throw new Error(`fetch /v1/markets/${marketId} failed`);
      return data;
    },
    staleTime: 30_000,
    enabled: Number.isFinite(marketId) && marketId >= 0,
  });
}
