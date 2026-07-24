"use client";

/**
 * Arena bots as leaderboard rows.
 *
 * These do NOT come from `/v1/leaderboard`. That endpoint ranks sequencer
 * accounts and only returns display-name opt-ins, and no bot publishes a
 * profile — so bots are invisible to it by construction. Their figures come
 * from the Arena feed instead (`/data/decisions.db`), which is a separate
 * ledger from engine state; rows are tagged `bot` so the table can say so
 * rather than implying the two were computed the same way.
 *
 * Only `scored` bots are listed. The Arena runtime also runs `load` and
 * `noise` cohorts (Fast-*, Noise-*) whose losses are a cost of generating
 * order flow, not a competitive result — `scored` is the backend's own signal
 * for "eligible for public competition totals".
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { components } from "../api/schema";
import type { LeaderboardRow, LeaderboardWindow } from "./use-leaderboard";

type BotFeed = components["schemas"]["BotDecisionFeedResponse"];
type BotSeries = components["schemas"]["BotEquitySeriesResponse"];
type BotPoint = components["schemas"]["BotEquityPointResponse"];

const DAY_MS = 24 * 3_600_000;

/** Lookback per window; `null` is all-time (no baseline subtraction). */
export const WINDOW_DAYS: Record<LeaderboardWindow, number | null> = {
  "7D": 7,
  "30D": 30,
  ALL: null,
};

/** Arena reports dollars as doubles; the table speaks integer nanodollars. */
function toNanos(dollars: number): bigint {
  return BigInt(Math.round(dollars * 1e9));
}

/**
 * Cumulative PnL at each bot's first in-window snapshot. Subtracting it turns
 * the summary's all-time PnL into a windowed one, matching what the server
 * does for human rows. Bots with no snapshot before the window opened simply
 * have no baseline, so their all-time figure already is the windowed one.
 */
export function baselinePnlByBot(points: BotPoint[] | undefined): Map<string, number> {
  const baseline = new Map<string, number>();
  for (const point of points ?? []) {
    if (!baseline.has(point.trader_name)) {
      baseline.set(point.trader_name, point.pnl ?? 0);
    }
  }
  return baseline;
}

/** Pure mapper: Arena feed → display rows. Ranks are assigned by the merge. */
export function toBotRows(
  feed: BotFeed | undefined,
  baseline: Map<string, number>,
): LeaderboardRow[] {
  return (feed?.summaries ?? [])
    .filter((summary) => summary.scored && summary.account_id != null)
    .map((summary) => {
      const equity = summary.portfolio_value ?? 0;
      const pnl = (summary.pnl ?? 0) - (baseline.get(summary.trader_name) ?? 0);
      // ROI is measured against the capital the bot started the window with,
      // which is what it holds now minus what it made or lost since.
      const opening = equity - pnl;
      return {
        kind: "bot" as const,
        rank: 0,
        accountId: summary.account_id as number,
        label: summary.trader_name,
        pnlNanos: toNanos(pnl),
        roiBps: opening > 0 ? Math.round((pnl / opening) * 10_000) : 0,
        // Arena tracks fills and orders, not distinct markets held. Reporting
        // total_fills under a "Markets" column would be a different quantity
        // wearing the same label, so leave it unknown.
        marketsTraded: null,
        equityNanos: toNanos(equity),
      };
    });
}

export interface UseBotLeaderboardResult {
  rows: LeaderboardRow[];
  /** True only when bots are missing because the Arena feed itself failed. */
  isUnavailable: boolean;
}

export function useBotLeaderboard(
  window: LeaderboardWindow,
): UseBotLeaderboardResult {
  const feed = useQuery({
    queryKey: ["bot-leaderboard-feed"],
    queryFn: async () => {
      // Only `summaries` is needed; ask for the smallest legal decision page.
      const { data, error } = await api.GET("/v1/bots/decisions", {
        params: { query: { limit: 1 } },
      });
      if (error || !data) throw new Error("Arena feed unavailable");
      return data;
    },
    refetchInterval: 15_000,
  });

  const days = WINDOW_DAYS[window];

  const series = useQuery({
    // Keyed by the window, not by the derived timestamp: a `Date.now()` taken
    // during render would produce a new key every pass and refetch forever.
    queryKey: ["bot-leaderboard-baseline", window],
    enabled: days != null,
    queryFn: async () => {
      const since = new Date(
        Date.now() - (days as number) * DAY_MS,
      ).toISOString();
      const { data, error } = await api.GET("/v1/bots/equity-series", {
        params: { query: { since, limit: 1000 } },
      });
      if (error || !data) throw new Error("Arena series unavailable");
      return data;
    },
    refetchInterval: 60_000,
  });

  const feedData = feed.data;
  const seriesPoints = (series.data as BotSeries | undefined)?.points;
  const dbDown = feedData != null && !feedData.db_available;

  // Memoized so the merged ranking downstream is not resorted on every render.
  const rows = useMemo(
    () => (dbDown ? [] : toBotRows(feedData, baselinePnlByBot(seriesPoints))),
    [dbDown, feedData, seriesPoints],
  );

  return { rows, isUnavailable: feed.error != null || dbDown };
}
