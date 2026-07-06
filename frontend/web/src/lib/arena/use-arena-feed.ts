"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type ArenaFeed = components["schemas"]["BotDecisionFeedResponse"];
export type ArenaBotSummary = components["schemas"]["BotSummaryResponse"];
export type ArenaDecision = components["schemas"]["BotDecisionResponse"];
export type ArenaTokenUsage = components["schemas"]["TokenUsageResponse"];

export function useArenaFeed({
  limit = 120,
  trader,
}: {
  limit?: number;
  trader?: string | undefined;
} = {}) {
  const cleanTrader = trader?.trim() || undefined;
  return useQuery({
    queryKey: ["arena", "bot-decisions", limit, cleanTrader ?? ""],
    queryFn: async () => {
      const query =
        cleanTrader == null ? { limit } : { limit, trader: cleanTrader };
      const { data, error } = await api.GET("/v1/bots/decisions", {
        params: { query },
      });
      if (error || !data) throw new Error("/v1/bots/decisions failed");
      return data;
    },
    refetchInterval: 30_000,
  });
}
