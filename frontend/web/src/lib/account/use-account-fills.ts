"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { selectLatestBlock, selectWsLive, useStore } from "@/lib/store";
import { ACCOUNT_POLL_MS } from "@/lib/constants";

export type AccountFill = components["schemas"]["AccountFillResponse"];

/**
 * GET /v1/accounts/{id}/fills — fill history for one account. Optional
 * market filter + pagination. Invalidates per block so fresh fills appear.
 * When the block WS is not live, falls back to interval polling so fills
 * still surface while the socket reconnects.
 */
export function useAccountFills(
  accountId: number | null,
  opts: { marketId?: number; limit?: number } = {},
) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);
  const wsLive = useStore(selectWsLive);
  const { marketId, limit = 50 } = opts;

  const key = ["account", accountId, "fills", { marketId, limit }] as const;

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "fills"] });
  }, [accountId, latest?.height, qc]);

  return useQuery({
    enabled: accountId !== null,
    queryKey: key,
    queryFn: async (): Promise<AccountFill[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/fills", {
        params: {
          path: { id: accountId },
          query: {
            ...(marketId !== undefined ? { market_id: marketId } : {}),
            limit,
          },
        },
      });
      if (error || !data) throw new Error("fetch account fills failed");
      return data.fills;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
    refetchInterval: wsLive ? false : ACCOUNT_POLL_MS,
  });
}
