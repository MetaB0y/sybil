"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type {
  DevMarket,
  DevMarketGroup,
  DevPendingOrder,
  DevAccountPortfolio,
  DevActivityOverview,
  DevOpenBatch,
  DevBotsResponse,
  DevLeaderboardResponse,
} from "./types";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE ?? "https://62-171-170-238.nip.io";

/** Raw fetch for endpoints/params not modelled in the generated schema. */
async function rawGet<T>(path: string): Promise<T | null> {
  try {
    const r = await fetch(API_BASE + path);
    if (!r.ok) return null;
    return (await r.json()) as T;
  } catch {
    return null;
  }
}

/**
 * Same, but surfaces the failure instead of swallowing it.
 *
 * Several dev endpoints are unreachable against a production deployment —
 * `/v1/orders/pending` is registered in the dev-only route table (404 when
 * `SYBIL_DEV_MODE` is false) and the per-account reads are bearer-gated (401).
 * Collapsing those into an empty array made the views report a confident zero
 * for data they had simply never received.
 */
async function rawGetOrThrow<T>(path: string): Promise<T> {
  const r = await fetch(API_BASE + path);
  if (!r.ok) throw new Error(`${path} → HTTP ${r.status}`);
  return (await r.json()) as T;
}

/** /v1/markets/summary — markets with volume/liquidity/24h fields. */
export function useDevMarkets() {
  return useQuery({
    queryKey: ["dev", "markets-summary"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/markets/summary");
      if (error || !data) throw new Error("/v1/markets/summary failed");
      return data as unknown as DevMarket[];
    },
    refetchInterval: 10_000,
  });
}

/**
 * The backend occasionally emits two groups that share one event title (a stale
 * copy plus a near-superset that adds a market). Merge same-named groups into a
 * single entry holding the union of their market_ids. This keeps the dropdown's
 * option keys/values unique and makes the group filter complete — `filterMarkets`
 * looks up groups by name and would otherwise see only the first arbitrary copy.
 */
function mergeGroupsByName(groups: DevMarketGroup[]): DevMarketGroup[] {
  const byName = new Map<string, Set<number>>();
  const order: string[] = [];
  for (const g of groups) {
    let ids = byName.get(g.name);
    if (!ids) {
      ids = new Set<number>();
      byName.set(g.name, ids);
      order.push(g.name);
    }
    for (const id of g.market_ids) ids.add(id);
  }
  return order.map((name) => ({
    name,
    market_ids: [...byName.get(name)!].sort((a, b) => a - b),
  }));
}

export function useDevGroups() {
  return useQuery({
    queryKey: ["dev", "market-groups"],
    queryFn: async () =>
      mergeGroupsByName(
        (await rawGet<DevMarketGroup[]>("/v1/markets/groups")) ?? [],
      ),
    staleTime: 60_000,
  });
}

export function useDevPendingOrders() {
  return useQuery({
    queryKey: ["dev", "pending-orders"],
    queryFn: () => rawGetOrThrow<DevPendingOrder[]>("/v1/orders/pending"),
    retry: false,
    refetchInterval: 10_000,
  });
}

/** Account portfolios discovered from durable Arena identities, orders, and leaderboard state. */
export function useDevAccounts(extraIds: number[] = []) {
  const pending = useDevPendingOrders().data ?? [];
  const bots = useDevBots().data?.summaries ?? [];
  const observedIds = [
    ...extraIds,
    ...pending.map((order) => Number(order.account_id)),
    ...bots.flatMap((bot) =>
      bot.account_id == null ? [] : [Number(bot.account_id)],
    ),
  ].filter(Number.isSafeInteger);

  return useQuery({
    queryKey: [
      "dev",
      "accounts",
      [...new Set(observedIds)].sort((a, b) => a - b),
    ],
    queryFn: async () => {
      const leaderboard = await rawGet<DevLeaderboardResponse>(
        "/v1/leaderboard?window=all&limit=100",
      );
      const ids = Array.from(
        new Set([
          ...observedIds,
          ...(leaderboard?.entries ?? []).map((entry) =>
            Number(entry.account_id),
          ),
        ]),
      ).sort((a, b) => a - b);
      const rows = await Promise.all(
        ids.map((id) =>
          rawGet<DevAccountPortfolio>(`/v1/accounts/${id}/portfolio`),
        ),
      );
      const found = rows.filter((r): r is DevAccountPortfolio => r != null);
      // Asking for accounts and receiving none back is a read failure, not an
      // empty system: `/v1/accounts/{id}/portfolio` is bearer-gated and answers
      // 401 to the dev zone, which holds no token.
      if (ids.length > 0 && found.length === 0) {
        throw new Error(
          "No account portfolio was readable — /v1/accounts/{id}/portfolio requires a bearer token",
        );
      }
      return found.sort((a, b) => a.account_id - b.account_id);
    },
    retry: false,
    refetchInterval: 60_000,
  });
}

export function useDevActivityOverview() {
  return useQuery({
    queryKey: ["dev", "activity-overview"],
    queryFn: async () =>
      (await rawGet<DevActivityOverview>("/v1/activity/overview")) ?? {},
    refetchInterval: 10_000,
  });
}

export function useDevBots() {
  return useQuery({
    queryKey: ["dev", "bots-decisions"],
    queryFn: async () => {
      const data = await rawGet<DevBotsResponse>("/v1/bots/decisions?limit=80");
      return (
        data ?? {
          db_available: false,
          error: "failed to load bot decision feed",
        }
      );
    },
    refetchInterval: 30_000,
  });
}

/** On-demand: open-batch indicative snapshot for one market. */
export function useDevOpenBatch(marketId: number) {
  return useQuery({
    queryKey: ["dev", "open-batch", marketId],
    queryFn: async () =>
      (await rawGet<DevOpenBatch>(`/v1/markets/${marketId}/open-batch`)) ?? {},
    enabled: marketId > 0,
  });
}

/** On-demand: a single account's portfolio (Aggregates cost-basis panel). */
export function useDevPortfolio(accountId: number) {
  return useQuery({
    queryKey: ["dev", "portfolio", accountId],
    queryFn: async () =>
      (await rawGet<DevAccountPortfolio>(
        `/v1/accounts/${accountId}/portfolio`,
      )) ?? null,
    enabled: accountId > 0,
  });
}

/** Recent fills per account, for the Participants table fill counts. */
export function useDevAccountFills(ids: number[]) {
  const sorted = [...new Set(ids)].slice(0, 24).sort((a, b) => a - b);
  return useQuery({
    queryKey: ["dev", "account-fills", sorted],
    queryFn: async () => {
      const entries = await Promise.all(
        sorted.map(async (id) => {
          const page = await rawGet<{ fills: unknown[] }>(
            `/v1/accounts/${id}/fills?limit=25`,
          );
          return [id, page?.fills ?? []] as const;
        }),
      );
      return Object.fromEntries(entries) as Record<number, unknown[]>;
    },
    enabled: sorted.length > 0,
    refetchInterval: 60_000,
  });
}
