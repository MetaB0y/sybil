import { describe, expect, it } from "vitest";
import {
  fmtPrice,
  fmtProb,
  fmtPct,
  pctWidth,
  dollars,
  moneySigned,
  fmtInt,
  shortRoot,
} from "./format";

describe("dev/format", () => {
  it("fmtPrice formats nanos as dollars to 3dp", () => {
    expect(fmtPrice(500_000_000)).toBe("$0.500");
    expect(fmtPrice("500000000")).toBe("$0.500");
    expect(fmtPrice(null)).toBe("-");
    expect(fmtPrice(undefined)).toBe("-");
  });
  it("fmtProb formats a 0-1 ratio as a percentage", () => {
    expect(fmtProb(0.5)).toBe("50.0%");
    expect(fmtProb(null)).toBe("-");
    expect(fmtProb(NaN)).toBe("-");
  });
  it("fmtPct formats a ratio, returns - for falsy", () => {
    expect(fmtPct(0.1)).toBe("10.0%");
    expect(fmtPct(0)).toBe("-");
  });
  it("pctWidth clamps a nanos price to 0-100", () => {
    expect(pctWidth(500_000_000)).toBe(50);
    expect(pctWidth(2_000_000_000)).toBe(100);
    expect(pctWidth(null)).toBe(0);
  });
  it("dollars rounds nanos to whole dollars with grouping", () => {
    expect(dollars(1_234_000_000_000)).toBe("1,234");
    expect(dollars(0)).toBe("0");
    expect(dollars(null)).toBe("0");
  });
  it("moneySigned prefixes +$ / -$", () => {
    expect(moneySigned(1234)).toBe("+$1,234");
    expect(moneySigned(-1234)).toBe("-$1,234");
    expect(moneySigned(0)).toBe("+$0");
  });
  it("fmtInt groups thousands", () => {
    expect(fmtInt(12345)).toBe("12,345");
    expect(fmtInt(null)).toBe("0");
  });
  it("shortRoot takes the first 8 chars", () => {
    expect(shortRoot("abcdef0123456789")).toBe("abcdef01");
    expect(shortRoot(null)).toBe("...");
  });
});
