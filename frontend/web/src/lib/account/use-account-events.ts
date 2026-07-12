"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { selectLatestBlock, useStore } from "@/lib/store";

export type AccountEvent = components["schemas"]["HistoryEventResponse"];

/**
 * GET /v1/accounts/{id}/events — the per-account event log (placed /
 * partial_fill / filled / expired / …). Invalidates each block so the degen
 * tracker sees fills as batches clear. Mirrors `useAccountOrders`.
 */
export function useAccountEvents(accountId: number | null) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "events"] });
  }, [accountId, latest?.height, qc]);

  return useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "events"],
    queryFn: async (): Promise<AccountEvent[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/events", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account events failed");
      return data.events;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
