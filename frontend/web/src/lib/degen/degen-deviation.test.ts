import { describe, expect, it } from "vitest";
import { DEGEN_PEAK_NANOS, ONE_DOLLAR_NANOS } from "./constants";
import { degenDeviation } from "./degen";

const cents = (c: number): bigint => BigInt(Math.round(c * 1e7)); // 1¢ = 1e7 nanos

describe("degenDeviation", () => {
  it("peaks at exactly DEGEN_PEAK_NANOS at 50¢", () => {
    expect(degenDeviation(ONE_DOLLAR_NANOS / 2n)).toBe(DEGEN_PEAK_NANOS);
  });

  it("matches the reference table within 0.02¢", () => {
    const tol = cents(0.02);
    const within = (priceCents: number, expectedCents: number) => {
      const got = degenDeviation(cents(priceCents));
      const diff = got > cents(expectedCents) ? got - cents(expectedCents) : cents(expectedCents) - got;
      expect(diff).toBeLessThanOrEqual(tol);
    };
    within(50, 4.0);
    within(25, 2.74);
    within(10, 1.06);
    within(5, 0.46);
    within(2, 0.15);
    within(1, 0.067);
  });

  it("is symmetric around 50¢", () => {
    for (const c of [1, 5, 10, 25, 40]) {
      expect(degenDeviation(cents(c))).toBe(degenDeviation(ONE_DOLLAR_NANOS - cents(c)));
    }
  });

  it("decreases monotonically from the center toward the edge", () => {
    const seq = [50, 25, 10, 5, 1].map((c) => degenDeviation(cents(c)));
    for (let i = 1; i < seq.length; i++) {
      expect(seq[i]).toBeLessThan(seq[i - 1]);
    }
  });

  it("never exceeds the peak and is zero at the boundaries", () => {
    for (const c of [1, 5, 25, 50, 75, 99]) {
      expect(degenDeviation(cents(c))).toBeLessThanOrEqual(DEGEN_PEAK_NANOS);
    }
    expect(degenDeviation(0n)).toBe(0n);
    expect(degenDeviation(ONE_DOLLAR_NANOS)).toBe(0n);
    expect(degenDeviation(-1n)).toBe(0n);
  });
});
