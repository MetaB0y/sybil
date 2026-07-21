import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  assemble,
  eventVisibleOnIndex,
  isClosed,
  isMirror,
  isNative,
  useMarketsIndex,
  type Market,
} from "./use-markets";

function mk(partial: Partial<Market> & { market_id: number }): Market {
  return {
    name: `m${partial.market_id}`,
    status: "active",
    ...partial,
  } as Market;
}

describe("markets/use-markets helpers", () => {
  it("isClosed only true for explicit closed===true", () => {
    expect(isClosed(mk({ market_id: 1, closed: true }))).toBe(true);
    expect(isClosed(mk({ market_id: 2, closed: false }))).toBe(false);
    expect(isClosed(mk({ market_id: 3 }))).toBe(false);
  });

  it("isMirror/isNative partition on polymarket_condition_id (SYB-149)", () => {
    const mirror = mk({ market_id: 1, polymarket_condition_id: "0xabc" });
    const native = mk({ market_id: 2 });
    const nativeExplicitNull = mk({
      market_id: 3,
      polymarket_condition_id: null,
    });

    expect(isMirror(mirror)).toBe(true);
    expect(isNative(mirror)).toBe(false);

    expect(isNative(native)).toBe(true);
    expect(isMirror(native)).toBe(false);

    // Explicit null (not just absent) is still native.
    expect(isNative(nativeExplicitNull)).toBe(true);
    expect(isMirror(nativeExplicitNull)).toBe(false);

    // Every market is exactly one of the two.
    for (const m of [mirror, native, nativeExplicitNull]) {
      expect(isMirror(m)).toBe(!isNative(m));
    }
  });

  it("eventVisibleOnIndex hides only when every market is closed", () => {
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: false }),
      ]),
    ).toBe(true);
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: true }),
      ]),
    ).toBe(false);
  });

  it("does not apply title-based discovery filtering", () => {
    const client = new QueryClient();
    const market = mk({ market_id: 8, name: "Any operator-chosen title" });
    client.setQueryData(["markets", "all"], [market]);

    function Probe() {
      const { bundle } = useMarketsIndex();
      return createElement(
        "span",
        null,
        [...(bundle?.byId.values() ?? [])]
          .map((market) => market.name)
          .join("|"),
      );
    }

    const html = renderToStaticMarkup(
      createElement(QueryClientProvider, { client }, createElement(Probe)),
    );

    expect(html).toContain("Any operator-chosen title");
    expect(client.getQueryData(["markets", "all"])).toEqual([market]);
  });

  it("assemble keeps closed markets in byId and groups", () => {
    const bundle = assemble([
      mk({ market_id: 1, event_id: "e1", event_title: "E1", closed: true }),
      mk({ market_id: 2, event_id: "e1", event_title: "E1", closed: false }),
      mk({ market_id: 3, closed: true }),
    ]);
    expect(bundle.byId.has(1)).toBe(true); // closed retained
    expect(bundle.byId.has(3)).toBe(true);
    const e1 = bundle.groups.find((g) => g.eventId === "e1");
    expect(e1?.markets.length).toBe(2); // both, incl. closed
    expect(bundle.total).toBe(3);
  });
});
