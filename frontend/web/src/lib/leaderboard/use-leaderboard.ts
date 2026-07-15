"use client";

/**
 * Trader leaderboard (SYB-59). Ranks accounts by windowed PnL over the
 * selected window. Ordering + ranks are computed server-side (`/v1/leaderboard`)
 * — this hook only maps the wire rows into display rows with bigint money.
 *
 * Only signed-profile opt-ins are returned. The display name is the explicit
 * publication boundary for the row's account ID and financial statistics.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { components } from "../api/schema";
import { parseNanos } from "../format/nanos";

export type LeaderboardWindow = "7D" | "30D" | "ALL";

/** Segmented-control window → API query token. */
export const WINDOW_QUERY: Record<LeaderboardWindow, "7d" | "30d" | "all"> = {
  "7D": "7d",
  "30D": "30d",
  ALL: "all",
};

type ApiEntry = components["schemas"]["LeaderboardEntryResponse"];
type ApiResponse = components["schemas"]["LeaderboardResponse"];

export interface LeaderboardRow {
  rank: number;
  accountId: number;
  /** Signed public display name. */
  label: string;
  pnlNanos: bigint;
  roiBps: number;
  marketsTraded: number;
  equityNanos: bigint;
}

/** Pure mapper: wire response → display rows. Exported for unit tests. */
export function toLeaderboardRows(
  data: ApiResponse | undefined,
): LeaderboardRow[] {
  if (!data?.entries) return [];
  return data.entries.map((e: ApiEntry) => ({
    rank: e.rank,
    accountId: e.account_id,
    label: e.display_name,
    pnlNanos: parseNanos(e.pnl_nanos),
    roiBps: e.roi_bps,
    marketsTraded: e.markets_traded,
    equityNanos: parseNanos(e.equity_nanos),
  }));
}

export interface UseLeaderboardResult {
  rows: LeaderboardRow[];
  isLoading: boolean;
  isRetrying: boolean;
  readState: "ready" | "unavailable" | "stale";
  errorMessage: string | null;
  retry: () => void;
}

export function useLeaderboard(
  window: LeaderboardWindow,
): UseLeaderboardResult {
  const q = useQuery({
    queryKey: ["leaderboard", window],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/leaderboard", {
        // Pull the full ranked set (server cap is 100) so the client can sort
        // and paginate across everyone, not just the default top 50.
        params: { query: { window: WINDOW_QUERY[window], limit: 100 } },
      });
      if (error || !data) throw new Error(leaderboardErrorMessage(error));
      return data;
    },
    refetchInterval: 15_000,
  });

  const hasData = q.data !== undefined;
  return {
    rows: toLeaderboardRows(q.data),
    isLoading: q.isPending,
    isRetrying: q.isFetching,
    readState: q.error == null ? "ready" : hasData ? "stale" : "unavailable",
    errorMessage: q.error instanceof Error ? q.error.message : null,
    retry: () => {
      void q.refetch();
    },
  };
}

function leaderboardErrorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null && "error" in error) {
    const detail = (error as { error?: unknown }).error;
    if (typeof detail === "string" && detail.trim() !== "") return detail;
  }
  return "Rankings could not be loaded";
}
