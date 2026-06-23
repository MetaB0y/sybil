import { describe, expect, it } from "vitest";
import { formatCentsPrecise, formatPercentPrecise } from "./nanos";

// 1 cent = 1e7 nanos; 1% probability = 1e7 nanos (a binary price IS its odds).
const CENT = 10_000_000n;

describe("formatCentsPrecise", () => {
  it("keeps whole cents clean (no trailing .0)", () => {
    expect(formatCentsPrecise(5n * CENT)).toBe("5¢");
    expect(formatCentsPrecise(12n * CENT)).toBe("12¢");
    expect(formatCentsPrecise(0n)).toBe("0¢");
  });

  it("shows one decimal of sub-cent precision", () => {
    // The motivating case: a weighted-average entry over 5¢ and 6¢ fills is 5.5¢,
    // which formatCents would round to "6¢" and visually break P&L reconciliation.
    expect(formatCentsPrecise(55_000_000n)).toBe("5.5¢");
    expect(formatCentsPrecise(49_000_000n)).toBe("4.9¢");
  });

  it("shows real sub-1¢ prices instead of clamping to <1¢", () => {
    expect(formatCentsPrecise(3_000_000n)).toBe("0.3¢"); // 0.3¢
  });

  it("shows real >99¢ prices instead of clamping to >99¢", () => {
    expect(formatCentsPrecise(997_000_000n)).toBe("99.7¢");
  });

  it("rounds to the nearest tenth of a cent", () => {
    expect(formatCentsPrecise(44_500_000n)).toBe("4.5¢"); // 4.45¢ → 4.5¢
  });

  it("never prints a positive sub-tenth price as a flat 0", () => {
    expect(formatCentsPrecise(400_000n)).toBe("<0.1¢"); // 0.04¢
  });

  it("accepts string / number nanos via parseNanos", () => {
    expect(formatCentsPrecise("55000000")).toBe("5.5¢");
    expect(formatCentsPrecise(55_000_000)).toBe("5.5¢");
  });
});

describe("formatPercentPrecise", () => {
  it("keeps whole percents clean", () => {
    expect(formatPercentPrecise(63n * CENT)).toBe("63%");
    expect(formatPercentPrecise(0n)).toBe("0%");
  });

  it("shows one decimal and surfaces edge odds (no <1% clamp)", () => {
    expect(formatPercentPrecise(634_000_000n)).toBe("63.4%");
    expect(formatPercentPrecise(3_000_000n)).toBe("0.3%");
  });
});
