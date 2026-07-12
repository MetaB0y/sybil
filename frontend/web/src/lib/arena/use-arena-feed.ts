"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type ArenaFeed = components["schemas"]["BotDecisionFeedResponse"];
export type ArenaBotSummary = components["schemas"]["BotSummaryResponse"];
export type ArenaDecision = components["schemas"]["BotDecisionResponse"];
export type ArenaTokenUsage = components["schemas"]["TokenUsageResponse"];
export type ArenaEquitySeries =
  components["schemas"]["BotEquitySeriesResponse"];
export type ArenaEquityPoint = components["schemas"]["BotEquityPointResponse"];

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

export function useArenaEquitySeries({
  trader,
  since,
  limit = 360,
}: {
  trader?: string | undefined;
  since?: string | undefined;
  limit?: number;
} = {}) {
  const cleanTrader = trader?.trim() || undefined;
  const cleanSince = since?.trim() || undefined;
  return useQuery({
    queryKey: [
      "arena",
      "bot-equity-series",
      cleanTrader ?? "",
      cleanSince ?? "",
      limit,
    ],
    queryFn: async () => {
      const query: {
        trader?: string;
        since?: string;
        limit: number;
      } = { limit };
      if (cleanTrader != null) query.trader = cleanTrader;
      if (cleanSince != null) query.since = cleanSince;
      const { data, error } = await api.GET("/v1/bots/equity-series", {
        params: { query },
      });
      if (error || !data) throw new Error("/v1/bots/equity-series failed");
      return data;
    },
    enabled: cleanTrader != null,
    refetchInterval: 30_000,
  });
}

export function useArenaDecisionHistory({
  trader,
  marketId,
  since,
  limit = 500,
}: {
  trader?: string | undefined;
  marketId?: number | undefined;
  since?: string | undefined;
  limit?: number;
} = {}) {
  const cleanTrader = trader?.trim() || undefined;
  const cleanSince = since?.trim() || undefined;
  return useQuery({
    queryKey: [
      "arena",
      "bot-decision-history",
      cleanTrader ?? "",
      marketId ?? "",
      cleanSince ?? "",
      limit,
    ],
    queryFn: async () => {
      const query: {
        trader?: string;
        market_id?: number;
        since?: string;
        limit: number;
      } = { limit };
      if (cleanTrader != null) query.trader = cleanTrader;
      if (marketId != null) query.market_id = marketId;
      if (cleanSince != null) query.since = cleanSince;
      const { data, error } = await api.GET("/v1/bots/decisions", {
        params: { query },
      });
      if (error || !data) throw new Error("/v1/bots/decisions failed");
      return data;
    },
    enabled: cleanTrader != null,
    refetchInterval: 30_000,
  });
}
