/**
 * Build the markets-index cards from a fetched bundle.
 *
 * One `CardItem` per binary market or multi-outcome event, each tagged with
 * `closed`, volume, newness and primary category. The grid (markets page) filters
 * these with `selectIndexCards` (one card per event); the nav search drills
 * further with `searchResultCards`, replacing an outcome-only event match with
 * the specific matching markets. Both start from the cards built here.
 */

import { pickDisplayCategory } from "@/lib/categorize";
import {
  eventVisibleOnIndex,
  isClosed,
  type Market,
  type MarketsListBundle,
} from "./use-markets";
import type { CardItem } from "./select-index-cards";

export function buildIndexCards(bundle: MarketsListBundle): CardItem[] {
  const all: CardItem[] = [];
  for (const g of bundle.groups) {
    if (g.markets.length >= 2) {
      // Multi-outcome event. Closed only when EVERY outcome is closed; a
      // partially-closed event stays open (its closed rows render greyed).
      const first = g.markets[0]!;
      const primary = pickDisplayCategory(first.categories, first.category).primary;
      all.push({
        kind: "multi",
        name: g.name,
        eventId: g.eventId,
        markets: g.markets,
        volumeNanos: sumVolume(g.markets),
        // Search matches the event name AND every outcome/market title, so
        // typing an outcome (e.g. "Anthropic") surfaces its parent event card
        // — not just events whose own title contains the query.
        sortKey: [g.name, ...g.markets.map((m) => m.name)]
          .join(" ")
          .toLowerCase(),
        createdMs: eventNewnessMs(g.markets),
        primaryCategory: primary,
        closed: !eventVisibleOnIndex(g.markets),
      });
    } else {
      for (const m of g.markets) all.push(binaryCardOf(m));
    }
  }
  for (const m of bundle.ungrouped) all.push(binaryCardOf(m));
  return all;
}

/**
 * Build one binary (single-market) card. Shared by the index builder and the
 * nav search, which drills a matching event outcome down into its own card so
 * "OpenAI" jumps to the specific market rather than its parent event. The search
 * key carries the market name plus its event title, so either finds it.
 */
export function binaryCardOf(m: Market): CardItem {
  return {
    kind: "binary",
    market: m,
    volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
    sortKey: [m.name, m.event_title].filter(Boolean).join(" ").toLowerCase(),
    createdMs: marketNewnessMs(m),
    primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
    closed: isClosed(m),
  };
}

export function sumVolume(markets: Market[]): bigint {
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
export function marketNewnessMs(m: Market): number {
  return Math.max(
    m.event_start_date_ms ?? 0,
    m.market_start_date_ms ?? 0,
    m.created_at_ms ?? 0
  );
}

export function eventNewnessMs(markets: Market[]): number {
  let max = 0;
  for (const m of markets) max = Math.max(max, marketNewnessMs(m));
  return max;
}
