/**
 * Build the markets-index cards from a fetched bundle.
 *
 * One `CardItem` per binary market or multi-outcome event, each tagged with
 * `closed`, volume, newness and primary category. The grid (markets page) and
 * the global nav search dropdown both build from this so their results stay
 * identical. Filtering/sorting lives in `selectIndexCards`.
 */

import { pickDisplayCategory } from "@/lib/categorize";
import {
  eventVisibleOnIndex,
  isClosed,
  type IndexMarket,
  type MarketsListBundle,
} from "./use-markets";
import type { CardItem } from "./select-index-cards";

export function buildIndexCards(
  bundle: MarketsListBundle<IndexMarket>,
): CardItem[] {
  const all: CardItem[] = [];
  for (const g of bundle.groups) {
    if (g.markets.length >= 2) {
      // Multi-outcome event. Closed only when EVERY outcome is closed; a
      // partially-closed event stays open (its closed rows render greyed).
      const first = g.markets[0]!;
      const primary = pickDisplayCategory(
        first.categories,
        first.category,
      ).primary;
      all.push({
        kind: "multi",
        name: g.name,
        eventId: g.eventId,
        markets: g.markets,
        volumeNanos: sumVolume(g.markets),
        sortKey: g.name.toLowerCase(),
        createdMs: eventNewnessMs(g.markets),
        primaryCategory: primary,
        closed: !eventVisibleOnIndex(g.markets),
      });
    } else {
      for (const m of g.markets) {
        all.push({
          kind: "binary",
          market: m,
          volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
          sortKey: m.name.toLowerCase(),
          createdMs: marketNewnessMs(m),
          primaryCategory: pickDisplayCategory(m.categories, m.category)
            .primary,
          closed: isClosed(m),
        });
      }
    }
  }
  for (const m of bundle.ungrouped) {
    all.push({
      kind: "binary",
      market: m,
      volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
      sortKey: m.name.toLowerCase(),
      createdMs: m.created_at_ms ?? 0,
      primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
      closed: isClosed(m),
    });
  }
  return all;
}

export function sumVolume(markets: IndexMarket[]): bigint {
  let total = 0n;
  for (const m of markets) {
    if (m.volume_nanos != null) total += BigInt(m.volume_nanos);
  }
  return total;
}

/**
 * "New" sort key: the most recent of the Polymarket event-start and
 * market-start dates, so a brand-new event AND a newly-added outcome inside an
 * existing event both surface. `created_at_ms` (the mirror's admit time, which
 * clusters at sync) is only a last-resort fallback.
 */
export function marketNewnessMs(m: IndexMarket): number {
  return Math.max(
    m.event_start_date_ms ?? 0,
    m.market_start_date_ms ?? 0,
    m.created_at_ms ?? 0,
  );
}

export function eventNewnessMs(markets: IndexMarket[]): number {
  let max = 0;
  for (const m of markets) max = Math.max(max, marketNewnessMs(m));
  return max;
}
