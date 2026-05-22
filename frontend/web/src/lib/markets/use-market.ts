"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type Market = components["schemas"]["MarketResponse"];

export function useMarket(marketId: number) {
  const qc = useQueryClient();
  return useQuery({
    queryKey: ["market", marketId],
    queryFn: async (): Promise<Market> => {
      const { data, error } = await api.GET("/v1/markets/{id}", {
        params: { path: { id: marketId } },
      });
      if (error || !data) throw new Error(`fetch /v1/markets/${marketId} failed`);
      return data;
    },
    // Changing outcome navigates to a sibling /m/{id}. Seed from the already
    // fetched markets list so the page renders the new market instantly instead
    // of flashing the full-screen "loading market…" placeholder (which unmounts
    // the whole page). The per-market fetch then refreshes it in the background.
    placeholderData: () =>
      qc
        .getQueryData<Market[]>(["markets", "all"])
        ?.find((m) => m.market_id === marketId),
    staleTime: 30_000,
    enabled: Number.isFinite(marketId) && marketId >= 0,
  });
}
