import { describe, expect, it } from "vitest";
import {
  searchResultCards,
  selectIndexCards,
  type CardItem,
} from "./select-index-cards";
import type { Market } from "./use-markets";

function mk(partial: Partial<Market> & { market_id: number }): Market {
  return { name: `m${partial.market_id}`, status: "active", ...partial } as Market;
}

function binary(
  id: number,
  opts: {
    vol?: bigint;
    closed?: boolean;
    category?: string | null;
    traders?: number;
  } = {},
): CardItem {
  return {
    kind: "binary",
    market: mk({ market_id: id, ...(opts.traders !== undefined ? { trader_count: opts.traders } : {}) }),
    volumeNanos: opts.vol ?? 0n,
    sortKey: `m${id}`,
    createdMs: 0,
    primaryCategory: opts.category ?? null,
    closed: opts.closed ?? false,
  };
}

function multi(
  eventId: string,
  opts: { closed?: boolean; vol?: bigint; name?: string } = {},
): CardItem {
  const name = opts.name ?? eventId;
  return {
    kind: "multi",
    name,
    eventId,
    markets: [],
    volumeNanos: opts.vol ?? 0n,
    sortKey: name.toLowerCase(),
    createdMs: 0,
    primaryCategory: null,
    closed: opts.closed ?? false,
  };
}

const NO_TRADERS = new Map<string, number>();
const base = {
  query: "",
  sort: "volume" as const,
  category: null,
  eventTraders: NO_TRADERS,
};

function ids(out: CardItem[]): number[] {
  return out.map((it) => (it.kind === "binary" ? it.market.market_id : -1));
}

describe("selectIndexCards", () => {
  it("hides closed cards by default (showClosed=false)", () => {
    const items = [binary(1, { closed: false }), binary(2, { closed: true })];
    expect(ids(selectIndexCards(items, { ...base, showClosed: false }))).toEqual([1]);
  });

  it("shows closed cards (open first) when showClosed=true", () => {
    const items = [binary(1, { closed: false }), binary(2, { closed: true })];
    const out = selectIndexCards(items, { ...base, showClosed: true });
    expect(out).toHaveLength(2);
    expect(ids(out)).toEqual([1, 2]); // open before closed
  });

  it("sinks closed cards below open ones under volume sort, even with higher volume", () => {
    const items = [
      binary(1, { vol: 10n, closed: false }),
      binary(2, { vol: 999n, closed: true }),
      binary(3, { vol: 5n, closed: false }),
    ];
    const out = selectIndexCards(items, { ...base, sort: "volume", showClosed: true });
    expect(ids(out)).toEqual([1, 3, 2]);
  });

  it("filters by category and query", () => {
    const items = [
      binary(1, { category: "Politics" }),
      binary(2, { category: "Sports" }),
    ];
    expect(ids(selectIndexCards(items, { ...base, category: "Sports", showClosed: true }))).toEqual([2]);
    expect(ids(selectIndexCards([...items], { ...base, query: "m1", showClosed: true }))).toEqual([1]);
  });

  it("sinks closed cards below open ones under 'traders' sort, even with more traders", () => {
    const items = [
      binary(1, { traders: 5 }),
      binary(2, { closed: true, traders: 999 }),
      binary(3, { traders: 10 }),
    ];
    const out = selectIndexCards(items, {
      ...base,
      sort: "traders",
      showClosed: true,
    });
    expect(ids(out)).toEqual([3, 1, 2]);
  });

  it("does not mutate the input array", () => {
    const items = [binary(1, { vol: 1n }), binary(2, { vol: 2n })];
    const snapshot = [...items];
    selectIndexCards(items, { ...base, showClosed: true });
    expect(items).toEqual(snapshot);
  });

  it("keeps a partially-closed multi event visible when showClosed=false", () => {
    // A multi event with at least one open outcome is tagged closed:false by
    // the page, so it must survive the hide-closed filter.
    const partial = multi("ev-partial", { closed: false });
    const fullyClosed = multi("ev-closed", { closed: true });
    const out = selectIndexCards([partial, fullyClosed], {
      ...base,
      showClosed: false,
    });
    expect(out).toHaveLength(1);
    expect(out[0]!.kind === "multi" && out[0]!.eventId).toBe("ev-partial");
  });
});

/** Multi-outcome event card with real outcome markets, for the drill-down. */
function event(
  eventId: string,
  name: string,
  outcomes: Array<{ id: number; name: string; closed?: boolean }>,
): CardItem {
  const markets = outcomes.map((o) =>
    mk({
      market_id: o.id,
      name: o.name,
      event_title: name,
      ...(o.closed ? { closed: true } : {}),
    } as Partial<Market> & { market_id: number }),
  );
  return {
    kind: "multi",
    name,
    eventId,
    markets,
    volumeNanos: 0n,
    sortKey: [name, ...markets.map((m) => m.name)].join(" ").toLowerCase(),
    createdMs: 0,
    primaryCategory: null,
    closed: false,
  };
}

describe("searchResultCards", () => {
  const best = event("ev-best", "Which company has best AI model?", [
    { id: 71, name: "OpenAI" },
    { id: 72, name: "Google" },
    { id: 73, name: "Anthropic" },
  ]);

  it("returns the event itself when the query matches its title", () => {
    const out = searchResultCards([best], "best ai model");
    expect(out).toHaveLength(1);
    expect(out[0]!.kind === "multi" && out[0]!.eventId).toBe("ev-best");
  });

  it("drills to the specific outcome market when only an outcome matches", () => {
    // Event title has no "openai"; the OpenAI outcome does → return that market,
    // not the parent event.
    const out = searchResultCards([best], "openai");
    expect(ids(out)).toEqual([71]);
    expect(out[0]!.kind).toBe("binary");
  });

  it("returns every matching outcome as its own market card", () => {
    // "team" is in two outcomes but not the event title → both drill through.
    const race = event("ev-race", "Best model 2026?", [
      { id: 81, name: "Team Alpha" },
      { id: 82, name: "Team Beta" },
      { id: 83, name: "Solo" },
    ]);
    const out = searchResultCards([race], "team");
    expect(ids(out).sort((a, b) => a - b)).toEqual([81, 82]);
  });

  it("matches a standalone binary by its search key", () => {
    const out = searchResultCards([binary(5)], "m5");
    expect(ids(out)).toEqual([5]);
  });

  it("drops closed matches", () => {
    const closedEvent = event("ev-x", "Race", [
      { id: 9, name: "OpenAI", closed: true },
    ]);
    expect(searchResultCards([closedEvent], "openai")).toEqual([]);
    expect(searchResultCards([binary(2, { closed: true })], "m2")).toEqual([]);
  });

  it("returns nothing for an empty query", () => {
    expect(searchResultCards([best, binary(5)], "  ")).toEqual([]);
  });
});
