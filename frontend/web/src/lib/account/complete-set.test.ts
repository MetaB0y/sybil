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
): CoverageOrder {
  return { order_id, market_id, side };
}

describe("findCompleteSetBlockers", () => {
  it("does not infer a group from event siblings", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: [],
        restingOrders: [order(1, 11, "BuyYes")],
        marketId: 11,
        side: "BuyNo",
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
