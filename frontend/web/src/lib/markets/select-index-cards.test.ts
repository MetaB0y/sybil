import { describe, expect, it } from "vitest";
import { selectIndexCards, type CardItem } from "./select-index-cards";
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

  it("sinks closed cards below open ones under 'new' sort, even when newer", () => {
    const open1: CardItem = { ...binary(1, { closed: false }), createdMs: 100 };
    const closedNewer: CardItem = { ...binary(2, { closed: true }), createdMs: 999 };
    const open2: CardItem = { ...binary(3, { closed: false }), createdMs: 200 };
    const out = selectIndexCards([open1, closedNewer, open2], {
      ...base,
      sort: "new",
      showClosed: true,
    });
    expect(ids(out)).toEqual([3, 1, 2]);
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
});
