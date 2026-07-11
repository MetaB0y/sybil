/**
 * Filter + order the markets-index cards.
 *
 * Cards are built once in the page (one `CardItem` per binary market or
 * multi-outcome event, each tagged with `closed`). This helper owns the
 * query/category/closed filtering and the sort. Closed cards are dropped unless
 * `showClosed`, and always sink below open cards regardless of the active sort.
 */

import type { IndexMarket } from "./use-markets";
import type { SortKey } from "./sort";

export type CardItem =
  | {
      kind: "multi";
      name: string;
      eventId: string;
      markets: IndexMarket[];
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
      closed: boolean;
    }
  | {
      kind: "binary";
      market: IndexMarket;
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
      closed: boolean;
    };

export type SelectOptions = {
  query: string;
  sort: SortKey;
  category: string | null;
  showClosed: boolean;
  eventTraders: Map<string, number>;
};

/** Outcomes a card represents (volume tie-break: bigger events first). */
export function sizeOf(item: CardItem): number {
  return item.kind === "multi" ? item.markets.length : 1;
}

/** Trader count for sorting: per-market for binary, event union for multi. */
export function traderCountOf(
  item: CardItem,
  eventTraders: Map<string, number>,
): number {
  if (item.kind === "binary") return item.market.trader_count ?? 0;
  return eventTraders.get(item.eventId) ?? 0;
}

function compareBySort(
  a: CardItem,
  b: CardItem,
  sort: SortKey,
  eventTraders: Map<string, number>,
): number {
  if (sort === "new") {
    return b.createdMs - a.createdMs;
  }
  if (sort === "traders") {
    const ta = traderCountOf(a, eventTraders);
    const tb = traderCountOf(b, eventTraders);
    if (ta !== tb) return tb - ta;
    if (a.volumeNanos === b.volumeNanos) return 0;
    return a.volumeNanos < b.volumeNanos ? 1 : -1;
  }
  // volume desc; tie-break by size desc.
  if (a.volumeNanos !== b.volumeNanos) {
    return a.volumeNanos < b.volumeNanos ? 1 : -1;
  }
  return sizeOf(b) - sizeOf(a);
}

export function selectIndexCards(
  items: CardItem[],
  opts: SelectOptions,
): CardItem[] {
  const q = opts.query.trim().toLowerCase();
  let out = items;
  if (q) out = out.filter((it) => it.sortKey.includes(q));
  if (opts.category) {
    out = out.filter((it) => it.primaryCategory === opts.category);
  }
  if (!opts.showClosed) {
    out = out.filter((it) => !it.closed);
  }
  out = [...out];
  out.sort((a, b) => {
    // Closed cards always sink below open ones, regardless of sort mode.
    if (a.closed !== b.closed) return a.closed ? 1 : -1;
    return compareBySort(a, b, opts.sort, opts.eventTraders);
  });
  return out;
}
