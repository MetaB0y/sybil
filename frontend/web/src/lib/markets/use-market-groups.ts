"use client";

/**
 * GET /v1/markets/groups — the engine's NegRisk market groups.
 *
 * Group membership drives the complete-set self-trade rule (see
 * `lib/account/complete-set.ts`). It is NOT derivable from `event_id`: only
 * mutually-exclusive NegRisk events are registered as groups, so an event's
 * siblings are a superset of what the engine treats as a group.
 *
 * Groups only change when a market is created, so this is cached hard.
 */

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

/** The market ids grouped with `marketId`, or `[]` when it isn't in a group. */
export function useGroupMarkets(marketId: number | null): number[] {
  const { data } = useMarketGroups();
  if (data == null || marketId == null) return [];
  return data.find((g) => g.market_ids.includes(marketId))?.market_ids ?? [];
}
