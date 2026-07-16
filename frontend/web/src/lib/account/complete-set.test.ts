import { describe, expect, it } from "vitest";
import {
  completeSetReason,
  findCompleteSetBlockers,
  type CoverageOrder,
} from "./complete-set";

const group = [11, 12, 13];

function order(
  order_id: number,
  market_id: number,
  side: CoverageOrder["side"],
  limit_price_nanos = "600000000",
): CoverageOrder {
  return { order_id, market_id, side, limit_price_nanos };
}

describe("findCompleteSetBlockers", () => {
  it("does not infer a group from event siblings", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: [],
        restingOrders: [order(1, 12, "BuyYes")],
        marketId: 11,
        side: "BuyNo",
        limitPriceNanos: 600_000_000n,
      }),
    ).toBeNull();
  });

  it("does not apply group STP to an independent binary market", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: [],
        restingOrders: [order(1, 11, "BuyYes")],
        marketId: 11,
        side: "BuyNo",
        limitPriceNanos: 600_000_000n,
      }),
    ).toBeNull();
  });

  it("finds the same-market opposite resting buy", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [order(7, 11, "BuyYes")],
        marketId: 11,
        side: "BuyNo",
        limitPriceNanos: 600_000_000n,
      }),
    ).toEqual([order(7, 11, "BuyYes")]);
  });

  it("finds coverage supplied by another outcome", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [order(8, 12, "BuyNo")],
        marketId: 12,
        side: "BuyYes",
        limitPriceNanos: 600_000_000n,
      }),
    ).toEqual([order(8, 12, "BuyNo")]);
  });

  it("ignores sells and incomplete coverage", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [
          order(1, 11, "SellYes"),
          order(2, 12, "BuyYes"),
        ],
        marketId: 11,
        side: "BuyYes",
        limitPriceNanos: 400_000_000n,
      }),
    ).toBeNull();
  });

  it("allows complete outcome coverage when the bids do not cross", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [
          order(1, 11, "BuyYes", "300000000"),
          order(2, 12, "BuyYes", "300000000"),
        ],
        marketId: 13,
        side: "BuyYes",
        limitPriceNanos: 300_000_000n,
      }),
    ).toBeNull();
  });

  it("blocks a full YES set only when its limits fund the group mint", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [
          order(1, 11, "BuyYes", "400000000"),
          order(2, 12, "BuyYes", "350000000"),
        ],
        marketId: 13,
        side: "BuyYes",
        limitPriceNanos: 300_000_000n,
      }),
    ).toEqual([
      order(1, 11, "BuyYes", "400000000"),
      order(2, 12, "BuyYes", "350000000"),
    ]);
  });

  it("allows multiple NO bids that span the group without sharing a mint", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: group,
        restingOrders: [order(1, 11, "BuyNo", "800000000")],
        marketId: 12,
        side: "BuyNo",
        limitPriceNanos: 800_000_000n,
      }),
    ).toBeNull();
  });
});

describe("completeSetReason", () => {
  it("names a same-market blocker", () => {
    expect(
      completeSetReason(
        [order(7, 11, "BuyYes")],
        "BuyNo",
        11,
        () => null,
        "order",
      ),
    ).toContain("open YES order on this outcome");
  });

  it("uses the other outcome label when available", () => {
    expect(
      completeSetReason(
        [order(8, 12, "BuyNo")],
        "BuyYes",
        11,
        (id) => (id === 12 ? "Outcome B" : null),
      ),
    ).toContain("on Outcome B");
  });
});
