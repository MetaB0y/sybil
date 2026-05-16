"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { selectLatestBlock, useStore } from "@/lib/store";

export type Portfolio = components["schemas"]["PortfolioResponse"];

/**
 * GET /v1/accounts/{id}/portfolio. Invalidates on every block so positions
 * and PnL reflect the latest clearing prices.
 */
export function usePortfolio(accountId: number | null) {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "portfolio"] });
  }, [accountId, latest?.height, qc]);

  return useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "portfolio"],
    queryFn: async (): Promise<Portfolio> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/portfolio", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch portfolio failed");
      return data;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
