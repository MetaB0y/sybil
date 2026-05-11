/**
 * Domain hook for the markets index page. Combines /v1/markets (full list
 * with descriptions) and /v1/markets/groups (Polymarket-style event groupings).
 *
 * Output shape:
 *   groups: [{ name, marketIds, markets }]   — non-empty groups
 *   ungrouped: Market[]                       — markets not in any group
 */

import { useQuery } from "@tanstack/react-query";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type Market = components["schemas"]["MarketResponse"];
export type MarketGroup = components["schemas"]["MarketGroupResponse"];

export type MarketsListBundle = {
  /** Every market keyed by id. */
  byId: Map<number, Market>;
  /** Groups (events) with their resolved markets. Empty groups dropped. */
  groups: Array<{ name: string; marketIds: number[]; markets: Market[] }>;
  /** Markets not in any group. */
  ungrouped: Market[];
  /** Total market count (active + others). */
  total: number;
};

async function fetchMarkets(): Promise<Market[]> {
  const { data, error } = await api.GET("/v1/markets");
  if (error || !data) throw new Error("fetch /v1/markets failed");
  return data;
}

async function fetchGroups(): Promise<MarketGroup[]> {
  const { data, error } = await api.GET("/v1/markets/groups");
  if (error || !data) throw new Error("fetch /v1/markets/groups failed");
  return data;
}

export function useMarketsList() {
  const marketsQ = useQuery({
    queryKey: ["markets", "all"],
    queryFn: fetchMarkets,
    staleTime: 60_000,
  });

  const groupsQ = useQuery({
    queryKey: ["markets", "groups"],
    queryFn: fetchGroups,
    staleTime: 60_000,
  });

  const isPending = marketsQ.isPending || groupsQ.isPending;
  const error = marketsQ.error ?? groupsQ.error;

  let bundle: MarketsListBundle | null = null;
  if (marketsQ.data && groupsQ.data) {
    bundle = assemble(marketsQ.data, groupsQ.data);
  }

  return { bundle, isPending, error };
}

function assemble(markets: Market[], groups: MarketGroup[]): MarketsListBundle {
  const byId = new Map<number, Market>();
  for (const m of markets) byId.set(m.market_id, m);

  const groupedIds = new Set<number>();
  const out: MarketsListBundle["groups"] = [];

  for (const g of groups) {
    const ms: Market[] = [];
    for (const id of g.market_ids) {
      const m = byId.get(id);
      if (m) {
        ms.push(m);
        groupedIds.add(id);
      }
    }
    if (ms.length > 0) {
      out.push({ name: g.name, marketIds: g.market_ids, markets: ms });
    }
  }

  const ungrouped = markets.filter((m) => !groupedIds.has(m.market_id));

  return {
    byId,
    groups: out,
    ungrouped,
    total: markets.length,
  };
}
