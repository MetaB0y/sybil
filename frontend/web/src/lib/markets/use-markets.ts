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

/** A market Polymarket has closed (resolved / past deadline). */
export function isClosed(m: Market): boolean {
  return m.closed === true;
}

/**
 * A market mirrored from Polymarket. Provenance marker per SYB-150:
 * a non-null `polymarket_condition_id` IS the mirror linkage (set by the
 * mirror via market metadata; native Sybil markets never carry one).
 */
export function isMirror(m: Market): boolean {
  return m.polymarket_condition_id != null;
}

/**
 * A Sybil-native market (SYB-151). The complement of {@link isMirror}: with no
 * `polymarket_condition_id` there is no Polymarket linkage, so the market was
 * created natively on Sybil. Natives carry their own `resolution_criteria` and
 * `external_url` (the resolution source) and — unlike mirrors — may have no
 * `event_id` or imagery.
 */
export function isNative(m: Market): boolean {
  return !isMirror(m);
}

/** An event is shown on the index if at least one of its markets is still open. */
export function eventVisibleOnIndex(markets: Market[]): boolean {
  return markets.some((m) => !isClosed(m));
}

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

export function useMarketsList(initialData?: Market[]) {
  const marketsQ = useQuery({
    queryKey: ["markets", "all"],
    queryFn: fetchMarkets,
    initialData,
    staleTime: 60_000,
  });

  const isPending = marketsQ.isPending;
  const error = marketsQ.error;

  let bundle: MarketsListBundle | null = null;
  if (marketsQ.data) {
    bundle = assemble(marketsQ.data);
  }

  return { bundle, isPending, error, refetch: marketsQ.refetch };
}

export function assemble(allMarkets: Market[]): MarketsListBundle {
  // Keep ALL markets (open + closed) in the bundle. Closed markets are needed
  // by the detail page (read-only state) and by multi-cards (greyed outcome
  // rows). Index-level visibility — hiding fully-closed events and standalone
  // closed binaries — is applied by the markets page, not here. Each market
  // carries its own `closed` flag (`isClosed`) for downstream display logic.
  const markets = allMarkets;

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
