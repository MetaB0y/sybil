"use client";

/**
 * Trader leaderboard (SYB-59). Ranks accounts by windowed PnL over the
 * selected window. Ordering + ranks are computed server-side (`/v1/leaderboard`)
 * — this hook only maps the wire rows into display rows with bigint money.
 *
 * Traders are anonymous by default ("Trader #<id>"); opt-in display names await
 * profiles (SYB-60).
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
  /** Anonymous display label, e.g. `Trader #42`. */
  label: string;
  pnlNanos: bigint;
  roiBps: number;
  marketsTraded: number;
  equityNanos: bigint;
}

/** Anonymous label for an account. Display-name opt-in awaits SYB-60. */
export function traderLabel(accountId: number): string {
  return `Trader #${accountId}`;
}

/** Pure mapper: wire response → display rows. Exported for unit tests. */
export function toLeaderboardRows(
  data: ApiResponse | undefined,
): LeaderboardRow[] {
  if (!data?.entries) return [];
  return data.entries.map((e: ApiEntry) => ({
    rank: e.rank,
    accountId: e.account_id,
    label: traderLabel(e.account_id),
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
  retry: () => void;
}

export function useLeaderboard(
  window: LeaderboardWindow,
): UseLeaderboardResult {
  const q = useQuery({
    queryKey: ["leaderboard", window],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/leaderboard", {
        params: { query: { window: WINDOW_QUERY[window] } },
      });
      if (error || !data) throw new Error("/v1/leaderboard failed");
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
    retry: () => {
      void q.refetch();
    },
  };
}
