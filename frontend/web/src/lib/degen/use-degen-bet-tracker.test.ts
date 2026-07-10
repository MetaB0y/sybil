import { describe, expect, it } from "vitest";
import type { PendingOrder } from "@/lib/markets/use-pending-orders";
import type { DegenActive } from "./use-degen-bet-tracker";
import { resolveDegenPendingStatus } from "./use-degen-bet-tracker";

const active: DegenActive = {
  accountId: 1,
  marketId: 7,
  outcome: "YES",
  targetQty: 20_000n,
  betUsd: 10,
  limitPriceNanos: 540_000_000n,
  submitHeight: 100,
  batchAnchorPerfMs: 1_000,
  submitPerfMs: 1_500,
  expiresAtBlock: 112,
  priorMaxOrderId: 40,
};

function pending(orderId: number): PendingOrder {
  return {
    account_id: active.accountId,
    created_at_block: active.submitHeight,
    expires_at_block: active.expiresAtBlock,
    limit_price_nanos: String(active.limitPriceNanos),
    market_id: active.marketId,
    order_id: orderId,
    original_quantity: Number(active.targetQty),
    remaining_quantity: Number(active.targetQty),
    side: "BuyYes",
  };
}

describe("resolveDegenPendingStatus", () => {
  it("reports the bound degen order as open while it remains pending", () => {
    expect(resolveDegenPendingStatus([pending(41)], active, 41, true)).toEqual({
      pendingBoundId: 41,
      orderOpen: true,
    });
  });

  it("reports the bound order as closed once it leaves pending orders", () => {
    expect(resolveDegenPendingStatus([], active, 41, true)).toEqual({
      pendingBoundId: null,
      orderOpen: false,
    });
  });

  it("does not claim the order is closed before open orders load", () => {
    expect(resolveDegenPendingStatus([], active, 41, false)).toEqual({
      pendingBoundId: null,
      orderOpen: null,
    });
  });
});
