/**
 * Filter + order the markets-index cards.
 *
 * Cards are built once in the page (one `CardItem` per binary market or
 * multi-outcome event, each tagged with `closed`). This helper owns the
 * query/category/closed filtering and the sort. Closed cards are dropped unless
 * `showClosed`, and always sink below open cards regardless of the active sort.
 */

import { binaryCardOf } from "./build-index-cards";
import type { Market } from "./use-markets";
import type { SortKey } from "./sort";

export type CardItem =
  | {
      kind: "multi";
      name: string;
      eventId: string;
      markets: Market[];
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
      closed: boolean;
    }
  | {
      kind: "binary";
      market: Market;
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

/**
 * Nav-search results (drill-down). Where the grid shows one card per event,
 * search resolves each match to the most specific thing:
 *   - an event whose *title* matches the query is returned as the event;
 *   - an event that matches only through its outcomes is replaced by those
 *     matching outcomes, each as its own market card;
 *   - a standalone market matches on its own name/event title.
 * So typing "OpenAI" jumps to the specific OpenAI market instead of the parent
 * "best AI model" event. Closed cards drop; the rest are volume-sorted (bigger
 * events break ties first), matching the dropdown's volume-preview intent.
 */
export function searchResultCards(
  items: CardItem[],
  query: string,
): CardItem[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const out: CardItem[] = [];
  for (const it of items) {
    if (it.kind === "binary") {
      if (it.sortKey.includes(q)) out.push(it);
    } else if (it.name.toLowerCase().includes(q)) {
      out.push(it);
    } else {
      for (const m of it.markets) {
        if (m.name.toLowerCase().includes(q)) out.push(binaryCardOf(m));
      }
    }
  }
  return out
    .filter((c) => !c.closed)
    .sort((a, b) => {
      if (a.volumeNanos !== b.volumeNanos) {
        return a.volumeNanos < b.volumeNanos ? 1 : -1;
      }
      return sizeOf(b) - sizeOf(a);
    });
}
