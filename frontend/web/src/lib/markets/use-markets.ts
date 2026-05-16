/**
 * Domain hook for the markets index page.
 *
 * Groups markets by Polymarket `event_id` (off-block metadata from the
 * mirror). This is independent of the matching engine's NegRisk
 * `MarketGroup` rejection logic — frontend grouping is purely cosmetic.
 *
 * Markets without an `event_id` (sybil-native, or a mirror cycle that
 * hasn't filled metadata yet) fall into `ungrouped`.
 *
 * Output shape:
 *   groups: [{ name, eventId, markets }]   — events with ≥1 market
 *   ungrouped: Market[]                     — markets with no event_id
 */

import { useQuery } from "@tanstack/react-query";
import type { components } from "@/lib/api/schema";
import { api } from "@/lib/api/client";

export type Market = components["schemas"]["MarketResponse"];

export type MarketsListBundle = {
  /** Every market keyed by id. */
  byId: Map<number, Market>;
  /** Event-grouped markets. */
  groups: Array<{ name: string; eventId: string; markets: Market[] }>;
  /** Markets with no `event_id`. */
  ungrouped: Market[];
  /** Total market count. */
  total: number;
};

async function fetchMarkets(): Promise<Market[]> {
  const { data, error } = await api.GET("/v1/markets");
  if (error || !data) throw new Error("fetch /v1/markets failed");
  return data;
}

export function useMarketsList() {
  const marketsQ = useQuery({
    queryKey: ["markets", "all"],
    queryFn: fetchMarkets,
    staleTime: 60_000,
  });

  const isPending = marketsQ.isPending;
  const error = marketsQ.error;

  let bundle: MarketsListBundle | null = null;
  if (marketsQ.data) {
    bundle = assemble(marketsQ.data);
  }

  return { bundle, isPending, error };
}

function assemble(markets: Market[]): MarketsListBundle {
  const byId = new Map<number, Market>();
  for (const m of markets) byId.set(m.market_id, m);

  const grouped = new Map<string, { name: string; markets: Market[] }>();
  const ungrouped: Market[] = [];

  for (const m of markets) {
    const eid = m.event_id;
    if (!eid) {
      ungrouped.push(m);
      continue;
    }
    let entry = grouped.get(eid);
    if (!entry) {
      // First market in this event sets the display name. event_title is the
      // authoritative source; we fall back to the market name only when the
      // mirror hasn't filled it (shouldn't happen post-Phase 2).
      entry = { name: m.event_title ?? m.name, markets: [] };
      grouped.set(eid, entry);
    }
    entry.markets.push(m);
  }

  const groups: MarketsListBundle["groups"] = [];
  for (const [eventId, { name, markets: ms }] of grouped) {
    groups.push({ name, eventId, markets: ms });
  }

  return {
    byId,
    groups,
    ungrouped,
    total: markets.length,
  };
}
