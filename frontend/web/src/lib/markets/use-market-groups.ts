"use client";

/** Real protocol MarketGroups, used by complete-set admission preflight. */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type MarketGroup = components["schemas"]["MarketGroupResponse"];

export function useMarketGroups() {
  return useQuery({
    queryKey: ["market-groups"],
    queryFn: async (): Promise<MarketGroup[]> => {
      const { data, error } = await api.GET("/v1/markets/groups");
      if (error || !data) throw new Error("fetch /v1/markets/groups failed");
      return data;
    },
    staleTime: 30 * 60_000,
    refetchOnWindowFocus: false,
  });
}

export function useGroupMarkets(marketId: number | null): number[] {
  const { data } = useMarketGroups();
  if (data == null || marketId == null) return [];
  return data.find((group) => group.market_ids.includes(marketId))?.market_ids ?? [];
}
