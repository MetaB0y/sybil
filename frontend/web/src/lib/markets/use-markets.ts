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

/**
 * Fields required to render, filter, and sort the markets index. The full
 * MarketResponse also carries long descriptions, resolution links, and
 * detail-only counters; serializing those through the Server→Client boundary
 * made the index HTML hundreds of KiB larger without changing its first paint.
 */
export const INDEX_MARKET_FIELDS = [
  "market_id",
  "name",
  "status",
  "closed",
  "categories",
  "category",
  "volume_nanos",
  "liquidity_avg10_nanos",
  "trader_count",
  "market_image_url",
  "market_icon_url",
  "event_id",
  "event_title",
  "event_image_url",
  "event_icon_url",
  "group_item_title",
  "polymarket_condition_id",
  "created_at_ms",
  "event_start_date_ms",
  "market_start_date_ms",
] as const satisfies readonly (keyof Market)[];

export type IndexMarket = Pick<Market, (typeof INDEX_MARKET_FIELDS)[number]>;

/** Keep null/undefined optionals out of the serialized RSC payload entirely. */
export function toIndexMarket(market: Market): IndexMarket {
  return Object.fromEntries(
    INDEX_MARKET_FIELDS.flatMap((key) => {
      const value = market[key];
      return value == null ? [] : [[key, value]];
    }),
  ) as IndexMarket;
}

/** A market Polymarket has closed (resolved / past deadline). */
export function isClosed(m: IndexMarket): boolean {
  return m.closed === true;
}

/**
 * A market mirrored from Polymarket. Provenance marker per SYB-150:
 * a non-null `polymarket_condition_id` IS the mirror linkage (set by the
 * mirror via market metadata; native Sybil markets never carry one).
 */
export function isMirror(m: IndexMarket): boolean {
  return m.polymarket_condition_id != null;
}

/**
 * A Sybil-native market (SYB-151). The complement of {@link isMirror}: with no
 * `polymarket_condition_id` there is no Polymarket linkage, so the market was
 * created natively on Sybil. Natives carry their own `resolution_criteria` and
 * `external_url` (the resolution source) and — unlike mirrors — may have no
 * `event_id` or imagery.
 */
export function isNative(m: IndexMarket): boolean {
  return !isMirror(m);
}

/** An event is shown on the index if at least one of its markets is still open. */
export function eventVisibleOnIndex(markets: IndexMarket[]): boolean {
  return markets.some((m) => !isClosed(m));
}

export type MarketsListBundle<M extends IndexMarket = Market> = {
  /** Every market keyed by id. */
  byId: Map<number, M>;
  /** Event-grouped markets. */
  groups: Array<{ name: string; eventId: string; markets: M[] }>;
  /** Markets with no `event_id`. */
  ungrouped: M[];
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

  return {
    bundle,
    isPending,
    isFetching: marketsQ.isFetching,
    error,
    refetch: marketsQ.refetch,
  };
}

/**
 * Index-only observer. Its compact server payload is placeholderData, which
 * React Query deliberately does not persist in the shared cache. Hydration
 * therefore starts an authoritative full `/v1/markets` fetch immediately;
 * detail/portfolio consumers either receive that full result or remain in
 * their normal loading state, never a partial MarketResponse masquerading as
 * canonical data.
 */
export function useMarketsIndex(initialData?: IndexMarket[]) {
  const marketsQ = useQuery<IndexMarket[]>({
    queryKey: ["markets", "all"],
    queryFn: fetchMarkets,
    ...(initialData ? { placeholderData: initialData } : {}),
    staleTime: 60_000,
  });

  const isPending = marketsQ.isPending;
  const error = marketsQ.error;
  const bundle = marketsQ.data ? assemble(marketsQ.data) : null;

  return {
    bundle,
    isPending,
    isFetching: marketsQ.isFetching,
    error,
    refetch: marketsQ.refetch,
  };
}

export function assemble<M extends IndexMarket>(
  allMarkets: M[],
): MarketsListBundle<M> {
  // Keep ALL markets (open + closed) in the bundle. Closed markets are needed
  // by the detail page (read-only state) and by multi-cards (greyed outcome
  // rows). Index-level visibility — hiding fully-closed events and standalone
  // closed binaries — is applied by the markets page, not here. Each market
  // carries its own `closed` flag (`isClosed`) for downstream display logic.
  const markets = allMarkets;

  const byId = new Map<number, M>();
  for (const m of markets) byId.set(m.market_id, m);

  const grouped = new Map<string, { name: string; markets: M[] }>();
  const ungrouped: M[] = [];

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

  const groups: MarketsListBundle<M>["groups"] = [];
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
