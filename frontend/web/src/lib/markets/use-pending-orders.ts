"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";
import { selectLatestBlock, selectWsLive, useStore } from "@/lib/store";
import { ACCOUNT_POLL_MS } from "@/lib/constants";

export type PendingOrder = components["schemas"]["PendingOrderResponse"];

/**
 * Fetches /v1/orders/pending (all markets) and filters to the given market.
 * Invalidates on every new block so the list refreshes per batch cadence.
 *
 * The endpoint doesn't accept a market_id filter today, so we filter
 * client-side. Tolerable while the total pending pool is in the low
 * hundreds; add server-side filter if it gets large.
 */
export function usePendingOrdersForMarket(marketId: number) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);
  const wsLive = useStore(selectWsLive);

  // Each new block invalidates the cached pending list so we pull fresh.
  useEffect(() => {
    qc.invalidateQueries({ queryKey: ["orders", "pending"] });
  }, [latest?.height, qc]);

  return useQuery({
    queryKey: ["orders", "pending"],
    queryFn: async (): Promise<PendingOrder[]> => {
      const { data, error } = await api.GET("/v1/orders/pending");
      if (error || !data) throw new Error("fetch /v1/orders/pending failed");
      return data;
    },
    select: (all): PendingOrder[] =>
      all.filter((o) => o.market_id === marketId),
    staleTime: 0,
    refetchOnWindowFocus: false,
    refetchInterval: wsLive ? false : ACCOUNT_POLL_MS,
  });
}
