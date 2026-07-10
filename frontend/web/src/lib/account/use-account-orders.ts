"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { selectLatestBlock, selectWsLive, useStore } from "@/lib/store";
import { ACCOUNT_POLL_MS } from "@/lib/constants";

export type AccountOrder = components["schemas"]["PendingOrderResponse"];

/**
 * GET /v1/accounts/{id}/orders — open orders for one account. Invalidates
 * each block; new fills shrink/remove entries. When the block WS is not live,
 * falls back to interval polling so the open-orders list still updates.
 */
export function useAccountOrders(accountId: number | null) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);
  const wsLive = useStore(selectWsLive);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "orders"] });
  }, [accountId, latest?.height, qc]);

  return useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "orders"],
    queryFn: async (): Promise<AccountOrder[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/orders", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account orders failed");
      return data;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
    refetchInterval: wsLive ? false : ACCOUNT_POLL_MS,
  });
}
