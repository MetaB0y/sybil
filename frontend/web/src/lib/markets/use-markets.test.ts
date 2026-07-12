import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  assemble,
  eventVisibleOnIndex,
  INDEX_MARKET_FIELDS,
  isClosed,
  isMirror,
  isNative,
  isInternalFixtureMarket,
  publicMarkets,
  toIndexMarket,
  useMarketsIndex,
  type IndexMarket,
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

  it("keeps deterministic deployment fixtures out of public market discovery", () => {
    const fixture = mk({
      market_id: 247,
      name: "SYB-247 deterministic crossing v1 run 1783836058392115051",
    });
    const real = mk({ market_id: 8, name: "Will the devnet launch?" });

    expect(isInternalFixtureMarket(fixture)).toBe(true);
    expect(isInternalFixtureMarket(real)).toBe(false);
    expect(publicMarkets([fixture, real])).toEqual([real]);

    const rawBundle = assemble([fixture, real]);
    expect(rawBundle.total).toBe(2);
    expect(rawBundle.byId.get(fixture.market_id)).toEqual(fixture);

    const publicBundle = assemble(publicMarkets([fixture, real]));
    expect(publicBundle.total).toBe(1);
    expect(publicBundle.byId.has(fixture.market_id)).toBe(false);
    expect(publicBundle.byId.get(real.market_id)).toEqual(real);
  });

  it("filters a raw shared cache only at the public discovery observer", () => {
    const client = new QueryClient();
    const fixture = mk({
      market_id: 247,
      name: "SYB-247 deterministic crossing v1 run 1",
    });
    const real = mk({ market_id: 8, name: "Real market" });
    client.setQueryData(["markets", "all"], [fixture, real]);

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
      createElement(
        QueryClientProvider,
        { client },
        createElement(Probe),
      ),
    );

    expect(html).toContain("Real market");
    expect(html).not.toContain("SYB-247 deterministic crossing");
    expect(client.getQueryData(["markets", "all"])).toEqual([fixture, real]);
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

  it("compacts the SSR index payload without dropping index behavior", () => {
    const full = mk({
      market_id: 7,
      name: "Will the compact index stay truthful?",
      description: "detail-only resolution prose ".repeat(20),
      external_url: "https://example.test/resolution",
      yes_price_nanos: "600000000",
      event_id: "event-7",
      event_title: "Compact index",
      category: "Technology",
      categories: ["Technology", "AI"],
      volume_nanos: "123000000000",
      liquidity_avg10_nanos: "4000000000",
      trader_count: 12,
      polymarket_condition_id: "0x07",
      group_item_title: "Yes",
      closed: false,
    });

    const compact = toIndexMarket(full);

    expect(Object.keys(compact).sort()).toEqual(
      INDEX_MARKET_FIELDS.filter((key) => full[key] != null).sort(),
    );
    expect(compact).toMatchObject({
      market_id: 7,
      name: full.name,
      event_id: "event-7",
      categories: ["Technology", "AI"],
      trader_count: 12,
      closed: false,
    });
    expect("description" in compact).toBe(false);
    expect("external_url" in compact).toBe(false);
    expect("yes_price_nanos" in compact).toBe(false);

    const bundle = assemble([compact]);
    expect(bundle.total).toBe(1);
    expect(bundle.groups[0]?.name).toBe("Compact index");
    expect(bundle.groups[0]?.markets[0]).toEqual(compact);
  });

  it("keeps the compact server snapshot out of the canonical query cache", () => {
    const client = new QueryClient();
    const compact = toIndexMarket(
      mk({
        market_id: 9,
        name: "Compact placeholder",
        description: "detail-only",
      }),
    );

    function Probe({ initial }: { initial: IndexMarket[] }) {
      const { bundle } = useMarketsIndex(initial);
      return createElement(
        "span",
        null,
        bundle?.byId.get(9)?.name ?? "missing",
      );
    }

    const html = renderToStaticMarkup(
      createElement(
        QueryClientProvider,
        { client },
        createElement(Probe, { initial: [compact] }),
      ),
    );

    expect(html).toContain("Compact placeholder");
    expect(client.getQueryData(["markets", "all"])).toBeUndefined();
  });
});
