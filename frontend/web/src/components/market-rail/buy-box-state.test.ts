import { describe, expect, it } from "vitest";
import { tradeCtaState } from "./buy-box";

const ready = {
  connected: true,
  submitting: false,
  direction: "buy" as const,
  balanceKnown: true,
  balancePending: false,
  positionsKnown: true,
  positionsPending: false,
  insufficientBuy: false,
  insufficientSell: false,
};

describe("tradeCtaState", () => {
  it("keeps Connect available before authentication", () => {
    expect(tradeCtaState({ ...ready, connected: false })).toBe("connect");
  });

  it("does not sign a buy while balance is pending or unavailable", () => {
    expect(
      tradeCtaState({ ...ready, balanceKnown: false, balancePending: true }),
    ).toBe("waiting_balance");
    expect(
      tradeCtaState({ ...ready, balanceKnown: false, balancePending: false }),
    ).toBe("balance_unavailable");
  });

  it("does not sign a sell while positions are pending or unavailable", () => {
    expect(
      tradeCtaState({
        ...ready,
        direction: "sell",
        positionsKnown: false,
        positionsPending: true,
      }),
    ).toBe("waiting_positions");
    expect(
      tradeCtaState({
        ...ready,
        direction: "sell",
        positionsKnown: false,
        positionsPending: false,
      }),
    ).toBe("positions_unavailable");
  });

  it("prioritizes signing, insufficiency, then readiness", () => {
    expect(tradeCtaState({ ...ready, submitting: true })).toBe("signing");
    expect(tradeCtaState({ ...ready, insufficientBuy: true })).toBe(
      "insufficient_buy",
    );
    expect(
      tradeCtaState({
        ...ready,
        direction: "sell",
        insufficientSell: true,
      }),
    ).toBe("insufficient_sell");
    expect(tradeCtaState(ready)).toBe("ready");
  });
});
