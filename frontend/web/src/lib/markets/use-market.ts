"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useSyncExternalStore } from "react";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type Market = components["schemas"]["MarketResponse"];

export function useMarket(marketId: number) {
  const qc = useQueryClient();
  // NavSearch can populate the shared markets list while a freshly loaded
  // market route is still hydrating. The server rendered the loading state, so
  // consuming that cache on the first client render would replace it with the
  // full market tree before React can claim the server HTML. Enable the cache
  // shortcut immediately after hydration; client-side outcome switches still
  // get their instant placeholder.
  const hydrated = useSyncExternalStore(
    subscribeToHydration,
    hydratedOnClient,
    hydratedOnServer,
  );

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
    ...(hydrated
      ? {
          placeholderData: () =>
            qc
              .getQueryData<Market[]>(["markets", "all"])
              ?.find((m) => m.market_id === marketId),
        }
      : {}),
    staleTime: 30_000,
    enabled: Number.isFinite(marketId) && marketId >= 0,
  });
}

function subscribeToHydration(): () => void {
  return () => {};
}

function hydratedOnClient(): boolean {
  return true;
}

function hydratedOnServer(): boolean {
  return false;
}
