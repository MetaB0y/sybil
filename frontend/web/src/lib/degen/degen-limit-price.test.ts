import { describe, expect, it } from "vitest";
import { ONE_DOLLAR_NANOS } from "./constants";
import { degenDeviation, degenLimitPrice } from "./degen";

const cents = (c: number): bigint => BigInt(Math.round(c * 1e7));

describe("degenLimitPrice", () => {
  it("adds the tax to the mark for an interior price", () => {
    const mark = ONE_DOLLAR_NANOS / 2n; // 50¢
    expect(degenLimitPrice(mark)).toBe(mark + degenDeviation(mark));
  });

  it("is strictly worse (higher) than the mark for interior prices", () => {
    for (const c of [8, 25, 50, 75, 92]) {
      const mark = cents(c);
      expect(degenLimitPrice(mark)).toBeGreaterThan(mark);
    }
  });

  it("clamps to the lower bound at price 0", () => {
    expect(degenLimitPrice(0n)).toBe(1n);
  });

  it("never reaches or exceeds $1, and stays positive, across the range", () => {
    for (const c of [0.1, 1, 5, 50, 95, 99, 99.99]) {
      const y = degenLimitPrice(cents(c));
      expect(y).toBeGreaterThan(0n);
      expect(y).toBeLessThan(ONE_DOLLAR_NANOS);
    }
  });
});
