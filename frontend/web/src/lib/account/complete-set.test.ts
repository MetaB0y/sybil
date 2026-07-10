import { describe, expect, it } from "vitest";
import {
  completeSetReason,
  findCompleteSetBlockers,
  type CoverageOrder,
} from "./complete-set";

const GROUP = [71, 72, 73];
const order = (
  order_id: number,
  market_id: number,
  side: string,
): CoverageOrder => ({ order_id, market_id, side });

describe("findCompleteSetBlockers", () => {
  it("allows a lone NO — it covers every outcome but its own", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: [],
        marketId: 72,
        side: "BuyNo",
      }),
    ).toBeNull();
  });

  it("blocks NO when a YES on the same outcome rests (the prod case)", () => {
    const resting = [order(688790, 72, "BuyYes")];
    const blockers = findCompleteSetBlockers({
      groupMarkets: GROUP,
      restingOrders: resting,
      marketId: 72,
      side: "BuyNo",
    });
    expect(blockers).toEqual(resting);
  });

  it("blocks YES when a NO on the same outcome rests (the mirror case)", () => {
    const resting = [order(1, 72, "BuyNo")];
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: resting,
        marketId: 72,
        side: "BuyYes",
      }),
    ).toEqual(resting);
  });

  it("blocks a second NO on a different outcome", () => {
    const resting = [order(1, 71, "BuyNo")];
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: resting,
        marketId: 72,
        side: "BuyNo",
      }),
    ).toEqual(resting);
  });

  it("allows NO when the resting YES is on a different outcome", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: [order(1, 71, "BuyYes")],
        marketId: 72,
        side: "BuyNo",
      }),
    ).toBeNull();
  });

  it("blocks YES once resting YES orders cover every other outcome", () => {
    const resting = [order(1, 71, "BuyYes"), order(2, 73, "BuyYes")];
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: resting,
        marketId: 72,
        side: "BuyYes",
      }),
    ).toEqual(resting);
  });

  it("ignores sells — they reduce exposure and never complete a set", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: [order(1, 72, "SellYes"), order(2, 71, "SellNo")],
        marketId: 72,
        side: "BuyNo",
      }),
    ).toBeNull();
  });

  it("never blocks a sell order itself", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: [order(1, 72, "BuyYes")],
        marketId: 72,
        side: "SellYes",
      }),
    ).toBeNull();
  });

  it("ignores resting orders outside the group", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: GROUP,
        restingOrders: [order(1, 999, "BuyYes")],
        marketId: 72,
        side: "BuyNo",
      }),
    ).toBeNull();
  });

  it("never blocks an ungrouped market, whatever rests against it", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: [],
        restingOrders: [order(1, 72, "BuyYes")],
        marketId: 72,
        side: "BuyNo",
      }),
    ).toBeNull();
  });

  it("handles a binary group: YES + NO on the same market completes it", () => {
    expect(
      findCompleteSetBlockers({
        groupMarkets: [10, 11],
        restingOrders: [order(1, 10, "BuyYes")],
        marketId: 10,
        side: "BuyNo",
      }),
    ).toHaveLength(1);
  });
});

describe("completeSetReason", () => {
  const labelOf = (m: number) => (m === 71 ? "OpenAI" : null);

  it("names the opposite side when the blocker is on this outcome", () => {
    const reason = completeSetReason(
      [order(1, 72, "BuyYes")],
      "BuyNo",
      72,
      labelOf,
    );
    expect(reason).toContain("open YES order on this outcome");
    expect(reason).toContain("cancel");
  });

  it("names the other outcome when a single foreign order blocks", () => {
    expect(completeSetReason([order(1, 71, "BuyNo")], "BuyNo", 72, labelOf)).toContain(
      "on OpenAI",
    );
  });

  it("counts blockers when several orders are responsible", () => {
    const reason = completeSetReason(
      [order(1, 71, "BuyYes"), order(2, 73, "BuyYes")],
      "BuyYes",
      72,
      labelOf,
    );
    expect(reason).toContain("2 open orders");
  });
});
