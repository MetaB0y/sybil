import { describe, expect, it } from "vitest";
import { DEGEN_BATCHES, ONE_DOLLAR_NANOS } from "./constants";
import { buildDegenOrder, degenLimitPrice } from "./degen";
import { SHARE_SCALE } from "@/lib/account/quantity";

const usd = (d: number): bigint => BigInt(Math.round(d * 1e9));

describe("buildDegenOrder", () => {
  it("composes a YES bet into a BuyYes order spec", () => {
    const mark = ONE_DOLLAR_NANOS / 2n; // 50¢
    const res = buildDegenOrder({
      side: "YES",
      betUsdNanos: usd(10),
      markNanos: mark,
      latestHeight: 1000n,
    });
    expect(res.ok).toBe(true);
    if (!res.ok) return;
    const limit = degenLimitPrice(mark); // 5.4e8
    expect(res.order.side).toBe("BuyYes");
    expect(res.order.limitPriceNanos).toBe(limit);
    expect(res.order.maxFill).toBe((usd(10) * SHARE_SCALE) / limit); // 18.518 shares
    expect(res.order.expiresAtBlock).toBe(1003n);
  });

  it("maps a NO bet to BuyNo", () => {
    const res = buildDegenOrder({
      side: "NO",
      betUsdNanos: usd(10),
      markNanos: ONE_DOLLAR_NANOS / 2n,
      latestHeight: 1n,
    });
    expect(res.ok).toBe(true);
    if (!res.ok) return;
    expect(res.order.side).toBe("BuyNo");
    expect(res.order.expiresAtBlock).toBe(1n + DEGEN_BATCHES);
  });

  it("reports below-minimum when the budget can't afford one share-unit", () => {
    const res = buildDegenOrder({
      side: "YES",
      betUsdNanos: 1n,
      markNanos: ONE_DOLLAR_NANOS / 2n,
      latestHeight: 1n,
    });
    expect(res.ok).toBe(false);
    if (res.ok) return;
    expect(res.reason).toBe("below-minimum");
  });
});
