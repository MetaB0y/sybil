import { describe, expect, it } from "vitest";
import {
  priceState,
  priceGap,
  filterMarkets,
  pendingIndex,
  topMarketsByVolume24h,
  latestBlockByMarketRows,
  buildInsights,
  fmtLiquidity,
  fmtYesDelta24h,
  accountAggregates,
  formatOrder,
} from "./derive";
import type {
  DevMarket,
  DevBlock,
  DevPendingOrder,
  DevAccountPortfolio,
} from "./types";

const mkt = (o: Partial<DevMarket> & { market_id: number; name: string }): DevMarket => o;

describe("dev/derive", () => {
  it("priceState classifies cleared / ref only / no clears", () => {
    expect(priceState(mkt({ market_id: 1, name: "a", yes_price_nanos: 5e8 }))).toBe("cleared");
    expect(priceState(mkt({ market_id: 1, name: "a", reference_price_nanos: 5e8 }))).toBe("ref only");
    expect(priceState(mkt({ market_id: 1, name: "a" }))).toBe("no clears");
  });

  it("priceGap is the absolute yes-vs-ref difference in dollars", () => {
    expect(priceGap(mkt({ market_id: 1, name: "a", yes_price_nanos: 6e8, reference_price_nanos: 5e8 }))).toBeCloseTo(0.1);
    expect(priceGap(mkt({ market_id: 1, name: "a", yes_price_nanos: 6e8 }))).toBe(0);
  });

  it("filterMarkets applies search and the cleared state filter", () => {
    const markets = [
      mkt({ market_id: 1, name: "Trump wins", yes_price_nanos: 5e8, volume_nanos: 100 }),
      mkt({ market_id: 2, name: "Rain tomorrow", volume_nanos: 200 }),
    ];
    const search = filterMarkets(markets, { search: "trump", group: "", state: "all" }, pendingIndex([]));
    expect(search.map((m) => m.market_id)).toEqual([1]);
    const cleared = filterMarkets(markets, { search: "", group: "", state: "cleared" }, pendingIndex([]));
    expect(cleared.map((m) => m.market_id)).toEqual([1]);
  });

  it("pendingIndex counts orders per market by side", () => {
    const orders: DevPendingOrder[] = [
      { market_id: 1, account_id: 0, side: "BuyYes" },
      { market_id: 1, account_id: 0, side: "BuyNo" },
    ];
    const idx = pendingIndex(orders);
    expect(idx.get(1)?.count).toBe(2);
    expect(idx.get(1)?.BuyYes).toBe(1);
  });

  it("topMarketsByVolume24h sorts by 24h volume desc", () => {
    const markets = [
      mkt({ market_id: 1, name: "a", volume_24h_nanos: 10 }),
      mkt({ market_id: 2, name: "b", volume_24h_nanos: 99 }),
    ];
    expect(topMarketsByVolume24h(markets).map((m) => m.market_id)).toEqual([2, 1]);
  });

  it("latestBlockByMarketRows expands the by_market sidecar", () => {
    const block: DevBlock = { height: 5, by_market: { "7": { placers: 2, volume_nanos: 30, matched: 1 } } };
    const rows = latestBlockByMarketRows(block, new Map());
    expect(rows[0]).toMatchObject({ market_id: 7, placers: 2, matched: 1 });
  });

  it("buildInsights always reports price coverage", () => {
    const insights = buildInsights({ markets: [mkt({ market_id: 1, name: "a", yes_price_nanos: 5e8 })], blocks: [], pendingOrders: [] });
    expect(insights.some((i) => i.title === "Price coverage")).toBe(true);
  });

  it("fmtLiquidity formats avg and band", () => {
    expect(fmtLiquidity(mkt({ market_id: 1, name: "a", liquidity_avg10_nanos: 2e9, liquidity_band_nanos: 5e8 }))).toBe("$2.00 ±$0.50");
    expect(fmtLiquidity(mkt({ market_id: 1, name: "a" }))).toBe("—");
  });

  it("fmtYesDelta24h shows a signed cent delta", () => {
    expect(fmtYesDelta24h(mkt({ market_id: 1, name: "a", yes_price_nanos: 6e8, yes_price_24h_ago_nanos: 5e8 }))).toBe("+10.0¢");
    expect(fmtYesDelta24h(mkt({ market_id: 1, name: "a" }))).toBe("—");
  });

  it("accountAggregates sums cash and flags an inactive account 0", () => {
    const accounts: DevAccountPortfolio[] = [
      { account_id: 0, balance_nanos: 1e9, positions: [] },
      {
        account_id: 1,
        balance_nanos: 5e9,
        positions: [{ market_id: 1, outcome: "YES", quantity: 10 }],
      },
      { account_id: 2, balance_nanos: 3e9, pnl_nanos: 2e9, positions: [] },
    ];
    const marketIdx = new Map<number, DevMarket>([
      [1, mkt({ market_id: 1, name: "a", reference_price_nanos: 5e8 })],
    ]);
    const agg = accountAggregates(accounts, marketIdx, null);
    expect(agg.mmCashNanos).toBe(8e9);
    expect(agg.accountZeroIsInactive).toBe(true);
    expect(agg.activeTradingAccounts.map((a) => a.account_id)).toEqual([1, 2]);
  });

  it("formatOrder formats side, quantity and price for both price branches", () => {
    expect(formatOrder({ side: "BuyYes", quantity: 10, price: 6e8 })).toBe("BuyYes 10 @ $0.600");
    expect(formatOrder({ side: "BuyNo", quantity: 5, price: 0.42 })).toBe("BuyNo 5 @ 42.0%");
  });
});
